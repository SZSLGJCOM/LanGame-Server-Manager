use super::*;
use std::future::Future;
use std::task::Poll;

fn job() -> KnowledgeSyncJob {
    KnowledgeSyncJob {
        id: uuid::Uuid::new_v4().to_string(),
        module_id: None,
        state: KnowledgeJobState::Running,
        started_at: 12,
        finished_at: None,
        progress: SyncProgress::default(),
        report: None,
        error: None,
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-test-knowledge-job-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn automatic_sync_respects_disabled_interval_and_backwards_clock() {
    let mut settings = KnowledgeSettings::default();
    assert!(due(&settings, None, 0));
    assert!(!due(&settings, Some(100), 99));
    assert!(!due(&settings, Some(100), 100 + 24 * 3600 - 1));
    assert!(due(&settings, Some(100), 100 + 24 * 3600));
    settings.auto_update = false;
    assert!(!due(&settings, None, u64::MAX));
}

#[test]
fn scheduler_starts_from_synchronous_setup_and_shuts_down() {
    assert!(tokio::runtime::Handle::try_current().is_err());
    let app = tauri::test::mock_builder()
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = DesktopState::default();
    let owner = state.knowledge.clone();
    // An unbootstrapped desktop must not acquire or update knowledge data.
    app.manage(state);
    spawn_knowledge_scheduler(app.handle().clone());
    assert!(owner.scheduler.lock().unwrap().is_some());
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(5), owner.shutdown())
            .await
            .expect("scheduler should acknowledge shutdown");
    });
    assert!(owner.scheduler.lock().unwrap().is_none());
    assert!(owner.library.try_lock().unwrap().is_none());
}

#[test]
fn cancellation_is_bound_to_active_job_and_preserves_ownership() {
    let owner = KnowledgeCoordinator::new(PathBuf::new(), PathBuf::new());
    let pending = job();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut state = owner.lock().unwrap();
        state.job = Some(pending.clone());
        state.cancellation = Some(cancel.clone());
    }
    assert!(!owner.cancel(&uuid::Uuid::new_v4().to_string()).unwrap());
    assert!(!cancel.load(Ordering::Acquire));
    assert!(owner.cancel(&pending.id).unwrap());
    assert!(cancel.load(Ordering::Acquire));
    assert_eq!(
        owner.lock().unwrap().job.as_ref().unwrap().state,
        KnowledgeJobState::Cancelling
    );
    assert!(
        owner.lock().unwrap().cancellation.is_some(),
        "requesting cancellation cannot release a running worker"
    );
    owner.lock().unwrap().job.as_mut().unwrap().state = KnowledgeJobState::Completed;
    assert!(!owner.cancel(&pending.id).unwrap());
    assert!(owner.cancel("not-an-id").is_err());
}

#[tokio::test]
async fn interrupted_job_is_persisted_without_touching_published_content() {
    let scratch = Scratch::new();
    let owner = KnowledgeCoordinator::new(scratch.0.clone(), PathBuf::new());
    let index = scratch.0.join("published-index.fixture");
    tokio::fs::write(&index, b"previous published content")
        .await
        .unwrap();
    let pending = job();
    owner.persist_job(&pending).await.unwrap();
    let recovered = owner.load_job().await.unwrap().unwrap();
    assert_eq!(recovered.id, pending.id);
    assert_eq!(recovered.state, KnowledgeJobState::Interrupted);
    assert!(recovered.finished_at.is_some());
    assert!(
        recovered
            .error
            .as_deref()
            .unwrap()
            .contains("last published index")
    );
    assert_eq!(
        owner.load_job().await.unwrap().unwrap().state,
        KnowledgeJobState::Interrupted
    );
    assert_eq!(
        tokio::fs::read(&index).await.unwrap(),
        b"previous published content"
    );
}

