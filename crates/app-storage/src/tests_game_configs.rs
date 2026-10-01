use super::*;

#[path = "tests_configuration_concurrency.rs"]
mod configuration_concurrency;

#[path = "tests_enshrouded_save.rs"]
mod enshrouded_save_tests;

fn meaningful_ini_lines(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn barotrauma_test_descriptor(root: &Path) -> ModuleDescriptor {
    let repo_barotrauma_root = repo_root().join("modules").join("barotrauma");

    ModuleDescriptor {
        root: root.join("modules").join("barotrauma"),
        manifest_toml: fs::read_to_string(repo_barotrauma_root.join("module.toml")).unwrap(),
        schema_json: Some(fs::read_to_string(repo_barotrauma_root.join("schema.json")).unwrap()),
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 27015,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 27016,
            },
        ],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("barotrauma"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: Some(String::from("DedicatedServer.exe")),
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("DedicatedServer.exe"),
            args_template: vec![String::from("{{settings.extra_launch_args}}")],
            working_directory_template: Some(String::from("{{paths.config_dir}}")),
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from("{{paths.config_dir}}/Multiplayer")),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("barotrauma"),
            name: String::from("Barotrauma Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(1026340),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_barotrauma_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(templates_root.join("Data")).unwrap();
    let repo_templates_root = repo_root()
        .join("modules")
        .join("barotrauma")
        .join("templates");
    fs::copy(
        repo_templates_root.join("serversettings.xml.hbs"),
        templates_root.join("serversettings.xml.hbs"),
    )
    .unwrap();
    fs::copy(
        repo_templates_root
            .join("Data")
            .join("clientpermissions.xml.hbs"),
        templates_root
            .join("Data")
            .join("clientpermissions.xml.hbs"),
    )
    .unwrap();
}

#[tokio::test]
async fn barotrauma_instance_materializes_workshop_content_packages() {
    let root = unique_test_root();
    let mut paths = test_paths(&root);
    paths.instances_root = root.join("instances & shared space");
    let descriptor = barotrauma_test_descriptor(&root);
    prepare_barotrauma_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("barotrauma");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("DedicatedServer.exe"), "").unwrap();
    fs::create_dir_all(install_root.join("Content")).unwrap();
    fs::create_dir_all(install_root.join("Data")).unwrap();
    fs::write(
        install_root.join("config_player.xml"),
        "<config>\n  <contentpackages>\n    <corepackage path=\"Content/ContentPackages/Vanilla.xml\" />\n    <regularpackages />\n  </contentpackages>\n</config>\n",
    )
    .unwrap();
    let workshop_root = paths
        .steamcmd_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("602960");
    fs::create_dir_all(workshop_root.join("123456789")).unwrap();
    fs::write(
        workshop_root.join("123456789").join("filelist.xml"),
        "<contentpackage name=\"Abyss Pack\" />",
    )
    .unwrap();
    fs::write(
        workshop_root.join("123456789").join("submarine.sub"),
        "submarine data",
    )
    .unwrap();

    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("barotrauma"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("barotrauma-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Barotrauma Abyss"),
            module_id: String::from("barotrauma"),
        },
    )
    .await
    .unwrap();

    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("0.0.0.0"),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: String::from(
                r#"{
  "server_name": "Barotrauma Abyss",
  "mod_workshop_ids": "123456789\nmissing"
}"#,
            ),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let config_root = paths
        .instances_root
        .join(&created.summary.id)
        .join("config");
    let local_mod_root = config_root.join("LocalMods").join("123456789");
    assert_eq!(
        fs::read_to_string(local_mod_root.join("filelist.xml")).unwrap(),
        "<contentpackage name=\"Abyss Pack\" />"
    );
    assert_eq!(
        fs::read_to_string(local_mod_root.join("submarine.sub")).unwrap(),
        "submarine data"
    );

    let config_player = fs::read_to_string(config_root.join("config_player.xml")).unwrap();
    let escaped_save_root = config_root.to_string_lossy().replace('&', "&amp;");
    assert!(config_player.contains(&format!("savepath=\"{escaped_save_root}\"")));
    assert!(!config_player.contains("savepath=\"shared\""));
    assert!(config_player.contains("<regularpackages>"));
    assert!(config_player.contains("path=\"LocalMods/123456789/filelist.xml\""));
    assert!(!config_player.contains("LocalMods/missing/filelist.xml"));

    cleanup_root(&root);
}

