#![cfg(test)]
use super::*;
use crate::commands::commands_file_mods::manage_manual_mod;

#[tokio::test(flavor = "current_thread")]
async fn file_mod_commands_manage_private_instance_and_preserve_configuration()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let root = temp_test_dir("file-mods");
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
    let shared_root = PathBuf::from(&settings.games_root).join("sevendaystodie");
    fs::create_dir_all(shared_root.join("Mods/0_TFP_Harmony"))?;
    fs::write(shared_root.join("7DaysToDieServer.exe"), b"package")?;
    fs::write(
        shared_root.join("Mods/0_TFP_Harmony/official.dll"),
        b"official",
    )?;
    record_fake_program_baseline(&settings, "sevendaystodie")?;
    save_app_settings(settings)?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    persist_descriptor_install_states(&storage, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, "sevendaystodie")?;
    let created = app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        CreateInstanceInput {
            name: "File Mods".into(),
            module_id: "sevendaystodie".into(),
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
    let id = created.summary.id;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let source = root.join("OperatorMod");
    fs::create_dir_all(source.join("Config"))?;
    fs::write(
        source.join("ModInfo.xml"),
        b"<xml><ModInfo><Name value=\"Operator\" /></ModInfo></xml>",
    )?;
    fs::write(source.join("plugin.dll"), b"owned-plugin")?;
    fs::write(source.join("Config/options.xml"), b"operator-options")?;
    stage_manual_mod_files(
        app.state::<DesktopState>(),
        id.clone(),
        vec![source.to_string_lossy().into_owned()],
    )
    .await?;
    let inventory = read_manual_mod_inventory(app.state::<DesktopState>(), id.clone()).await?;
    let package = inventory
        .items
        .iter()
        .find(|item| item.name == "OperatorMod")
        .expect("imported Mod");
    let key = package
        .management
        .as_ref()
        .expect("import recorded ownership")
        .package_id
        .clone();
    let active = PathBuf::from(&package.path);
    assert!(!shared_root.join("Mods/OperatorMod").exists());

    let held = app
        .state::<DesktopState>()
        .acquire_instance_mutation(&id)
        .await;
    let handle = app.handle().clone();
    let disabled_id = id.clone();
    let disabled_key = key.clone();
    let mut disable = tokio::spawn(async move {
        manage_manual_mod(
            handle.state::<DesktopState>(),
            disabled_id,
            disabled_key,
            "disable".into(),
        )
        .await
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut disable)
            .await
            .is_err()
    );
    assert!(active.join("plugin.dll").exists());
    drop(held);
    let disabled = command_result(tokio::time::timeout(Duration::from_secs(10), disable).await??)?;
    assert!(!active.exists());
    assert!(
        !disabled
            .inventory
            .items
            .iter()
            .find(|item| item.name == "OperatorMod")
            .unwrap()
            .management
            .as_ref()
            .unwrap()
            .enabled
    );

    // A fresh desktop state reads durable disabled ownership, not an in-memory flag.
    let fresh_app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let after_restart =
        read_manual_mod_inventory(fresh_app.state::<DesktopState>(), id.clone()).await?;
    assert!(
        !after_restart
            .items
            .iter()
            .find(|item| item.name == "OperatorMod")
            .unwrap()
            .management
            .as_ref()
            .unwrap()
            .enabled
    );
    manage_manual_mod(
        app.state::<DesktopState>(),
        id.clone(),
        key.clone(),
        "enable".into(),
    )
    .await?;
    assert_eq!(
        fs::read(active.join("Config/options.xml"))?,
        b"operator-options"
    );
    fs::write(active.join("operator.json"), b"untracked-settings")?;

    let run = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &id,
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
    let error = manage_manual_mod(
        app.state::<DesktopState>(),
        id.clone(),
        key.clone(),
        "delete".into(),
    )
    .await
    .expect_err("running instance rejects deletion");
    assert!(error.contains("Stop instance"), "{error}");
    assert!(active.join("plugin.dll").exists());
    mark_instance_process_stopped(&storage.paths, &id, run.run_id, Some(0), false).await?;

    let deleted = manage_manual_mod(
        app.state::<DesktopState>(),
        id.clone(),
        key,
        "delete".into(),
    )
    .await?;
    assert!(!active.exists());
    assert!(
        !deleted
            .inventory
            .items
            .iter()
            .any(|item| item.name == "OperatorMod")
    );
    let preserved = PathBuf::from(
        deleted
            .preserved_path
            .expect("configuration remains accessible"),
    );
    assert_eq!(
        fs::read(preserved.join("Config/options.xml"))?,
        b"operator-options"
    );
    assert_eq!(
        fs::read(preserved.join("operator.json"))?,
        b"untracked-settings"
    );
    assert!(!preserved.join("plugin.dll").exists());
    assert_eq!(
        fs::read(shared_root.join("Mods/0_TFP_Harmony/official.dll"))?,
        b"official"
    );
    let error = manage_manual_mod(
        app.state::<DesktopState>(),
        id,
        "0_TFP_Harmony".into(),
        "adopt".into(),
    )
    .await
    .expect_err("official Mod directory cannot be adopted");
    assert!(error.contains("protected"), "{error}");
    fs::remove_dir_all(root)?;
    Ok(())
}
