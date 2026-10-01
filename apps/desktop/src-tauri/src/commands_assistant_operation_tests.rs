use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_start_server_without_app_handle() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-requires-handle");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Start Requires Handle")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Start the selected Minecraft server now."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("start action should require app handle");

    assert_eq!(
        error,
        String::from("AI start_server execution requires an application handle.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_unbound_start_without_module_selection() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-no-target");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let _settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Start the selected server now."),
            context: None,
            selected_instance_id: None,
            selected_module_id: None,
        },
    )
    .await
    .expect_err("start action must require an existing instance");

    assert_eq!(
        error,
        "Create and configure an instance before requesting start_server."
    );
    let storage = bootstrap_storage().expect("bootstrap isolated storage");
    assert!(
        list_instances(&storage.paths)
            .await
            .expect("list isolated instances")
            .is_empty(),
        "an unbound start must not create an instance"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_unbound_start_with_selected_minecraft_module() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-no-handle-with-module");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Start a Minecraft server."),
            context: None,
            selected_instance_id: None,
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("selecting a module must not implicitly create an instance");

    assert_eq!(
        error,
        "Create and configure an instance before requesting start_server."
    );
    let storage = bootstrap_storage().expect("bootstrap isolated storage");
    assert!(
        list_instances(&storage.paths)
            .await
            .expect("list isolated instances")
            .is_empty(),
        "an unbound start must not create an instance"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_start_server_without_app_handle_for_selected_fake_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-with-verified-install");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_enshrouded_install(&settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "enshrouded",
        "AI Start Enshrouded",
    )
    .await
    .expect("seed test instance");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor =
        find_descriptor(&descriptors, "enshrouded").map_err(|error| error.to_string())?;
    let record = build_game_install_sync_record(&storage.settings, descriptor, true, None);
    sync_game_installs(&storage.paths, &[record])
        .await
        .map_err(|error| error.to_string())?;

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Start the selected Enshrouded server now."),
            context: Some(String::from(
                "Smoke requirement: choose action start_server and start the selected instance.",
            )),
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("enshrouded")),
        },
    )
    .await
    .expect_err("start action should require app handle");

    assert_eq!(
        error,
        String::from("AI start_server execution requires an application handle.")
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_unbound_start_with_selected_enshrouded_module()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-create-instance-on-module");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_enshrouded_install(&settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor =
        find_descriptor(&descriptors, "enshrouded").map_err(|error| error.to_string())?;
    let record = build_game_install_sync_record(&storage.settings, descriptor, true, None);
    sync_game_installs(&storage.paths, &[record])
        .await
        .map_err(|error| error.to_string())?;

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Start an enshrouded server now."),
            context: Some(String::from(
                "Smoke requirement: choose start action; no instance exists, create and start one.",
            )),
            selected_instance_id: None,
            selected_module_id: Some(String::from("enshrouded")),
        },
    )
    .await
    .expect_err("start action must not implicitly create the requested instance");

    assert_eq!(
        error,
        "Create and configure an instance before requesting start_server."
    );
    assert!(
        list_instances(&storage.paths).await?.is_empty(),
        "an unbound start must not create an instance"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_mocked_startserver_without_app_handle_for_selected_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-selected-with-app-handle");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_enshrouded_install(&settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor =
        find_descriptor(&descriptors, "enshrouded").map_err(|error| error.to_string())?;
    let record = build_game_install_sync_record(&storage.settings, descriptor, true, None);
    sync_game_installs(&storage.paths, &[record])
        .await
        .map_err(|error| error.to_string())?;

    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "enshrouded",
        "AI Start Selected With Mock Action",
    )
    .await
    .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "mock-action:startserver\nStart the selected Enshrouded server now.",
            ),
            context: Some(String::from(
                "Smoke requirement: keep transport stable and require app handle.",
            )),
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("enshrouded")),
        },
    )
    .await
    .expect_err("start action should require app handle");

    assert_eq!(
        error,
        String::from("AI start_server execution requires an application handle.")
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_mocked_unbound_start_with_selected_module()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-start-created-with-app-handle");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_enshrouded_install(&settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor =
        find_descriptor(&descriptors, "enshrouded").map_err(|error| error.to_string())?;
    let record = build_game_install_sync_record(&storage.settings, descriptor, true, None);
    sync_game_installs(&storage.paths, &[record])
        .await
        .map_err(|error| error.to_string())?;

    let error = assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: String::from(
                    "mock-action:startserver\nStart a dedicated Enshrouded server now.",
                ),
                context: Some(String::from(
                    "Smoke requirement: infer module enshrouded, auto-create an instance, and start it.",
                )),
                selected_instance_id: None,
                selected_module_id: Some(String::from("enshrouded")),
            },
        )
        .await
        .expect_err("a mocked start action must not implicitly create an instance");

    assert_eq!(
        error,
        "Create and configure an instance before requesting start_server."
    );
    assert!(
        list_instances(&storage.paths).await?.is_empty(),
        "an unbound start must not create an instance"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_preserves_explicit_mock_none_for_restart_text() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-restart-mock-action-noop");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Mock Restart Action Noop")
            .await
            .expect("seed test instance");
    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("mock-action:none\nRestart the selected Minecraft server now."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect("mock action should be handled as no-op");

    assert!(!output.handled);
    assert_eq!(output.action, AssistantOperationAction::None);
    assert_eq!(output.message, String::from("mocked assistant response"));
    assert!(output.instance_id.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_does_not_infer_restart_from_text_without_tool_plan() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-restart-request-noop");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Restart Request Noop")
            .await
            .expect("seed test instance");

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("mock-action:none\nRestart the selected Minecraft server now."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect("an explicit mock none plan must not be promoted into a restart");

    assert!(!output.handled);
    assert_eq!(output.action, AssistantOperationAction::None);
    assert_eq!(output.message, String::from("mocked assistant response"));
    assert!(output.instance_id.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_does_not_infer_stop_from_text_without_tool_plan() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-shutdown-request-noop");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Shutdown Request Noop")
            .await
            .expect("seed test instance");

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Shut down the selected Minecraft server now."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect("text without a supported tool plan must not stop an instance");

    assert!(!output.handled);
    assert_eq!(output.action, AssistantOperationAction::None);
    assert_eq!(output.message, String::from("mocked assistant response"));
    assert!(output.instance_id.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_executes_minecraft_gm_command_via_source_rcon()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-run-gm-command-source-rcon");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake Minecraft install");

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Run Minecraft GM Command")
            .await
            .expect("seed test instance");
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;

    let (rcon_port, command_rx, rcon_server) = spawn_source_rcon_capture_server("ok");
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut settings_json: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| std::io::Error::other(format!("parse instance settings: {error}")))?;
    let settings_object = settings_json
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("minecraft settings should be object"))?;
    settings_object.insert(String::from("enable_rcon"), Value::Bool(true));
    settings_object.insert(
        String::from("rcon_password"),
        Value::String(String::from("gm-test-rcon-9zK4pT7vQ2mX")),
    );

    let mut ports = details.ports.clone();
    for port in &mut ports {
        if port.name == "rcon" {
            port.port = rcon_port;
        }
    }

    command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: details.summary.bind_ip.clone(),
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: serde_json::to_string_pretty(&settings_json)?,
                ports,
            },
        )
        .await,
    )?;

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("mock-action:runGmCommand\nmock-runtime-commands:[\"list\"]"),
            context: Some(String::from(
                "AI maintenance tool: execute one GM command now.",
            )),
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await?;

    let (password, command, terminator_id) = command_rx.recv_timeout(Duration::from_secs(10))?;
    rcon_server.join().unwrap();

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));
    assert_eq!(password, "gm-test-rcon-9zK4pT7vQ2mX");
    assert_eq!(command, "list");
    assert_eq!(terminator_id, 14003);
    assert_eq!(output.runtime_commands, vec![String::from("list")]);
    assert_eq!(
        output.runtime_response_texts.first().map(String::as_str),
        Some("ok")
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_executes_dontstarve_gm_command_via_stdin()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-dst-gm-command-stdin");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_dontstarve_install(&settings).expect("prepare fake Don't Starve Together install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "dontstarve",
        "AI Mock DontStarve GM Command",
    )
    .await
    .expect("seed test instance");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let capture_path = register_smoke_stdin_capture_process(
        app.state::<DesktopState>(),
        &storage,
        &details,
        &run_root,
        "master",
    )
    .await?;

    let expected_command = String::from(
        "for _,v in ipairs(AllPlayers) do for i=1,20 do v.components.inventory:GiveItem(SpawnPrefab(\"log\")) end end",
    );
    let output_result = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: format!(
                "mock-action:runGmCommand\nmock-runtime-commands:{}",
                serde_json::to_string(&vec![expected_command.clone()])?
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("dontstarve")),
        },
    )
    .await;

    let mut captured_command = None;
    if output_result.is_ok() {
        for _ in 0..50 {
            if capture_path.exists() {
                captured_command = Some(fs::read_to_string(&capture_path)?.trim().to_string());
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if let Some(mut running) = app
        .state::<DesktopState>()
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&provisioning.summary.id)
    {
        let _ = stop_managed_instance(&mut running);
    }

    let output = output_result?;
    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("dontstarve"));
    assert_eq!(output.runtime_commands, vec![expected_command.clone()]);
    assert_eq!(captured_command.as_deref(), Some(expected_command.as_str()));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_executes_terraria_gm_command_via_stdin()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-terraria-gm-command-stdin");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_terraria_install(&settings).expect("prepare fake Terraria install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "terraria",
        "AI Mock Terraria GM Command",
    )
    .await
    .expect("seed test instance");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let capture_path = register_smoke_stdin_capture_process(
        app.state::<DesktopState>(),
        &storage,
        &details,
        &run_root,
        "main",
    )
    .await?;

    let expected_command = String::from("playing");
    let output_result = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: format!(
                "mock-action:runGmCommand\nmock-runtime-commands:{}",
                serde_json::to_string(&vec![expected_command.clone()])?
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("terraria")),
        },
    )
    .await;

    let mut captured_command = None;
    if output_result.is_ok() {
        for _ in 0..50 {
            if capture_path.exists() {
                captured_command = Some(fs::read_to_string(&capture_path)?.trim().to_string());
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    if let Some(mut running) = app
        .state::<DesktopState>()
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop(&provisioning.summary.id)
    {
        let _ = stop_managed_instance(&mut running);
    }

    let output = output_result?;
    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("terraria"));
    assert_eq!(output.runtime_commands, vec![expected_command.clone()]);
    assert_eq!(captured_command.as_deref(), Some(expected_command.as_str()));
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_executes_sevendaystodie_gm_command_via_telnet()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-7d2-gm-command-telnet");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_sevendaystodie_install(&settings).expect("prepare fake 7 Days to Die install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "sevendaystodie",
        "AI Mock 7 Days to Die GM Command",
    )
    .await
    .expect("seed test instance");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut settings_json: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| std::io::Error::other(format!("parse instance settings: {error}")))?;
    let settings_object = settings_json
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("7 Days to Die settings should be an object"))?;
    settings_object.insert(
        String::from("telnet_password"),
        Value::String(String::from("7d2-gm-password")),
    );
    settings_object.insert(String::from("telnet_enabled"), Value::Bool(true));

    let (telnet_port, command_rx, telnet_server) =
        spawn_telnet_capture_server("7d2 status: players list generated");
    let mut ports = details.ports.clone();
    for port in &mut ports {
        if port.name.eq_ignore_ascii_case("telnet") {
            port.port = telnet_port;
        }
    }

    command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: String::from("127.0.0.1"),
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: serde_json::to_string_pretty(&settings_json)
                    .map_err(std::io::Error::other)?,
                ports,
            },
        )
        .await,
    )?;

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "mock-action:runGmCommand\nmock-runtime-commands:[\"listplayerids\"]",
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("sevendaystodie")),
        },
    )
    .await?;
    let (password, command) = command_rx.recv_timeout(Duration::from_secs(10))?;
    telnet_server.join().unwrap();

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("sevendaystodie"));
    assert_eq!(password, "7d2-gm-password");
    assert_eq!(command, "listplayerids");
    assert_eq!(output.runtime_commands, vec![String::from("listplayerids")]);
    assert!(
        output
            .runtime_response_texts
            .iter()
            .any(|response| response.contains("7d2 status: players list generated"))
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_executes_rust_gm_command_via_websocket_rcon()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-rust-gm-command-websocket");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_rust_install(&settings).expect("prepare fake Rust install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "rust",
        "AI Mock Rust GM Command",
    )
    .await
    .expect("seed test instance");

    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut settings_json: Value = serde_json::from_str(&details.settings_json)
        .map_err(|error| std::io::Error::other(format!("parse instance settings: {error}")))?;
    let settings_object = settings_json
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("Rust settings should be an object"))?;
    settings_object.insert(
        String::from("rcon_password"),
        Value::String(String::from("rust-gm-password")),
    );
    settings_object.insert(String::from("rcon_web"), Value::Bool(true));

    let (rcon_port, command_rx, rcon_server) =
        spawn_websocket_rcon_capture_server("rust gm status accepted");
    let mut ports = details.ports.clone();
    for port in &mut ports {
        if port.name.eq_ignore_ascii_case("rcon") {
            port.port = rcon_port;
        }
    }
    command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: String::from("127.0.0.1"),
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: serde_json::to_string_pretty(&settings_json)
                    .map_err(std::io::Error::other)?,
                ports,
            },
        )
        .await,
    )?;

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("mock-action:runGmCommand\nmock-runtime-commands:[\"status\"]"),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("rust")),
        },
    )
    .await?;
    let (password, command) = command_rx.recv_timeout(Duration::from_secs(10))?;
    rcon_server.join().unwrap();

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("rust"));
    assert_eq!(password, "rust-gm-password");
    assert_eq!(command, "status");
    assert_eq!(output.runtime_commands, vec![String::from("status")]);
    assert!(
        output
            .runtime_response_texts
            .iter()
            .any(|response| response.contains("rust gm status accepted"))
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_terraria_gm_transport_mismatch() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-terraria-gm-transport");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_terraria_install(&settings).expect("prepare fake terraria install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "terraria",
        "AI Terraria GM Transport Check",
    )
    .await
    .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Run this AI GM command: save. Use websocket transport."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("terraria")),
        },
    )
    .await
    .expect_err("terraria GM action should enforce stdin transport");

    assert_eq!(
        error,
        String::from("AI Terraria GM command execution requires stdin transport.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_gm_command_not_in_allowlist() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-gm-unsafe-command");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI GM Unsafe Command")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Run this GM command: shutdown now."),
            context: Some(String::from(
                "Smoke requirement: choose action run_gm_command and reject unsafe command.",
            )),
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("GM action should reject command outside allowlist");

    assert_eq!(
        error,
        String::from("AI GM command `shutdown now` is not in the Minecraft GM command allowlist.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_gm_command_with_unsupported_syntax() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-gm-unsupported-syntax");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI GM Unsupported Syntax")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Run this GM command: status && reboot."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("GM command execution should reject unsupported syntax");

    assert_eq!(
        error,
        String::from("GM runtime command contains unsupported command syntax `&&`.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_multiple_gm_commands() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-multiple-gm-commands");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Multiple GM Commands")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "Run this GM command: status. mock-runtime-commands:[\"list\",\"status\"]",
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("GM execution should require exactly one runtime command");

    assert_eq!(
        error,
        String::from("AI GM command execution requires exactly one runtime command.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_config_action_without_identifiable_instance() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-config-no-target");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let _left = create_fake_module_instance(
        app.state::<DesktopState>(),
        "minecraft",
        "AI Missing Target Alpha",
    )
    .await
    .expect("seed first target");
    let _right = create_fake_module_instance(
        app.state::<DesktopState>(),
        "minecraft",
        "AI Missing Target Beta",
    )
    .await
    .expect("seed second target");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Update server settings using a safe preset."),
            context: None,
            selected_instance_id: None,
            selected_module_id: None,
        },
    )
    .await
    .expect_err("config action should require identifiable instance");

    assert!(
        error.contains("AI could not identify which server settings to update."),
        "{error}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_install_server_without_identifiable_module() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-server-no-module");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let _app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Install dedicated server for this machine."),
            context: None,
            selected_instance_id: None,
            selected_module_id: None,
        },
    )
    .await
    .expect_err("installServer should require a target module");

    assert_eq!(
        error,
        String::from("AI could not identify which game server to install.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_install_fun_mod_without_explicit_workshop_ids() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-fun-mod-no-default-ids");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_terraria_install(&app_settings).expect("prepare fake terraria install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "terraria",
        "AI Install Fun Mod No Defaults",
    )
    .await
    .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Install a fun mod for the selected Terraria server."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("terraria")),
        },
    )
    .await
    .expect_err("Workshop installation should require explicit item ids");

    assert_eq!(
        error,
        String::from(
            "Workshop installation requires 1 to 20 explicit numeric item IDs in the reviewed plan. No default Mod is selected."
        )
    );
    assert!(
        read_background_jobs(app.state::<DesktopState>())
            .expect("read background jobs")
            .iter()
            .all(|job| !matches!(job.kind, JobKind::DownloadWorkshop)),
        "a plan without explicit item ids must not start a Workshop download"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_install_fun_mod_without_identifiable_instance() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-fun-mod-no-instance");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_dontstarve_install(&settings).expect("prepare fake dontstarve install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "Install a fun mod for the selected Don't Starve Together server.\nmock-workshop-item-ids:[\"123456789\"]",
            ),
            context: Some(String::from(
                "Smoke requirement: choose action install_fun_mod and no instance is targetable.",
            )),
            selected_instance_id: None,
            selected_module_id: Some(String::from("dontstarve")),
        },
    )
    .await
    .expect_err("fun mod action should require an identifiable server");

    assert_eq!(
        error,
        String::from("AI could not identify which server should receive mods.")
    );
    assert!(
        read_background_jobs(app.state::<DesktopState>())
            .expect("read background jobs")
            .iter()
            .all(|job| !matches!(job.kind, JobKind::DownloadWorkshop)),
        "a plan without a target instance must not start a Workshop download"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_install_site_mod_without_workflow() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-site-mod-no-workflow");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_dontstarve_install(&app_settings).expect("prepare fake dontstarve install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "dontstarve",
        "AI Install Site Mod Missing Workflow",
    )
    .await
    .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "Install https://www.nexusmods.com/example/mods/123 for the selected Don't Starve Together server.",
            ),
            context: Some(String::from(
                "Smoke requirement: choose action install_site_mod with modReferences [\"https://www.nexusmods.com/example/mods/123\"].",
            )),
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("dontstarve")),
        },
    )
    .await
    .expect_err("installSiteMod should require module workflow");

    assert_eq!(
        error,
        String::from("module `dontstarve` does not declare a mod-site installation workflow")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_broadcast_without_identifiable_instance() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-broadcast-no-target");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let _app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Send a broadcast that maintenance starts in 10 minutes."),
            context: None,
            selected_instance_id: None,
            selected_module_id: None,
        },
    )
    .await
    .expect_err("broadcast should require an identifiable instance");

    assert_eq!(
        error,
        String::from("AI could not identify which server should receive the broadcast.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_broadcast_when_instance_not_running() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-broadcast-not-running");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Broadcast Not Running")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: String::from(
                    "Send a broadcast to the selected Minecraft server: maintenance starts in 10 minutes.",
                ),
                context: Some(String::from(
                    "Smoke requirement: choose action broadcast and keep the message short and one line.",
                )),
                selected_instance_id: Some(provisioning.summary.id.clone()),
                selected_module_id: Some(String::from("minecraft")),
            },
        )
        .await
        .expect_err("broadcast should reject when server is not running");

    assert_eq!(
        error,
        String::from("Start the instance before sending a broadcast.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_gm_transport_mismatch_for_non_default_transport_game()
 {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-gm-transport-mismatch-generic");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&app_settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI GM Transport Mismatch")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Run this AI GM command: list. Use websocket transport."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("GM command transport should match module default");

    assert_eq!(
        error,
        String::from(
            "AI GM command execution currently supports only the approved transport for this game."
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_install_site_mod_without_references() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-site-mod-no-reference");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let app_settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&app_settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Install Site Mod No Ref")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: String::from("Install this server mod for the selected Minecraft server."),
                context: Some(String::from(
                    "Smoke requirement: choose action install_site_mod with no references and no local files.",
                )),
                selected_instance_id: Some(provisioning.summary.id.clone()),
                selected_module_id: Some(String::from("minecraft")),
            },
        )
        .await
        .expect_err("installSiteMod should require mod reference input");

    assert_eq!(
        error,
        String::from("AI did not provide a mod link, mod id, local archive, or local folder path.")
    );
}

