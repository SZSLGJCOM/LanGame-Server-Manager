use super::*;
use std::collections::HashSet;

fn settings(root: &str) -> AppSettings {
    AppSettings {
        archives_root: String::new(),
        servers_root: format!(r"{root}\instances"),
        games_root: format!(r"{root}\games"),
        modules_root: format!(r"{root}\modules"),
        steamcmd_root: format!(r"{root}\steamcmd"),
    }
}

#[test]
fn path_change_detection_distinguishes_runtime_roots_from_steamcmd() {
    let current = settings(r"D:\current");
    let mut steamcmd_only = current.clone();
    steamcmd_only.steamcmd_root = String::from(r"D:\tools\steamcmd");

    assert!(app_paths_changed(&current, &steamcmd_only));
    assert!(!runtime_roots_changed(&current, &steamcmd_only));
    assert!(!app_paths_changed(&current, &current));

    let mut instances_changed = current.clone();
    instances_changed.servers_root = String::from(r"E:\LanGame\instances");
    assert!(runtime_roots_changed(&current, &instances_changed));

    let mut archives_changed = current.clone();
    archives_changed.archives_root = String::from(r"D:\archives");
    assert!(app_paths_changed(&current, &archives_changed));
    assert!(runtime_roots_changed(&current, &archives_changed));
}

#[test]
fn idle_context_allows_runtime_root_changes() {
    ensure_app_path_update_allowed(&AppPathUpdateBlockers::default(), true)
        .expect("an idle runtime context should allow a root change");
}

#[test]
fn active_runs_block_runtime_roots_but_not_a_steamcmd_only_change() {
    let blockers = AppPathUpdateBlockers {
        active_run_instance_ids: vec![String::from("running-server")],
        ..AppPathUpdateBlockers::default()
    };

    let error = ensure_app_path_update_allowed(&blockers, true)
        .expect_err("runtime roots must stay stable while a run is active");
    assert!(error.contains("active runs: running-server"));
    ensure_app_path_update_allowed(&blockers, false)
        .expect("an active run does not consume the SteamCMD path");
}

#[test]
fn pending_starts_jobs_restarts_and_shutdown_block_every_path_change() {
    for blockers in [
        AppPathUpdateBlockers {
            pending_start_instance_ids: vec![String::from("starting-server")],
            ..AppPathUpdateBlockers::default()
        },
        AppPathUpdateBlockers {
            active_job_ids: vec![String::from("module-install:test")],
            ..AppPathUpdateBlockers::default()
        },
        AppPathUpdateBlockers {
            pending_restart_count: 1,
            ..AppPathUpdateBlockers::default()
        },
        AppPathUpdateBlockers {
            shutdown_in_progress: true,
            ..AppPathUpdateBlockers::default()
        },
    ] {
        assert!(ensure_app_path_update_allowed(&blockers, false).is_err());
    }
}

#[test]
fn stale_settings_snapshot_cannot_enter_a_path_update() {
    let state = DesktopState::default();
    let captured = state.app_state.read().expect("app state").settings.clone();
    state
        .app_state
        .write()
        .expect("app state")
        .settings
        .servers_root = String::from(r"E:\new-context\instances");

    assert!(
        !storage_context_snapshot_is_current(&state, &captured).expect("compare storage context")
    );
}

#[test]
fn storage_operations_and_path_transitions_are_mutually_exclusive() {
    let state = DesktopState::default();
    let operation = state
        .begin_storage_context_operation("instance creation")
        .expect("reserve storage operation");
    assert!(state.begin_storage_context_transition().is_err());

    drop(operation);
    let transition = state
        .begin_storage_context_transition()
        .expect("begin path transition");
    assert!(
        state
            .begin_storage_context_operation("instance creation")
            .is_err()
    );

    drop(transition);
    state
        .begin_storage_context_operation("instance creation")
        .expect("operation should resume after transition");
}

#[test]
fn path_transition_rejects_new_runtime_start_reservations() {
    let state = DesktopState::default();
    let transition = state
        .begin_storage_context_transition()
        .expect("begin path transition");

    let error = state
        .try_reserve_runtime_start("server", "manual")
        .expect_err("runtime start must not cross a path transition");
    assert!(error.contains("paths are being updated"));

    drop(transition);
    assert!(matches!(
        state
            .try_reserve_runtime_start("server", "manual")
            .expect("reserve after transition"),
        RuntimeStartReservationAttempt::Reserved(_)
    ));
}

#[test]
fn background_job_ids_are_unique_for_the_same_target() {
    let ids = (0..128)
        .map(|_| new_background_job_id("module-install", "shared-target"))
        .collect::<HashSet<_>>();

    assert_eq!(ids.len(), 128);
    assert!(
        ids.iter()
            .all(|id| id.starts_with("module-install:shared-target:"))
    );
}
