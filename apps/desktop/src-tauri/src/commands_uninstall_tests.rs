use super::*;

#[cfg(windows)]
#[path = "commands_uninstall_cancellation_tests.rs"]
mod cancellation_tests;

#[path = "commands_library_cleanup_tests.rs"]
mod library_cleanup_tests;

async fn installed_astroneer_fixture(
    root: &Path,
) -> Result<(StorageBootstrap, ModuleDescriptor), Box<dyn std::error::Error>> {
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.join("instances").to_string_lossy().into_owned(),
        games_root: root.join("games").to_string_lossy().into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: root.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "astroneer")?.clone();
    seed_astroneer_library(&storage, &descriptor).await?;
    fs::write(
        storage.paths.games_root.join("other-game.sentinel"),
        b"keep unrelated files",
    )?;
    Ok((storage, descriptor))
}

async fn seed_astroneer_library(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
) -> Result<(), Box<dyn std::error::Error>> {
    let install_root = storage.paths.games_root.join("astroneer");
    fs::create_dir_all(install_root.join("Astro/Saved/Config/WindowsServer"))?;
    fs::write(
        install_root.join("AstroServer.exe"),
        b"fixture server binary",
    )?;
    fs::write(
        install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini"),
        b"[Fixture]\nValue=preserved\n",
    )?;
    persist_descriptor_install_states(storage, std::slice::from_ref(descriptor)).await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_astroneer_without_save_data_removes_program_and_preserves_native_config()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-uninst");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, descriptor) = installed_astroneer_fixture(&root).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let native_config = install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini");
    let native_config_before = fs::read(&native_config)?;
    assert!(!install_root.join("Astro/Saved/SaveGames").exists());
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    let result = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;

    assert!(matches!(result.install_state, InstallState::NotInstalled));
    assert!(!result.executable_exists);
    assert!(
        !install_root.join("AstroServer.exe").exists(),
        "the command must remove real filesystem fixtures, not just return success"
    );
    assert_eq!(fs::read(&native_config)?, native_config_before);
    assert_eq!(
        fs::read(storage.paths.games_root.join("other-game.sentinel"))?,
        b"keep unrelated files"
    );
    assert!(descriptor.root.join("module.toml").is_file());
    let state = app.state::<DesktopState>();
    let app_state = state.app_state.read().unwrap();
    assert!(
        app_state
            .jobs
            .iter()
            .any(|job| matches!(job.status, JobStatus::Completed))
    );
    assert!(
        app_state
            .modules
            .iter()
            .any(|module| module.id == "astroneer"
                && matches!(module.install_state, InstallState::NotInstalled))
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_astroneer_without_retained_data_removes_install_root()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-no-data");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, _) = installed_astroneer_fixture(&root).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let native_config = install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini");
    assert!(
        native_config
            .canonicalize()?
            .starts_with(root.canonicalize()?)
    );
    assert_eq!(fs::read(&native_config)?, b"[Fixture]\nValue=preserved\n");
    // Remove only the file created by this fixture to model a package with no
    // native settings. Empty directories alone are not retained user data.
    fs::remove_file(&native_config)?;
    assert!(!install_root.join("Astro/Saved/SaveGames").exists());
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    let result = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;

    assert!(matches!(result.install_state, InstallState::NotInstalled));
    assert!(!result.executable_exists);
    assert!(!install_root.exists());
    assert_eq!(
        fs::read(storage.paths.games_root.join("other-game.sentinel"))?,
        b"keep unrelated files"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_astroneer_rejects_nested_instance_root_and_preserves_data()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-nested");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, descriptor) = installed_astroneer_fixture(&root).await?;
    let replacement_games = root.join("replacement-games");
    let install_root = replacement_games.join("astroneer");
    let mut settings = storage.settings;
    settings.servers_root = install_root
        .join("instances")
        .to_string_lossy()
        .into_owned();
    save_app_settings(settings.clone())?;
    let storage = bootstrap_storage()?;
    let instance = create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Nested instance without world".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let config_before = fs::read(&instance.config_file_path)?;
    let runtime_root = Path::new(&instance.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("runtime");
    assert!(runtime_root.join("AstroServer.exe").is_file());
    let runtime_program_before = fs::read(runtime_root.join("AstroServer.exe"))?;
    let instance_before = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let owner_before =
        app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
            .await?
            .expect("private instance has a registered program owner");
    assert_eq!(
        owner_before.install.scope,
        app_storage::ProgramInstallScope::Instance
    );
    assert_eq!(
        owner_before.install.owner_instance_id.as_deref(),
        Some(instance.summary.id.as_str())
    );
    assert_eq!(
        owner_before.install.install_root.canonicalize()?,
        runtime_root.canonicalize()?
    );
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let removed = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;
    assert!(matches!(removed.install_state, InstallState::NotInstalled));
    assert_eq!(fs::read(&instance.config_file_path)?, config_before);
    assert_eq!(
        fs::read(runtime_root.join("AstroServer.exe"))?,
        runtime_program_before
    );
    let instance_after = read_instance_details(&storage.paths, &instance.summary.id).await?;
    assert_eq!(
        serde_json::to_value(instance_after)?,
        serde_json::to_value(&instance_before)?
    );
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    // Retire the original registration before the configured root can select
    // the later package surrounding this private instance.
    settings.games_root = replacement_games.to_string_lossy().into_owned();
    update_app_settings(
        app.state::<DesktopState>(),
        AppPathSettingsInput {
            servers_root: settings.servers_root,
            archives_root: settings.archives_root,
            games_root: settings.games_root,
            steamcmd_root: settings.steamcmd_root,
        },
    )
    .await?;
    let storage = bootstrap_storage()?;
    assert!(storage_context_snapshot_is_current(
        &app.state::<DesktopState>(),
        &storage.settings
    )?);
    assert_eq!(
        resolve_module_install_root(&storage.paths, "astroneer").await?,
        None
    );
    let library_program = b"later library package";
    fs::write(install_root.join("AstroServer.exe"), library_program)?;
    assert!(
        !runtime_root.join("instances").exists(),
        "private runtime must not recursively copy its enclosing instances root"
    );
    assert!(
        !install_root.join("Astro/Saved/SaveGames").exists(),
        "instance creation must not create saves in the shared package"
    );
    let error = uninstall_module_game(app.handle().clone(), "astroneer".into())
        .await
        .expect_err("uninstall must reject an install root containing managed instances");
    assert!(
        error.contains("library path overlaps an instance-owned installation"),
        "{error}"
    );
    assert_eq!(fs::read(&instance.config_file_path)?, config_before);
    assert!(install_root.join("AstroServer.exe").is_file());
    assert!(runtime_root.join("AstroServer.exe").is_file());
    assert_eq!(
        fs::read(install_root.join("AstroServer.exe"))?,
        library_program
    );
    assert_eq!(
        fs::read(runtime_root.join("AstroServer.exe"))?,
        runtime_program_before
    );
    let instance_after = read_instance_details(&storage.paths, &instance.summary.id).await?;
    assert_eq!(
        serde_json::to_value(instance_after)?,
        serde_json::to_value(instance_before)?
    );
    let owner_after =
        app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
            .await?
            .expect("rejected uninstall preserves the instance program owner");
    assert_eq!(owner_after.install.id, owner_before.install.id);
    assert_eq!(owner_after.install.scope, owner_before.install.scope);
    assert_eq!(
        owner_after.install.owner_instance_id,
        owner_before.install.owner_instance_id
    );
    assert_eq!(
        owner_after.install.install_root,
        owner_before.install.install_root
    );
    assert_eq!(
        owner_after.install.install_state,
        owner_before.install.install_state
    );
    assert_eq!(owner_after.runtime_mode, owner_before.runtime_mode);
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_astroneer_with_empty_first_instance_saves_keeps_private_world_and_backup()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-kept");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, descriptor) = installed_astroneer_fixture(&root).await?;
    let first = create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "First empty world".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let first_details = read_instance_details(&storage.paths, &first.summary.id).await?;
    let config_before = fs::read(&first.config_file_path)?;
    fs::create_dir_all(Path::new(&first_details.saves_path).join("empty/nested"))?;
    seed_astroneer_library(&storage, &descriptor).await?;
    let second = create_instance(
        &storage.paths,
        &descriptor,
        CreateInstanceInput {
            name: "Private world".into(),
            module_id: "astroneer".into(),
        },
    )
    .await?;
    let second_details = read_instance_details(&storage.paths, &second.summary.id).await?;
    let private_save = PathBuf::from(&second_details.saves_path).join("retained-world.savegame");
    assert!(private_save.starts_with(&storage.paths.instances_root));
    fs::create_dir_all(private_save.parent().unwrap())?;
    fs::write(&private_save, b"retained private world")?;
    let backup = create_instance_backup_snapshot(&storage.paths, &second.summary.id).await?;
    let backup_file = PathBuf::from(&backup.backup_path).join("saves/retained-world.savegame");
    assert_eq!(fs::read(&backup_file)?, b"retained private world");
    seed_astroneer_library(&storage, &descriptor).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let native_config = install_root.join("Astro/Saved/Config/WindowsServer/Engine.ini");
    let native_config_before = fs::read(&native_config)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    let result = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;

    assert!(matches!(result.install_state, InstallState::NotInstalled));
    assert!(!result.executable_exists);
    assert!(!install_root.join("AstroServer.exe").exists());
    assert_eq!(fs::read(&native_config)?, native_config_before);
    assert_eq!(fs::read(&first.config_file_path)?, config_before);
    assert_eq!(fs::read(&private_save)?, b"retained private world");
    assert_eq!(fs::read(&backup_file)?, b"retained private world");
    assert_eq!(list_instances(&storage.paths).await?.len(), 2);
    assert_eq!(
        list_instance_backups_snapshot(&storage.paths, &second.summary.id)
            .await?
            .len(),
        1
    );
    assert!(descriptor.root.join("module.toml").is_file());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn uninstall_astroneer_preserves_unregistered_world_at_original_path()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("astro-saves");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
    let (storage, _) = installed_astroneer_fixture(&root).await?;
    let install_root = storage.paths.games_root.join("astroneer");
    let save_root = install_root.join("Astro/Saved/SaveGames");
    fs::create_dir_all(&save_root)?;
    fs::write(
        save_root.join("world.savegame"),
        b"retained unregistered world",
    )?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;

    let result = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;
    assert!(matches!(result.install_state, InstallState::NotInstalled));
    assert_eq!(
        fs::read(save_root.join("world.savegame"))?,
        b"retained unregistered world"
    );
    assert!(!install_root.join("AstroServer.exe").exists());
    let repeated = uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;
    assert!(matches!(repeated.install_state, InstallState::NotInstalled));
    assert_eq!(
        fs::read(save_root.join("world.savegame"))?,
        b"retained unregistered world"
    );
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .jobs
            .iter()
            .all(|job| matches!(job.status, JobStatus::Completed))
    );
    Ok(())
}
