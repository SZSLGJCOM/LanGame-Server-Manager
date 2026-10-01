use super::*;

#[tokio::test(flavor = "current_thread")]
async fn manual_mod_deployment_waits_for_instance_lock_and_rejects_running_instance()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let root = temp_test_dir("manual-mod-instance-lock");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let modules_root = workspace_root().join("modules");
    let settings = AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: modules_root.to_string_lossy().into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    };
    let shared_root = PathBuf::from(&settings.games_root).join("squad");
    fs::create_dir_all(&shared_root)?;
    fs::write(shared_root.join("SquadGameServer.exe"), b"package")?;
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
            name: String::from("Private Squad Mods"),
            module_id: String::from("squad"),
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
    let instance_id = created.summary.id.clone();
    let instance = read_instance_details(&bootstrap_storage()?.paths, &instance_id).await?;
    let instance_root = Path::new(&instance.config_file_path)
        .parent()
        .and_then(Path::parent)
        .expect("instance root");
    let private_mods = instance_root.join("runtime/SquadGame/Plugins/Mods");
    let source = root.join("SelectedMod");
    fs::create_dir_all(&source)?;
    fs::write(source.join("ModInfo.xml"), b"operator mod")?;

    let held_lock = app
        .state::<DesktopState>()
        .acquire_instance_mutation(&instance_id)
        .await;
    let handle = app.handle().clone();
    let staged_instance_id = instance_id.clone();
    let mut stage = tokio::spawn(async move {
        stage_manual_mod_files(
            handle.state::<DesktopState>(),
            staged_instance_id,
            vec![source.to_string_lossy().into_owned()],
        )
        .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(150), &mut stage)
            .await
            .is_err(),
        "deployment must wait while startup owns this instance"
    );
    assert!(!private_mods.join("SelectedMod/ModInfo.xml").exists());
    drop(held_lock);
    let deployed = command_result(tokio::time::timeout(Duration::from_secs(10), stage).await??)?;
    assert_eq!(deployed.copied_file_count, 1);
    assert_eq!(
        fs::read(private_mods.join("SelectedMod/ModInfo.xml"))?,
        b"operator mod"
    );
    assert!(
        !shared_root
            .join("SquadGame/Plugins/Mods/SelectedMod")
            .exists()
    );

    let storage = bootstrap_storage()?;
    let run = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &instance_id,
            session_id: None,
            process_key: "main",
            display_name: "Server",
            pid: 12345,
            log_path: "synthetic-run.log",
            is_primary: true,
        },
        None,
    )
    .await?;
    let later = root.join("LaterMod");
    fs::create_dir_all(&later)?;
    fs::write(later.join("ModInfo.xml"), b"do not stage while running")?;
    let error = stage_manual_mod_files(
        app.state::<DesktopState>(),
        instance_id.clone(),
        vec![later.to_string_lossy().into_owned()],
    )
    .await
    .expect_err("running instance must reject Mod deployment");
    assert!(error.contains("Stop instance"), "{error}");
    assert!(!private_mods.join("LaterMod").exists());
    mark_instance_process_stopped(&storage.paths, &instance_id, run.run_id, Some(0), false).await?;
    fs::remove_dir_all(root)?;
    Ok(())
}
