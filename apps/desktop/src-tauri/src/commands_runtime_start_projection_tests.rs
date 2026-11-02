use std::fs;
use std::path::{Path, PathBuf};

use tauri::Manager;

use super::*;
use crate::commands::commands_storage::{
    list_instances_from_storage, read_instance_details_from_storage,
    read_instance_runtime_overview_from_storage,
};
use crate::commands::tests::{prepare_fake_registered_program, temp_test_dir};
use crate::state::{RuntimeStartReservationAttempt, RuntimeStartReservationLease};

fn reserve(state: &DesktopState, id: &str) -> RuntimeStartReservationLease {
    match state.try_reserve_runtime_start(id, "manual").unwrap() {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation result: {other:?}"),
    }
}

fn summary(id: &str, status: InstanceStatus) -> InstanceSummary {
    InstanceSummary {
        id: id.into(),
        name: id.into(),
        module_id: "sevendaystodie".into(),
        status,
        active_process_count: 0,
        bind_ip: "0.0.0.0".into(),
        port_count: 0,
        autostart: false,
    }
}

#[test]
fn pending_start_summary_projection_is_transient_and_instance_scoped() {
    let state = DesktopState::default();
    let stored = vec![
        summary("pending", InstanceStatus::Stopped),
        summary("other", InstanceStatus::Running),
    ];
    state.app_state.write().unwrap().instances = stored.clone();
    let reservation = reserve(&state, "pending");
    let mut snapshot = state.app_state.read().unwrap().clone();
    project_pending_runtime_start_summaries(&state, &mut snapshot.instances).unwrap();
    assert!(matches!(
        snapshot.instances[0].status,
        InstanceStatus::Starting
    ));
    assert!(matches!(
        snapshot.instances[1].status,
        InstanceStatus::Running
    ));
    assert!(matches!(
        state.app_state.read().unwrap().instances[0].status,
        InstanceStatus::Stopped
    ));

    drop(reservation);
    let mut failed_start = stored.clone();
    project_pending_runtime_start_summaries(&state, &mut failed_start).unwrap();
    assert!(matches!(failed_start[0].status, InstanceStatus::Stopped));
    let mut completed_start = stored;
    completed_start[0].status = InstanceStatus::Running;
    project_pending_runtime_start_summaries(&state, &mut completed_start).unwrap();
    assert!(matches!(completed_start[0].status, InstanceStatus::Running));
}

fn fixture_paths(root: &Path) -> app_storage::StoragePaths {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    app_storage::StoragePaths {
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
    }
}

