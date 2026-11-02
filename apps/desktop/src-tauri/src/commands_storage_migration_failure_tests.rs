use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

struct CopyFailure {
    app: tauri::App<tauri::test::MockRuntime>,
    fixture: Fixture,
    run: Run,
    initial: StorageMigrationStatus,
    cancelled: bool,
    message: String,
}

impl CopyFailure {
    async fn new(cancelled: bool) -> Self {
        let fixture = Fixture::new().await;
        let app = fixture.app();
        let source = &fixture.storage.paths.instances_root;
        fs::write(source.join("first.dat"), b"first source payload").unwrap();
        fs::write(source.join("second.dat"), b"second source payload").unwrap();
        let source_before = tree_snapshot(source);
        let target = fixture.root.join("moved-instances");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("existing.dat"), b"original target payload").unwrap();
        let target_before = tree_snapshot(&target);

        let tracker = app.state::<DesktopState>().storage_migration.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let Claim::Started(preview_run, _) = tracker
            .begin_preview(core::initial_status(id.clone(), fixture.input()))
            .unwrap()
        else {
            panic!("isolated preview must start");
        };
        let mut next = fixture.storage.settings.clone();
        next.servers_root = target.to_string_lossy().into_owned();
        next.archives_root = target.join(".trash").to_string_lossy().into_owned();
        let prepared = core::preview(
            &fixture.storage.paths,
            next,
            id.clone(),
            Vec::new(),
            preview_run.cancellation.clone(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
        tracker
            .finish(&preview_run, prepared.status(), false)
            .unwrap();
        let Claim::Started(run, initial) = tracker.begin_execute(&id).unwrap() else {
            panic!("isolated execution must start");
        };
        let progress_tracker = tracker.clone();
        let progress_run = run.clone();
        let injected = Arc::new(AtomicBool::new(false));
        let inject_once = injected.clone();
        let changed_target = target.clone();
        let progress: core::Progress = Arc::new(move |status| {
            let copying = status.phase == Phase::Copying && status.completed_files > 0;
            progress_tracker.update(&progress_run, status).unwrap();
            if copying && !inject_once.swap(true, Ordering::AcqRel) {
                if cancelled {
                    progress_run.cancellation.store(true, Ordering::Release);
                } else {
                    // The real copy verifier must retain this foreign entry and
                    // fail before publication; no timing or mock I/O is involved.
                    fs::write(changed_target.join("unplanned.dat"), b"external payload").unwrap();
                }
            }
        });
        let error = core::execute(
            &fixture.storage.paths,
            prepared,
            run.cancellation.clone(),
            progress,
        )
        .await
        .unwrap_err();
        assert!(injected.load(Ordering::Acquire));
        assert_eq!(
            matches!(error, app_storage::StorageError::StorageMigrationCancelled),
            cancelled
        );
        if !cancelled {
            assert!(error.to_string().contains("unexpected destination entry"));
            assert_eq!(
                fs::read(target.join("unplanned.dat")).unwrap(),
                b"external payload"
            );
        }
        assert_eq!(tree_snapshot(source), source_before);
        let saved = core::read_status(&fixture.storage.paths).unwrap().unwrap();
        assert_eq!(saved.operation_id, id);
        assert_eq!(saved.preserved_destinations.len(), 1);
        assert_eq!(
            tree_snapshot(&preserved_backup(&saved, &target, &fixture.root)),
            target_before
        );
        let latest = tracker.snapshot().unwrap().unwrap();
        assert!(latest.preserved_destinations.is_empty());
        assert!(latest.completed_bytes > saved.completed_bytes);
        assert!(latest.completed_files > saved.completed_files);
        Self {
            fixture,
            app,
            run,
            initial,
            cancelled,
            message: error.to_string(),
        }
    }

    async fn resolve(&self) -> (StorageMigrationStatus, bool) {
        failure_status(
            &self.fixture.storage.paths,
            &self.app.state::<DesktopState>().storage_migration,
            &self.run,
            self.initial.clone(),
            true,
            Failure {
                message: self.message.clone(),
                cancelled: self.cancelled,
                recovery_required: false,
            },
        )
        .await
    }

    fn rewrite_saved_status(&self, update: impl FnOnce(&mut serde_json::Value)) {
        let path = self
            .fixture
            .storage
            .paths
            .app_data_root
            .join("storage-migration.json");
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        update(&mut record["prepared"]["status"]);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    }
}

async fn assert_failure_status_preserves_saved_backups(cancelled: bool) {
    let case = CopyFailure::new(cancelled).await;
    let tracker = case.app.state::<DesktopState>().storage_migration.clone();
    let latest = tracker.snapshot().unwrap().unwrap();
    let saved = core::read_status(&case.fixture.storage.paths)
        .unwrap()
        .unwrap();
    let (status, needs_recovery) = case.resolve().await;
    assert!(!needs_recovery);
    assert!(!status.recovery_required);
    assert_eq!(
        status.phase,
        if cancelled {
            Phase::Cancelled
        } else {
            Phase::Failed
        }
    );
    assert_eq!(status.operation_id, case.run.id);
    assert_eq!(status.completed_bytes, latest.completed_bytes);
    assert_eq!(status.completed_files, latest.completed_files);
    assert_eq!(status.total_bytes, latest.total_bytes);
    assert!(!status.can_cancel);
    assert_eq!(status.error.as_deref(), Some(case.message.as_str()));
    assert_eq!(
        serde_json::to_value(&status.preserved_destinations).unwrap(),
        serde_json::to_value(&saved.preserved_destinations).unwrap()
    );
    tracker.finish(&case.run, status, needs_recovery).unwrap();
    let observed = read_app_storage_migration(case.app.state::<DesktopState>())
        .await
        .unwrap()
        .unwrap();
    assert!(!observed.recovery_required);
    assert_eq!(observed.completed_bytes, latest.completed_bytes);
    assert_eq!(
        serde_json::to_value(observed.preserved_destinations).unwrap(),
        serde_json::to_value(saved.preserved_destinations).unwrap()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failure_status_cancelled_keeps_saved_backups_and_live_progress() {
    assert_failure_status_preserves_saved_backups(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failure_status_copy_error_keeps_saved_backups_and_live_progress() {
    assert_failure_status_preserves_saved_backups(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn failure_status_saved_recovery_and_backups_belong_to_the_same_operation() {
    let case = CopyFailure::new(true).await;
    // Only the saved status is varied here to exercise the supervisor boundary;
    // this fixture never attempts commit recovery from the altered journal.
    case.rewrite_saved_status(|status| {
        status["phase"] = serde_json::json!("applying");
        status.as_object_mut().unwrap().remove("recovery_required");
    });
    let (matching, needs_recovery) = case.resolve().await;
    assert!(needs_recovery);
    assert!(matching.recovery_required);
    assert_eq!(matching.phase, Phase::Failed);
    assert_eq!(matching.preserved_destinations.len(), 1);

    case.rewrite_saved_status(|status| {
        status["operation_id"] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    });
    let (unrelated, needs_recovery) = case.resolve().await;
    assert!(!needs_recovery);
    assert!(!unrelated.recovery_required);
    assert_eq!(unrelated.operation_id, case.run.id);
    assert_eq!(unrelated.phase, Phase::Cancelled);
    assert!(unrelated.preserved_destinations.is_empty());
    assert_eq!(
        unrelated.completed_bytes,
        case.app
            .state::<DesktopState>()
            .storage_migration
            .snapshot()
            .unwrap()
            .unwrap()
            .completed_bytes
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failure_status_unreadable_journal_keeps_live_progress_and_recovery_barrier() {
    let case = CopyFailure::new(true).await;
    let latest = case
        .app
        .state::<DesktopState>()
        .storage_migration
        .snapshot()
        .unwrap()
        .unwrap();
    fs::write(
        case.fixture
            .storage
            .paths
            .app_data_root
            .join("storage-migration.json"),
        b"invalid fixture journal",
    )
    .unwrap();
    let (status, needs_recovery) = case.resolve().await;
    assert!(needs_recovery);
    assert!(status.recovery_required);
    assert_eq!(status.phase, Phase::Failed);
    assert_eq!(status.completed_bytes, latest.completed_bytes);
    assert_eq!(status.completed_files, latest.completed_files);
    assert!(status.error.unwrap().contains("无法确认磁盘迁移状态"));
}

#[tokio::test(flavor = "current_thread")]
async fn read_command_exposes_recovery_from_old_journal_phases_and_explicit_flags() {
    let case = CopyFailure::new(true).await;
    for (saved_phase, saved_recovery, expected_phase, needs_recovery) in [
        ("applying", None, Phase::Failed, true),
        ("cleaning", None, Phase::Failed, true),
        ("copying", None, Phase::Failed, false),
        ("failed", None, Phase::Failed, false),
        ("cancelled", None, Phase::Cancelled, false),
        ("completed", None, Phase::Completed, false),
        ("failed", Some(true), Phase::Failed, true),
        ("completed", Some(true), Phase::Completed, true),
    ] {
        // This varies only the read boundary; it never replays altered journals.
        case.rewrite_saved_status(|status| {
            status["phase"] = serde_json::json!(saved_phase);
            status.as_object_mut().unwrap().remove("recovery_required");
            if let Some(required) = saved_recovery {
                status["recovery_required"] = serde_json::json!(required);
            }
        });
        let app = case.fixture.app();
        let observed = read_app_storage_migration(app.state::<DesktopState>())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.phase, expected_phase, "{saved_phase}");
        assert_eq!(observed.recovery_required, needs_recovery, "{saved_phase}");
        let cancelled = cancel_app_storage_migration(
            app.state::<DesktopState>(),
            observed.operation_id.clone(),
        )
        .await
        .unwrap();
        assert_eq!(cancelled.recovery_required, needs_recovery, "{saved_phase}");
        assert_eq!(
            app.state::<DesktopState>()
                .begin_storage_context_operation("check restored recovery barrier")
                .is_err(),
            needs_recovery,
            "{saved_phase}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn failure_status_preserves_explicit_recovery_for_the_same_saved_operation() {
    let case = CopyFailure::new(true).await;
    for phase in ["failed", "completed"] {
        case.rewrite_saved_status(|status| {
            status["phase"] = serde_json::json!(phase);
            status["recovery_required"] = serde_json::json!(true);
        });
        let (status, needs_recovery) = case.resolve().await;
        assert!(needs_recovery, "{phase}");
        assert!(status.recovery_required, "{phase}");
    }
}