fn conan_test_descriptor(root: &Path) -> ModuleDescriptor {
    let repo_conan_root = repo_root().join("modules").join("conanexiles");

    ModuleDescriptor {
        root: root.join("modules").join("conanexiles"),
        manifest_toml: fs::read_to_string(repo_conan_root.join("module.toml")).unwrap(),
        schema_json: Some(fs::read_to_string(repo_conan_root.join("schema.json")).unwrap()),
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 7777,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 27015,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 25575,
            },
        ],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("conanexiles"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from(
                "ConanSandbox/Binaries/Win64/ConanSandboxServer-Win64-Shipping.exe",
            ),
            args_template: vec![
                String::from("ConanSandbox?listen"),
                String::from("-Port={{ports.game.port}}"),
                String::from("-QueryPort={{ports.query.port}}"),
                String::from("-MaxPlayers={{settings.max_players}}"),
                String::from("-ServerName={{settings.server_name}}"),
                String::from("-RconPort={{ports.rcon.port}}"),
                String::from("-server"),
                String::from("-NullRHI"),
                String::from("-Unattended"),
                String::from("-NoSplash"),
                String::from("-abslog={{paths.logs_dir}}/conan-server.log"),
                String::from("-useallavailablecores"),
                String::from("{{conanexiles.custom_launch_flags}}"),
                String::from("-MULTIHOME={{instance.bind_ip}}"),
                String::from("-MULTIHOMEHTTP={{instance.bind_ip}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from("{{paths.install_root}}/ConanSandbox/Saved")),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("conanexiles"),
            name: String::from("Conan Exiles Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(443030),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_conan_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
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
        .join("conanexiles")
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
async fn conan_instance_materializes_live_server_config_and_declares_saved_root() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = conan_test_descriptor(&root);
    prepare_conan_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("conanexiles");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("conanexiles"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("conan-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Conan Ember"),
            module_id: String::from("conanexiles"),
        },
    )
    .await
    .unwrap();

    assert_eq!(created.summary.port_count, 3);

    let details =
        configure_test_instance_runtime(&paths, &created.summary.id, "192.168.1.44", false).await;
    let expected_saves_root = instance_private_runtime_root(&created)
        .join("ConanSandbox")
        .join("Saved");
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let live_config_root = instance_private_runtime_root(&created)
        .join("ConanSandbox")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let engine_ini_text = fs::read_to_string(config_root.join("Engine.ini")).unwrap();
    let game_ini_text = fs::read_to_string(config_root.join("Game.ini")).unwrap();
    let server_settings_text = fs::read_to_string(config_root.join("ServerSettings.ini")).unwrap();
    let live_engine_ini_text = fs::read_to_string(live_config_root.join("Engine.ini")).unwrap();
    let live_game_ini_text = fs::read_to_string(live_config_root.join("Game.ini")).unwrap();
    let live_server_settings_text =
        fs::read_to_string(live_config_root.join("ServerSettings.ini")).unwrap();

    assert!(engine_ini_text.contains("[URL]"));
    assert!(engine_ini_text.contains("Port=7777"));
    assert!(engine_ini_text.contains("GameServerQueryPort=27015"));
    assert!(engine_ini_text.contains("ServerDefaultMap=ConanSandbox"));
    assert!(engine_ini_text.contains("ExcludedRegions="));
    assert!(engine_ini_text.contains("ServerPassword="));
    assert!(game_ini_text.contains("[RconPlugin]"));
    assert!(game_ini_text.contains("RconEnabled=true"));
    let generated_settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    let rcon_password = generated_settings["rcon_password"].as_str().unwrap();
    let admin_password = generated_settings["admin_password"].as_str().unwrap();
    assert_eq!(rcon_password.len(), 32);
    assert_eq!(admin_password.len(), 32);
    assert!(game_ini_text.contains(&format!("RconPassword={rcon_password}")));
    assert!(game_ini_text.contains("RconPort=25575"));
    assert!(game_ini_text.contains("RconMaxKarma=60"));
    assert!(game_ini_text.contains("MaxPlayers=20"));
    assert!(server_settings_text.contains(&format!("AdminPassword={admin_password}")));
    assert!(server_settings_text.contains("ServerName=Conan Ember"));
    assert!(server_settings_text.contains("PVPEnabled=false"));
    assert!(server_settings_text.contains("IsBattlEyeEnabled=true"));
    assert!(server_settings_text.contains("PlayerXPRateMultiplier=1"));
    assert!(server_settings_text.contains("EnablePurge=true"));
    assert!(server_settings_text.contains("PurgeLevel=6"));
    assert_eq!(
        meaningful_ini_lines(&live_engine_ini_text),
        meaningful_ini_lines(&engine_ini_text)
    );
    assert_eq!(
        meaningful_ini_lines(&live_game_ini_text),
        meaningful_ini_lines(&game_ini_text)
    );
    assert_eq!(
        meaningful_ini_lines(&live_server_settings_text),
        meaningful_ini_lines(&server_settings_text)
    );
    assert!(
        !config_root.join("launch-conanexiles.bat").exists(),
        "Conan Exiles should no longer render a generated launch script"
    );

    let workshop_root = paths
        .steamcmd_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("440900");
    fs::create_dir_all(workshop_root.join("111111111")).unwrap();
    fs::create_dir_all(workshop_root.join("222222222")).unwrap();
    fs::write(
        workshop_root.join("111111111").join("Pippi.pak"),
        "pippi pak",
    )
    .unwrap();
    fs::write(
        workshop_root.join("222222222").join("Fashionist.pak"),
        "fashionist pak",
    )
    .unwrap();

    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("192.168.1.44"),
            auto_backup_on_stop: false,
            backup_retention_count: 10,
            settings_json: String::from(
                r#"{
  "server_name": "Conan Ember",
  "max_players": 20,
  "admin_password": "change-me-admin",
  "mod_workshop_ids": "111111111\n222222222\nmissing"
}"#,
            ),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let live_mods_root = instance_private_runtime_root(&created)
        .join("ConanSandbox")
        .join("Mods");
    assert_eq!(
        fs::read_to_string(live_mods_root.join("modlist.txt")).unwrap(),
        "Pippi.pak\nFashionist.pak"
    );
    assert_eq!(
        fs::read_to_string(live_mods_root.join("Pippi.pak")).unwrap(),
        "pippi pak"
    );
    assert_eq!(
        fs::read_to_string(live_mods_root.join("Fashionist.pak")).unwrap(),
        "fashionist pak"
    );

    cleanup_root(&root);
}

fn terraria_test_descriptor(root: &Path) -> ModuleDescriptor {
    let repo_terraria_root = repo_root().join("modules").join("terraria");

    ModuleDescriptor {
        root: root.join("modules").join("terraria"),
        manifest_toml: fs::read_to_string(repo_terraria_root.join("module.toml")).unwrap(),
        schema_json: Some(fs::read_to_string(repo_terraria_root.join("schema.json")).unwrap()),
        default_ports: vec![PortBinding {
            name: String::from("game"),
            protocol: String::from("tcp"),
            port: 7777,
        }],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("terraria"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("TerrariaServer.exe"),
            args_template: vec![
                String::from("-config"),
                String::from("{{paths.config_dir}}/serverconfig.txt"),
                String::from("-ip"),
                String::from("{{instance.bind_ip}}"),
            ],
            working_directory_template: None,
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from("{{paths.instance_root}}/saves")),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("terraria"),
            name: String::from("Terraria Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(105610),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_terraria_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    prepare_shared_install(&paths, descriptor);
    let repo_terraria_templates = repo_root()
        .join("modules")
        .join("terraria")
        .join("templates");
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(&templates_root).unwrap();

    for template_name in ["serverconfig.txt.hbs", "banlist.txt.hbs"] {
        fs::copy(
            repo_terraria_templates.join(template_name),
            templates_root.join(template_name),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn terraria_instance_renders_server_config_file() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = terraria_test_descriptor(&root);
    prepare_terraria_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Terraria Meadow"),
            module_id: String::from("terraria"),
        },
    )
    .await
    .unwrap();

    configure_test_instance_runtime(&paths, &created.summary.id, "10.10.0.25", false).await;

    assert_eq!(created.summary.port_count, 1);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let server_config_text = fs::read_to_string(config_root.join("serverconfig.txt")).unwrap();
    let banlist_text = fs::read_to_string(config_root.join("banlist.txt")).unwrap();

    assert!(server_config_text.contains("# Managed by LanGame Server Manager."));
    assert!(server_config_text.contains(&format!(
            "world={}\\world.wld",
            root.join("instances")
                .join(&created.summary.id)
                .join("saves")
                .to_string_lossy()
        )));
    assert!(server_config_text.contains("autocreate=2"));
    assert!(server_config_text.contains("worldname=Terraria Meadow"));
    assert!(server_config_text.contains("difficulty=0"));
    assert!(server_config_text.contains("maxplayers=8"));
    assert!(server_config_text.contains("port=7777"));
    assert!(server_config_text.contains("motd=Welcome to the server!"));
    assert!(server_config_text.contains("worldrollbackstokeep=2"));
    assert!(server_config_text.contains("language=en-US"));
    assert!(server_config_text.contains("npcstream=60"));
    assert!(server_config_text.contains("priority=1"));
    assert!(server_config_text.contains(&format!(
        "banlist={}\\banlist.txt",
        config_root.to_string_lossy()
    )));
    assert_eq!(banlist_text.trim_end(), "");

    cleanup_root(&root);
}

#[tokio::test]
async fn terraria_instance_renders_optional_server_flags() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = terraria_test_descriptor(&root);
    prepare_terraria_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Terraria Journey"),
            module_id: String::from("terraria"),
        },
    )
    .await
    .unwrap();

    let updated = update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("192.168.10.40"),
            auto_backup_on_stop: true,
            backup_retention_count: 6,
            settings_json: String::from(
                r#"{
  "world_name": "Journey Grove",
  "world_file": "journey-grove.wld",
  "world_size": 3,
  "difficulty": 3,
  "seed": "LanGameJourney",
  "special_seed": "remix",
  "max_players": 12,
  "motd": "Bring wiring tools.",
  "worldrollbackstokeep": 5,
  "password": "greenleaf",
  "secure": true,
  "steam": true,
  "lobby": "friends",
  "banlist_entries": "BadActor42\n203.0.113.24",
  "language": "zh-Hans",
  "upnp": true,
  "npcstream": 90,
  "slowliquids": true,
  "disableannouncementbox": true,
  "announcementboxrange": -1,
  "priority": 2,
  "journeypermission_time_setfrozen": 1,
  "journeypermission_setdifficulty": 2,
  "journeypermission_godmode": 1
}"#,
            ),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let server_config_text = fs::read_to_string(config_root.join("serverconfig.txt")).unwrap();
    let banlist_text = fs::read_to_string(config_root.join("banlist.txt")).unwrap();

    assert_eq!(updated.summary.bind_ip, "192.168.10.40");
    assert!(server_config_text.contains("worldname=Journey Grove"));
    assert!(server_config_text.contains("world="));
    assert!(server_config_text.contains("\\journey-grove.wld"));
    assert!(server_config_text.contains("difficulty=3"));
    assert!(server_config_text.contains("maxplayers=12"));
    assert!(server_config_text.contains("motd=Bring wiring tools."));
    assert!(server_config_text.contains("worldrollbackstokeep=5"));
    assert!(server_config_text.contains("seed=LanGameJourney"));
    assert!(server_config_text.contains("seed_remix=1"));
    assert!(server_config_text.contains("password=greenleaf"));
    assert!(server_config_text.contains("secure=1"));
    assert!(server_config_text.contains("steam=1"));
    assert!(server_config_text.contains("lobby=friends"));
    assert!(server_config_text.contains("language=zh-Hans"));
    assert!(server_config_text.contains("upnp=1"));
    assert!(server_config_text.contains("npcstream=90"));
    assert!(server_config_text.contains("disableannouncementbox=1"));
    assert!(server_config_text.contains("announcementboxrange=-1"));
    assert!(server_config_text.contains("priority=2"));
    assert!(server_config_text.contains("slowliquids=1"));
    assert!(server_config_text.contains("journeypermission_time_setfrozen=1"));
    assert!(server_config_text.contains("journeypermission_setdifficulty=2"));
    assert!(server_config_text.contains("journeypermission_godmode=1"));
    assert_eq!(banlist_text.trim_end(), "BadActor42\n203.0.113.24");

    cleanup_root(&root);
}

