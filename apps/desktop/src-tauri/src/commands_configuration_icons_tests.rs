use super::*;
use base64::Engine;
use std::collections::BTreeMap;

#[tokio::test(flavor = "current_thread")]
async fn configuration_icons_without_artwork_leave_instance_files_untouched()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("dst-setting-icons");
    let env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let result = async {
        let settings = isolated_smoke_app_settings(&run_root)?;
        prepare_fake_dontstarve_install(&settings)?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let provisioning = create_fake_module_instance(
            app.state::<DesktopState>(),
            "dontstarve",
            "DST setting icons",
        )
        .await?;
        let storage = bootstrap_storage()?;
        let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
        let master_path = dontstarve_cluster_root_from_config_file_path(&details.config_file_path)
            .join("Master/worldgenoverride.lua");
        let retained = "return { override_enabled = false }\n";
        fs::write(&master_path, retained)?;
        let (first, second) = tokio::join!(
            read_module_configuration_icons(
                app.state::<DesktopState>(),
                String::from("dontstarve")
            ),
            read_module_configuration_icons(
                app.state::<DesktopState>(),
                String::from("dontstarve")
            ),
        );
        assert!(first?.is_empty());
        assert!(second?.is_empty());
        for module_id in ["minecraft", "../../outside"] {
            let icons = read_module_configuration_icons(
                app.state::<DesktopState>(),
                String::from(module_id),
            )
            .await?;
            assert!(icons.is_empty());
        }
        assert_eq!(fs::read_to_string(&master_path)?, retained);
        assert_eq!(
            read_instance_details(&storage.paths, &provisioning.summary.id)
                .await?
                .settings_json,
            details.settings_json,
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(env_guard);
    let cleanup = fs::remove_dir_all(&run_root);
    result?;
    cleanup?;
    Ok(())
}

fn write_synthetic_icon_cache(app_data_root: &Path) -> std::io::Result<()> {
    let images_root = app_data_root.join("cache/dontstarve-configuration-icons/data/images");
    fs::create_dir_all(&images_root)?;
    // A single RGBA mipmap containing four opaque white pixels, not game artwork.
    let flags: u32 = 0xfffc_0000 | (1 << 13) | (1 << 9) | (4 << 4) | 12;
    let mut texture = b"KTEX".to_vec();
    texture.extend_from_slice(&flags.to_le_bytes());
    for value in [2u16, 2, 8] {
        texture.extend_from_slice(&value.to_le_bytes());
    }
    texture.extend_from_slice(&16u32.to_le_bytes());
    texture.extend_from_slice(&[255; 16]);
    for (atlas, element) in [
        ("worldgen_customization", "world_size.tex"),
        ("worldsettings_customization", "rain.tex"),
    ] {
        let xml = format!(
            "<Atlas><Texture filename=\"{atlas}.tex\"/><Elements>\
             <Element name=\"{element}\" u1=\"0\" u2=\"1\" v1=\"0\" v2=\"1\"/>\
             </Elements></Atlas>"
        );
        fs::write(images_root.join(format!("{atlas}.xml")), xml)?;
        fs::write(images_root.join(format!("{atlas}.tex")), &texture)?;
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn configuration_icon_ipc_reads_persistent_cache_without_changing_instance_or_install()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("dst-cached-icons");
    let env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let result = async {
        let settings = isolated_smoke_app_settings(&run_root)?;
        prepare_fake_dontstarve_install(&settings)?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .invoke_handler(tauri::generate_handler![
                crate::commands::commands_configuration_icons::read_module_configuration_icons
            ])
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let owner = create_fake_module_instance(
            app.state::<DesktopState>(),
            "dontstarve",
            "DST first exclusive program",
        )
        .await?;
        let provisioning = create_fake_module_instance(
            app.state::<DesktopState>(),
            "dontstarve",
            "DST cached setting icons",
        )
        .await?;
        let storage = bootstrap_storage()?;
        assert!(storage.paths.app_data_root.starts_with(&run_root));
        let descriptors = discover_modules(&storage.paths.modules_root)?;
        let descriptor = find_descriptor(&descriptors, "dontstarve")?;
        let owner_program =
            app_storage::read_instance_program_install(&storage.paths, &owner.summary.id)
                .await?
                .unwrap()
                .install;
        assert_eq!(
            owner_program.scope,
            app_storage::ProgramInstallScope::Library
        );
        assert_eq!(
            owner_program.install_root,
            PathBuf::from(&settings.games_root).join("dontstarve")
        );
        app_storage::delete_instance(&storage.paths, &owner.summary.id).await?;
        let record = build_game_install_sync_record(&storage.settings, descriptor, true, None);
        assert_eq!(record.install_state, InstallState::Installed);
        let install_root = record.install_root.clone();
        sync_game_installs(&storage.paths, &[record]).await?;
        let fixture_root = run_root.canonicalize()?;
        let installation = Path::new(&install_root).canonicalize()?;
        assert!(installation.starts_with(&fixture_root));
        let private_runtime = app_storage::resolve_instance_runtime_root(
            &storage.paths.instances_root.join(&provisioning.summary.id),
        )?
        .canonicalize()?;
        assert!(private_runtime.starts_with(&fixture_root));
        assert_ne!(private_runtime, installation);
        let private_executable = private_runtime.join(
            &descriptor
                .process
                .as_ref()
                .expect("DST process contract")
                .executable,
        );
        let retained_program = fs::read(&private_executable)?;
        assert_eq!(
            retained_program,
            fs::read(installation.join(&descriptor.process.as_ref().unwrap().executable))?
        );
        assert_eq!(
            app_storage::read_instance_program_install(&storage.paths, &provisioning.summary.id)
                .await?
                .unwrap()
                .install
                .scope,
            app_storage::ProgramInstallScope::Instance
        );
        let provisioning_artifact = fixture_root.join("provisioning-artifact");
        assert!(!provisioning_artifact.exists());
        fs::rename(&installation, &provisioning_artifact)?;
        assert!(!Path::new(&install_root).exists());
        let record = build_game_install_sync_record(&storage.settings, descriptor, false, None);
        assert_eq!(record.install_state, InstallState::NotInstalled);
        assert_eq!(record.install_root, install_root);
        sync_game_installs(&storage.paths, &[record]).await?;
        let retained_install_root =
            resolve_module_install_root(&storage.paths, "dontstarve").await?;
        assert_eq!(retained_install_root, None);
        let retained_library =
            app_storage::read_library_program_install(&storage.paths, "dontstarve")
                .await?
                .expect("missing library retains its registration");
        assert_eq!(retained_library.install_root, PathBuf::from(&install_root));
        assert_eq!(retained_library.install_state, InstallState::NotInstalled);
        // An empty descriptor batch only queries stored summaries; it does not
        // update module rows or rescan installations before the state assertions.
        let retained_modules = sync_modules(&storage.paths, &[]).await?;
        let retained_module_install_state = &retained_modules
            .iter()
            .find(|module| module.id == "dontstarve")
            .expect("stored DST module")
            .install_state;
        // The independent instance still contributes an installed program.
        assert_eq!(*retained_module_install_state, InstallState::Installed);
        let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
        let master_path = dontstarve_cluster_root_from_config_file_path(&details.config_file_path)
            .join("Master/worldgenoverride.lua");
        let retained_world = "return { override_enabled = false }\n";
        fs::write(&master_path, retained_world)?;
        write_synthetic_icon_cache(&storage.paths.app_data_root)?;
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default()).build()?;
        let request = tauri::webview::InvokeRequest {
            cmd: String::from("read_module_configuration_icons"),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: if cfg!(any(windows, target_os = "android")) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()?,
            body: tauri::ipc::InvokeBody::Json(json!({ "moduleId": "dontstarve" })),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };
        let response =
            tokio::task::spawn_blocking(move || tauri::test::get_ipc_response(&webview, request))
                .await?
                .map_err(|error| {
                    std::io::Error::other(format!("configuration icon IPC failed: {error}"))
                })?;
        let icons = response.deserialize::<BTreeMap<String, String>>()?;
        assert_eq!(
            icons.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "caves_weather",
                "caves_world_size",
                "master_weather",
                "master_world_size"
            ]
        );
        for data_uri in icons.values() {
            let encoded = data_uri
                .strip_prefix("data:image/png;base64,")
                .expect("configuration icon is a PNG data URI");
            let png = base64::engine::general_purpose::STANDARD.decode(encoded)?;
            let image = tauri::image::Image::from_bytes(&png)?;
            assert_eq!((image.width(), image.height()), (2, 2));
            assert_eq!(image.rgba(), &[255; 16]);
        }
        assert_eq!(fs::read_to_string(&master_path)?, retained_world);
        assert_eq!(
            read_instance_details(&storage.paths, &provisioning.summary.id)
                .await?
                .settings_json,
            details.settings_json,
        );
        assert_eq!(
            resolve_module_install_root(&storage.paths, "dontstarve").await?,
            retained_install_root,
        );
        let stored_library =
            app_storage::read_library_program_install(&storage.paths, "dontstarve")
                .await?
                .expect("icon loading preserves the missing library registration");
        assert_eq!(stored_library.id, retained_library.id);
        assert_eq!(stored_library.install_root, retained_library.install_root);
        assert_eq!(stored_library.install_state, retained_library.install_state);
        let stored_modules = sync_modules(&storage.paths, &[]).await?;
        assert_eq!(
            &stored_modules
                .iter()
                .find(|module| module.id == "dontstarve")
                .expect("stored DST module")
                .install_state,
            retained_module_install_state,
        );
        assert!(!Path::new(&install_root).exists());
        assert_eq!(fs::read(&private_executable)?, retained_program);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(env_guard);
    let cleanup = fs::remove_dir_all(&run_root);
    result?;
    cleanup?;
    Ok(())
}