#[test]
fn assistant_operation_settings_patch_rejects_unknown_only_keys() {
    let current = json!({
        "max_players": 8,
        "difficulty": "normal"
    });
    let patch = json!({
        "no_such_setting": true,
        "another_unknown": "value"
    });

    let merged = merge_assistant_settings_patch(&current, &patch).expect("merge settings patch");

    assert!(merged.applied_keys.is_empty());
    assert_eq!(
        merged.rejected_keys,
        vec![
            String::from("another_unknown"),
            String::from("no_such_setting")
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_installs_site_mod_with_local_source_path()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-install-site-mod-local-copy");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_minecraft_install(&settings)?;
    let local_mod_path = run_root.join("modpacks").join("site-mod-mock.jar");
    fs::create_dir_all(
        local_mod_path
            .parent()
            .ok_or_else(|| std::io::Error::other("local mod path should have parent"))?,
    )?;
    fs::write(&local_mod_path, "fake jar content")?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_minecraft_instance(
        app.state::<DesktopState>(),
        "AI Install Site Mod Local Copy",
    )
    .await
    .expect("seed test instance");

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: format!(
                "Install this server mod for the selected Minecraft server from local path: {}",
                local_mod_path.to_string_lossy()
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await?;

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::InstallSiteMod);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));
    assert_eq!(output.source_paths.len(), 1);
    assert_eq!(
        output.source_paths[0]
            .replace('\\', "/")
            .to_ascii_lowercase(),
        local_mod_path
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
    );

    let inventory =
        command_result(read_manual_mod_inventory_inner(provisioning.summary.id.clone()).await)?;
    assert_eq!(inventory.module_id, "minecraft");
    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.name == "site-mod-mock.jar"),
        "local mod should be staged from source path"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn stored_openai_compatible_ai_generates_and_sends_broadcast_mock()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let ai_settings = stored_openai_compatible_ai_mock_settings();
    let run_root = temp_test_dir("stored-ai-broadcast-mock-smoke");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_minecraft_install(&settings)?;

    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Broadcast Mock Smoke")
            .await?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut settings_json: Value = serde_json::from_str(&details.settings_json)?;
    let settings_object = settings_json
        .as_object_mut()
        .ok_or_else(|| std::io::Error::other("minecraft settings should be an object"))?;
    settings_object.insert(String::from("enable_rcon"), Value::Bool(true));
    settings_object.insert(
        String::from("rcon_password"),
        Value::String(String::from("broadcast-smoke-rcon-9zK4pT7vQ2mX")),
    );

    let (rcon_port, command_rx, rcon_server) =
        spawn_source_rcon_capture_server("broadcast accepted");
    let mut ports = details.ports.clone();
    for port in &mut ports {
        if port.name == "rcon" {
            port.port = rcon_port;
        }
    }
    command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            UpdateInstanceInput {
                id: details.summary.id.clone(),
                bind_ip: String::from("127.0.0.1"),
                auto_backup_on_stop: details.auto_backup_on_stop,
                backup_retention_count: details.backup_retention_count,
                settings_json: serde_json::to_string_pretty(&settings_json)?,
                ports,
            },
        )
        .await,
    )?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    register_smoke_instance_running(
        app.state::<DesktopState>(),
        &storage,
        &details,
        &run_root,
        "broadcast-mock",
    )
    .await?;

    let preview = command_result(
        assistant_preview_operation_inner(
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: ai_settings.clone(),
                prompt: String::from(
                    "Broadcast to the selected Minecraft server: restart in 10 minutes, players should return to a safe place. Only send a broadcast.",
                ),
                context: Some(String::from(
                    "Smoke requirement: choose action broadcast and use a short single-line broadcastIntent. Do not include markdown or command text.",
                )),
                selected_instance_id: Some(provisioning.summary.id.clone()),
                selected_module_id: Some(String::from("minecraft")),
            },
        )
        .await,
    )?;
    assert!(preview.requires_confirmation);
    assert!(matches!(
        command_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let confirmation_token = preview
        .confirmation_token
        .expect("broadcast preview confirmation token");
    let plan_summary = preview.plan_summary.expect("broadcast preview summary");
    let exact_message_json = plan_summary
        .lines()
        .find_map(|line| line.strip_prefix("Exact broadcast text: "))
        .expect("preview should expose exact broadcast text");
    let exact_message = serde_json::from_str::<String>(exact_message_json)?;
    let output = command_result(
        assistant_confirm_operation_inner(
            app.state::<DesktopState>(),
            AssistantConfirmOperationInput {
                continue_task: false,
                conversation_id: preview.conversation_id.clone(),
                settings: ai_settings,
                confirmation_token,
                plan_summary,
            },
        )
        .await,
    )?;
    let (password, command, terminator_id) = command_rx.recv_timeout(Duration::from_secs(10))?;
    rcon_server.join().unwrap();

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::Broadcast);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));
    assert_eq!(password, "broadcast-smoke-rcon-9zK4pT7vQ2mX");
    assert_eq!(terminator_id, 14003);
    assert_eq!(command, format!("say {exact_message}"));
    assert!(
        command.starts_with("say "),
        "broadcast should be sent through Minecraft say command: {command}"
    );
    assert!(
        command.contains("10"),
        "broadcast should preserve the restart timing intent: {command}"
    );
    assert!(
        !command.contains('\n') && !command.contains("Conclusion"),
        "broadcast should be one clean line: {command}"
    );
    Ok(())
}