#[tokio::test]
async fn terraria_instance_materializes_tmodloader_workshop_modpack_files() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = terraria_test_descriptor(&root);
    prepare_terraria_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Terraria Workshop"),
            module_id: String::from("terraria"),
        },
    )
    .await
    .unwrap();

    update_instance(
        &paths,
        UpdateInstanceInput {
            id: created.summary.id.clone(),
            bind_ip: String::from("127.0.0.1"),
            auto_backup_on_stop: true,
            backup_retention_count: 3,
            settings_json: String::from(
                r#"{
  "server_runtime": "tmodloader",
  "tmodloader_workshop_item_ids": "https://steamcommunity.com/sharedfiles/filedetails/?id=2824688072\nworkshop-2563309347\n2824688072\nnot-a-workshop",
  "tmodloader_enabled_mod_names": "CalamityMod\nMagicStorage\nCalamityMod\n# comment\n-- comment"
}"#,
            ),
            ports: created.ports.clone(),
        },
    )
    .await
    .unwrap();

    let instance_root = root.join("instances").join(&created.summary.id);
    let tmodloader_root = instance_root.join("tmodloader");
    let mods_root = tmodloader_root.join("Mods");
    let workshop_root = tmodloader_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("1281930");

    assert_eq!(
        fs::read_to_string(mods_root.join("install.txt")).unwrap(),
        "2824688072\n2563309347\n"
    );

    let enabled_mods_text = fs::read_to_string(mods_root.join("enabled.json")).unwrap();
    let enabled_mods: serde_json::Value = serde_json::from_str(&enabled_mods_text).unwrap();
    assert_eq!(
        enabled_mods,
        serde_json::json!(["CalamityMod", "MagicStorage"])
    );
    assert!(workshop_root.is_dir());

    cleanup_root(&root);
}

fn palworld_test_descriptor(root: &Path) -> ModuleDescriptor {
    let repo_palworld_root = repo_root().join("modules").join("palworld");

    ModuleDescriptor {
        root: root.join("modules").join("palworld"),
        manifest_toml: fs::read_to_string(repo_palworld_root.join("module.toml")).unwrap(),
        schema_json: Some(fs::read_to_string(repo_palworld_root.join("schema.json")).unwrap()),
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 8211,
            },
            PortBinding {
                name: String::from("rcon"),
                protocol: String::from("tcp"),
                port: 25575,
            },
            PortBinding {
                name: String::from("rest_api"),
                protocol: String::from("tcp"),
                port: 8212,
            },
        ],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("palworld"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"),
            args_template: vec![
                String::from("-port={{ports.game.port}}"),
                String::from("-players={{settings.max_players}}"),
                String::from("-logformat={{settings.log_format}}"),
                String::from("{{palworld.public_lobby_flag}}"),
                String::from("{{palworld.public_ip_flag}}"),
                String::from("{{palworld.public_port_flag}}"),
                String::from("{{palworld.use_perf_threads_flag}}"),
                String::from("{{palworld.no_async_loading_thread_flag}}"),
                String::from("{{palworld.use_multithread_for_ds_flag}}"),
                String::from("{{palworld.worker_thread_count_flag}}"),
            ],
            working_directory_template: Some(String::from(
                "{{paths.install_root}}/Pal/Binaries/Win64",
            )),
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from(
                "{{paths.install_root}}/Pal/Saved/SaveGames/0/{{instance.id}}",
            )),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("palworld"),
            name: String::from("Palworld Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(2394010),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_palworld_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    let repo_palworld_templates = repo_root()
        .join("modules")
        .join("palworld")
        .join("templates");
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(&templates_root).unwrap();

    for template_name in ["GameUserSettings.ini.hbs", "PalWorldSettings.ini.hbs"] {
        fs::copy(
            repo_palworld_templates.join(template_name),
            templates_root.join(template_name),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn palworld_instance_renders_windows_server_config() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = palworld_test_descriptor(&root);
    prepare_palworld_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("palworld");
    let pal_root = install_root.join("Pal");
    fs::create_dir_all(pal_root.join("Binaries").join("Win64")).unwrap();
    fs::create_dir_all(pal_root.join("Saved").join("Config").join("WindowsServer")).unwrap();
    fs::write(
        pal_root
            .join("Binaries")
            .join("Win64")
            .join("PalServer-Win64-Shipping-Cmd.exe"),
        [],
    )
    .unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("palworld"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("palworld-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Palworld Azure"),
            module_id: String::from("palworld"),
        },
    )
    .await
    .unwrap();

    assert_eq!(created.summary.port_count, 3);

    let details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let expected_saves_root = instance_private_runtime_root(&created)
        .join("Pal")
        .join("Saved")
        .join("SaveGames")
        .join("0")
        .join(&created.summary.id);
    assert_eq!(PathBuf::from(&details.saves_path), expected_saves_root);
    assert!(details.backup_uses_declared_saves_path);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let game_user_settings_text =
        fs::read_to_string(config_root.join("GameUserSettings.ini")).unwrap();
    let palworld_settings_text =
        fs::read_to_string(config_root.join("PalWorldSettings.ini")).unwrap();
    let materialized_config_root = instance_private_runtime_root(&created)
        .join("Pal")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let materialized_game_user_settings =
        fs::read_to_string(materialized_config_root.join("GameUserSettings.ini")).unwrap();
    let materialized_palworld_settings =
        fs::read_to_string(materialized_config_root.join("PalWorldSettings.ini")).unwrap();

    assert!(game_user_settings_text.contains("[/Script/Pal.PalGameLocalSettings]"));
    assert!(
        game_user_settings_text.contains(&format!("DedicatedServerName={}", created.summary.id))
    );
    assert!(palworld_settings_text.contains("[/Script/Pal.PalGameWorldSettings]"));
    assert!(palworld_settings_text.contains("ServerName=\"Palworld Azure\""));
    assert!(palworld_settings_text.contains("ServerDescription=\"\""));
    let settings: serde_json::Value = serde_json::from_str(&details.settings_json).unwrap();
    let admin_password = settings["admin_password"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(admin_password).is_ok());
    assert!(palworld_settings_text.contains(&format!("AdminPassword=\"{admin_password}\"")));
    assert!(palworld_settings_text.contains("ServerPassword=\"\""));
    assert!(palworld_settings_text.contains("ServerPlayerMaxNum=32"));
    assert!(palworld_settings_text.contains("PublicPort=8211"));
    assert!(palworld_settings_text.contains("CrossplayPlatforms=(Steam,Xbox,PS5,Mac)"));
    assert!(palworld_settings_text.contains("bUseAuth=True"));
    assert!(palworld_settings_text.contains("RCONPort=25575"));
    assert!(palworld_settings_text.contains("RESTAPIPort=8212"));
    assert!(palworld_settings_text.contains("LogFormatType=Text"));
    assert!(palworld_settings_text.contains("bEnableFastTravel=True"));
    assert!(palworld_settings_text.contains("DeathPenalty=Item"));
    assert!(palworld_settings_text.contains("bIsStartLocationSelectByMap=False"));
    assert!(palworld_settings_text.contains("PalEggDefaultHatchingTime=1.0"));
    assert!(palworld_settings_text.contains("bAllowClientMod=True"));
    assert!(palworld_settings_text.contains("bShowPlayerList=False"));
    assert!(
        palworld_settings_text
            .contains("BanListURL=\"https://b.palworldgame.com/api/banlist.txt\"")
    );
    assert!(palworld_settings_text.contains("BaseCampMaxNumInGuild=4"));
    assert!(palworld_settings_text.contains("BaseCampWorkerMaxNum=15"));
    assert!(palworld_settings_text.contains("BaseCampMaxNum=128"));
    assert!(palworld_settings_text.contains("BuildObjectHpRate=1.0"));
    assert!(palworld_settings_text.contains("ItemContainerForceMarkDirtyInterval=1.0"));
    assert!(palworld_settings_text.contains("PlayerDataPalStorageUpdateCheckTickInterval=1.0"));
    assert!(palworld_settings_text.contains("ItemCorruptionMultiplier=1.0"));
    assert!(palworld_settings_text.contains("MonsterFarmActionSpeedRate=1.0"));
    assert!(palworld_settings_text.contains("RandomizerSeed=\"\""));
    assert!(palworld_settings_text.contains("RespawnPenaltyDurationThreshold=0.0"));
    assert!(palworld_settings_text.contains("RespawnPenaltyTimeScale=2.0"));
    assert!(
        palworld_settings_text
            .contains("AdditionalDropItemWhenPlayerKillingInPvPMode=\"PlayerDropItem\"")
    );
    assert!(palworld_settings_text.contains("AdditionalDropItemNumWhenPlayerKillingInPvPMode=1"));
    assert!(palworld_settings_text.contains("bAdditionalDropItemWhenPlayerKillingInPvPMode=False"));
    assert!(palworld_settings_text.contains("EnablePredatorBossPal=True"));
    assert!(palworld_settings_text.contains("PhysicsActiveDropItemMaxNum=-1"));
    assert!(palworld_settings_text.contains("AutoTransferMasterCheckIntervalSeconds=3600.0"));
    assert!(palworld_settings_text.contains("AutoTransferMasterThresholdDays=14"));
    assert!(palworld_settings_text.contains("MaxGuildsPerFrame=10"));
    assert!(palworld_settings_text.contains("bEnableVoiceChat=False"));
    assert!(palworld_settings_text.contains("VoiceChatMaxVolumeDistance=3000.0"));
    assert!(palworld_settings_text.contains("VoiceChatZeroVolumeDistance=15000.0"));
    assert!(palworld_settings_text.contains("bEnableBuildingPlayerUIdDisplay=False"));
    assert!(palworld_settings_text.contains("BuildingNameDisplayCacheTTLSeconds=60"));

    assert!(!palworld_settings_text.contains("CoopPlayerMaxNum="));
    assert!(!palworld_settings_text.contains("bIsMultiplay="));
    assert!(!palworld_settings_text.contains("Difficulty="));
    assert!(!palworld_settings_text.contains("bEnableDefenseOtherGuildPlayer="));
    assert!(!palworld_settings_text.contains("bEnableNonLoginPenalty="));
    let rendered_option_keys = parse_palworld_option_keys(&palworld_settings_text);
    let expected_option_keys = EXPECTED_PALWORLD_OPTION_KEYS
        .iter()
        .map(|key| String::from(*key))
        .collect::<BTreeSet<_>>();
    let missing_option_keys = expected_option_keys
        .difference(&rendered_option_keys)
        .cloned()
        .collect::<Vec<_>>();
    let extra_option_keys = rendered_option_keys
        .difference(&expected_option_keys)
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        missing_option_keys.is_empty() && extra_option_keys.is_empty(),
        "Palworld OptionSettings drifted from the official default keyset. Missing: {:?}. Extra: {:?}",
        missing_option_keys,
        extra_option_keys
    );

    assert_eq!(materialized_game_user_settings, game_user_settings_text);
    assert_eq!(materialized_palworld_settings, palworld_settings_text);
    assert!(
        !config_root.join("launch-palworld.bat").exists(),
        "Palworld should no longer materialize a generated batch script in the instance config root"
    );

    cleanup_root(&root);
}
fn valheim_test_descriptor(root: &Path) -> ModuleDescriptor {
    ModuleDescriptor {
        root: root.join("modules").join("valheim"),
        manifest_toml: String::from(
            "id = \"valheim\"\nname = \"Valheim\"\nversion = \"0.1.0\"\nsupported_platforms = [\"windows\"]\n\n[storage]\nsaves_path_template = \"{{paths.instance_root}}/saves\"\n",
        ),
        schema_json: Some(
            r#"{
                    "type": "object",
                    "properties": {
                        "server_name": { "type": "string", "default": "Valheim Server", "x-lsgm-default-source": "instance_name" },
                        "world_name": { "type": "string", "default": "Dedicated", "x-lsgm-default-source": "instance_name" },
                        "server_password": { "type": "string", "default": "change-me" },
                        "public_server": { "type": "integer", "default": 1 },
                        "crossplay_enabled": { "type": "boolean", "default": false },
                        "save_interval_seconds": { "type": "integer", "default": 1800 },
                        "backup_count": { "type": "integer", "default": 4 },
                        "backup_short_seconds": { "type": "integer", "default": 7200 },
                        "backup_long_seconds": { "type": "integer", "default": 43200 },
                        "admin_list": { "type": "string", "default": "" },
                        "banned_list": { "type": "string", "default": "" },
                        "permitted_list": { "type": "string", "default": "" }
                    }
                }"#
            .to_string(),
        ),
        default_ports: vec![
            PortBinding {
                name: String::from("game"),
                protocol: String::from("udp"),
                port: 2456,
            },
            PortBinding {
                name: String::from("query"),
                protocol: String::from("udp"),
                port: 2457,
            },
        ],
        install: Some(app_core::InstallSpec {
            shared_game_dir: String::from("valheim"),
            download_url_windows: None,
            download_integrity_windows: None,
            source: None,
            verification_path: None,
            minecraft: None,
        }),
        process: Some(app_core::ProcessSpec {
            environment_template: Default::default(),
            executable: String::from("valheim_server.exe"),
            args_template: vec![
                String::from("-nographics"),
                String::from("-batchmode"),
                String::from("-name"),
                String::from("{{settings.server_name}}"),
                String::from("-port"),
                String::from("{{ports.game.port}}"),
                String::from("-world"),
                String::from("{{settings.world_name}}"),
                String::from("-password"),
                String::from("{{settings.server_password}}"),
                String::from("-savedir"),
                String::from("{{paths.saves_dir}}"),
                String::from("-public"),
                String::from("{{settings.public_server}}"),
                String::from("-saveinterval"),
                String::from("{{settings.save_interval_seconds}}"),
                String::from("-backups"),
                String::from("{{settings.backup_count}}"),
                String::from("-backupshort"),
                String::from("{{settings.backup_short_seconds}}"),
                String::from("-backuplong"),
                String::from("{{settings.backup_long_seconds}}"),
                String::from("{{valheim.crossplay_flag}}"),
            ],
            working_directory_template: Some(String::from("{{paths.install_root}}")),
            window_policy: app_core::ProcessWindowPolicy::Background,
            host_surface: app_core::ProcessHostSurface::ManagedTerminal,
            host_notes: None,
        }),
        workshop: None,
        runtime: app_core::ModuleRuntimeSpec::default(),
        storage: app_modules::ModuleStorageSpec {
            saves_path_template: Some(String::from("{{paths.instance_root}}/saves")),
            ..Default::default()
        },
        summary: ModuleSummary {
            id: String::from("valheim"),
            name: String::from("Valheim Dedicated Server"),
            version: String::from("0.1.0"),
            description: Some(String::from("Test module")),
            steam_app_id: Some(896660),
            install_state: InstallState::NotInstalled,
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![String::from("windows")],
        },
    }
}

