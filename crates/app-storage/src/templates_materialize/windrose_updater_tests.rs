use super::windrose_tests::{
    TestRoot, seed_rendered_server, seed_server, seed_world, server_path, world_settings,
};
use super::*;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn prestart_updater_uses_fixed_executable_cwd_and_one_verified_argument() {
    let root = TestRoot::new("updater-invocation");
    let config = TestRoot::new("updater-invocation-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&world_path).unwrap()).unwrap()["WorldDescription"]
            ["WorldName"],
        "Before"
    );

    let invocation = Arc::new(Mutex::new(None));
    let observed = Arc::clone(&invocation);
    apply_pending_world_update_with(
        root.path(),
        config.path(),
        &world_settings(world_id),
        move |command| {
            *observed.lock().unwrap() = Some(command);
            async { Ok(WindroseUpdaterStatus::success()) }
        },
    )
    .await
    .unwrap();

    let invocation = invocation.lock().unwrap().take().unwrap();
    assert_eq!(
        invocation.executable,
        fs::canonicalize(root.path().join(WINDROSE_WORLD_UPDATER_FILE)).unwrap()
    );
    assert_eq!(
        invocation.working_directory,
        fs::canonicalize(root.path()).unwrap()
    );
    assert_eq!(
        invocation.argument,
        world_path.strip_prefix(root.path()).unwrap()
    );
    let server: Value =
        serde_json::from_slice(&fs::read(server_path(root.path())).unwrap()).unwrap();
    let world: Value = serde_json::from_slice(&fs::read(world_path).unwrap()).unwrap();
    assert_eq!(server["UnknownServerRoot"]["preserve"], true);
    assert_eq!(
        server["ServerDescription_Persistent"]["UnknownPersistent"],
        "keep-me"
    );
    assert_eq!(
        server["ServerDescription_Persistent"]["ServerName"],
        "After"
    );
    assert_eq!(world["WorldDescription"]["WorldName"], "海风群岛 ⚓");
    assert_eq!(
        world["WorldDescription"]["UnknownWorldDescription"]["nested"],
        "keep-me"
    );
    assert!(!config.path().join(WINDROSE_WORLD_UPDATE_PLAN_FILE).exists());
}

#[test]
fn updater_process_uses_the_windows_no_window_flag_without_detaching() {
    const DETACHED_PROCESS: u32 = 0x00000008;
    assert_eq!(windrose_updater_creation_flags(), CREATE_NO_WINDOW);
    assert_eq!(windrose_updater_creation_flags() & DETACHED_PROCESS, 0);
}

#[tokio::test]
async fn updater_nonzero_rolls_back_both_native_documents_and_keeps_pending_plan() {
    let root = TestRoot::new("updater-rollback");
    let config = TestRoot::new("updater-rollback-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "Staged");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();
    seed_rendered_server(config.path(), world_id, "Pre-start");
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();

    let result = apply_pending_world_update_with(
        root.path(),
        config.path(),
        &world_settings(world_id),
        |_command| async { Ok(WindroseUpdaterStatus::failed(Some(7))) },
    )
    .await;
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::UpdaterFailed { code: Some(7) })
    ));
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    let plan: Value = serde_json::from_slice(
        &fs::read(config.path().join(WINDROSE_WORLD_UPDATE_PLAN_FILE)).unwrap(),
    )
    .unwrap();
    assert_eq!(plan["pending"], true);
}

