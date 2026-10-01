use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_start_rechecks_preview_after_waiting_for_the_instance_lock()
-> Result<(), Box<dyn std::error::Error>> {
    use std::future::Future;
    use std::task::Poll;

    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-start-race");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_project_zomboid_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_module_instance(state.clone(), "projectzomboid", "Start race").await?;
    let storage = bootstrap_storage()?;
    {
        let mut app_state = state.app_state.write().unwrap();
        app_state.settings = storage.settings.clone();
        app_state.storage = storage.storage_status.clone();
    }
    let expected = read_instance_details(&storage.paths, &created.summary.id).await?;
    let reservation = match state.try_reserve_runtime_start(&created.summary.id, "test")? {
        RuntimeStartReservationAttempt::Reserved(value) => value,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let lock = state.acquire_instance_mutation(&created.summary.id).await;
    let mut start = Box::pin(start_instance_process_after_reconcile_reserved(
        None,
        &state,
        &storage,
        created.summary.id.clone(),
        "test",
        &reservation,
        RuntimeStartPreconditions {
            world_start: None,
            instance: Some(expected.clone()),
            file_changes: Vec::new(),
        },
    ));
    assert!(std::future::poll_fn(|cx| Poll::Ready(start.as_mut().poll(cx).is_pending())).await);
    let mut ports = expected.ports.clone();
    ports[0].port += 10;
    // Simulate the current lock owner finishing an edit while start is queued.
    update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: expected.summary.id.clone(),
            bind_ip: expected.summary.bind_ip.clone(),
            auto_backup_on_stop: expected.auto_backup_on_stop,
            backup_retention_count: expected.backup_retention_count,
            settings_json: expected.settings_json.clone(),
            ports,
        },
    )
    .await?;
    drop(lock);
    let error = tokio::time::timeout(Duration::from_secs(5), start)
        .await?
        .expect_err("queued start must reject a changed preview before launching");
    assert!(
        error.contains("changed after the start operation"),
        "{error}"
    );
    assert!(
        read_active_instance_run(&storage.paths, &created.summary.id)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_start_rechecks_confirmed_file_hash_after_waiting_for_instance_lock()
-> Result<(), Box<dyn std::error::Error>> {
    use std::future::Future;
    use std::task::Poll;

    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-start-file-race");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created = create_fake_module_instance(state.clone(), "dontstarve", "File race").await?;
    let storage = bootstrap_storage()?;
    {
        let mut app_state = state.app_state.write().unwrap();
        app_state.settings = storage.settings.clone();
        app_state.storage = storage.storage_status.clone();
    }
    let expected = read_instance_details(&storage.paths, &created.summary.id).await?;
    let instance_root = Path::new(&expected.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("fixture instance root")?;
    let file = "data/ugc/Master/content/322330/123/modmain.lua";
    let path = instance_root.join(file);
    fs::create_dir_all(path.parent().ok_or("fixture file parent")?)?;
    fs::write(&path, "local value = 1\n")?;
    let patch = |source_sha256, before: &str, after: &str| app_storage::InstanceTextPatch {
        file: file.to_owned(),
        source_sha256,
        before: before.to_owned(),
        after: after.to_owned(),
    };
    let document =
        app_storage::read_instance_patch_file(&storage.paths, &created.summary.id, file).await?;
    let prepared = app_storage::prepare_instance_file_patch(
        &storage.paths,
        &created.summary.id,
        patch(document.source_sha256, "value = 1", "value = 2"),
    )
    .await?;
    let confirmed =
        app_storage::apply_instance_file_patch(&storage.paths, &created.summary.id, prepared)
            .await?;
    let reservation = match state.try_reserve_runtime_start(&created.summary.id, "test")? {
        RuntimeStartReservationAttempt::Reserved(value) => value,
        other => panic!("unexpected reservation: {other:?}"),
    };
    let lock = state.acquire_instance_mutation(&created.summary.id).await;
    let mut start = Box::pin(start_instance_process_after_reconcile_reserved(
        None,
        &state,
        &storage,
        created.summary.id.clone(),
        "test",
        &reservation,
        RuntimeStartPreconditions {
            world_start: None,
            instance: Some(expected.clone()),
            file_changes: vec![confirmed],
        },
    ));
    assert!(std::future::poll_fn(|cx| Poll::Ready(start.as_mut().poll(cx).is_pending())).await);

    // Complete the prior lock owner's legitimate second patch while the start
    // remains queued. Database configuration and run identity stay unchanged.
    let document =
        app_storage::read_instance_patch_file(&storage.paths, &created.summary.id, file).await?;
    let prepared = app_storage::prepare_instance_file_patch(
        &storage.paths,
        &created.summary.id,
        patch(document.source_sha256, "value = 2", "value = 3"),
    )
    .await?;
    app_storage::apply_instance_file_patch(&storage.paths, &created.summary.id, prepared).await?;
    assert_eq!(
        serde_json::to_value(read_instance_details(&storage.paths, &created.summary.id).await?)?,
        serde_json::to_value(&expected)?,
    );
    drop(lock);
    let error = tokio::time::timeout(Duration::from_secs(5), start)
        .await?
        .expect_err("queued start must reject changed Mod bytes before launching");
    assert!(
        error.contains("Confirmed instance file") && error.contains("changed after its patch"),
        "{error}"
    );
    assert_eq!(fs::read_to_string(&path)?, "local value = 3\n");
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
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_investigation_mod_repair_preserves_ids_and_verifies_native_order()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-mod-repair");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_project_zomboid_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let instance = create_fake_module_instance(
        app.state::<DesktopState>(),
        "projectzomboid",
        "Mod order fixture",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let mut values: Value = serde_json::from_str(&details.settings_json)?;
    values["mods"] = json!("addon\ndependency");
    let original = update_instance_record(
        app.state::<DesktopState>(),
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: values.to_string(),
            ports: details.ports.clone(),
        },
    )
    .await?;
    let oversized_path = Path::new(&original.config_file_path)
        .parent()
        .unwrap()
        .join("00-oversized.ini");
    fs::write(&oversized_path, vec![b'x'; 256 * 1024 + 1])?;
    let input = AssistantExecuteOperationInput {
        task: commands_assistant_ops::AssistantTaskRequest {
            goal: commands_assistant_ops::AssistantTaskGoal::ApplyChange,
            preserve_existing_mods: true,
        },
        settings: stored_openai_compatible_ai_mock_settings(),
        prompt: String::from(
            "mock-investigate-mod-order mock-require-read-failure: inspect selected mod settings and put dependency before addon",
        ),
        context: None,
        selected_instance_id: Some(instance.summary.id.clone()),
        selected_module_id: Some(String::from("projectzomboid")),
    };
    let preview =
        assistant_preview_operation_inner(app.state::<DesktopState>(), input.clone()).await?;
    assert!(preview.requires_confirmation);
    assert!(
        preview
            .plan_summary
            .as_deref()
            .unwrap()
            .contains("dependency")
    );
    let before = read_instance_details(&storage.paths, &instance.summary.id).await?;
    assert_eq!(
        before.settings_json, original.settings_json,
        "investigation and preview must not write"
    );
    let confirm = AssistantConfirmOperationInput {
        continue_task: false,
        conversation_id: preview.conversation_id.clone(),
        settings: input.settings.clone(),
        confirmation_token: preview.confirmation_token.unwrap(),
        plan_summary: preview.plan_summary.unwrap(),
    };
    let result =
        assistant_confirm_operation_inner(app.state::<DesktopState>(), confirm.clone()).await?;
    assert!(result.message.contains("read back"));
    let task = result
        .task
        .as_ref()
        .ok_or("confirmed task receipt missing")?;
    assert!(task.preserve_existing_mods);
    assert_eq!(task.status, AssistantTaskStatus::Completed);
    assert!(task.checks.iter().any(|check| {
        check.name == "mod_configuration_preserved"
            && check.status == AssistantTaskCheckStatus::Satisfied
    }));
    let after = read_instance_details(&storage.paths, &instance.summary.id).await?;
    values["mods"] = json!("dependency\naddon");
    assert_eq!(serde_json::from_str::<Value>(&after.settings_json)?, values);
    let config_root = Path::new(&after.config_file_path).parent().unwrap();
    let native = fs::read_to_string(config_root.join("server.ini"))?;
    assert!(
        native.contains("Mods=dependency;addon"),
        "native config must match the confirmed order"
    );
    let runtime_native = fs::read_to_string(
        config_root
            .join("runtime-home/Zomboid/Server")
            .join(format!("{}.ini", instance.summary.id)),
    )?;
    assert!(
        runtime_native.contains("Mods=dependency;addon"),
        "the actual launch config must also preserve the confirmed order"
    );
    assert!(
        assistant_confirm_operation_inner(app.state::<DesktopState>(), confirm)
            .await
            .is_err(),
        "confirmation is single-use"
    );

    let preview =
        assistant_preview_operation_inner(app.state::<DesktopState>(), input.clone()).await?;
    let mut changed_ports = after.ports.clone();
    changed_ports[0].port += 10;
    update_instance_record(
        app.state::<DesktopState>(),
        UpdateInstanceInput {
            id: after.summary.id.clone(),
            bind_ip: after.summary.bind_ip.clone(),
            auto_backup_on_stop: after.auto_backup_on_stop,
            backup_retention_count: after.backup_retention_count,
            settings_json: after.settings_json.clone(),
            ports: changed_ports,
        },
    )
    .await?;
    let error = assistant_confirm_operation_inner(
        app.state::<DesktopState>(),
        AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: input.settings,
            confirmation_token: preview.confirmation_token.unwrap(),
            plan_summary: preview.plan_summary.unwrap(),
        },
    )
    .await
    .expect_err("port-only edits invalidate the preview");
    assert!(error.contains("changed"));
    Ok(())
}