fn prepare_valheim_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    fs::create_dir_all(&templates_root).unwrap();
    fs::write(templates_root.join("adminlist.txt.hbs"), "{{admin_list}}\n").unwrap();
    fs::write(
        templates_root.join("bannedlist.txt.hbs"),
        "{{banned_list}}\n",
    )
    .unwrap();
    fs::write(
        templates_root.join("permittedlist.txt.hbs"),
        "{{permitted_list}}\n",
    )
    .unwrap();
}

#[tokio::test]
async fn valheim_instance_materializes_support_lists_into_save_root() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = valheim_test_descriptor(&root);
    prepare_valheim_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("valheim");
    fs::create_dir_all(&install_root).unwrap();
    fs::write(install_root.join("valheim_server.exe"), []).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("valheim"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("valheim-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Valheim Ridge"),
            module_id: String::from("valheim"),
        },
    )
    .await
    .unwrap();

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let admin_list_text = fs::read_to_string(config_root.join("adminlist.txt")).unwrap();
    let banned_list_text = fs::read_to_string(config_root.join("bannedlist.txt")).unwrap();
    let permitted_list_text = fs::read_to_string(config_root.join("permittedlist.txt")).unwrap();
    let saves_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("saves");
    let materialized_admin_list = fs::read_to_string(saves_root.join("adminlist.txt")).unwrap();
    let materialized_banned_list = fs::read_to_string(saves_root.join("bannedlist.txt")).unwrap();
    let materialized_permitted_list =
        fs::read_to_string(saves_root.join("permittedlist.txt")).unwrap();

    assert_eq!(admin_list_text, "\n");
    assert_eq!(banned_list_text, "\n");
    assert_eq!(permitted_list_text, "\n");
    assert_eq!(materialized_admin_list, admin_list_text);
    assert_eq!(materialized_banned_list, banned_list_text);
    assert_eq!(materialized_permitted_list, permitted_list_text);
    assert!(
        !config_root.join("launch-valheim.bat").exists(),
        "Valheim should no longer materialize a generated launch batch script"
    );

    cleanup_root(&root);
}

fn prepare_enshrouded_environment(root: &Path) -> ModuleDescriptor {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();

    let repo_module_root = repo_root().join("modules").join("enshrouded");
    let target_module_root = paths.modules_root.join("enshrouded");
    let target_templates_root = target_module_root.join("templates");
    fs::create_dir_all(&target_templates_root).unwrap();
    fs::write(
        target_module_root.join("module.toml"),
        fs::read_to_string(repo_module_root.join("module.toml")).unwrap(),
    )
    .unwrap();
    fs::write(
        target_module_root.join("schema.json"),
        fs::read_to_string(repo_module_root.join("schema.json")).unwrap(),
    )
    .unwrap();
    fs::write(
        target_templates_root.join("enshrouded_server.json.hbs"),
        fs::read_to_string(
            repo_module_root
                .join("templates")
                .join("enshrouded_server.json.hbs"),
        )
        .unwrap(),
    )
    .unwrap();
    app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "enshrouded")
        .expect("enshrouded module descriptor")
}

