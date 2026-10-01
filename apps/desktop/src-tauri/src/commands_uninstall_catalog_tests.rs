use super::install_launch_matrix_tests::seed_installed_package;
use super::*;

#[tokio::test(flavor = "current_thread")]
async fn every_catalog_module_uninstalls_and_reinstalls_without_losing_instance_data()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("uninstall-all");
    let _env_guard = ProgramDataEnvGuard::set(&root.join("programdata"));
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
    assert_eq!(
        descriptors.len(),
        32,
        "update the lifecycle matrix when the catalog changes"
    );
    sync_modules(&storage.paths, &descriptors).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let mut retained = Vec::new();
    for descriptor in &descriptors {
        let id = &descriptor.summary.id;
        let install = descriptor
            .install
            .as_ref()
            .expect("catalog install contract");
        let install_root = storage.paths.games_root.join(&install.shared_game_dir);
        seed_installed_package(descriptor, &install_root)?;
        persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await?;
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: format!("Retained {id}"),
                module_id: id.clone(),
            },
            app_storage::InstanceCreationOptions {
                program_mode: Some(app_storage::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| format!("{id}: create: {error}"))?
        .provisioning;
        let private_root = app_storage::resolve_instance_runtime_root(
            &storage.paths.instances_root.join(&created.summary.id),
        )?;
        let private_program = private_root.join(
            install.verification_path.as_deref().unwrap_or(
                &descriptor
                    .process
                    .as_ref()
                    .expect("catalog process contract")
                    .executable,
            ),
        );
        let private_program_bytes = fs::read(&private_program)?;
        // Independent creation may adopt the download. Uninstall exercises a
        // separate library package while the instance retains its own program.
        seed_installed_package(descriptor, &install_root)?;
        persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await?;
        assert_ne!(
            private_root.canonicalize()?,
            install_root.canonicalize()?,
            "{id}: instance must own its program"
        );
        let details = read_instance_details(&storage.paths, &created.summary.id).await?;
        let saves = PathBuf::from(&details.saves_path);
        fs::create_dir_all(&saves)?;
        let sentinel = saves.join("retained-world.sentinel");
        fs::write(&sentinel, id.as_bytes())?;
        let config_before = fs::read(&created.config_file_path)?;
        let mut native_configs = Vec::new();
        for relative in &descriptor.storage.retained_paths {
            let target = install_root.join(relative);
            let target = if target.is_dir() {
                target.join("unmanaged-config.sentinel")
            } else {
                target
            };
            if !target.exists() {
                fs::create_dir_all(target.parent().unwrap())?;
                fs::write(&target, b"unmanaged native configuration")?;
            }
            if id == "runescapedragonwilds" {
                let mut native = fs::read_to_string(&target)?;
                native.push_str("\nServerGuid=retained-guid\nAdminUsers=retained-admin\n");
                fs::write(&target, native)?;
            }
            native_configs.push((target.clone(), fs::read(target)?));
        }

        let result = uninstall_module_game(app.handle().clone(), id.clone())
            .await
            .map_err(|error| format!("{id}: uninstall: {error}"))?;
        assert!(
            matches!(result.install_state, InstallState::NotInstalled),
            "{id}: {result:?}"
        );
        assert!(
            !result.executable_exists,
            "{id}: server executable survived uninstall"
        );
        assert_eq!(
            fs::read(&sentinel)?,
            id.as_bytes(),
            "{id}: save bytes changed"
        );
        assert_eq!(
            fs::read(&created.config_file_path)?,
            config_before,
            "{id}: config changed"
        );
        for (path, expected) in &native_configs {
            assert_eq!(
                fs::read(path)?,
                *expected,
                "{id}: native config changed during uninstall"
            );
        }
        let probe = probe_module_install_state_with_override(
            &storage.settings,
            id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            None,
        );
        assert!(
            matches!(probe.install_state, InstallState::NotInstalled),
            "{id}: refresh changed install state"
        );

        // Package-layout fixtures exercise redetection and retained data, not
        // network acquisition or execution of third-party game binaries.
        seed_installed_package(descriptor, &install_root)?;
        persist_descriptor_install_states(&storage, std::slice::from_ref(descriptor)).await?;
        let probe = probe_module_install_state_with_override(
            &storage.settings,
            id,
            descriptor.summary.steam_app_id,
            descriptor.install.as_ref(),
            descriptor.process.as_ref(),
            None,
        );
        assert!(
            matches!(probe.install_state, InstallState::Installed),
            "{id}: reinstall probe failed"
        );
        let reloaded = read_instance_details(&storage.paths, &created.summary.id).await?;
        assert_eq!(
            Path::new(&reloaded.saves_path),
            saves,
            "{id}: reinstall detached the world"
        );
        assert_eq!(
            fs::read(&sentinel)?,
            id.as_bytes(),
            "{id}: reinstall changed save bytes"
        );
        uninstall_module_game(app.handle().clone(), id.clone())
            .await
            .map_err(|error| format!("{id}: second uninstall: {error}"))?;
        for (path, expected) in native_configs {
            assert_eq!(
                fs::read(path)?,
                expected,
                "{id}: reinstall discarded native config"
            );
        }
        assert_eq!(
            fs::read(&private_program)?,
            private_program_bytes,
            "{id}: library uninstall or reinstall changed the instance program"
        );
        retained.push((sentinel, id.clone()));
    }
    for (path, id) in retained {
        assert_eq!(
            fs::read(path)?,
            id.as_bytes(),
            "{id}: another module removed retained data"
        );
    }
    assert_eq!(
        list_instances(&storage.paths).await?.len(),
        descriptors.len()
    );
    Ok(())
}