#[test]
fn normalize_runtime_command_input_rejects_multiline_commands() {
    let error = normalize_runtime_command_input("say welcome\ngive welcome");

    assert_eq!(
        error.unwrap_err(),
        "Send one runtime command at a time. Multiline command batches are not supported."
    );
}

#[test]
fn normalize_runtime_command_input_rejects_empty_command() {
    let error = normalize_runtime_command_input("\n  \r\n");

    assert_eq!(error.unwrap_err(), "Enter one runtime command line first.");
}

#[test]
fn normalize_runtime_command_input_trims_single_line() {
    let command = normalize_runtime_command_input("  say welcome  \n");

    assert_eq!(command.expect("normalize runtime command"), "say welcome");
}

#[test]
fn assistant_operation_plan_parser_accepts_none_action() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "none",
                "instanceId": null,
                "moduleId": null,
                "reason": "only provide guidance for now"
            }"#,
    )
    .expect("parse none action");

    assert_eq!(plan.action, AssistantOperationAction::None);
    assert!(plan.instance_id.is_none());
    assert!(plan.module_id.is_none());
    assert_eq!(
        plan.reason.as_deref(),
        Some("only provide guidance for now")
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_null_workshop_item_ids_as_empty() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "start_server",
                "instanceId": "valheim-main",
                "moduleId": "valheim",
                "workshopItemIds": null
            }"#,
    )
    .expect("parse null workshop item ids");

    assert_eq!(plan.workshop_item_ids, Vec::<String>::new());
}

