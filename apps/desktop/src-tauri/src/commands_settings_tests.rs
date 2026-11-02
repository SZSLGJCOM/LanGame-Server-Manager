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
fn blank_paths_keep_the_bootstrapped_volume() {
    for root in [r"E:\LanGame", r"F:\LanGame"] {
        let current = settings(root);
        let input = AppPathSettingsInput {
            servers_root: String::from(" "),
            games_root: String::from("\t"),
            archives_root: String::new(),
            steamcmd_root: String::from("\n"),
        };

        let next = normalize_app_path_settings(&current, &input);

        assert_eq!(next.servers_root, current.servers_root);
        assert_eq!(next.games_root, current.games_root);
        assert_eq!(next.steamcmd_root, current.steamcmd_root);
        assert_eq!(next.modules_root, current.modules_root);
        assert_eq!(
            Path::new(&next.archives_root),
            Path::new(&current.servers_root).join(".trash")
        );
    }
}

#[test]
fn blank_archive_path_follows_the_effective_new_instance_root() {
    let current = settings(r"E:\LanGame");
    let input = AppPathSettingsInput {
        servers_root: String::from(r"  F:\LanGame\instances  "),
        games_root: String::new(),
        archives_root: String::from(" \t"),
        steamcmd_root: String::new(),
    };

    let next = normalize_app_path_settings(&current, &input);

    assert_eq!(next.servers_root, r"F:\LanGame\instances");
    assert_eq!(
        Path::new(&next.archives_root),
        Path::new(r"F:\LanGame\instances").join(".trash")
    );
    assert_eq!(next.games_root, current.games_root);
    assert_eq!(next.steamcmd_root, current.steamcmd_root);
}

#[test]
fn explicit_paths_are_preserved_without_changing_the_module_root() {
    let current = settings(r"E:\LanGame");
    let input = AppPathSettingsInput {
        servers_root: String::from(r"F:\servers"),
        games_root: String::from(r"G:\games"),
        archives_root: String::from(r"H:\archives"),
        steamcmd_root: String::from(r"I:\steamcmd"),
    };

    let next = normalize_app_path_settings(&current, &input);

    assert_eq!(next.servers_root, input.servers_root);
    assert_eq!(next.games_root, input.games_root);
    assert_eq!(next.archives_root, input.archives_root);
    assert_eq!(next.steamcmd_root, input.steamcmd_root);
    assert_eq!(next.modules_root, current.modules_root);
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

#[tokio::test]
async fn path_settings_log_rejected_roots_and_persist_joint_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let root = crate::commands::tests::temp_test_dir("path-settings");
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
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
    let storage = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    app_storage::save_app_settings_with_paths(storage.settings.clone(), &paths)?;
    initialize_database(&paths).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let original_settings = fs::read(&paths.settings_path)?;
    let retained = paths.instances_root.join("existing-world.dat");
    fs::write(&retained, b"keep the original workspace")?;
    let next_instances = root.join("selected/instances");
    let next_archives = next_instances.join(".trash");
    let input = |archives: &Path| AppPathSettingsInput {
        servers_root: next_instances.to_string_lossy().into_owned(),
        archives_root: archives.to_string_lossy().into_owned(),
        games_root: storage.settings.games_root.clone(),
        steamcmd_root: storage.settings.steamcmd_root.clone(),
    };

    // The first choice is invalid as a pair. It must fail before creating or
    // persisting either root, and leave enough evidence to diagnose the failure.
    let error = update_app_settings(app.state::<DesktopState>(), input(&next_instances))
        .await
        .unwrap_err();
    assert!(error.contains("must be separate"), "{error}");
    assert_eq!(fs::read(&paths.settings_path)?, original_settings);
    assert!(!next_instances.exists());
    assert_eq!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .settings
            .servers_root,
        storage.settings.servers_root
    );
    let log = fs::read_to_string(paths.app_log_path())?;
    let entries = log
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(entries.iter().any(|entry| {
        entry["action"] == "settings.paths.update.failed"
            && entry["context"]["phase"] == "archive_roots"
            && entry["message"]
                .as_str()
                .is_some_and(|message| message.contains("must be separate"))
    }));

    // A single valid request can change both roots, even when the selected
    // workspace already contains files. Changing settings does not move them.
    let selected_world = next_instances.join("existing-instance/saves/world.dat");
    fs::create_dir_all(selected_world.parent().unwrap())?;
    fs::write(&selected_world, b"keep the selected workspace")?;
    let saved = update_app_settings(app.state::<DesktopState>(), input(&next_archives)).await?;
    assert_eq!(Path::new(&saved.servers_root), next_instances);
    assert_eq!(Path::new(&saved.archives_root), next_archives);
    let restarted = app_storage::bootstrap_storage_with_paths(paths.clone())?;
    assert_eq!(restarted.paths.instances_root, next_instances);
    assert_eq!(restarted.paths.archives_root, next_archives);
    assert_eq!(restarted.paths.database_path, paths.database_path);
    assert_eq!(fs::read(&retained)?, b"keep the original workspace");
    assert_eq!(fs::read(&selected_world)?, b"keep the selected workspace");
    drop(app);
    fs::remove_dir_all(root)?;
    Ok(())
}
