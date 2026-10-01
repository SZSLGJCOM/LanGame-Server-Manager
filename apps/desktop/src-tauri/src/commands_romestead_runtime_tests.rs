use super::*;

struct FixtureRoot(PathBuf);

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn romestead_second_private_instance_resolves_configured_portable_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let root = FixtureRoot(temp_test_dir("romestead-tools"));
    let _environment = ProgramDataEnvGuard::set(&root.0.join("programdata"));
    let tools = root.0.join("configured tools");
    save_app_settings(AppSettings {
        archives_root: String::new(),
        servers_root: root.0.join("instances").to_string_lossy().into_owned(),
        games_root: root
            .0
            .join("unused-default-games")
            .to_string_lossy()
            .into_owned(),
        modules_root: workspace_root()
            .join("modules")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: tools.join("steamcmd").to_string_lossy().into_owned(),
    })?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "romestead")?;
    let install_root = root.0.join("relocated-package");
    fs::create_dir_all(&install_root)?;
    fs::write(install_root.join("Server.exe"), b"synthetic apphost")?;
    fs::write(install_root.join("Server.runtimeconfig.json"), serde_json::json!({
        "runtimeOptions": { "frameworks": [{ "name": "Microsoft.NETCore.App", "version": "8.0.0" }] }
    }).to_string())?;
    app_storage::record_library_program_baseline(&install_root, descriptor, true, None)?;
    let runtime_root = tools.join("dotnet");
    for relative in [
        "dotnet.exe",
        "host/fxr/8.0.28/hostfxr.dll",
        "shared/Microsoft.NETCore.App/8.0.28/coreclr.dll",
        "shared/Microsoft.NETCore.App/8.0.28/hostpolicy.dll",
        "shared/Microsoft.NETCore.App/8.0.28/System.Private.CoreLib.dll",
    ] {
        let path = runtime_root.join(relative);
        fs::create_dir_all(path.parent().ok_or("fixture host parent")?)?;
        fs::write(path, b"synthetic runtime")?;
    }
    let install_record = app_storage::GameInstallSyncRecord {
        module_id: String::from("romestead"),
        install_root: install_root.to_string_lossy().into_owned(),
        install_state: InstallState::Installed,
        current_version: None,
        mark_verified: true,
    };
    app_storage::sync_game_installs(&storage.paths, std::slice::from_ref(&install_record)).await?;
    let original_program = fs::read(install_root.join("Server.exe"))?;
    for name in ["First", "Second"] {
        let created = app_storage::create_instance_with_options(
            &storage.paths,
            descriptor,
            CreateInstanceInput {
                name: name.into(),
                module_id: String::from("romestead"),
            },
            app_storage::InstanceCreationOptions {
                prefer_existing_install: true,
                require_clean_program: true,
                program_mode: Some(app_core::InstanceProgramMode::Independent),
                ..Default::default()
            },
        )
        .await?;
        let details = app_storage::materialize_instance_configuration_for_start(
            &storage.paths,
            &created.provisioning.summary.id,
        )
        .await?;
        let plan = build_instance_launch_preview(&storage.settings, descriptor, &details)?;
        assert!(plan.uses_private_runtime);
        assert_eq!(
            Path::new(&plan.install_root),
            created.effective_install_root
        );
        let binding = app_storage::read_instance_program_install(
            &storage.paths,
            &created.provisioning.summary.id,
        )
        .await?
        .unwrap();
        if name == "First" {
            assert_eq!(
                fs::canonicalize(&created.effective_install_root)?,
                fs::canonicalize(&install_root)?
            );
            assert_eq!(
                binding.install.scope,
                app_storage::ProgramInstallScope::Library
            );
        } else {
            assert_ne!(
                fs::canonicalize(&created.effective_install_root)?,
                fs::canonicalize(&install_root)?
            );
            assert_eq!(
                binding.install.scope,
                app_storage::ProgramInstallScope::Instance
            );
        }
        assert_eq!(
            fs::read(created.effective_install_root.join("Server.exe"))?,
            original_program
        );
        assert_eq!(fs::read(install_root.join("Server.exe"))?, original_program);
        assert_eq!(
            plan.environment.get("DOTNET_ROOT_X64"),
            Some(&runtime_root.to_string_lossy().into_owned())
        );
        assert_eq!(
            plan.environment.get("DOTNET_ROOT"),
            plan.environment.get("DOTNET_ROOT_X64")
        );
        assert!(plan.ready_to_launch, "{:?}", plan.validation_issues);
        let wrapper =
            fs::read_to_string(Path::new(&plan.install_root).join("start-romestead.bat"))?;
        assert!(
            !wrapper.contains("%~dp0.."),
            "wrapper must not infer configured tools from instance depth"
        );
        assert!(wrapper.contains("\"%~dp0Server.exe\" %*"));
    }
    Ok(())
}