#[tokio::test]
async fn enshrouded_instance_renders_full_server_config_and_roles() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = prepare_enshrouded_environment(&root);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = paths.games_root.join("enshrouded");
    fs::create_dir_all(&install_root).unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Cinder Hollow"),
            module_id: String::from("enshrouded"),
        },
    )
    .await
    .unwrap();

    let initial_details =
        configure_test_instance_runtime(&paths, &created.summary.id, "192.168.50.10", false).await;
    let expected_saves_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("savegame");
    assert_eq!(
        PathBuf::from(&initial_details.saves_path),
        expected_saves_root
    );
    assert!(
        initial_details.backup_uses_declared_saves_path,
        "Enshrouded should declare its live save root after provisioning"
    );
    assert!(
        initial_details
            .settings_json
            .contains("\"server_name\": \"Cinder Hollow\""),
        "server_name default was not written:\n{}",
        initial_details.settings_json
    );
    assert!(
        initial_details
            .settings_json
            .contains("\"game_settings_preset\": \"Default\""),
        "game_settings_preset default was not written:\n{}",
        initial_details.settings_json
    );
    let initial_settings: serde_json::Value =
        serde_json::from_str(&initial_details.settings_json).unwrap();
    let role_passwords = [
        "admin_password",
        "friend_password",
        "guest_password",
        "visitor_password",
    ]
    .map(|key| initial_settings[key].as_str().unwrap());
    for password in role_passwords {
        assert!(uuid::Uuid::parse_str(password).is_ok());
    }
    assert_eq!(
        role_passwords
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        4
    );

    let mut updated_settings_value: serde_json::Value =
        serde_json::from_str(&initial_details.settings_json).unwrap();
    let updated_settings = updated_settings_value
        .as_object_mut()
        .expect("enshrouded settings should be a JSON object");
    updated_settings.insert(
        String::from("server_name"),
        serde_json::Value::String(String::from("Fog Keep \"A\"")),
    );
    updated_settings.insert(
        String::from("max_players"),
        serde_json::Value::Number(8.into()),
    );
    updated_settings.insert(
        String::from("server_tags"),
        serde_json::Value::String(String::from("Chinese\nLookingForPlayers\nExploration")),
    );
    updated_settings.insert(
        String::from("voice_chat_mode"),
        serde_json::Value::String(String::from("Global")),
    );
    updated_settings.insert(
        String::from("enable_voice_chat"),
        serde_json::Value::Bool(true),
    );
    updated_settings.insert(
        String::from("enable_text_chat"),
        serde_json::Value::Bool(true),
    );
    updated_settings.insert(
        String::from("game_settings_preset"),
        serde_json::Value::String(String::from("Custom")),
    );
    updated_settings.insert(
        String::from("player_health_factor"),
        serde_json::json!(1.25),
    );
    updated_settings.insert(String::from("boss_health_factor"), serde_json::json!(1.75));
    updated_settings.insert(
        String::from("day_time_minutes"),
        serde_json::Value::Number(42.into()),
    );
    updated_settings.insert(
        String::from("night_time_minutes"),
        serde_json::Value::Number(17.into()),
    );
    updated_settings.insert(
        String::from("hunger_to_starving_minutes"),
        serde_json::Value::Number(13.into()),
    );
    updated_settings.insert(
        String::from("admin_password"),
        serde_json::Value::String(String::from("torch-admin")),
    );
    updated_settings.insert(
        String::from("friend_password"),
        serde_json::Value::String(String::new()),
    );
    for permission in [
        "friend_can_access_inventories",
        "friend_can_edit_world",
        "friend_can_edit_base",
    ] {
        updated_settings.insert(String::from(permission), serde_json::Value::Bool(false));
    }
    updated_settings.insert(
        String::from("guest_can_edit_world"),
        serde_json::Value::Bool(false),
    );
    updated_settings.insert(
        String::from("visitor_reserved_slots"),
        serde_json::Value::Number(2.into()),
    );
    updated_settings.insert(
        String::from("custom_user_groups_json"),
        serde_json::json!([
            {
                "name": "Helper",
                "password": "helper-pass",
                "canKickBan": false,
                "canAccessInventories": false,
                "canEditWorld": true,
                "canEditBase": false,
                "canExtendBase": false,
                "reservedSlots": 1
            }
        ])
        .to_string()
        .into(),
    );
    updated_settings.insert(
        String::from("banned_player_ids"),
        serde_json::Value::String(String::from("76561198000000001\n76561198000000002")),
    );

    let details = update_instance(
        &paths,
        UpdateInstanceInput {
            id: initial_details.summary.id.clone(),
            bind_ip: initial_details.summary.bind_ip.clone(),
            auto_backup_on_stop: initial_details.auto_backup_on_stop,
            backup_retention_count: initial_details.backup_retention_count,
            settings_json: serde_json::to_string_pretty(&updated_settings_value).unwrap(),
            ports: initial_details.ports.clone(),
        },
    )
    .await
    .unwrap();

    let config_root = PathBuf::from(&details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .expect("enshrouded config root");
    let config_path = config_root.join("enshrouded_server.json");
    let install_config_path =
        instance_private_runtime_root(&created).join("enshrouded_server.json");
    assert!(
        config_path.exists(),
        "Enshrouded config file was not rendered:\n{}",
        config_path.display()
    );
    assert_file_has_no_utf8_bom(&config_path);

    let config_text = fs::read_to_string(&config_path).unwrap();
    let config_json: serde_json::Value = serde_json::from_str(&config_text).unwrap();

    assert_eq!(config_json["name"], "Fog Keep \"A\"");
    assert_eq!(
        config_json["saveDirectory"],
        expected_saves_root.to_string_lossy().into_owned()
    );
    assert_eq!(
        config_json["logDirectory"],
        root.join("instances")
            .join(&created.summary.id)
            .join("logs")
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(config_json["ip"], "192.168.50.10");
    assert_eq!(config_json["queryPort"], 15637);
    assert_eq!(config_json["slotCount"], 8);
    assert_eq!(
        config_json["tags"],
        serde_json::json!(["Chinese", "LookingForPlayers", "Exploration"])
    );
    assert_eq!(config_json["voiceChatMode"], "Global");
    assert_eq!(config_json["enableVoiceChat"], true);
    assert_eq!(config_json["enableTextChat"], true);
    assert_eq!(config_json["gameSettingsPreset"], "Custom");
    assert_eq!(config_json["gameSettings"]["playerHealthFactor"], 1.25);
    assert_eq!(config_json["gameSettings"]["bossHealthFactor"], 1.75);
    assert_eq!(
        config_json["gameSettings"]["fromHungerToStarving"],
        serde_json::json!(780_000_000_000u64)
    );
    assert_eq!(
        config_json["gameSettings"]["dayTimeDuration"],
        serde_json::json!(2_520_000_000_000u64)
    );
    assert_eq!(
        config_json["gameSettings"]["nightTimeDuration"],
        serde_json::json!(1_020_000_000_000u64)
    );
    assert_eq!(config_json["userGroups"][0]["name"], "Admin");
    assert_eq!(config_json["userGroups"][0]["password"], "torch-admin");
    assert_eq!(config_json["userGroups"][1]["name"], "Friend");
    assert_eq!(config_json["userGroups"][1]["password"], "");
    assert_eq!(config_json["userGroups"][1]["canEditWorld"], false);
    assert_eq!(config_json["userGroups"][2]["name"], "Guest");
    assert_eq!(config_json["userGroups"][2]["canEditWorld"], false);
    assert_eq!(config_json["userGroups"][3]["name"], "Visitor");
    assert_eq!(config_json["userGroups"][3]["reservedSlots"], 2);
    assert_eq!(config_json["userGroups"][4]["name"], "Helper");
    assert_eq!(config_json["userGroups"][4]["password"], "helper-pass");
    assert_eq!(config_json["userGroups"][4]["reservedSlots"], 1);
    assert_eq!(
        config_json["bans"],
        serde_json::json!([
            {
                "accountId": 76561198000000001u64,
                "displayName": "",
                "characterName": "",
                "banDate": { "value": 0u64 }
            },
            {
                "accountId": 76561198000000002u64,
                "displayName": "",
                "characterName": "",
                "banDate": { "value": 0u64 }
            }
        ])
    );
    assert!(
        config_json.get("bannedAccounts").is_none(),
        "Current official Enshrouded config examples use the top-level bans array"
    );

    let live_config_json: Value =
        serde_json::from_str(&fs::read_to_string(&install_config_path).unwrap()).unwrap();
    assert_eq!(
        live_config_json, config_json,
        "Enshrouded should materialize the live install-root config before launch"
    );
    assert!(
        !config_root.join("launch-enshrouded.bat").exists(),
        "Enshrouded should no longer render a generated launch script"
    );

    cleanup_root(&root);
}

fn prepare_abioticfactor_environment(root: &Path) -> ModuleDescriptor {
    let paths = test_paths(root);
    fs::create_dir_all(paths.games_root.join("abioticfactor")).unwrap();
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();

    let repo_module_root = repo_root().join("modules").join("abioticfactor");
    let target_module_root = paths.modules_root.join("abioticfactor");
    let target_templates_root = target_module_root.join("templates");
    fs::create_dir_all(&target_templates_root).unwrap();
    fs::write(
        target_module_root.join("module.toml"),
        fs::read_to_string(repo_module_root.join("module.toml")).unwrap(),
    )
    .unwrap();
    fs::write(
        target_module_root.join("schema.json"),
        fs::read_to_string(repo_module_root.join("schema.json")).unwrap(),
    )
    .unwrap();

    for template_name in ["SandboxSettings.ini.hbs", "Admin.ini.hbs"] {
        fs::write(
            target_templates_root.join(template_name),
            fs::read_to_string(repo_module_root.join("templates").join(template_name)).unwrap(),
        )
        .unwrap();
    }

    app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "abioticfactor")
        .expect("abiotic factor module descriptor")
}