#[test]
fn assistant_operation_plan_parser_ignores_text_around_json() {
    let plan = parse_assistant_operation_plan_response(
        concat!(
            "I can run this for you:\n```json\n",
            r#"{"action":"repair_ports","instanceId":"valheim-1","moduleId":"valheim","portPatch":{"game":28016}}"#,
            "\n``` \nDone."
        ),
    )
    .expect("parse fenced json with prefix and suffix text");

    assert_eq!(plan.action, AssistantOperationAction::RepairPorts);
    assert_eq!(plan.instance_id.as_deref(), Some("valheim-1"));
    assert_eq!(plan.module_id.as_deref(), Some("valheim"));
    assert_eq!(
        plan.port_patch
            .as_ref()
            .and_then(|v| v.get("game"))
            .and_then(Value::as_u64),
        Some(28016)
    );
}

#[test]
fn assistant_operation_settings_patch_keeps_only_existing_keys() {
    let current = json!({
        "max_players": 8,
        "motd": "Welcome",
        "world_size": 2
    });
    let patch = json!({
        "max_players": 4,
        "motd": "新手友好服务器",
        "unknown_setting": true
    });

    let merged = merge_assistant_settings_patch(&current, &patch).expect("merge settings patch");

    assert_eq!(
        merged.settings,
        json!({
            "max_players": 4,
            "motd": "新手友好服务器",
            "world_size": 2
        })
    );
    assert_eq!(merged.applied_keys, vec!["max_players", "motd"]);
    assert_eq!(merged.rejected_keys, vec!["unknown_setting"]);
}

