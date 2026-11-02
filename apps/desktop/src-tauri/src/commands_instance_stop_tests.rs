use super::*;
use crate::commands::tests::{prepare_fake_registered_program, temp_test_dir};
use crate::state::RuntimeStartReservationAttempt;
use tauri::Manager;

async fn fixture() -> (PathBuf, StorageBootstrap, String) {
    let root = temp_test_dir("cancel-start");
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let paths = app_storage::StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("app-data/logs"),
        modules_root: workspace.join("modules"),
        migrations_root: workspace.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances/.trash"),
    };
    let storage = app_storage::bootstrap_storage_with_paths(paths.clone()).unwrap();
    initialize_database(&paths).await.unwrap();
    let modules = discover_modules(&paths.modules_root).unwrap();
    let descriptor = modules
        .iter()
        .find(|module| module.summary.id == "terraria")
        .unwrap();
    sync_modules(&paths, std::slice::from_ref(descriptor))
        .await
        .unwrap();
    prepare_fake_registered_program(&paths, descriptor)
        .await
        .unwrap();
    let instance = app_storage::create_instance(
        &paths,
        descriptor,
        app_core::CreateInstanceInput {
            name: "Cancelled startup fixture".into(),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await
    .unwrap();
    (root, storage, instance.summary.id)
}

#[tokio::test]
async fn stop_cancels_pending_start_before_waiting_for_its_mutation_lock() {
    let (root, storage, id) = fixture().await;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = app.state::<DesktopState>();
    let reservation = match state.try_reserve_runtime_start(&id, "manual").unwrap() {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let mutation = state.acquire_instance_mutation(&id).await;
    let worker = reservation.clone();
    let handle = app.handle().clone();
    let target = id.clone();
    let stop = tokio::spawn(async move {
        stop_instance(handle.state::<DesktopState>(), target, None, None).await
    });
    tokio::time::timeout(Duration::from_secs(3), reservation.cancelled())
        .await
        .unwrap();
    assert!(reservation.cancelled_by_user());
    assert!(
        !stop.is_finished(),
        "a cancellation receipt is not cleanup completion"
    );
    drop(reservation);
    assert!(
        !stop.is_finished(),
        "the preparatory worker still owns its lease"
    );
    drop(worker);
    drop(mutation);
    let result = tokio::time::timeout(Duration::from_secs(3), stop)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        result.outcome,
        app_core::StopInstanceOutcome::StartupCancelled
    );
    assert!(result.run_id.is_none());
    assert!(result.auto_backup_error.is_none());
    assert_eq!(result.process_count, 0);
    assert!(matches!(result.summary.status, InstanceStatus::Stopped));
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .unwrap()
            .is_empty()
    );
    assert!(
        read_active_instance_run(&storage.paths, &id)
            .await
            .unwrap()
            .is_none()
    );
    let log = fs::read_to_string(storage.paths.app_log_path()).unwrap();
    assert!(log.contains("instance.startup.cancelled"));
    assert!(!log.contains("instance.stop.auto_backup_created"));
    assert!(!log.contains("instance.stop.completed"));
    drop(app);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn automatic_backup_failure_returns_warning_and_preserves_stopped_world() {
    let (root, storage, id) = fixture().await;
    let mut details = read_instance_details(&storage.paths, &id).await.unwrap();
    details.auto_backup_on_stop = true;
    let saves = Path::new(&details.saves_path);
    fs::create_dir_all(saves).unwrap();
    let world = saves.join("world.fixture");
    fs::write(&world, b"stopped world bytes").unwrap();
    let backups = crate::commands::commands_program_storage::instance_root(&details)
        .unwrap()
        .join("backups");
    if backups.is_dir() {
        fs::remove_dir(&backups).unwrap();
    }
    fs::write(&backups, b"blocked destination").unwrap();

    let warning = backup_after_stop(&storage, &details, InstanceShutdownSource::Manual)
        .await
        .expect("backup destination failure must reach the stop response");
    assert!(!warning.is_empty());
    assert_eq!(fs::read(&world).unwrap(), b"stopped world bytes");
    let current = read_instance_details(&storage.paths, &id).await.unwrap();
    assert!(matches!(current.summary.status, InstanceStatus::Stopped));
    assert!(current.active_run.is_none());
    let log = fs::read_to_string(storage.paths.app_log_path()).unwrap();
    assert!(log.contains("instance.stop.auto_backup_failed"));
    assert!(!log.contains("instance.stop.auto_backup_created"));
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn disabled_automatic_backup_returns_no_warning_or_backup() {
    let (root, storage, id) = fixture().await;
    let mut details = read_instance_details(&storage.paths, &id).await.unwrap();
    details.auto_backup_on_stop = false;
    assert!(
        backup_after_stop(&storage, &details, InstanceShutdownSource::Manual)
            .await
            .is_none()
    );
    assert!(
        app_storage::list_instance_backups(&storage.paths, &id)
            .await
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn stop_cancelled_start_keeps_cleanup_survivor_record_and_reports_failure() {
    let (root, storage, id) = fixture().await;
    let state = DesktopState::from_storage(&storage);
    let reservation = match state.try_reserve_runtime_start(&id, "manual").unwrap() {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let cancellation = state
        .request_runtime_start_cancellation(&id)
        .unwrap()
        .unwrap();
    let active = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &id,
            session_id: Some("cleanup-survivor"),
            process_key: "main",
            display_name: "Retained cleanup survivor",
            pid: u32::MAX,
            log_path: "fixture.log",
            is_primary: true,
        },
        None,
    )
    .await
    .unwrap();
    drop(reservation);
    let error = cancel_startup(&state, &storage, &id, cancellation)
        .await
        .unwrap_err();
    assert!(error.contains("could not confirm complete process cleanup"));
    assert_eq!(
        read_active_instance_run(&storage.paths, &id)
            .await
            .unwrap()
            .unwrap()
            .run_id,
        active.run_id
    );
    assert!(
        !fs::read_to_string(storage.paths.app_log_path())
            .unwrap()
            .contains("\"instance.startup.cancelled\"")
    );
    drop(state);
    fs::remove_dir_all(root).unwrap();
}
