use super::*;

#[tokio::test(flavor = "current_thread")]
async fn workshop_collection_removal_command_and_readback_recovery_use_instance_guards()
-> Result<(), Box<dyn std::error::Error>> {
    use sha2::Digest;
    let _serial = command_smoke_lock().lock().await;
    let root = temp_test_dir("collection-removal");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let shared = PathBuf::from(&settings.games_root).join("squad");
    fs::create_dir_all(&shared)?;
    fs::write(shared.join("SquadGameServer.exe"), b"package")?;
    record_fake_program_baseline(&settings, "squad")?;
    save_app_settings(settings)?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    persist_descriptor_install_states(&storage, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "squad")?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let created = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "Squad collection remove".into(),
            module_id: "squad".into(),
        },
        app_storage::InstanceCreationOptions {
            prefer_existing_install: false,
            program_mode: Some(app_core::InstanceProgramMode::Independent),
            require_clean_program: true,
            ..Default::default()
        },
    )
    .await?
    .provisioning;
    let storage = bootstrap_storage()?;
    let mut current = read_instance_details(&storage.paths, &created.summary.id).await?;
    let mut values: Value = serde_json::from_str(&current.settings_json)?;
    values["steam_workshop_collections"] =
        json!([{"id":"111111", "title":"One", "member_ids":["222222"]}]);
    let input = |details: &InstanceDetails, settings: &Value| UpdateInstanceInput {
        id: details.summary.id.clone(),
        bind_ip: details.summary.bind_ip.clone(),
        auto_backup_on_stop: details.auto_backup_on_stop,
        backup_retention_count: details.backup_retention_count,
        settings_json: settings.to_string(),
        ports: details.ports.clone(),
    };
    current = update_instance_if_current(
        &storage.paths,
        input(&current, &values),
        &current.settings_json,
    )
    .await?;
    let config = PathBuf::from(&current.config_file_path);
    let instance_root = config.parent().unwrap().parent().unwrap();
    let source = instance_root.join("runtime/SquadGame/Plugins/Mods/222222");
    fs::create_dir_all(&source)?;
    fs::write(source.join("payload.pak"), b"preserve")?;
    let before = fs::read(&config)?;
    values["steam_workshop_collections"] = json!([]);
    let next = input(&current, &values);
    let run = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &current.summary.id,
            session_id: None,
            process_key: "main",
            display_name: "Server",
            pid: 12345,
            log_path: "synthetic.log",
            is_primary: true,
        },
        None,
    )
    .await?;
    let error = remove_instance_workshop_collection(
        app.state::<DesktopState>(),
        next.clone(),
        current.settings_json.clone(),
        "111111".into(),
        vec!["222222".into()],
        None,
    )
    .await
    .unwrap_err();
    assert!(error.contains("stop the instance"), "{error}");
    assert!(source.join("payload.pak").exists());
    mark_instance_process_stopped(
        &storage.paths,
        &current.summary.id,
        run.run_id,
        Some(0),
        false,
    )
    .await?;

    let operation_id = uuid::Uuid::new_v4().to_string();
    let retained = instance_root
        .join(".lgsm-workshop-retained")
        .join(&operation_id);
    fs::create_dir_all(&retained)?;
    let mut after: Value = serde_json::from_slice(&before)?;
    after["settings"] = values;
    let hash = |bytes: &[u8]| {
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let journal = serde_json::to_vec(&json!({"version":1, "instance_id": current.summary.id,
        "operation_id": operation_id, "original_sha256": hash(&before),
        "replacement_sha256": hash(&serde_json::to_vec_pretty(&after)?), "members":["222222"]}))?;
    fs::write(retained.join("owner.json"), &journal)?;
    fs::write(instance_root.join(".lgsm-workshop-removal.json"), &journal)?;
    fs::rename(&source, retained.join("222222"))?;
    let operation = command_result(
        app.state::<DesktopState>()
            .begin_storage_context_operation("test collection recovery"),
    )?;
    let recovered = command_result(
        read_instance_details_with_runtime_recovery(
            &app.state::<DesktopState>(),
            &operation,
            &storage.paths,
            &current.summary.id,
        )
        .await,
    )?;
    assert!(source.join("payload.pak").exists());
    assert_eq!(fs::read(&config)?, before);
    let original_settings: Value = serde_json::from_str(&recovered.settings_json)?;
    let member_saved = command_result(
        remove_instance_workshop_collection(
            app.state::<DesktopState>(),
            input(&recovered, &original_settings),
            recovered.settings_json.clone(),
            "111111".into(),
            vec!["222222".into()],
            Some(true),
        )
        .await,
    )?;
    assert_eq!(fs::read(&config)?, before);
    assert_eq!(
        serde_json::from_str::<Value>(&member_saved.settings_json)?,
        original_settings
    );
    assert!(!source.exists());
    // A crash after the member-only commit is recovered by ordinary readback.
    let member_operation = fs::read_dir(instance_root.join(".lgsm-workshop-retained"))?
        .map(|entry| entry.unwrap().path())
        .find(|path| path.join("committed.json").exists())
        .unwrap();
    fs::write(
        instance_root.join(".lgsm-workshop-removal.json"),
        fs::read(member_operation.join("owner.json"))?,
    )?;
    command_result(
        read_instance_details_with_runtime_recovery(
            &app.state::<DesktopState>(),
            &operation,
            &storage.paths,
            &current.summary.id,
        )
        .await,
    )?;
    assert!(!source.exists());
    assert!(member_operation.join("222222/payload.pak").exists());
    assert!(!instance_root.join(".lgsm-workshop-removal.json").exists());
    // Synthetic redeployment models repairing the still-saved collection.
    fs::create_dir_all(&source)?;
    fs::write(source.join("payload.pak"), b"preserve")?;
    let saved = command_result(
        remove_instance_workshop_collection(
            app.state::<DesktopState>(),
            next,
            recovered.settings_json,
            "111111".into(),
            vec!["222222".into()],
            None,
        )
        .await,
    )?;
    assert!(!source.exists());
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json)?["steam_workshop_collections"],
        json!([])
    );
    drop(operation);
    fs::remove_dir_all(root)?;
    Ok(())
}