#[test]
fn assistant_operation_port_patch_updates_existing_ports_only() {
    let current = vec![
        PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 16261,
        },
        PortBinding {
            name: String::from("direct"),
            protocol: String::from("udp"),
            port: 16262,
        },
        PortBinding {
            name: String::from("rcon"),
            protocol: String::from("tcp"),
            port: 27015,
        },
    ];
    let patch = json!({
        "game": 16271,
        "direct": { "port": 16272 },
        "rcon": 0,
        "unknown": 27016
    });

    let merged = merge_assistant_port_patch(&current, &patch).expect("merge port patch");

    assert_eq!(
        merged
            .ports
            .iter()
            .find(|port| port.name == "game")
            .map(|port| (port.protocol.as_str(), port.port)),
        Some(("udp", 16271))
    );
    assert_eq!(
        merged
            .ports
            .iter()
            .find(|port| port.name == "direct")
            .map(|port| (port.protocol.as_str(), port.port)),
        Some(("udp", 16272))
    );
    assert_eq!(
        merged
            .ports
            .iter()
            .find(|port| port.name == "rcon")
            .map(|port| (port.protocol.as_str(), port.port)),
        Some(("tcp", 27015))
    );
    assert_eq!(merged.applied_names, vec!["direct", "game"]);
    assert_eq!(merged.rejected_names, vec!["rcon", "unknown"]);
}

