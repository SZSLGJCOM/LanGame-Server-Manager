use super::*;

fn write_mod_evidence_fixture(root: &Path, metadata: &str) -> std::io::Result<()> {
    fs::create_dir_all(root)?;
    fs::write(root.join("modinfo.lua"), metadata)?;
    fs::write(
        root.join("modmain.lua"),
        "error('Metadata inspection must not execute modmain')",
    )
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_installed_mod_evidence_requires_the_marked_private_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-mod-private");
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
    let storage = bootstrap_storage()?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    let descriptor = find_descriptor(&descriptors, "dontstarve")?;
    // This test exercises private-marker rejection, so explicitly request a
    // copied installation instead of the first instance's normal library binding.
    let created = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: String::from("Private mod metadata"),
            module_id: String::from("dontstarve"),
        },
        app_storage::InstanceCreationOptions {
            prefer_existing_install: false,
            program_mode: Some(app_core::InstanceProgramMode::Independent),
            require_clean_program: true,
            ..Default::default()
        },
    )
    .await?;
    let instance = read_instance_details(&storage.paths, &created.provisioning.summary.id).await?;
    let instance_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .expect("instance root");
    let private_root = created.effective_install_root;
    let shared_root = Path::new(&settings.games_root).join("dontstarve");
    assert_eq!(private_root, instance_root.join("runtime"));
    assert_eq!(
        app_storage::resolve_instance_runtime_root(instance_root)?,
        private_root
    );
    assert_ne!(private_root, shared_root);
    let marker = private_root.join(".langame-private-runtime");
    assert_eq!(fs::read(&marker)?, b"managed\n");
    write_mod_evidence_fixture(
        &shared_root.join("mods/local_library"),
        "name='Shared library';version='1';priority=10;api_version=10;dst_compatible=true",
    )?;
    write_mod_evidence_fixture(
        &private_root.join("mods/local_library"),
        "name='Private library';version='2';priority=20;api_version=10;dst_compatible=true",
    )?;
    fs::create_dir_all(private_root.join("bin64"))?;
    fs::write(
        private_root.join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
        "fake private DST executable",
    )?;
    fs::remove_file(&marker)?;
    let missing_marker = read_assistant_installed_mods(
        &app.state::<DesktopState>(),
        &storage,
        &instance,
        vec![String::from("local_library")],
        0,
    )
    .await;
    assert!(
        missing_marker.is_err(),
        "unmarked runtime must not read shared Mods"
    );
    fs::write(&marker, b"managed\n")?;
    let page = read_assistant_installed_mods(
        &app.state::<DesktopState>(),
        &storage,
        &instance,
        vec![String::from("local_library")],
        0,
    )
    .await?;
    assert_eq!(page["totalMatches"], 1);
    assert!(page["nextOffset"].is_null());
    let entries = page["entries"].as_array().expect("serialized entries");
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry["folderName"], "local_library");
    assert_eq!(entry["status"], "read");
    assert_eq!(entry["source"], "install/mods/local_library");
    assert_eq!(entry["metadata"]["name"], "Private library");
    assert_eq!(entry["metadata"]["priority"], 20);
    assert_eq!(entry["metadata"]["api_version"], 10);
    assert_eq!(entry["files"]["modmain"], "present");
    assert_eq!(
        entry["configuredEnablementByShard"][0]["declaredState"],
        "not_declared"
    );
    let mut updated: Value = serde_json::from_str(&instance.settings_json)?;
    updated["master_modoverrides_lua"] = json!("return {local_library={enabled=true}}");
    update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: instance.summary.id.clone(),
            bind_ip: instance.summary.bind_ip.clone(),
            auto_backup_on_stop: instance.auto_backup_on_stop,
            backup_retention_count: instance.backup_retention_count,
            settings_json: updated.to_string(),
            ports: instance.ports.clone(),
        },
    )
    .await?;
    // The original instance snapshot predates the edit; evidence must use the
    // current saved settings, just like the configuration reader does.
    let page = read_assistant_installed_mods(
        &app.state::<DesktopState>(),
        &storage,
        &instance,
        vec![String::from("local_library")],
        0,
    )
    .await?;
    assert_eq!(
        page["entries"][0]["configuredEnablementByShard"][0]["declaredState"],
        "enabled"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_installed_mod_evidence_binds_ugc_to_the_selected_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-mod-ugc");
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
    let selected = create_fake_module_instance(
        app.state::<DesktopState>(),
        "dontstarve",
        "Selected mod metadata",
    )
    .await?;
    let other = create_fake_module_instance(
        app.state::<DesktopState>(),
        "dontstarve",
        "Other mod metadata",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let selected = read_instance_details(&storage.paths, &selected.summary.id).await?;
    let other = read_instance_details(&storage.paths, &other.summary.id).await?;
    let ugc_mod_root = |instance: &InstanceDetails| {
        Path::new(&instance.config_file_path)
            .parent()
            .and_then(Path::parent)
            .expect("instance root")
            .join("data/ugc/Master/content/322330/123456")
    };
    write_mod_evidence_fixture(
        &ugc_mod_root(&other),
        "name='Other instance copy';version='1'",
    )?;
    let absent = read_assistant_installed_mods(
        &app.state::<DesktopState>(),
        &storage,
        &selected,
        vec![String::from("workshop-123456")],
        0,
    )
    .await?;
    assert_eq!(absent["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(absent["entries"][0]["status"], "missing");
    assert!(absent["entries"][0]["metadata"].is_null());

    let empty =
        read_assistant_installed_mods(&app.state::<DesktopState>(), &storage, &selected, vec![], 0)
            .await?;
    assert_eq!(empty["totalMatches"], 0);
    assert!(empty["entries"].as_array().expect("entries").is_empty());

    write_mod_evidence_fixture(
        &ugc_mod_root(&selected),
        "name='Selected instance copy';version='2'",
    )?;
    for (instance, expected_name) in [
        (&selected, "Selected instance copy"),
        (&other, "Other instance copy"),
    ] {
        let page = read_assistant_installed_mods(
            &app.state::<DesktopState>(),
            &storage,
            instance,
            vec![],
            0,
        )
        .await?;
        assert_eq!(page["totalMatches"], 1);
        assert_eq!(page["entries"].as_array().expect("entries").len(), 1);
        let entry = &page["entries"][0];
        assert_eq!(entry["folderName"], "workshop-123456");
        assert_eq!(entry["status"], "read");
        assert_eq!(entry["metadata"]["name"], expected_name);
        assert_eq!(entry["source"], "extra-1/ugc/Master/content/322330/123456");
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_installed_mod_evidence_rejects_unsupported_game_before_storage_access()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-mod-unsupported");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>()).await?;
    let provisioning = create_fake_module_instance(
        app.state::<DesktopState>(),
        "minecraft",
        "Unsupported mod inspection",
    )
    .await?;
    let mut storage = bootstrap_storage()?;
    let instance = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    storage.paths.modules_root = run_root.join("unavailable-module-catalog");
    let state = app.state::<DesktopState>();
    let _transition = state.begin_storage_context_transition()?;
    let error = read_assistant_installed_mods(
        &state,
        &storage,
        &instance,
        vec![String::from("local_library")],
        0,
    )
    .await
    .expect_err("unsupported module must be rejected before entering storage work");
    assert_eq!(
        error,
        "Installed mod metadata inspection currently supports DST only; no files were read."
    );
    Ok(())
}
