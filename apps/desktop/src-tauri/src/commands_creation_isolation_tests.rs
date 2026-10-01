use super::*;
use std::collections::BTreeMap;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const TEST_PASSWORD: &str = "isolated-test-password-12345678";

fn headless_app() -> TestResult<tauri::App<tauri::test::MockRuntime>> {
    Ok(tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?)
}

fn files(root: &Path) -> TestResult<BTreeMap<PathBuf, Vec<u8>>> {
    fn collect(root: &Path, path: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) -> TestResult {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            assert!(
                !kind.is_symlink(),
                "fixture must not traverse external links"
            );
            if kind.is_dir() {
                collect(root, &entry.path(), result)?;
            } else {
                result.insert(
                    entry.path().strip_prefix(root)?.to_owned(),
                    fs::read(entry.path())?,
                );
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result)?;
    Ok(result)
}

fn instance_root(details: &InstanceDetails) -> &Path {
    Path::new(&details.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
}

async fn change_settings(
    app: &tauri::App<tauri::test::MockRuntime>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    label: &str,
    slots: u32,
) -> TestResult<InstanceDetails> {
    let mut settings: Value = serde_json::from_str(&details.settings_json)?;
    let (name, password) = if details.summary.module_id == "astroneer" {
        ("server_name", "server_password")
    } else {
        ("motd", "rcon_password")
    };
    settings[name] = json!(label);
    settings[password] = json!(TEST_PASSWORD);
    settings["max_players"] = json!(slots);
    update_instance_record_if_current(
        app.state::<DesktopState>(),
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: settings.to_string(),
            ports: details.ports.clone(),
        },
        details.settings_json.clone(),
    )
    .await?;
    // Exercise the same materialization used before launch, without executing
    // the synthetic game binary or connecting to any game provider.
    Ok(
        app_storage::materialize_instance_configuration(&storage.paths, &details.summary.id)
            .await?,
    )
}

async fn verify_two_instances(module_id: &str, mode: app_core::InstanceProgramMode) -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let library = storage.paths.games_root.join(module_id);
    if module_id == "minecraft" {
        fs::create_dir_all(library.join("jre/bin"))?;
        fs::write(
            library.join("server.jar"),
            b"synthetic server jar, never executed",
        )?;
        fs::write(
            library.join("jre/bin/java.exe"),
            b"synthetic java, never executed",
        )?;
        let descriptors = discover_modules(&storage.paths.modules_root)?;
        app_storage::record_library_program_baseline(
            &library,
            find_descriptor(&descriptors, module_id)?,
            true,
            None,
        )?;
        app_storage::sync_game_installs(
            &storage.paths,
            &[app_storage::GameInstallSyncRecord {
                module_id: module_id.into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: true,
            }],
        )
        .await?;
    }
    let app = headless_app()?;
    let create_input = |name: &str| CreateInstanceInput {
        name: name.into(),
        module_id: module_id.into(),
    };
    let first = create_instance_record(
        app.state::<DesktopState>(),
        create_input("Isolation first"),
        Some(mode),
    )
    .await?;
    let before = read_instance_details(&storage.paths, &first.summary.id).await?;
    let defaults: Value = serde_json::from_str(&before.settings_json)?;
    let first = change_settings(&app, &storage, &before, "First customized", 4).await?;
    let first_root = instance_root(&first).to_owned();
    let first_program = app_storage::resolve_instance_runtime_root(&first_root)?;
    let first_owner = app_storage::read_instance_program_install(&storage.paths, &first.summary.id)
        .await?
        .unwrap();
    assert_eq!(
        fs::canonicalize(&first_program)?,
        fs::canonicalize(&library)?
    );
    let config_relative = if module_id == "astroneer" {
        Path::new("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini")
    } else {
        Path::new("server.properties")
    };
    let first_native = if module_id == "astroneer" {
        first_program.join(config_relative)
    } else {
        first_root.join(config_relative)
    };
    let native_text = fs::read_to_string(&first_native)?;
    assert!(native_text.contains("First customized"));
    assert!(native_text.contains(TEST_PASSWORD));
    if module_id == "astroneer" {
        assert!(
            fs::read_to_string(first_program.join("Astro/Saved/Config/WindowsServer/Game.ini"))?
                .contains("MaxPlayers=4")
        );
    } else {
        assert!(native_text.contains("max-players=4"));
    }
    // Model game-generated data and an operator's direct native-file edit.
    fs::write(
        &first_native,
        format!("{native_text}\n# first-instance-only-native-edit\n"),
    )?;
    let save_relative = Path::new("nested/first-world.sav");
    let first_save = Path::new(&first.saves_path).join(save_relative);
    fs::create_dir_all(first_save.parent().unwrap())?;
    fs::write(&first_save, b"first instance world bytes")?;
    let first_mod = if module_id == "astroneer" {
        first_program.join("Astro/Saved/Paks/first-only.pak")
    } else {
        first_root.join("mods/first-only.jar")
    };
    fs::create_dir_all(first_mod.parent().unwrap())?;
    fs::write(&first_mod, b"first instance mod bytes")?;
    fs::create_dir_all(first_root.join("logs"))?;
    fs::write(
        first_root.join("logs/first-only.log"),
        b"first instance log",
    )?;
    let first_tree = files(&first_root)?;
    let first_program_tree = files(&first_program)?;
    let first_saved_settings = fs::read(&first.config_file_path)?;

    // Recreate desktop state to ensure isolation comes from persisted ownership,
    // not in-memory knowledge of the first creation. Neither instance is archived.
    drop(app);
    let app = headless_app()?;
    let second = create_instance_record(
        app.state::<DesktopState>(),
        create_input("Isolation second"),
        Some(mode),
    )
    .await?;
    let second =
        app_storage::materialize_instance_configuration(&storage.paths, &second.summary.id).await?;
    let second_root = instance_root(&second).to_owned();
    let second_program = app_storage::resolve_instance_runtime_root(&second_root)?;
    let second_settings: Value = serde_json::from_str(&second.settings_json)?;
    let name = if module_id == "astroneer" {
        "server_name"
    } else {
        "motd"
    };
    let password = if module_id == "astroneer" {
        "server_password"
    } else {
        "rcon_password"
    };
    assert_eq!(second_settings[name], json!("Isolation second"));
    assert_eq!(second_settings["max_players"], defaults["max_players"]);
    assert_ne!(second_settings[password], json!(TEST_PASSWORD));
    assert_ne!(first.summary.id, second.summary.id);
    assert_ne!(
        fs::canonicalize(&first_root)?,
        fs::canonicalize(&second_root)?
    );
    assert_ne!(first.saves_path, second.saves_path);
    let second_data_root = if module_id == "astroneer" {
        &second_program
    } else {
        &second_root
    };
    assert!(Path::new(&second.saves_path).starts_with(second_data_root));
    assert!(!Path::new(&second.saves_path).join(save_relative).exists());
    let second_native = if module_id == "astroneer" {
        second_program.join(config_relative)
    } else {
        second_root.join(config_relative)
    };
    let second_native_text = fs::read_to_string(second_native)?;
    assert!(second_native_text.contains("Isolation second"));
    for marker in [
        "First customized",
        TEST_PASSWORD,
        "first-instance-only-native-edit",
    ] {
        assert!(
            !second_native_text.contains(marker),
            "inherited native config: {marker}"
        );
    }
    assert!(!second_root.join("logs/first-only.log").exists());
    let second_mod = if module_id == "astroneer" {
        second_program.join("Astro/Saved/Paks/first-only.pak")
    } else {
        second_root.join("mods/first-only.jar")
    };
    assert!(!second_mod.exists());
    for port in &first.ports {
        assert!(
            !second
                .ports
                .iter()
                .any(|other| other.protocol == port.protocol && other.port == port.port)
        );
    }
    let second_owner =
        app_storage::read_instance_program_install(&storage.paths, &second.summary.id)
            .await?
            .unwrap();
    if mode == app_core::InstanceProgramMode::Shared {
        assert_eq!(first_owner.install.id, second_owner.install.id);
        assert_eq!(
            fs::canonicalize(&first_program)?,
            fs::canonicalize(&second_program)?
        );
    } else {
        assert_ne!(first_owner.install.id, second_owner.install.id);
        assert_ne!(
            fs::canonicalize(&first_program)?,
            fs::canonicalize(&second_program)?
        );
        assert_eq!(
            second_owner.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
    }
    let executable = if module_id == "astroneer" {
        "AstroServer.exe"
    } else {
        "server.jar"
    };
    assert_eq!(
        fs::read(first_program.join(executable))?,
        fs::read(second_program.join(executable))?
    );
    assert!(
        files(&first_root)? == first_tree,
        "second creation changed first instance files"
    );
    assert!(
        files(&first_program)? == first_program_tree,
        "second creation changed first program files"
    );
    assert_eq!(fs::read(&first.config_file_path)?, first_saved_settings);
    assert_eq!(fs::read(&first_save)?, b"first instance world bytes");

    // Mutating the second after creation must still leave the first unchanged.
    change_settings(&app, &storage, &second, "Second customized", 3).await?;
    fs::create_dir_all(Path::new(&second.saves_path).join("nested"))?;
    fs::write(
        Path::new(&second.saves_path).join(save_relative),
        b"second instance world bytes",
    )?;
    assert!(
        files(&first_root)? == first_tree,
        "second update changed first instance files"
    );
    assert!(
        files(&first_program)? == first_program_tree,
        "second update changed first program files"
    );
    let reopened = read_instance_details(&storage.paths, &first.summary.id).await?;
    assert_eq!(reopened.settings_json, first.settings_json);
    assert_eq!(reopened.summary.bind_ip, first.summary.bind_ip);
    assert_eq!(
        serde_json::to_value(&reopened.ports)?,
        serde_json::to_value(&first.ports)?
    );
    let owner = app_storage::read_instance_program_install(&storage.paths, &first.summary.id)
        .await?
        .unwrap();
    assert_eq!(owner.install.id, first_owner.install.id);
    assert_eq!(owner.install.install_root, first_owner.install.install_root);
    assert_eq!(list_instances(&storage.paths).await?.len(), 2);
    assert!(
        app_storage::list_instance_archives(&storage.paths)
            .await?
            .archives
            .is_empty()
    );
    assert!(!storage.paths.steamcmd_root.join("steamcmd.exe").exists());
    println!(
        "HEADLESS_CREATION_ISOLATION module={module_id} mode={mode:?} instances=2 first_preserved=true second_config=fresh save_and_mod_leak=false ports=distinct state_recreated_readback=passed"
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn headless_second_astroneer_instance_keeps_first_config_and_saves_isolated() -> TestResult {
    verify_two_instances("astroneer", app_core::InstanceProgramMode::Independent).await
}

#[tokio::test(flavor = "current_thread")]
async fn headless_second_minecraft_independent_instance_keeps_first_config_and_saves_isolated()
-> TestResult {
    verify_two_instances("minecraft", app_core::InstanceProgramMode::Independent).await
}

#[tokio::test(flavor = "current_thread")]
async fn headless_second_minecraft_shared_instance_keeps_first_config_and_saves_isolated()
-> TestResult {
    verify_two_instances("minecraft", app_core::InstanceProgramMode::Shared).await
}
