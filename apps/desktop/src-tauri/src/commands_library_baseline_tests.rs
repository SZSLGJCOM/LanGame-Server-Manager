use super::*;
use app_core::InstallVerificationStage;
use std::sync::mpsc;
use std::time::Duration;

#[path = "commands_program_update_tests.rs"]
mod program_update;

struct WorkerResource(Arc<AtomicBool>);

impl Drop for WorkerResource {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn verification_snapshot(stage: InstallVerificationStage) -> InstallVerificationProgress {
    InstallVerificationProgress {
        stage,
        completed_bytes: 64,
        total_bytes: 128,
        completed_files: 1,
        total_files: 2,
    }
}

#[tokio::test]
async fn library_baseline_progress_runs_on_caller_and_keeps_final_worker_snapshot() {
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("baseline progress test")
        .unwrap();
    let installation = app_steamcmd::InstallCancellation::new();
    let caller_thread = std::thread::current().id();
    let released = Arc::new(AtomicBool::new(false));
    let resource = WorkerResource(Arc::clone(&released));
    let (continue_tx, continue_rx) = mpsc::channel();
    let mut reports = Vec::new();
    run_baseline_worker(
        &operation,
        &installation,
        "fixture",
        |progress| {
            assert_eq!(std::thread::current().id(), caller_thread);
            if progress.stage == InstallVerificationStage::SteamFiles {
                assert!(!released.load(Ordering::Acquire));
                continue_tx.send(()).unwrap();
            }
            reports.push(progress);
        },
        move |_, progress| {
            let _resource = resource;
            assert_ne!(std::thread::current().id(), caller_thread);
            progress(verification_snapshot(InstallVerificationStage::SteamFiles));
            continue_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|error| error.to_string())?;
            progress(verification_snapshot(
                InstallVerificationStage::RecordingBaseline,
            ));
            Ok(())
        },
    )
    .await
    .unwrap();
    assert!(released.load(Ordering::Acquire));
    assert_eq!(
        reports,
        vec![
            verification_snapshot(InstallVerificationStage::SteamFiles),
            verification_snapshot(InstallVerificationStage::RecordingBaseline),
        ]
    );
}

#[tokio::test]
async fn library_baseline_pre_cancelled_job_or_storage_never_starts_the_worker() {
    for cancel_storage in [false, true] {
        let state = DesktopState::default();
        let operation = state
            .begin_storage_context_operation("baseline test")
            .unwrap();
        let installation = app_steamcmd::InstallCancellation::new();
        if cancel_storage {
            operation
                .cancellation_token()
                .store(true, Ordering::Release);
        } else {
            installation.cancel();
        }
        let started = Arc::new(AtomicBool::new(false));
        let worker_started = Arc::clone(&started);
        let result = run_baseline_worker(
            &operation,
            &installation,
            "fixture",
            |_| {},
            move |_, _| {
                worker_started.store(true, Ordering::Release);
                Ok(())
            },
        )
        .await;
        assert!(matches!(
            result,
            Err(app_steamcmd::SteamCmdError::InstallCancelled { .. })
        ));
        assert!(!started.load(Ordering::Acquire));
    }
}

#[tokio::test]
async fn library_baseline_inflight_cancellation_joins_worker_before_releasing_resources() {
    for cancel_storage in [false, true] {
        let state = DesktopState::default();
        let operation = state
            .begin_storage_context_operation("baseline test")
            .unwrap();
        let installation = app_steamcmd::InstallCancellation::new();
        let worker_installation = installation.clone();
        let worker_operation = operation.clone();
        let released = Arc::new(AtomicBool::new(false));
        let progress_after_cancel = Arc::new(AtomicBool::new(false));
        let progress_observed = Arc::clone(&progress_after_cancel);
        let resource = WorkerResource(Arc::clone(&released));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (observed_tx, observed_rx) = tokio::sync::oneshot::channel();
        let (cleanup_tx, cleanup_rx) = mpsc::channel();
        let task = tokio::spawn(async move {
            run_baseline_worker(
                &worker_operation,
                &worker_installation,
                "fixture",
                move |_| {
                    progress_observed.store(true, Ordering::Release);
                },
                move |cancel, progress| {
                    let _resource = resource;
                    let _ = started_tx.send(());
                    while !cancel.load(Ordering::Acquire) {
                        if matches!(cleanup_rx.try_recv(), Err(mpsc::TryRecvError::Disconnected)) {
                            return Err(String::from("test controller exited before cancellation"));
                        }
                        std::thread::yield_now();
                    }
                    let _ = observed_tx.send(());
                    progress(verification_snapshot(
                        InstallVerificationStage::PackageFiles,
                    ));
                    cleanup_rx.recv().map_err(|error| error.to_string())?;
                    Err::<(), String>(String::from("baseline observed cancellation"))
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(5), started_rx)
            .await
            .unwrap()
            .unwrap();
        if cancel_storage {
            operation
                .cancellation_token()
                .store(true, Ordering::Release);
        } else {
            installation.cancel();
        }
        tokio::time::timeout(Duration::from_secs(5), observed_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !task.is_finished(),
            "the caller must wait for worker cleanup"
        );
        assert!(
            !released.load(Ordering::Acquire),
            "worker-owned resources must remain held"
        );
        cleanup_tx.send(()).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            result,
            Err(app_steamcmd::SteamCmdError::InstallCancelled { .. })
        ));
        assert!(released.load(Ordering::Acquire));
        assert!(!progress_after_cancel.load(Ordering::Acquire));
    }
}

#[tokio::test]
async fn library_baseline_worker_failure_preserves_the_original_error() {
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("baseline test")
        .unwrap();
    let installation = app_steamcmd::InstallCancellation::new();
    let result = run_baseline_worker(
        &operation,
        &installation,
        "fixture",
        |_| {},
        |_, _| Err::<(), String>(String::from("fixture disk write failed")),
    )
    .await;
    match result {
        Err(app_steamcmd::SteamCmdError::InstallationVerificationFailed {
            module_id,
            detail,
            ..
        }) => {
            assert_eq!(module_id, "fixture");
            assert_eq!(detail, "fixture disk write failed");
        }
        other => panic!("unexpected baseline result: {other:?}"),
    }
}