#[tokio::test]
async fn updater_failure_removes_a_server_description_created_for_first_start() {
    let root = TestRoot::new("updater-first-start-rollback");
    let config = TestRoot::new("updater-first-start-rollback-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "First start");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    let settings = world_settings(world_id);
    let world_before = fs::read(&world_path).unwrap();

    materialize_windrose_documents(root.path(), config.path(), &settings, false).unwrap();
    assert!(!server_path(root.path()).exists());

    let result =
        apply_pending_world_update_with(root.path(), config.path(), &settings, |_command| async {
            Ok(WindroseUpdaterStatus::failed(Some(11)))
        })
        .await;

    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::UpdaterFailed { code: Some(11) })
    ));
    assert!(!server_path(root.path()).exists());
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn rollback_conflict_keeps_the_pending_plan_and_reports_the_exact_target() {
    let root = TestRoot::new("updater-rollback-conflict");
    let config = TestRoot::new("updater-rollback-conflict-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();

    let result = apply_pending_world_update_with(
        root.path(),
        config.path(),
        &world_settings(world_id),
        |command| async move {
            fs::write(
                command.working_directory.join(command.argument),
                b"external",
            )
            .unwrap();
            Ok(WindroseUpdaterStatus::failed(Some(9)))
        },
    )
    .await;
    let canonical_world_path = fs::canonicalize(&world_path).unwrap();
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::RollbackFailed { path }) if path == canonical_world_path
    ));
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn updater_launch_failure_rolls_back_and_does_not_clear_pending_plan() {
    let root = TestRoot::new("updater-launch-failure");
    let config = TestRoot::new("updater-launch-failure-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();

    let result = apply_pending_world_update_with(
        root.path(),
        config.path(),
        &world_settings(world_id),
        |_command| async { Err(io::Error::other("injected launch failure")) },
    )
    .await;
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::UpdaterLaunch { .. })
    ));
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn missing_updater_fails_before_either_native_document_changes() {
    let root = TestRoot::new("updater-missing");
    let config = TestRoot::new("updater-missing-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();
    let invoked = Arc::new(Mutex::new(false));
    let observed = Arc::clone(&invoked);

    let result = apply_pending_world_update_with(
        root.path(),
        config.path(),
        &world_settings(world_id),
        move |_command| {
            *observed.lock().unwrap() = true;
            async { Ok(WindroseUpdaterStatus::success()) }
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::MissingUpdater { .. })
    ));
    assert!(!*invoked.lock().unwrap());
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn updater_timeout_rolls_back_and_keeps_the_pending_plan() {
    let root = TestRoot::new("updater-timeout");
    let config = TestRoot::new("updater-timeout-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();
    let server_before = fs::read(server_path(root.path())).unwrap();
    let world_before = fs::read(&world_path).unwrap();

    let result = apply_pending_world_update_with_timeout(
        root.path(),
        config.path(),
        &world_settings(world_id),
        std::time::Duration::from_millis(20),
        |_command| async { std::future::pending::<io::Result<WindroseUpdaterStatus>>().await },
    )
    .await;
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::UpdaterTimeout { .. })
    ));
    assert_eq!(fs::read(server_path(root.path())).unwrap(), server_before);
    assert_eq!(fs::read(world_path).unwrap(), world_before);
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn unconfirmed_termination_keeps_staged_files_and_pending_plan_for_recovery() {
    let root = TestRoot::new("updater-termination-unconfirmed");
    let config = TestRoot::new("updater-termination-unconfirmed-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    let world_path = seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    materialize_windrose_documents(root.path(), config.path(), &world_settings(world_id), false)
        .unwrap();

    let result = apply_pending_world_update_with_runner(
        root.path(),
        config.path(),
        &world_settings(world_id),
        |_command| async {
            WindroseUpdaterRunResult::TerminationUnconfirmed(String::from("injected"))
        },
    )
    .await;
    assert!(matches!(
        result,
        Err(WindroseWorldTargetError::UpdaterTermination { .. })
    ));
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(&world_path).unwrap()).unwrap()["WorldDescription"]
            ["WorldName"],
        "海风群岛 ⚓"
    );
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file()
    );
}

#[tokio::test]
async fn pending_plan_retries_updater_when_native_json_is_already_staged() {
    let root = TestRoot::new("updater-retry-staged");
    let config = TestRoot::new("updater-retry-staged-config");
    let world_id = "E24A22C9C8D3448951AFD002162576D5";
    seed_server(root.path(), world_id);
    seed_world(root.path(), "0.10.0", world_id, world_id);
    seed_rendered_server(config.path(), world_id, "After");
    fs::write(root.path().join(WINDROSE_WORLD_UPDATER_FILE), b"fixture").unwrap();
    let settings = world_settings(world_id);
    materialize_windrose_documents(root.path(), config.path(), &settings, false).unwrap();
    let prepared = prepare_windrose_documents(root.path(), config.path(), &settings).unwrap();
    let world = prepared.world.as_ref().unwrap();
    fs::write(&world.path, &world.replacement).unwrap();
    fs::write(&prepared.server_path, &prepared.server_replacement).unwrap();
    materialize_windrose_documents(root.path(), config.path(), &settings, false).unwrap();
    assert!(
        config
            .path()
            .join(WINDROSE_WORLD_UPDATE_PLAN_FILE)
            .is_file(),
        "the save stage must not discard an updater plan whose application was never confirmed"
    );
    let invoked = Arc::new(Mutex::new(false));
    let observed = Arc::clone(&invoked);

    apply_pending_world_update_with(root.path(), config.path(), &settings, move |_command| {
        *observed.lock().unwrap() = true;
        async { Ok(WindroseUpdaterStatus::success()) }
    })
    .await
    .unwrap();
    assert!(*invoked.lock().unwrap());
    assert!(!config.path().join(WINDROSE_WORLD_UPDATE_PLAN_FILE).exists());
}