#[tokio::test]
async fn partial_and_failed_updates_retain_honest_durable_outcomes() {
    let scratch = Scratch::new();
    let owner = KnowledgeCoordinator::new(scratch.0.clone(), PathBuf::new());
    owner
        .finish(
            job(),
            Ok(SyncReport {
                sources_succeeded: 4,
                sources_failed: 1,
                errors: vec!["official source unavailable".into()],
                ..SyncReport::default()
            }),
        )
        .await;
    let partial = owner.load_job().await.unwrap().unwrap();
    assert_eq!(partial.state, KnowledgeJobState::Partial);
    assert_eq!(partial.report.unwrap().sources_failed, 1);
    owner.finish(job(), Ok(SyncReport {
        sources_succeeded: 4, sources_failed: 0,
        errors: vec!["Documentation synchronization reached its 30-minute budget; remaining sources will resume on the next sync".into()],
        ..SyncReport::default()
    })).await;
    let budget_limited = owner.load_job().await.unwrap().unwrap();
    assert_eq!(
        budget_limited.state,
        KnowledgeJobState::Partial,
        "An unfinished source list cannot be reported as a complete update"
    );
    assert_eq!(budget_limited.report.unwrap().sources_failed, 0);
    owner
        .finish(job(), Err(app_knowledge::KnowledgeError::Cancelled))
        .await;
    assert_eq!(
        owner.load_job().await.unwrap().unwrap().state,
        KnowledgeJobState::Cancelled
    );
    owner
        .finish(
            job(),
            Err(app_knowledge::KnowledgeError::Model(
                "model hash mismatch".into(),
            )),
        )
        .await;
    let failed = owner.load_job().await.unwrap().unwrap();
    assert_eq!(failed.state, KnowledgeJobState::Failed);
    assert!(failed.error.unwrap().contains("model hash mismatch"));
}

#[tokio::test]
async fn shutdown_cannot_reopen_the_library_after_a_waiting_reader_passed_its_first_check() {
    let scratch = Scratch::new();
    let owner = KnowledgeCoordinator::new(scratch.0.clone(), PathBuf::new());
    let held_library = owner.library.lock().await;
    let mut reader = Box::pin(owner.library());
    std::future::poll_fn(|context| {
        assert!(reader.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    let mut shutdown = Box::pin(owner.shutdown());
    std::future::poll_fn(|context| {
        assert!(shutdown.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(owner.stopping.load(Ordering::Acquire));
    drop(held_library);
    let result = reader.await;
    let rejected = result.is_err();
    if let Ok(library) = result {
        library.close().await;
    }
    shutdown.await;
    assert!(
        rejected,
        "A reader waiting before shutdown must not create or reopen a library after sealing"
    );
    assert!(!scratch.0.join("library.sqlite3").exists());
}

#[tokio::test]
async fn shutdown_waits_for_registered_work_to_acknowledge_cancellation_and_settle() {
    let owner = Arc::new(KnowledgeCoordinator::new(PathBuf::new(), PathBuf::new()));
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
    let (settled_tx, settled_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        while !worker_cancel.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
        observed_tx.send(()).unwrap();
        settled_rx.await.unwrap();
    });
    {
        let mut state = owner.lock().unwrap();
        state.job = Some(job());
        state.cancellation = Some(cancel);
        state.worker = Some(worker);
    }
    let admission = owner.admission.lock().await;
    let mut closing = Box::pin(owner.shutdown());
    std::future::poll_fn(|context| {
        assert!(closing.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(
        !owner.stopping.load(Ordering::Acquire),
        "Admission registration must finish before shutdown seals the coordinator"
    );
    drop(admission);
    let stopping = tokio::spawn({
        let owner = owner.clone();
        async move {
            owner.shutdown().await;
        }
    });
    drop(closing);
    tokio::time::timeout(Duration::from_secs(2), observed_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !stopping.is_finished(),
        "Cancellation acknowledgement is not cleanup completion"
    );
    settled_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), stopping)
        .await
        .unwrap()
        .unwrap();
    assert!(owner.lock().unwrap().worker.is_none());
    assert!(owner.library().await.is_err());
}