#[tokio::test]
async fn abioticfactor_instance_renders_server_files_and_declared_world_path() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = prepare_abioticfactor_environment(&root);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("abioticfactor");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("abioticfactor"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("abioticfactor-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Facility Alpha"),
            module_id: String::from("abioticfactor"),
        },
    )
    .await
    .unwrap();

    let initial_details = read_instance_details(&paths, &created.summary.id)
        .await
        .unwrap();
    let expected_initial_saves_root = instance_private_runtime_root(&created)
        .join("AbioticFactor")
        .join("Saved")
        .join("SaveGames")
        .join("Server")
        .join("Worlds")
        .join(&created.summary.id);
    assert_eq!(
        PathBuf::from(&initial_details.saves_path),
        expected_initial_saves_root
    );
    assert!(
        !expected_initial_saves_root.exists(),
        "Abiotic Factor should not pre-create an empty world folder before first launch"
    );
    assert!(
        expected_initial_saves_root
            .parent()
            .expect("world path should have a parent")
            .exists(),
        "Abiotic Factor should still materialize its private Worlds parent directory"
    );
    assert!(
        initial_details.backup_uses_declared_saves_path,
        "Abiotic Factor should back up the declared live world path"
    );
    assert!(
        initial_details
            .settings_json
            .contains(&format!("\"world_save_name\": \"{}\"", created.summary.id)),
        "world_save_name default was not written:\n{}",
        initial_details.settings_json
    );
    assert!(
        initial_details
            .settings_json
            .contains("\"server_name\": \"Facility Alpha\""),
        "server_name default was not written:\n{}",
        initial_details.settings_json
    );

    let mut updated_settings_value: serde_json::Value =
        serde_json::from_str(&initial_details.settings_json).unwrap();
    let updated_settings = updated_settings_value
        .as_object_mut()
        .expect("abiotic factor settings should be a JSON object");
    updated_settings.insert(
        String::from("server_name"),
        serde_json::Value::String(String::from("Sector Prime")),
    );
    updated_settings.insert(
        String::from("world_save_name"),
        serde_json::Value::String(String::from("sector-prime")),
    );
    updated_settings.insert(String::from("max_server_players"), serde_json::json!(12));
    updated_settings.insert(
        String::from("server_password"),
        serde_json::Value::String(String::from("join-me")),
    );
    updated_settings.insert(
        String::from("admin_password"),
        serde_json::Value::String(String::from("top-secret-admin")),
    );
    updated_settings.insert(
        String::from("moderator_steam_ids"),
        serde_json::Value::String(String::from(
            "76561198077777777\n# comment\n76561198000000000\n76561198077777777",
        )),
    );
    updated_settings.insert(String::from("lan_only"), serde_json::Value::Bool(true));
    updated_settings.insert(
        String::from("platform_limited"),
        serde_json::Value::String(String::from("PC")),
    );
    updated_settings.insert(
        String::from("multihome_address"),
        serde_json::Value::String(String::from("192.168.50.10")),
    );
    updated_settings.insert(String::from("use_local_ips"), serde_json::Value::Bool(true));
    updated_settings.insert(
        String::from("use_perf_threads"),
        serde_json::Value::Bool(true),
    );
    updated_settings.insert(
        String::from("disable_async_loading_thread"),
        serde_json::Value::Bool(true),
    );
    updated_settings.insert(String::from("hardcore_mode"), serde_json::Value::Bool(true));
    updated_settings.insert(
        String::from("loot_respawn_enabled"),
        serde_json::Value::Bool(true),
    );
    updated_settings.insert(
        String::from("power_sockets_off_at_night"),
        serde_json::Value::Bool(false),
    );
    updated_settings.insert(String::from("day_night_cycle_state"), serde_json::json!(2));
    updated_settings.insert(String::from("weather_frequency"), serde_json::json!(5));
    updated_settings.insert(
        String::from("allow_recipe_sharing"),
        serde_json::Value::Bool(false),
    );
    updated_settings.insert(
        String::from("radiation_deals_damage"),
        serde_json::Value::Bool(true),
    );

    let updated_details = update_instance(
        &paths,
        UpdateInstanceInput {
            id: initial_details.summary.id.clone(),
            bind_ip: String::from("192.168.50.10"),
            auto_backup_on_stop: true,
            backup_retention_count: 6,
            settings_json: serde_json::to_string_pretty(&updated_settings_value).unwrap(),
            ports: initial_details.ports.clone(),
        },
    )
    .await
    .unwrap();

    let expected_updated_saves_root = instance_private_runtime_root(&created)
        .join("AbioticFactor")
        .join("Saved")
        .join("SaveGames")
        .join("Server")
        .join("Worlds")
        .join("sector-prime");
    assert_eq!(
        PathBuf::from(&updated_details.saves_path),
        expected_updated_saves_root
    );
    assert!(
        updated_details.backup_uses_declared_saves_path,
        "Abiotic Factor backups should keep following the declared world root after updates"
    );

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .expect("abiotic factor config root");
    let sandbox_path = config_root.join("SandboxSettings.ini");
    let admin_path = config_root.join("Admin.ini");
    let live_saved_root = instance_private_runtime_root(&created)
        .join("AbioticFactor")
        .join("Saved");
    let live_sandbox_path = live_saved_root
        .join("Config")
        .join("WindowsServer")
        .join("LanGame")
        .join(format!("{}-SandboxSettings.ini", created.summary.id));
    let live_admin_path = live_saved_root
        .join("SaveGames")
        .join("Server")
        .join("LanGame")
        .join(format!("{}-Admin.ini", created.summary.id));
    assert!(sandbox_path.exists(), "missing sandbox config");
    assert!(admin_path.exists(), "missing admin config");
    assert_file_has_no_utf8_bom(&sandbox_path);
    assert_file_has_no_utf8_bom(&admin_path);

    let sandbox_text = fs::read_to_string(&sandbox_path).unwrap();
    let admin_text = fs::read_to_string(&admin_path).unwrap();

    assert!(sandbox_text.contains("[SandboxSettings]"));
    assert!(sandbox_text.contains("GameDifficulty=1"));
    assert!(sandbox_text.contains("HardcoreMode=True"));
    assert!(sandbox_text.contains("LootRespawnEnabled=True"));
    assert!(sandbox_text.contains("PowerSocketsOffAtNight=False"));
    assert!(sandbox_text.contains("DayNightCycleState=2"));
    assert!(sandbox_text.contains("WeatherFrequency=5"));
    assert!(sandbox_text.contains("AllowRecipeSharing=False"));
    assert!(sandbox_text.contains("RadiationDealsDamage=True"));
    assert!(sandbox_text.contains("EnemySpawnRate=1"));
    assert!(sandbox_text.contains("ShowDeathMessages=True"));

    assert_eq!(
        admin_text,
        "[Moderators]\nModerator=76561198077777777\nModerator=76561198000000000\n"
    );
    assert_eq!(fs::read_to_string(live_sandbox_path).unwrap(), sandbox_text);
    assert_eq!(fs::read_to_string(live_admin_path).unwrap(), admin_text);
    assert!(
        !config_root.join("launch-abioticfactor.bat").exists(),
        "Abiotic Factor should no longer render a generated launch script"
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn abioticfactor_runtime_overview_marks_live_session_as_ready() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = prepare_abioticfactor_environment(&root);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Abiotic Runtime"),
            module_id: String::from("abioticfactor"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-ready.log");
    fs::write(
        &log_path,
        concat!(
            "LogAbioticRemoteConsole: Warning: Remote console through HTTPS is explicitly disabled.\n",
            "LogOnline: Warning: OSS: [FOnlineVoiceEOSPlus::Initialize] BaseVoiceInterface delegates not bound. Base interface not valid\n",
            "LogAbiotic: Display: Dedicated Server entered ServerEntry, checking world save for corruption\n",
            "LogAbiotic: Warning: Dedicated Server could not find any files for the save, this is fine if it's a new save.\n",
            "LogNet: Name:GameNetDriver NetDriverEOS_2147482350 IpNetDriver listening on port 7777\n",
            "LogAbiotic: Warning: Session short code: XGSJP\n",
            "LogOnlineSession: Warning: STEAM: Can't start an online game for session (GameSession) that hasn't been created\n",
        ),
    )
    .unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        7788,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "ready");
    assert!(
        overview
            .health
            .summary
            .to_ascii_lowercase()
            .contains("session short code"),
        "unexpected runtime health summary: {}",
        overview.health.summary
    );
    assert!(
        overview
            .health
            .matched_line
            .as_deref()
            .unwrap_or_default()
            .contains("Session short code"),
        "expected runtime health to capture the Abiotic short code line"
    );
    assert_eq!(overview.diagnostics.len(), 3);
    assert!(
        overview.diagnostics.iter().any(|signal| signal.code
            == "abiotic_remote_console_https_disabled"
            && !signal.actionable),
        "expected Abiotic runtime diagnostics to keep the remote console notice"
    );
    assert!(
        overview.diagnostics.iter().any(|signal| signal.code
            == "abiotic_headless_eos_interfaces_unavailable"
            && signal.severity == "info"),
        "expected Abiotic runtime diagnostics to explain the EOS headless warnings"
    );
    assert!(
        overview.diagnostics.iter().any(|signal| {
            signal.code == "abiotic_online_session_start_warning"
                && signal.severity == "info"
                && !signal.actionable
        }),
        "expected late Abiotic online-session chatter to stay non-actionable after a join code exists"
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn abioticfactor_runtime_overview_marks_world_corruption_as_error() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = prepare_abioticfactor_environment(&root);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Abiotic Corrupt"),
            module_id: String::from("abioticfactor"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-corrupt.log");
    fs::write(
        &log_path,
        concat!(
            "LogAbiotic: Display: Dedicated Server entered ServerEntry, checking world save for corruption\n",
            "LogAbiotic: Display: World save Integrity State: Corrupt\n",
            "LogAbiotic: Warning: Dedicated Server will shut down in 5 minutes due to world save corruption\n",
        ),
    )
    .unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        8899,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(overview.health.status, "error");
    assert!(
        overview
            .health
            .summary
            .to_ascii_lowercase()
            .contains("world save corruption"),
        "unexpected runtime health summary: {}",
        overview.health.summary
    );
    assert!(
        overview
            .health
            .matched_line
            .as_deref()
            .unwrap_or_default()
            .contains("corruption"),
        "expected runtime health to capture the world corruption line"
    );

    cleanup_root(&root);
}

#[tokio::test]
async fn abioticfactor_runtime_overview_marks_pre_join_session_warning_as_actionable() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = prepare_abioticfactor_environment(&root);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("Abiotic Session Warning"),
            module_id: String::from("abioticfactor"),
        },
    )
    .await
    .unwrap();

    let log_path = root
        .join("instances")
        .join(&created.summary.id)
        .join("logs")
        .join("run-session-warning.log");
    fs::write(
        &log_path,
        concat!(
            "LogAbioticRemoteConsole: Warning: Remote console through HTTPS is explicitly disabled.\n",
            "LogOnline: Warning: OSS: [FOnlineVoiceEOSPlus::Initialize] BaseVoiceInterface delegates not bound. Base interface not valid\n",
            "LogAbiotic: Display: Dedicated Server is now loading the main map\n",
            "LogOnlineSession: Warning: STEAM: Can't start an online game for session (GameSession) that hasn't been created\n",
        ),
    )
    .unwrap();

    record_started_test_instance(
        &paths,
        &created.summary.id,
        9900,
        &log_path.to_string_lossy(),
    )
    .await
    .unwrap();

    let overview = read_instance_runtime_overview(&paths, &created.summary.id)
        .await
        .unwrap();
    let session_signal = overview
        .diagnostics
        .iter()
        .find(|signal| signal.code == "abiotic_online_session_start_warning")
        .expect("expected Abiotic session warning diagnostic");
    assert_eq!(session_signal.severity, "warning");
    assert!(
        session_signal.actionable,
        "pre-join Abiotic session warnings should stay actionable"
    );
    assert!(
        session_signal
            .summary
            .to_ascii_lowercase()
            .contains("join code"),
        "unexpected session warning summary: {}",
        session_signal.summary
    );

    cleanup_root(&root);
}