#[tokio::test]
async fn pending_start_read_commands_keep_progress_without_persisting_starting()
-> Result<(), Box<dyn std::error::Error>> {
    let root = temp_test_dir("start-proj");
    let paths = fixture_paths(&root);
    let storage = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    app_storage::initialize_database(&paths).await?;
    let descriptors = app_modules::discover_modules(&paths.modules_root)?;
    let descriptor = descriptors
        .iter()
        .find(|module| module.summary.id == "sevendaystodie")
        .ok_or("7 Days to Die fixture module is missing")?;
    app_storage::sync_modules(&paths, std::slice::from_ref(descriptor)).await?;
    prepare_fake_registered_program(&paths, descriptor).await?;
    let instance = app_storage::create_instance(
        &paths,
        descriptor,
        app_core::CreateInstanceInput {
            name: "Pending startup".into(),
            module_id: descriptor.summary.id.clone(),
        },
    )
    .await?;
    let id = instance.summary.id;
    let logs = paths.instances_root.join(&id).join("logs");
    let old_log = logs.join("run-1-server.log");
    fs::write(&old_log, "Previous run completed\n")?;
    let previous = app_storage::mark_instance_process_started_with_identity(
        &paths,
        &app_storage::StartedInstanceProcess {
            instance_id: &id,
            session_id: Some("previous"),
            process_key: "server",
            display_name: "Server",
            pid: u32::MAX,
            log_path: old_log.to_str().ok_or("fixture log path is not UTF-8")?,
            is_primary: true,
        },
        None,
    )
    .await?;
    app_storage::mark_instance_process_stopped(&paths, &id, previous.run_id, Some(0), false)
        .await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let reservation = reserve(state.inner(), &id);

    let listed = list_instances_from_storage(app.state::<DesktopState>()).await?;
    assert!(matches!(listed[0].status, InstanceStatus::Starting));
    let details =
        read_instance_details_from_storage(app.state::<DesktopState>(), id.clone()).await?;
    assert!(matches!(details.summary.status, InstanceStatus::Starting));
    let pending =
        read_instance_runtime_overview_from_storage(app.state::<DesktopState>(), id.clone())
            .await?;
    assert_eq!(pending.health.status, "starting");
    assert_eq!(pending.health.reason.code, "startup_pending");
    assert!(pending.health.matched_line.is_none());
    assert_eq!(pending.recent_runs[0].run_id, previous.run_id);
    assert!(pending.log_tail.source_path.is_none());
    assert!(pending.log_tail.lines.is_empty());
    assert_eq!(pending.performance.status, "pending");
    assert!(matches!(
        app_storage::list_instances(&paths).await?[0].status,
        InstanceStatus::Stopped
    ));
    assert!(matches!(
        state.app_state.read().unwrap().instances[0].status,
        InstanceStatus::Stopped
    ));

    let current_log = logs.join("run-2-server.log");
    fs::write(&current_log, "Checking server update\n")?;
    assert!(
        state
            .runtime_start_reservations
            .lock()
            .unwrap()
            .set_console_log_path(
                &id,
                current_log
                    .to_str()
                    .ok_or("fixture log path is not UTF-8")?,
            )
    );
    let progress =
        read_instance_runtime_overview_from_storage(app.state::<DesktopState>(), id.clone())
            .await?;
    assert_eq!(progress.log_tail.lines, ["Checking server update"]);
    assert_eq!(progress.health.status, "starting");

    // A failed or cancelled attempt releases its lease without changing the DB.
    drop(reservation);
    let stopped = list_instances_from_storage(app.state::<DesktopState>()).await?;
    assert!(matches!(stopped[0].status, InstanceStatus::Stopped));
    let stopped =
        read_instance_runtime_overview_from_storage(app.state::<DesktopState>(), id.clone())
            .await?;
    assert_eq!(stopped.health.status, "stopped");

    // Successful startup writes the real running state before releasing its
    // reservation. Hold the same mutation lease as production; no process runs.
    let reservation = reserve(state.inner(), &id);
    let mutation = state.acquire_instance_mutation(&id).await;
    let started = app_storage::mark_instance_process_started_with_identity(
        &paths,
        &app_storage::StartedInstanceProcess {
            instance_id: &id,
            session_id: Some("current"),
            process_key: "server",
            display_name: "Server",
            pid: u32::MAX,
            log_path: current_log
                .to_str()
                .ok_or("fixture log path is not UTF-8")?,
            is_primary: true,
        },
        None,
    )
    .await?;
    let finishing = list_instances_from_storage(app.state::<DesktopState>()).await?;
    assert!(matches!(finishing[0].status, InstanceStatus::Starting));
    assert_eq!(finishing[0].active_process_count, 1);
    drop(reservation);
    let running = list_instances_from_storage(app.state::<DesktopState>()).await?;
    assert!(matches!(running[0].status, InstanceStatus::Running));
    app_storage::mark_instance_process_stopped(&paths, &id, started.run_id, Some(0), false).await?;
    drop(mutation);
    drop(app);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn pending_start_projection_reports_poisoned_reservation_lock() {
    let state = DesktopState::default();
    let reservations = std::sync::Arc::clone(&state.runtime_start_reservations);
    assert!(
        std::thread::spawn(move || {
            let _guard = reservations.lock().unwrap();
            panic!("poison the fixture reservation lock");
        })
        .join()
        .is_err()
    );
    let mut instances = [summary("pending", InstanceStatus::Stopped)];
    assert_eq!(
        project_pending_runtime_start_summaries(&state, &mut instances).unwrap_err(),
        "runtime start reservation lock poisoned"
    );
    assert!(matches!(instances[0].status, InstanceStatus::Stopped));
}