#[test]
fn assistant_operation_port_patch_rejects_unknown_or_invalid_entries() {
    let current = vec![
        PortBinding {
            name: String::from("game"),
            protocol: String::from("udp"),
            port: 16261,
        },
        PortBinding {
            name: String::from("direct"),
            protocol: String::from("udp"),
            port: 16262,
        },
    ];
    let patch = json!({
        "game": "not-a-port",
        "": 27015,
        "mystery": 30000,
        "direct": 0
    });

    let merged = merge_assistant_port_patch(&current, &patch).expect("merge port patch");

    assert_eq!(
        merged
            .ports
            .iter()
            .find(|port| port.name == "game")
            .map(|port| port.port),
        Some(16261)
    );
    assert_eq!(
        merged
            .ports
            .iter()
            .find(|port| port.name == "direct")
            .map(|port| port.port),
        Some(16262)
    );
    assert!(merged.applied_names.is_empty());
    assert_eq!(
        merged.rejected_names,
        vec![
            String::from("<empty>"),
            String::from("direct"),
            String::from("game"),
            String::from("mystery")
        ]
    );
}

#[test]
fn assistant_gm_command_policy_allows_known_ark_commands_only() {
    assert!(
        validate_assistant_gm_runtime_command(
            "arksurvivalascended",
            "GMSummon \"Rex_Character_BP_C\" 150"
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "arksurvivalascended",
            "GiveItemNumToPlayer 123456789 9 50 1 0"
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command("arksurvivalascended", "DestroyWildDinos").is_ok()
    );

    assert!(validate_assistant_gm_runtime_command("arksurvivalascended", "quit").is_err());
    assert!(
        validate_assistant_gm_runtime_command("arksurvivalascended", "Broadcast hello").is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "dontstarve",
            "local p=AllPlayers[1]; p.components.health:SetPercent(1)"
        )
        .is_err()
    );
}

#[test]
fn assistant_gm_command_policy_allows_templated_dst_commands_only() {
    assert!(validate_assistant_gm_runtime_command(
            "dontstarve",
            "for _,v in ipairs(AllPlayers) do for i=1,20 do v.components.inventory:GiveItem(SpawnPrefab(\"log\")) end end"
        )
        .is_ok());
    assert!(
        validate_assistant_gm_runtime_command(
            "dontstarve",
            "c_give(\"footballhat\", 2, AllPlayers[1])"
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "dontstarve",
            "AllPlayers[1]:PushEvent(\"respawnfromghost\")"
        )
        .is_ok()
    );

    assert!(
        validate_assistant_gm_runtime_command(
            "dontstarve",
            "TheWorld:PushEvent(\"ms_setseason\", \"winter\")"
        )
        .is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "dontstarve",
            "local p=AllPlayers[1]; p.components.health:SetPercent(1)"
        )
        .is_err()
    );
    assert!(validate_assistant_gm_runtime_command("dontstarve", "c_shutdown(true)").is_err());
}

