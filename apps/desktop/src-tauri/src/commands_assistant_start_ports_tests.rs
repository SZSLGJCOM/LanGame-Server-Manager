use super::*;

fn reserve_loopback_port_with_remap_room() -> std::io::Result<std::net::TcpListener> {
    for _ in 0..16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        if listener.local_addr()?.port() < u16::MAX {
            return Ok(listener);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrNotAvailable,
        "Could not reserve a loopback port below the remapper's upper bound",
    ))
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_start_rejects_occupied_confirmed_port_before_materializing_configuration()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-confirmed-port");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created = create_fake_module_instance(state.clone(), "minecraft", "Confirmed port").await?;
    let storage = bootstrap_storage()?;
    {
        let mut app_state = state.app_state.write().unwrap();
        app_state.settings = storage.settings.clone();
        app_state.storage = storage.storage_status.clone();
    }
    // The fixture cannot update, download, or run a game:
    // installation verification is fresh and no Java executable is present.
    app_storage::sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: String::from("minecraft"),
            install_root: Path::new(&settings.games_root)
                .join("minecraft")
                .to_string_lossy()
                .into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await?;
    assert!(
        !Path::new(&settings.games_root)
            .join("minecraft/jre/bin/java.exe")
            .exists()
    );

    let listener = reserve_loopback_port_with_remap_room()?;
    let occupied_port = listener.local_addr()?.port();
    let mut details = read_instance_details(&storage.paths, &created.summary.id).await?;
    details
        .ports
        .iter_mut()
        .find(|port| port.name == "game")
        .ok_or("game port missing")?
        .port = occupied_port;
    update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: String::from("127.0.0.1"),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: details.settings_json,
            ports: details.ports,
        },
    )
    .await?;
    let expected = read_instance_details(&storage.paths, &created.summary.id).await?;
    let instance_root = Path::new(&expected.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("instance root missing")?;
    assert!(
        instance_root
            .canonicalize()?
            .starts_with(root.canonicalize()?)
    );
    let config_paths = [
        PathBuf::from(&expected.config_file_path),
        instance_root.join("config/server.properties"),
        instance_root.join("config/eula.txt"),
        instance_root.join("server.properties"),
        instance_root.join("eula.txt"),
    ];
    let config_before = config_paths
        .iter()
        .map(fs::read)
        .collect::<Result<Vec<_>, _>>()?;

    let error = tokio::time::timeout(
        Duration::from_secs(10),
        Box::pin(start_instance_process_with_preconditions(
            None,
            &state,
            &storage,
            expected.summary.id.clone(),
            "manual",
            RuntimeStartPreconditions {
                world_start: None,
                instance: Some(expected.clone()),
                file_changes: Vec::new(),
            },
        )),
    )
    .await?
    .expect_err("confirmed startup must not silently choose another port");
    assert!(
        error.contains("confirmed ports") && error.contains("new configuration preview"),
        "{error}"
    );
    let after = read_instance_details(&storage.paths, &created.summary.id).await?;
    assert_eq!(
        serde_json::to_value(&after)?,
        serde_json::to_value(&expected)?
    );
    for (path, before) in config_paths.iter().zip(config_before) {
        assert_eq!(
            fs::read(path)?,
            before,
            "startup materialized {}",
            path.display()
        );
    }
    assert!(
        read_active_instance_run(&storage.paths, &created.summary.id)
            .await?
            .is_none()
    );
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .unwrap()
            .is_tracked(&created.summary.id)
    );
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    assert_eq!(listener.local_addr()?.port(), occupied_port);
    Ok(())
}

#[test]
fn runtime_port_remapper_retains_automatic_conflict_recovery_without_a_confirmed_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let listener = reserve_loopback_port_with_remap_room()?;
    let port = listener.local_addr()?.port();
    let original = vec![PortBinding {
        name: String::from("game"),
        protocol: String::from("tcp"),
        port,
    }];
    let remapped = app_runtime::remap_taken_port_bindings_for_module(
        "minecraft",
        "127.0.0.1",
        &original,
        &[],
    )?
    .ok_or("occupied port must still produce an automatic remapping candidate")?;
    assert_eq!(remapped.len(), 1);
    assert_eq!(remapped[0].name, "game");
    assert_eq!(remapped[0].protocol, "tcp");
    assert_ne!(remapped[0].port, port);
    assert_eq!(original[0].port, port);
    Ok(())
}
