use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_bind_ip_patch_rejects_invalid_values_without_saving_and_preserves_unrelated_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-bind-ip");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_minecraft_instance(state.clone(), "Bind address verification").await?;
    let storage = bootstrap_storage()?;
    let baseline = read_instance_details(&storage.paths, &created.summary.id).await?;
    let baseline_settings: Value = serde_json::from_str(&baseline.settings_json)?;
    let baseline_file = fs::read(&baseline.config_file_path)?;
    let request = |action: &str, patch: Value| AssistantExecuteOperationInput {
        task: Default::default(),
        settings: stored_openai_compatible_ai_mock_settings(),
        prompt: format!(
            "Update the selected Minecraft configuration.\nmock-action:{action}\nmock-settings-patch:{patch}"
        ),
        context: None,
        selected_instance_id: Some(created.summary.id.clone()),
        selected_module_id: Some("minecraft".into()),
    };

    for action in ["customizeConfig", "applyBeginnerConfig"] {
        for invalid in [json!("not-an-address"), json!(""), Value::Null, json!(127)] {
            let error = assistant_execute_operation_inner(
                None,
                state.clone(),
                request(action, json!({"bind_ip": invalid, "max_players": 24})),
            )
            .await
            .expect_err("an invalid listener address must fail before any setting is saved");
            assert!(error.contains("bind_ip"), "{error}");
            let unchanged = read_instance_details(&storage.paths, &created.summary.id).await?;
            assert_eq!(unchanged.summary.bind_ip, baseline.summary.bind_ip);
            assert_eq!(
                serde_json::from_str::<Value>(&unchanged.settings_json)?,
                baseline_settings
            );
            assert_eq!(fs::read(&unchanged.config_file_path)?, baseline_file);
            assert!(unchanged.active_run.is_none());
        }
    }

    let saved = assistant_execute_operation_inner(
        None,
        state.clone(),
        request("customizeConfig", json!({"bind_ip": "127.0.0.1"})),
    )
    .await?;
    assert_eq!(saved.applied_settings_keys, ["bind_ip"]);
    let bound = read_instance_details(&storage.paths, &created.summary.id).await?;
    let mut expected = baseline_settings;
    expected["bind_ip"] = json!("127.0.0.1");
    assert_eq!(bound.summary.bind_ip, "127.0.0.1");
    assert_eq!(
        serde_json::from_str::<Value>(&bound.settings_json)?,
        expected
    );

    let updated = assistant_execute_operation_inner(
        None,
        state,
        request("applyBeginnerConfig", json!({"max_players": 24})),
    )
    .await?;
    assert_eq!(updated.applied_settings_keys, ["max_players"]);
    let unrelated = read_instance_details(&storage.paths, &created.summary.id).await?;
    expected["max_players"] = json!(24);
    assert_eq!(unrelated.summary.bind_ip, "127.0.0.1");
    assert_eq!(
        serde_json::from_str::<Value>(&unrelated.settings_json)?,
        expected
    );
    assert!(unrelated.active_run.is_none());
    Ok(())
}