#[test]
fn assistant_gm_command_policy_allows_known_palworld_commands_only() {
    assert!(validate_assistant_gm_runtime_command("palworld", "ShowPlayers").is_ok());
    assert!(validate_assistant_gm_runtime_command("palworld", "Save").is_ok());
    assert!(
        validate_assistant_gm_runtime_command("palworld", "KickPlayer 76561198000000000").is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command("palworld", "BanPlayer 76561198000000000").is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command("palworld", "UnBanPlayer 76561198000000000").is_err()
    );

    assert!(validate_assistant_gm_runtime_command("palworld", "Shutdown 10 test").is_err());
    assert!(validate_assistant_gm_runtime_command("palworld", "Broadcast hello").is_err());
    assert!(validate_assistant_gm_runtime_command("palworld", "KickPlayer player-name").is_err());
    assert!(
        validate_assistant_gm_runtime_command(
            "palworld",
            "KickPlayer 76561198000000000; Shutdown 10"
        )
        .is_err()
    );
}

#[test]
fn assistant_gm_command_policy_allows_known_sevendaystodie_commands_only() {
    assert!(validate_assistant_gm_runtime_command("sevendaystodie", "listplayerids").is_ok());
    assert!(validate_assistant_gm_runtime_command("sevendaystodie", "saveworld").is_ok());
    assert!(
        validate_assistant_gm_runtime_command("sevendaystodie", "kick 76561198000000000 LanGame")
            .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "sevendaystodie",
            "ban add 76561198000000000 10 years LanGame"
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command("sevendaystodie", "ban remove 76561198000000000")
            .is_ok()
    );

    assert!(validate_assistant_gm_runtime_command("sevendaystodie", "shutdown").is_err());
    assert!(validate_assistant_gm_runtime_command("sevendaystodie", "say hello").is_err());
    assert!(
        validate_assistant_gm_runtime_command("sevendaystodie", "kick bad target LanGame").is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "sevendaystodie",
            "ban add 76561198000000000 1 hour LanGame"
        )
        .is_err()
    );
}

#[test]
fn assistant_gm_command_policy_allows_known_rust_commands_only() {
    assert!(validate_assistant_gm_runtime_command("rust", "status").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "players").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "users").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "banlistex").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "server.writecfg").is_ok());
    assert!(
        validate_assistant_gm_runtime_command("rust", "kick \"Friendly Player\" \"LanGame\"")
            .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "rust",
            "banid 76561198000000000 \"LanGame\" \"Banned by LanGame\""
        )
        .is_ok()
    );
    assert!(validate_assistant_gm_runtime_command("rust", "unban 76561198000000000").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "ownerid 76561198000000000").is_ok());
    assert!(validate_assistant_gm_runtime_command("rust", "moderatorid 76561198000000000").is_ok());

    assert!(validate_assistant_gm_runtime_command("rust", "say hello").is_err());
    assert!(validate_assistant_gm_runtime_command("rust", "quit").is_err());
    assert!(
        validate_assistant_gm_runtime_command("rust", "kick \"Bad; Player\" \"LanGame\"").is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "rust",
            "banid player-name \"LanGame\" \"Banned by LanGame\""
        )
        .is_err()
    );
}

#[test]
fn assistant_gm_command_policy_allows_known_terraria_commands_only() {
    assert!(validate_assistant_gm_runtime_command("terraria", "playing").is_ok());
    assert!(validate_assistant_gm_runtime_command("terraria", "save").is_ok());
    assert!(validate_assistant_gm_runtime_command("terraria", "kick BluePlayer").is_ok());
    assert!(validate_assistant_gm_runtime_command("terraria", "ban BluePlayer").is_ok());

    assert!(validate_assistant_gm_runtime_command("terraria", "exit").is_err());
    assert!(validate_assistant_gm_runtime_command("terraria", "say hello").is_err());
    assert!(validate_assistant_gm_runtime_command("terraria", "kick bad target").is_err());
    assert!(validate_assistant_gm_runtime_command("terraria", "playing; exit").is_err());
}

#[test]
fn assistant_gm_command_policy_allows_known_minecraft_commands_only() {
    assert!(validate_assistant_gm_runtime_command("minecraft", "list").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "save-all flush").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "kick Blue_Player LanGame").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "ban Blue_Player LanGame").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "pardon Blue_Player").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "op Blue_Player").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "deop Blue_Player").is_ok());
    assert!(validate_assistant_gm_runtime_command("minecraft", "whitelist list").is_ok());
    assert!(
        validate_assistant_gm_runtime_command("minecraft", "whitelist add Blue_Player").is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command("minecraft", "whitelist remove Blue_Player").is_ok()
    );

    assert!(validate_assistant_gm_runtime_command("minecraft", "stop").is_err());
    assert!(validate_assistant_gm_runtime_command("minecraft", "say hello").is_err());
    assert!(validate_assistant_gm_runtime_command("minecraft", "kick Bad Player LanGame").is_err());
    assert!(validate_assistant_gm_runtime_command("minecraft", "kick Blue_Player; stop").is_err());
    assert!(
        validate_assistant_gm_runtime_command("minecraft", "give Blue_Player diamond 64").is_err()
    );
}

#[test]
fn assistant_gm_command_policy_allows_known_project_zomboid_commands_only() {
    assert!(validate_assistant_gm_runtime_command("projectzomboid", "players").is_ok());
    assert!(validate_assistant_gm_runtime_command("projectzomboid", "save").is_ok());
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "kickuser \"BluePlayer\" -r \"LanGame\""
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "banuser \"BluePlayer\" -r \"LanGame\""
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command("projectzomboid", "banid 76561198000000000").is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command("projectzomboid", "unbanuser \"BluePlayer\"").is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "setaccesslevel \"BluePlayer\" moderator"
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "addusertowhitelist \"BluePlayer\""
        )
        .is_ok()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "removeuserfromwhitelist \"BluePlayer\""
        )
        .is_ok()
    );

    assert!(validate_assistant_gm_runtime_command("projectzomboid", "quit").is_err());
    assert!(validate_assistant_gm_runtime_command("projectzomboid", "servermsg hello").is_err());
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "kickuser \"Bad;User\" -r \"LanGame\""
        )
        .is_err()
    );
    assert!(
        validate_assistant_gm_runtime_command(
            "projectzomboid",
            "setaccesslevel \"BluePlayer\" admin"
        )
        .is_err()
    );
}

