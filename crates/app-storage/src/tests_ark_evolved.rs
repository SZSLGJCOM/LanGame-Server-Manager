use super::super::*;

pub(super) fn ark_test_descriptor(root: &Path) -> ModuleDescriptor {
    ModuleDescriptor {
        root: root.join("modules").join("arksurvivalevolved"),
        manifest_toml: String::from(
            "id = \"arksurvivalevolved\"\nname = \"ARK: Survival Evolved\"\nversion = \"0.1.0\"\nsupported_platforms = [\"windows\"]\n\n[storage]\nsaves_path_template = \"{{paths.install_root}}/ShooterGame/Saved/{{instance.id}}\"\n",
        ),
        schema_json: Some(
            fs::read_to_string(
                repo_root()
                    .join("modules")
                    .join("arksurvivalevolved")
                    .join("schema.json"),
            )
            .unwrap(),
        ),
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 7777,
            },
            PortBinding {
                name: String::from("peer"),
                protocol: String::from("udp"),
                port: 7778,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 27015,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 27020,
            },
        ],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("arksurvivalevolved"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("ShooterGame/Binaries/Win64/ShooterGameServer.exe"),
            args_template: vec![
                String::from("{{arkse.server_url}}"),
                String::from("-NullRHI"),
                String::from("-Unattended"),
                String::from("-NoSplash"),
                String::from("-abslog={{paths.logs_dir}}/ark-evolved-server.log"),
                String::from("{{arkse.multihome_flag}}"),
                String::from("{{arkse.cluster_dir_override_flag}}"),
                String::from("{{arkse.official_launch_flags}}"),
                String::from("{{arkse.custom_launch_flags}}"),
            ],
            working_directory_template: Some(String::from(
                "{{paths.install_root}}/ShooterGame/Binaries/Win64",
            )),
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from(
                "{{paths.install_root}}/ShooterGame/Saved/{{instance.id}}",
            )),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("arksurvivalevolved"),
            name: String::from("ARK: Survival Evolved Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(376030),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

pub(super) fn prepare_ark_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    prepare_shared_install(&paths, descriptor);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(&templates_root).unwrap();
    let repo_templates_root = repo_root()
        .join("modules")
        .join("arksurvivalevolved")
        .join("templates");
    for entry in fs::read_dir(repo_templates_root).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        if source_path.is_file() {
            fs::copy(source_path, templates_root.join(entry.file_name())).unwrap();
        }
    }
}

#[tokio::test]
async fn ark_instance_materializes_live_server_config() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = ark_test_descriptor(&root);
    prepare_ark_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("arkse");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("arksurvivalevolved"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("ark-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("ARK Alpha"),
            module_id: String::from("arksurvivalevolved"),
        },
    )
    .await
    .unwrap();

    assert_eq!(created.summary.port_count, 4);

    let details =
        configure_test_instance_runtime(&paths, &created.summary.id, "192.168.50.10", false).await;
    let expected_saves_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Saved");
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let live_saved_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Saved");
    let live_config_root = live_saved_root.join("Config").join("WindowsServer");
    let live_binary_root = instance_private_runtime_root(&created)
        .join("ShooterGame")
        .join("Binaries")
        .join("Win64");
    let game_user_settings_text =
        fs::read_to_string(config_root.join("GameUserSettings.ini")).unwrap();
    let game_ini_text = fs::read_to_string(config_root.join("Game.ini")).unwrap();
    let live_game_user_settings_text =
        fs::read_to_string(live_config_root.join("GameUserSettings.ini")).unwrap();
    let live_game_ini_text = fs::read_to_string(live_config_root.join("Game.ini")).unwrap();

    assert!(game_user_settings_text.contains("[ServerSettings]"));
    let settings: serde_json::Value = serde_json::from_str(&details.settings_json).unwrap();
    let admin_password = settings["admin_password"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(admin_password).is_ok());
    assert!(game_user_settings_text.contains(&format!("ServerAdminPassword={admin_password}")));
    assert!(game_user_settings_text.contains("ServerPassword="));
    assert!(game_user_settings_text.contains("RCONEnabled=true"));
    assert!(game_user_settings_text.contains("RCONPort=27020"));
    assert!(game_user_settings_text.contains("serverPVE=true"));
    assert!(
        !game_user_settings_text.contains("ActiveMods="),
        "an empty Mods selection must not emit a synthetic ActiveMods assignment"
    );
    assert!(game_user_settings_text.contains("AutoSavePeriodMinutes=15"));
    assert!(game_user_settings_text.contains("noTributeDownloads=false"));
    assert!(game_user_settings_text.contains("[SessionSettings]"));
    assert!(game_user_settings_text.contains("Port=7777"));
    assert!(game_user_settings_text.contains("QueryPort=27015"));
    assert!(game_user_settings_text.contains("SessionName=ARK Alpha"));
    assert!(game_user_settings_text.contains("[/Script/Engine.GameSession]"));
    assert!(game_user_settings_text.contains("MaxPlayers=20"));

    assert!(game_ini_text.contains("[/script/shootergame.shootergamemode]"));
    assert!(game_ini_text.contains("ResourceNoReplenishRadiusPlayers=1"));
    assert!(game_ini_text.contains("MatingIntervalMultiplier=1"));
    assert_eq!(live_game_user_settings_text, game_user_settings_text);
    assert_eq!(live_game_ini_text, game_ini_text);
    assert_eq!(
        fs::read_to_string(live_saved_root.join("AllowedCheaterSteamIDs.txt")).unwrap(),
        fs::read_to_string(config_root.join("AllowedCheaterSteamIDs.txt")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(live_binary_root.join("PlayersExclusiveJoinList.txt")).unwrap(),
        fs::read_to_string(config_root.join("PlayersExclusiveJoinList.txt")).unwrap()
    );
    assert_eq!(
        fs::read_to_string(live_binary_root.join("PlayersJoinNoCheckList.txt")).unwrap(),
        fs::read_to_string(config_root.join("PlayersJoinNoCheckList.txt")).unwrap()
    );
    assert!(
        !config_root.join("launch-arkse.bat").exists(),
        "ARK: Survival Evolved should no longer render a generated launch script"
    );

    cleanup_root(&root);
}