fn vrising_test_descriptor(root: &Path) -> ModuleDescriptor {
    ModuleDescriptor {
            root: root.join("modules").join("vrising"),
            manifest_toml: String::from(
                "id = \"vrising\"\nname = \"V Rising\"\nversion = \"0.1.0\"\nsupported_platforms = [\"windows\"]\n\n[storage]\nsaves_path_template = \"{{paths.instance_root}}/Saves\"\n",
            ),
            schema_json: Some(
                r#"{
                    "type": "object",
                    "properties": {
                        "server_name": { "type": "string", "default": "V Rising Server", "x-lsgm-default-source": "instance_name" },
                        "server_description": { "type": "string", "default": "Managed by LanGame Server Manager." },
                        "max_players": { "type": "integer", "default": 40 },
                        "max_admins": { "type": "integer", "default": 4 },
                        "server_password": { "type": "string", "default": "" },
                        "hide_ip_address": { "type": "boolean", "default": false },
                        "secure_mode": { "type": "boolean", "default": true },
                        "list_on_eos": { "type": "boolean", "default": true },
                        "save_name": { "type": "string", "default": "world1" },
                        "autosave_count": { "type": "integer", "default": 20 },
                        "autosave_interval_seconds": { "type": "integer", "default": 120 },
                        "autosave_smart_keep": { "type": "string", "default": "10:1:1,30:0:1,60:0:1,120:0:1,180:0:1,240:0:1,360:0:1,720:0:1,1440:0:1,2880:0:1,52560000:99:0" },
                        "rcon_enabled": { "type": "boolean", "default": false },
                        "rcon_password": { "type": "string", "default": "change-me-rcon" },
                        "admin_list": { "type": "string", "default": "" },
                        "ban_list": { "type": "string", "default": "" },
                        "game_difficulty": { "type": "string", "default": "Normal" },
                        "game_mode_type": { "type": "string", "default": "PvP" },
                        "castle_damage_mode": { "type": "string", "default": "Never" },
                        "can_loot_enemy_containers": { "type": "boolean", "default": true },
                        "castle_limit": { "type": "integer", "default": 2 },
                        "castle_tick_period": { "type": "number", "default": 5.0 },
                        "vs_player_weekday_start_hour": { "type": "integer", "default": 20 },
                        "vs_player_weekday_start_minute": { "type": "integer", "default": 0 },
                        "vs_player_weekday_end_hour": { "type": "integer", "default": 22 },
                        "vs_player_weekday_end_minute": { "type": "integer", "default": 0 },
                        "day_duration_in_seconds": { "type": "number", "default": 1080.0 },
                        "trader_stock_modifier": { "type": "number", "default": 1.0 },
                        "server_game_settings_json": { "type": "string", "default": "{\n}\n" }
                    }
                }"#
                .to_string(),
            ),
            default_ports: vec![
                PortBinding {
                    name: String::from("game"),
                    protocol: String::from("udp"),
                    port: 9876,
                },
                PortBinding {
                    name: String::from("query"),
                    protocol: String::from("udp"),
                    port: 9877,
                },
                PortBinding {
                    name: String::from("rcon"),
                    protocol: String::from("tcp"),
                    port: 25575,
                },
            ],
            install: Some(app_core::InstallSpec {
                shared_game_dir: String::from("vrising"),
                download_url_windows: None,
                download_integrity_windows: None,
                source: None,
                verification_path: None,
                minecraft: None,
            }),
            process: Some(app_core::ProcessSpec {
                environment_template: Default::default(),
                executable: String::from("VRisingServer.exe"),
                args_template: vec![
                    String::from("-persistentDataPath"),
                    String::from("{{paths.instance_root}}"),
                    String::from("{{vrising.bind_address_flag}}"),
                    String::from("{{vrising.bind_address_value}}"),
                ],
                working_directory_template: Some(String::from("{{paths.install_root}}")),
                window_policy: app_core::ProcessWindowPolicy::Background,
                host_surface: app_core::ProcessHostSurface::ManagedTerminal,
                host_notes: None,
            }),
            workshop: None,
            runtime: app_core::ModuleRuntimeSpec::default(),
            storage: app_modules::ModuleStorageSpec { saves_path_template: Some(String::from("{{paths.instance_root}}/Saves")), ..Default::default() },
            summary: ModuleSummary {
                id: String::from("vrising"),
                name: String::from("V Rising Dedicated Server"),
                version: String::from("0.1.0"),
                description: Some(String::from("Test module")),
                steam_app_id: Some(1829350),
                install_state: InstallState::NotInstalled,
                instance_program_count: 0,
                archived_program_count: 0,
                supported_platforms: vec![String::from("windows")],
            },
        }
}