#[test]
fn assistant_gm_command_policy_rejects_vrising_in_game_admin_commands_over_rcon() {
    for command in [
        "ListUsers",
        "banned",
        "reloadbanlist",
        "kick \"Friendly Vampire\"",
        "banuser 76561198000000000",
        "unban 7",
        "shutdown 0 test",
        "announce hello",
    ] {
        assert!(
            validate_assistant_gm_runtime_command("vrising", command).is_err(),
            "V Rising assistant RCON unexpectedly accepted `{command}`"
        );
    }
}

#[test]
fn assistant_gm_command_policy_rejects_unknown_module() {
    assert!(validate_assistant_gm_runtime_command("nonexistentgame", "status").is_err());
}

#[test]
fn assistant_operation_prompt_includes_selected_ports_and_repair_action() {
    let summary = assistant_test_instance("project-zomboid-1", "projectzomboid", "PZ Main");
    let details = InstanceDetails {
        summary: summary.clone(),
        config_file_path: String::from("D:/LanGame/instances/project-zomboid-1/config"),
        saves_path: String::from("D:/LanGame/instances/project-zomboid-1/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: true,
        backup_retention_count: 3,
        settings_json: String::from(r#"{"server_name":"PZ Main"}"#),
        ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 16261,
            },
            PortBinding {
                name: String::from("direct"),
                protocol: String::from("udp"),
                port: 16262,
            },
        ],
        active_run: None,
    };
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: AssistantProviderSettings {
            provider: String::from("openai-compatible"),
            model: String::from("smoke-model"),
            base_url: String::from("http://127.0.0.1/v1"),
            api_key: String::new(),
        },
        prompt: String::from("latest log says game port 16261 is already in use"),
        context: None,
        selected_instance_id: Some(String::from("project-zomboid-1")),
        selected_module_id: Some(String::from("projectzomboid")),
    };

    let prompt = build_assistant_operation_planner_prompt(
        &input,
        &[summary],
        &[],
        Some(&details),
        None,
        &[],
        None,
    )
    .expect("planner prompt");

    assert!(prompt.contains("- repair_ports:"));
    assert!(prompt.contains("Selected instance ports:"));
    assert!(prompt.contains("- game | protocol=udp | port=16261"));
    assert!(prompt.contains("portPatch (object of selected port names and integer values)"));
    assert!(
        !prompt.contains("16271"),
        "examples must not invent a replacement port"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn palworld_rest_dispatch_uses_declared_actions_and_requires_snapshot_moderation()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("palworld-rest-dispatch");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_palworld_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let instance = create_fake_module_instance(
        app.state::<DesktopState>(),
        "palworld",
        "Palworld REST dispatch",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let (port, fixture) =
        crate::live_players::palworld_rest::capture_response("200 OK", "", b"", Duration::ZERO);
    let mut ports = details.ports.clone();
    ports
        .iter_mut()
        .find(|binding| binding.name == "rest_api")
        .unwrap()
        .port = port;
    let mut settings: Value = serde_json::from_str(&details.settings_json)?;
    settings["rest_api_enabled"] = json!(true);
    settings["admin_password"] = json!(uuid::Uuid::new_v4().simple().to_string());
    update_instance_record(
        app.state::<DesktopState>(),
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: "127.0.0.1".into(),
            auto_backup_on_stop: false,
            backup_retention_count: 1,
            ports,
            settings_json: settings.to_string(),
        },
    )
    .await?;
    register_smoke_instance_running(
        app.state::<DesktopState>(),
        &storage,
        &details,
        &run_root,
        "rest-dispatch",
    )
    .await?;
    let request = |action_id: Option<&str>| InstanceRuntimeCommandInput {
        instance_id: details.summary.id.clone(),
        command: String::from("Save"),
        process_key: None,
        transport: Some("palworld_rest".into()),
        port_name: Some("attacker-port".into()),
        password_setting_key: Some("attacker-key".into()),
        enabled_setting_key: None,
        runtime_action_id: action_id.map(str::to_owned),
        runtime_action_target: None,
        runtime_action_role: None,
    };
    let raw_error = send_instance_runtime_command(app.state::<DesktopState>(), request(None))
        .await
        .unwrap_err();
    assert!(raw_error.contains("declared runtime action"));
    let mut raw_rcon = request(None);
    raw_rcon.transport = Some("source_rcon".into());
    let rcon_error = send_instance_runtime_command(app.state::<DesktopState>(), raw_rcon)
        .await
        .unwrap_err();
    assert!(rcon_error.contains("REST API"));
    let mut moderation = request(Some("kick_player"));
    moderation.runtime_action_target = Some("steam_76561190000000001".into());
    let rejected = send_instance_runtime_command(app.state::<DesktopState>(), moderation)
        .await
        .unwrap_err();
    assert!(rejected.contains("live-player service"));
    let saved =
        send_instance_runtime_command(app.state::<DesktopState>(), request(Some("save_world")))
            .await?;
    assert_eq!(
        serde_json::from_str::<Value>(&saved.command)?,
        json!({"operation":"save"})
    );
    assert!(saved.response_text.unwrap().contains("HTTP 200"));
    let sent = fixture.join().unwrap();
    assert!(sent.starts_with("POST /v1/api/save HTTP/1.1\r\n"));
    assert!(sent.split_once("\r\n\r\n").unwrap().1.is_empty());
    Ok(())
}
