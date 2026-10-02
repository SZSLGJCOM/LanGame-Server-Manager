use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_confirmed_dst_guided_edit_verifies_canonical_lua_and_preserves_custom_values()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-dst-guided-edit");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "dontstarve",
        "DST guided edit verification",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let mut original: Value = serde_json::from_str(&details.settings_json)?;
    let master_lua = "return {override_enabled=true,preset='SURVIVAL_TOGETHER',overrides={day='default',world_size='default',custom_note='preserved'}}";
    let caves_lua =
        "return {override_enabled=true,preset='DST_CAVE',overrides={world_size='small'}}";
    original["master_worldgenoverride_lua"] = json!(master_lua);
    original["master_world_overrides_extra"] = json!("day='default', custom_extra=false,");
    original["caves_worldgenoverride_lua"] = json!(caves_lua);
    original["enable_caves"] = json!(false);
    original["cluster_description"] = json!("Preserve the operator's description");
    let baseline = update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: original.to_string(),
            ports: details.ports.clone(),
        },
    )
    .await?;
    let baseline_settings: Value = serde_json::from_str(&baseline.settings_json)?;
    assert_eq!(baseline_settings["master_day"], "default");

    let provider = stored_openai_compatible_ai_mock_settings();
    let preview = assistant_preview_operation_inner(
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: provider.clone(),
            prompt: String::from(
                "Update the selected DST world's day setting.\nmock-action:customizeConfig\nmock-settings-patch:{\"master_day\":\"onlynight\"}",
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("dontstarve")),
        },
    )
    .await?;
    assert!(preview.requires_confirmation);
    let before_confirmation =
        read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    assert_eq!(before_confirmation.settings_json, baseline.settings_json);

    let output = assistant_confirm_operation_inner(
        app.state::<DesktopState>(),
        AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider,
            confirmation_token: preview
                .confirmation_token
                .expect("preview confirmation token"),
            plan_summary: preview.plan_summary.expect("preview summary"),
        },
    )
    .await?;
    assert!(output.handled);
    assert!(!output.requires_confirmation);
    assert_eq!(output.action, AssistantOperationAction::CustomizeConfig);
    assert_eq!(output.applied_settings_keys, ["master_day"]);

    let persisted = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let saved: Value = serde_json::from_str(&persisted.settings_json)?;
    assert_eq!(saved["master_day"], "onlynight");
    assert_eq!(
        saved["cluster_description"],
        "Preserve the operator's description"
    );
    assert_eq!(saved["enable_caves"], false);
    assert_eq!(saved["caves_worldgenoverride_lua"], caves_lua);
    let saved_master = saved["master_worldgenoverride_lua"]
        .as_str()
        .expect("saved Lua source");
    assert_ne!(saved_master, master_lua);
    let lua = mlua::Lua::new();
    let world: mlua::Table = lua.load(saved_master).eval()?;
    let overrides: mlua::Table = world.get("overrides")?;
    assert_eq!(overrides.get::<String>("day")?, "onlynight");
    assert_eq!(overrides.get::<String>("custom_note")?, "preserved");
    let extra_source = saved["master_world_overrides_extra"]
        .as_str()
        .expect("saved extra overrides");
    let extra: mlua::Table = lua.load(format!("return {{{extra_source}}}")).eval()?;
    assert_eq!(extra.get::<String>("day")?, "onlynight");
    assert!(!extra.get::<bool>("custom_extra")?);
    let generated_path = Path::new(&persisted.config_file_path)
        .parent()
        .expect("instance configuration directory")
        .join("clusters/main/Master/worldgenoverride.lua");
    let generated = fs::read_to_string(&generated_path).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "read generated world settings {}: {error}",
                generated_path.display()
            ),
        )
    })?;
    assert_eq!(generated.trim(), saved_master.trim());
    Ok(())
}