fn prepare_vrising_environment(root: &Path, descriptor: &ModuleDescriptor) {
    let paths = test_paths(root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    fs::create_dir_all(&paths.logs_root).unwrap();
    fs::create_dir_all(&paths.steamcmd_root).unwrap();
    fs::create_dir_all(&paths.games_root).unwrap();
    fs::create_dir_all(&paths.instances_root).unwrap();
    write_module_descriptor_files(descriptor);
    let templates_root = descriptor.root.join("templates");
    let settings_templates_root = templates_root.join("Settings");
    fs::create_dir_all(&settings_templates_root).unwrap();
    fs::write(
            settings_templates_root.join("ServerHostSettings.json.hbs"),
            "{\n  \"Name\": {{json.settings.server_name}},\n  \"Description\": {{json.settings.server_description}},\n  \"Port\": {{ports.game.port}},\n  \"QueryPort\": {{ports.query.port}},\n  \"HideIPAddress\": {{hide_ip_address}},\n  \"MaxConnectedUsers\": {{max_players}},\n  \"MaxConnectedAdmins\": {{max_admins}},\n  \"Password\": {{json.settings.server_password}},\n  \"Secure\": {{secure_mode}},\n  \"ListOnEOS\": {{list_on_eos}},\n  \"SaveName\": {{json.settings.save_name}},\n  \"AutoSaveCount\": {{autosave_count}},\n  \"AutoSaveInterval\": {{autosave_interval_seconds}},\n  \"AutoSaveSmartKeep\": {{json.settings.autosave_smart_keep}},\n  \"Rcon\": {\n    \"Enabled\": {{rcon_enabled}},\n    \"Password\": {{json.settings.rcon_password}},\n    \"Port\": {{ports.rcon.port}}\n  }\n}\n",
        )
        .unwrap();
    fs::write(
        settings_templates_root.join("ServerGameSettings.json.hbs"),
        "{{vrising.server_game_settings_json}}\n",
    )
    .unwrap();
    fs::write(
        settings_templates_root.join("adminlist.txt.hbs"),
        "{{admin_list}}\n",
    )
    .unwrap();
    fs::write(
        settings_templates_root.join("banlist.txt.hbs"),
        "{{ban_list}}\n",
    )
    .unwrap();
}

#[tokio::test]
async fn vrising_instance_materializes_persistent_data_layout() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let descriptor = vrising_test_descriptor(&root);
    prepare_vrising_environment(&root, &descriptor);

    initialize_database(&paths).await.unwrap();
    sync_modules(&paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();

    let install_root = root.join("verified-installs").join("vrising");
    fs::create_dir_all(&install_root).unwrap();
    sync_game_installs(
        &paths,
        &[GameInstallSyncRecord {
            module_id: String::from("vrising"),
            install_root: install_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some(String::from("vrising-test-build")),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();

    let created = create_instance(
        &paths,
        &descriptor,
        CreateInstanceInput {
            name: String::from("V Rising \"Keep\" Δ"),
            module_id: String::from("vrising"),
        },
    )
    .await
    .unwrap();

    configure_test_instance_runtime(&paths, &created.summary.id, "192.168.10.25", false).await;

    assert_eq!(created.summary.port_count, 3);

    let config_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("config");
    let settings_root = config_root.join("Settings");
    let live_settings_root = root
        .join("instances")
        .join(&created.summary.id)
        .join("Settings");
    let host_settings_text =
        fs::read_to_string(settings_root.join("ServerHostSettings.json")).unwrap();
    let game_settings_text =
        fs::read_to_string(settings_root.join("ServerGameSettings.json")).unwrap();
    let admin_list_text = fs::read_to_string(settings_root.join("adminlist.txt")).unwrap();
    let ban_list_text = fs::read_to_string(settings_root.join("banlist.txt")).unwrap();

    assert!(host_settings_text.contains("\"Name\": \"V Rising \\\"Keep\\\" Δ\""));
    assert!(host_settings_text.contains("\"Description\": \"Managed by LanGame Server Manager.\""));
    assert!(host_settings_text.contains("\"Port\": 9876"));
    assert!(host_settings_text.contains("\"QueryPort\": 9877"));
    assert!(host_settings_text.contains("\"MaxConnectedUsers\": 40"));
    assert!(host_settings_text.contains("\"MaxConnectedAdmins\": 4"));
    assert!(host_settings_text.contains("\"Password\": \"\""));
    assert!(host_settings_text.contains("\"Secure\": true"));
    assert!(host_settings_text.contains("\"ListOnEOS\": true"));
    assert!(host_settings_text.contains("\"SaveName\": \"world1\""));
    assert!(host_settings_text.contains("\"AutoSaveCount\": 20"));
    assert!(host_settings_text.contains("\"AutoSaveInterval\": 120"));
    assert!(host_settings_text.contains("\"AutoSaveSmartKeep\": \"10:1:1,30:0:1,60:0:1,120:0:1,180:0:1,240:0:1,360:0:1,720:0:1,1440:0:1,2880:0:1,52560000:99:0\""));
    assert!(host_settings_text.contains("\"Enabled\": false"));
    assert!(host_settings_text.contains("\"Password\": \"change-me-rcon\""));
    assert!(host_settings_text.contains("\"Port\": 25575"));
    let host_settings: Value = serde_json::from_str(&host_settings_text).unwrap();
    assert_eq!(
        host_settings["Name"],
        serde_json::json!("V Rising \"Keep\" Δ")
    );

    let game_settings: Value = serde_json::from_str(&game_settings_text).unwrap();
    assert_eq!(game_settings["GameDifficulty"], serde_json::json!("Normal"));
    assert_eq!(game_settings["GameModeType"], serde_json::json!("PvP"));
    assert_eq!(
        game_settings["CastleDamageMode"],
        serde_json::json!("Never")
    );
    assert_eq!(
        game_settings["CanLootEnemyContainers"],
        serde_json::json!(true)
    );
    assert_eq!(
        game_settings["CastleStatModifiers_Global"]["CastleLimit"],
        serde_json::json!(2)
    );
    assert_eq!(
        game_settings["CastleStatModifiers_Global"]["TickPeriod"],
        serde_json::json!(5.0)
    );
    assert_eq!(
        game_settings["PlayerInteractionSettings"]["VSPlayerWeekdayTime"]["StartHour"],
        serde_json::json!(20)
    );
    assert_eq!(
        game_settings["PlayerInteractionSettings"]["VSPlayerWeekdayTime"]["EndHour"],
        serde_json::json!(22)
    );
    assert_eq!(
        game_settings["GameTimeModifiers"]["DayDurationInSeconds"],
        serde_json::json!(1080.0)
    );
    assert_eq!(
        game_settings["TraderModifiers"]["StockModifier"],
        serde_json::json!(1.0)
    );
    assert_eq!(admin_list_text, "\n");
    assert_eq!(ban_list_text, "\n");
    let live_host_settings: Value = serde_json::from_str(
        &fs::read_to_string(live_settings_root.join("ServerHostSettings.json")).unwrap(),
    )
    .unwrap();
    let live_game_settings: Value = serde_json::from_str(
        &fs::read_to_string(live_settings_root.join("ServerGameSettings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(live_host_settings, host_settings);
    assert_eq!(live_game_settings, game_settings);
    assert!(!live_settings_root.join("adminlist.txt").exists());
    assert!(!live_settings_root.join("banlist.txt").exists());
    assert!(
        root.join("instances")
            .join(&created.summary.id)
            .join("Saves")
            .exists(),
        "V Rising should pre-create the live Saves root"
    );
    assert!(
        !config_root.join("launch-vrising.bat").exists(),
        "V Rising should no longer render a generated launch script"
    );

    cleanup_root(&root);
}
