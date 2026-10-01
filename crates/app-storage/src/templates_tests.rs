use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "templates_rimworld_tests.rs"]
mod rimworld;

#[path = "templates_projectzomboid_seed_tests.rs"]
mod projectzomboid_seed;

#[path = "templates_dragonwilds_platform_tests.rs"]
mod dragonwilds_platform;

#[path = "templates_enshrouded_bans_tests.rs"]
mod enshrouded_bans;

static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn unique_test_root() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "langame-templates-test-{}-{stamp}-{sequence}",
        std::process::id()
    ))
}

fn test_storage_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("appdata"),
        settings_path: root.join("appdata").join("settings.json"),
        database_path: root.join("appdata").join("db").join("lgs.db"),
        logs_root: root.join("appdata").join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("programdata").join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

#[test]
fn projectzomboid_semicolon_list_preserves_commas_and_deduplicates() {
    let entries = normalize_projectzomboid_semicolon_list(
        "Muldraugh, KY\nRavenCreek\n# ignore me\nravencreek\nWest Point, KY",
    );

    assert_eq!(
        entries,
        vec![
            String::from("Muldraugh, KY"),
            String::from("RavenCreek"),
            String::from("West Point, KY")
        ]
    );
}

#[test]
fn projectzomboid_welcome_message_uses_line_tokens() {
    let mut settings = Map::new();
    settings.insert(
        String::from("welcome_message"),
        Value::String(String::from("Welcome survivor\nStay alive")),
    );

    assert_eq!(
        render_projectzomboid_welcome_message(&settings),
        "Welcome survivor <LINE> Stay alive"
    );
}

#[test]
fn conan_modlist_renders_pak_filenames_in_order() {
    assert_eq!(
        render_conan_modlist_lines(&[
            String::from("Pippi.pak"),
            String::from("fashionist.pak"),
            String::from("Pippi.pak"),
            String::from("../unsafe.pak"),
            String::from("notes.txt"),
        ]),
        "Pippi.pak\nfashionist.pak"
    );
}

#[test]
fn windrose_template_preserves_password_and_network_settings_without_fabricating_identity() {
    let mut settings = Map::new();
    settings.insert(
        String::from("server_password"),
        Value::String(String::from("secret")),
    );
    settings.insert(
        String::from("p2p_proxy_address"),
        Value::String(String::from("")),
    );
    settings.insert(
        String::from("direct_connection_proxy_address"),
        Value::String(String::from("198.51.100.20")),
    );

    assert_eq!(
        lookup_windrose_template_token(&settings, "203.0.113.10", "is_password_protected"),
        Some(String::from("true"))
    );
    assert_eq!(
        lookup_windrose_template_token(&settings, "203.0.113.10", "p2p_proxy_address_json"),
        Some(String::from("\"203.0.113.10\""))
    );
    assert_eq!(
        lookup_windrose_template_token(
            &settings,
            "203.0.113.10",
            "direct_connection_proxy_address_json"
        ),
        Some(String::from("\"198.51.100.20\""))
    );
    let schema_defaults = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/windrose/schema.json")),
        SchemaDefaultContext {
            instance_id: Some("windrose-test-server"),
            instance_name: Some("Windrose test"),
        },
    )
    .expect("Windrose schema defaults");
    let root = Path::new("C:/LanGame/tests");
    let ports = [PortBinding {
        name: String::from("direct"),
        protocol: String::from("udp"),
        port: 28017,
    }];
    let rendered = render_template_text(
        include_str!("../../../modules/windrose/templates/ServerDescription.json.hbs"),
        &TemplateRenderContext {
            instance_root: root,
            config_dir: root,
            install_root: root,
            data_dir: root,
            logs_dir: root,
            saves_dir: root,
            instance_id: "windrose-test-server",
            instance_name: "Windrose test",
            module_id: "windrose",
            bind_ip: "203.0.113.10",
            autostart: false,
            schema_defaults: &schema_defaults,
            settings: &settings,
            ports: &ports,
        },
    );
    let rendered = rendered.expect("render Windrose template");
    let document: Value = serde_json::from_str(&rendered).expect("rendered native JSON");
    let persistent = &document["ServerDescription_Persistent"];
    assert!(persistent.get("PersistentServerId").is_none());
    assert_eq!(persistent["IsPasswordProtected"], true);
    assert_eq!(persistent["Password"], "secret");
    assert_eq!(persistent["DirectConnectionServerPort"], 28017);
    assert_eq!(persistent["P2pProxyAddress"], "203.0.113.10");
    assert_eq!(persistent["DirectConnectionProxyAddress"], "198.51.100.20");
}

#[test]
fn windrose_materializes_server_description_into_r5_runtime() {
    let test_root = unique_test_root();
    let storage_paths = test_storage_paths(&test_root);
    let install_root = test_root.join("games").join("windrose");
    let config_dir = test_root
        .join("instances")
        .join("srv-windrose")
        .join("config");
    let saves_dir = test_root.join("saves");
    fs::create_dir_all(&install_root).expect("install root");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join(WINDROSE_SERVER_DESCRIPTION_FILE),
        "{\"Version\":1,\"ServerDescription_Persistent\":{\"WorldIslandId\":\"\"}}\n",
    )
    .expect("server description");

    materialize_module_support_files(&ModuleSupportMaterializationContext {
        storage_paths: &storage_paths,
        module_id: "windrose",
        install_root: &install_root,
        shared_install_root: &install_root,
        config_dir: &config_dir,
        saves_dir: &saves_dir,
        instance_id: "srv-windrose",
        instance_running: false,
        settings: &Map::new(),
    })
    .expect("materialize Windrose support files");

    let runtime_document: Value = serde_json::from_str(
        &fs::read_to_string(
            install_root
                .join("R5")
                .join(WINDROSE_SERVER_DESCRIPTION_FILE),
        )
        .expect("runtime ServerDescription.json"),
    )
    .expect("parse runtime ServerDescription.json");
    assert_eq!(runtime_document["Version"], 1);
    assert!(
        runtime_document["ServerDescription_Persistent"]
            .get("WorldIslandId")
            .is_none()
    );
    assert!(!install_root.join(WINDROSE_SERVER_DESCRIPTION_FILE).exists());

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn humanitz_tokens_render_rosters_and_extra_settings_lines() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_steam_ids"),
        Value::String(String::from(
            "76561198077777777\n|FEDCBA9876543210fedcba9876543210\n76561198077777777",
        )),
    );
    settings.insert(
        String::from("allowed_player_steam_ids"),
        Value::String(String::from("76561198000000000\n76561198011111111")),
    );
    settings.insert(
        String::from("reserved_player_steam_ids"),
        Value::String(String::from("76561198033333333\n76561198033333333")),
    );
    settings.insert(
        String::from("banned_player_steam_ids"),
        Value::String(String::from("76561198022222222\n# ignored")),
    );
    settings.insert(
        String::from("settings_extra"),
        Value::String(String::from("CustomKey=1\n// ignored\nOtherKey=two")),
    );

    assert_eq!(
        lookup_humanitz_template_token(&settings, "admin_list_lines").as_deref(),
        Some("76561198077777777\n|FEDCBA9876543210fedcba9876543210")
    );
    assert_eq!(
        lookup_humanitz_template_token(&settings, "allowed_player_lines").as_deref(),
        None
    );
    assert_eq!(
        lookup_humanitz_template_token(&settings, "reserved_player_lines").as_deref(),
        Some("76561198033333333")
    );
    assert_eq!(
        lookup_humanitz_template_token(&settings, "banned_player_lines").as_deref(),
        Some("76561198022222222")
    );
    assert_eq!(
        lookup_humanitz_template_token(&settings, "settings_extra_lines").as_deref(),
        Some("CustomKey=1\nOtherKey=two")
    );
    settings.insert(
        "admin_steam_ids".to_owned(),
        json!("76561198077777777\nbad"),
    );
    assert_eq!(
        lookup_humanitz_template_token(&settings, "admin_list_lines"),
        None
    );
}

#[test]
fn sonsoftheforest_tokens_render_deduplicated_owner_steam64_lines() {
    let mut settings = Map::new();
    settings.insert(
        String::from("owner_whitelist_steam_ids"),
        Value::String(String::from(
            "76561198077777777\ninvalid\n76561198077777777\n76561198000000000",
        )),
    );

    assert_eq!(
        lookup_sonsoftheforest_template_token(&settings, "owner_whitelist_lines"),
        Some(String::from("76561198077777777\n76561198000000000"))
    );
}

#[test]
fn theforest_boolean_tokens_render_native_on_off_values() {
    let mut settings = Map::new();
    settings.insert(String::from("tree_regrowth"), Value::Bool(true));

    let mut schema_defaults = Map::new();
    schema_defaults.insert(String::from("vac_enabled"), Value::Bool(false));

    assert_eq!(
        lookup_theforest_template_token(&settings, &schema_defaults, "tree_regrowth_on_off"),
        Some(String::from("on"))
    );
    assert_eq!(
        lookup_theforest_template_token(&settings, &schema_defaults, "vac_enabled_on_off"),
        Some(String::from("off"))
    );
    assert_eq!(
        lookup_theforest_template_token(&settings, &schema_defaults, "unknown_on_off"),
        None
    );
}

#[test]
fn scum_tokens_render_deduplicated_admin_ids() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_steam_ids"),
        Value::String(String::from(
            "76561198077777777\ninvalid\n76561198077777777\n76561198000000000",
        )),
    );

    assert_eq!(
        lookup_scum_template_token(&settings, "admin_steam_ids_lines"),
        Some(String::from("76561198077777777\n76561198000000000"))
    );
}

#[test]
fn minecraft_roster_json_filters_invalid_identity_rows() {
    let mut settings = Map::new();
    settings.insert(
            String::from("operator_entries"),
            Value::String(String::from(
                "00000000-0000-0000-0000-000000000000,Steve,9,true\nbaduuid,Alex,4,true\n11111111-1111-1111-1111-111111111111,,4,true\n00000000-0000-0000-0000-000000000000,Duplicate,1,false\n22222222-2222-2222-2222-222222222222,Builder,0,no",
            )),
        );
    settings.insert(
            String::from("whitelist_entries"),
            Value::String(String::from(
                "33333333-3333-3333-3333-333333333333,Friend\n33333333-3333-3333-3333-333333333333,Dupe\nnot-a-uuid,Name\n44444444-4444-4444-4444-444444444444,Bad Name",
            )),
        );
    settings.insert(
            String::from("banned_player_entries"),
            Value::String(String::from(
                "55555555-5555-5555-5555-555555555555,Raider,Griefing, repeated\n66666666-6666-6666-6666-666666666666,NoReason,\n77777777-7777-7777-7777-777777777777,Bad Name,Spaces\ninvalid,Player,Reason",
            )),
        );
    settings.insert(
        String::from("banned_ip_entries"),
        Value::String(String::from(
            "192.168.1.25,spam, repeated\nnot an ip\n192.168.1.25,duplicate\n2001:db8::1,",
        )),
    );

    let ops: Value = serde_json::from_str(&render_minecraft_ops_json(&settings)).unwrap();
    assert_eq!(ops.as_array().map(Vec::len), Some(2));
    assert_eq!(
        ops[0]["uuid"],
        json!("00000000-0000-0000-0000-000000000000")
    );
    assert_eq!(ops[0]["name"], json!("Steve"));
    assert_eq!(ops[0]["level"], json!(4));
    assert_eq!(ops[0]["bypassesPlayerLimit"], json!(true));
    assert_eq!(
        ops[1]["uuid"],
        json!("22222222-2222-2222-2222-222222222222")
    );
    assert_eq!(ops[1]["level"], json!(1));

    let whitelist: Value = serde_json::from_str(&render_minecraft_named_uuid_json(
        &settings,
        "whitelist_entries",
    ))
    .unwrap();
    assert_eq!(whitelist.as_array().map(Vec::len), Some(1));
    assert_eq!(
        whitelist[0]["uuid"],
        json!("33333333-3333-3333-3333-333333333333")
    );
    assert_eq!(whitelist[0]["name"], json!("Friend"));

    let banned_players: Value =
        serde_json::from_str(&render_minecraft_banned_players_json(&settings)).unwrap();
    assert_eq!(banned_players.as_array().map(Vec::len), Some(2));
    assert_eq!(
        banned_players[0]["uuid"],
        json!("55555555-5555-5555-5555-555555555555")
    );
    assert_eq!(banned_players[0]["reason"], json!("Griefing, repeated"));
    assert_eq!(
        banned_players[1]["uuid"],
        json!("66666666-6666-6666-6666-666666666666")
    );
    assert_eq!(
        banned_players[1]["reason"],
        json!("Banned by server operator")
    );

    let banned_ips: Value =
        serde_json::from_str(&render_minecraft_banned_ips_json(&settings)).unwrap();
    assert_eq!(banned_ips.as_array().map(Vec::len), Some(2));
    assert_eq!(banned_ips[0]["ip"], json!("192.168.1.25"));
    assert_eq!(banned_ips[0]["reason"], json!("spam, repeated"));
    assert_eq!(banned_ips[1]["ip"], json!("2001:db8::1"));
    assert_eq!(banned_ips[1]["reason"], json!("Banned by server operator"));
}

#[test]
fn palworld_mod_settings_ini_renders_active_package_names() {
    let mut settings = Map::new();
    settings.insert(
        String::from("mod_package_names"),
        Value::String(String::from(
            "GamingCattiva\nFarmingQuivern\nGamingCattiva\nbad package\n../unsafe",
        )),
    );

    let rendered = render_palworld_mod_settings_ini(&settings);

    assert!(rendered.contains("[PalModSettings]"));
    assert!(rendered.contains("bGlobalEnableMod=true"));
    assert_eq!(rendered.matches("ActiveModList=GamingCattiva").count(), 1);
    assert!(rendered.contains("ActiveModList=FarmingQuivern"));
    assert!(!rendered.contains("bad package"));
    assert!(!rendered.contains("../unsafe"));

    let empty = render_palworld_mod_settings_ini(&Map::new());
    assert!(empty.contains("bGlobalEnableMod=false"));
    assert!(!empty.contains("ActiveModList="));
}

#[test]
fn projectzomboid_materialization_syncs_runtime_ini_and_cleans_stale_scripts() {
    let test_root = unique_test_root();
    let storage_paths = test_storage_paths(&test_root);
    let install_root = storage_paths.games_root.join("projectzomboid");
    let config_dir = storage_paths
        .instances_root
        .join("srv-projectzomboid")
        .join("config");
    fs::create_dir_all(&install_root).expect("install root");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("server.ini"),
        "PublicName=Smoke Project Zomboid\r\nMap=Muldraugh, KY\r\n",
    )
    .expect("server.ini");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_SANDBOX_VARS_FILE),
        "SandboxVars = {\n    VERSION = 5,\n}\n",
    )
    .expect("sandbox vars");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_SPAWNPOINTS_FILE),
        "function SpawnPoints()\n    return {}\nend\n",
    )
    .expect("spawnpoints");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_SPAWNREGIONS_FILE),
        "function SpawnRegions()\n    return {}\nend\n",
    )
    .expect("spawnregions");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_GENERATED_LAUNCH_SCRIPT),
        "stale launch script",
    )
    .expect("stale generated launch script");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_LEGACY_PREPARE_SCRIPT),
        "legacy",
    )
    .expect("legacy prepare");
    fs::write(
        config_dir.join(PROJECT_ZOMBOID_LEGACY_GENERATED_SCRIPT),
        "legacy",
    )
    .expect("legacy generated script");

    materialize_module_support_files(&ModuleSupportMaterializationContext {
        storage_paths: &storage_paths,
        module_id: "projectzomboid",
        install_root: &install_root,
        shared_install_root: &install_root,
        config_dir: &config_dir,
        saves_dir: &test_root.join("saves"),
        instance_id: "srv-projectzomboid",
        instance_running: false,
        settings: &Map::new(),
    })
    .expect("materialize projectzomboid support files");

    let runtime_server_ini = fs::read_to_string(
        config_dir
            .join("runtime-home")
            .join("Zomboid")
            .join("Server")
            .join("srv-projectzomboid.ini"),
    )
    .expect("runtime-home server.ini");

    assert_eq!(
        runtime_server_ini,
        "PublicName=Smoke Project Zomboid\r\nMap=Muldraugh, KY\r\n"
    );

    let runtime_server_dir = config_dir
        .join("runtime-home")
        .join("Zomboid")
        .join("Server");
    assert_eq!(
        fs::read_to_string(runtime_server_dir.join("srv-projectzomboid_SandboxVars.lua"))
            .expect("runtime-home SandboxVars.lua"),
        "SandboxVars = {\n    VERSION = 5,\n}\n"
    );
    assert_eq!(
        fs::read_to_string(runtime_server_dir.join("srv-projectzomboid_spawnpoints.lua"))
            .expect("runtime-home spawnpoints.lua"),
        "function SpawnPoints()\n    return {}\nend\n"
    );
    assert_eq!(
        fs::read_to_string(runtime_server_dir.join("srv-projectzomboid_spawnregions.lua"))
            .expect("runtime-home spawnregions.lua"),
        "function SpawnRegions()\n    return {}\nend\n"
    );
    assert!(
        !config_dir
            .join(PROJECT_ZOMBOID_GENERATED_LAUNCH_SCRIPT)
            .exists(),
        "project zomboid should no longer materialize a generated launch script"
    );
    assert!(
        !config_dir
            .join(PROJECT_ZOMBOID_LEGACY_PREPARE_SCRIPT)
            .exists()
    );
    assert!(
        !config_dir
            .join(PROJECT_ZOMBOID_LEGACY_GENERATED_SCRIPT)
            .exists()
    );

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn humanitz_materializes_settings_and_rosters_to_server_root() {
    let test_root = unique_test_root();
    let storage_paths = test_storage_paths(&test_root);
    let install_root = test_root.join("games").join("humanitz");
    let config_dir = test_root
        .join("instances")
        .join("srv-humanitz")
        .join("config");
    let saves_dir = test_root.join("saves");

    fs::create_dir_all(install_root.join("HumanitZServer")).expect("install root");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::create_dir_all(&saves_dir).expect("saves dir");

    for (file_name, content) in [
        (
            HUMANITZ_GAME_SERVER_SETTINGS_FILE,
            "[Host Settings]\nServerName=HumanitZ\n",
        ),
        (HUMANITZ_WELCOME_MESSAGE_FILE, "WelcomeMessage.txt\n"),
        (HUMANITZ_ADMIN_LIST_FILE, "AdminList.txt\n"),
        ("F_MVPAccess.txt", "historical source must not be copied\n"),
        (HUMANITZ_RESERVED_SLOTS_FILE, "F_ReservedSlots.txt\n"),
        (HUMANITZ_BANNED_PLAYERS_FILE, "F_BannedPlayers.txt\n"),
    ] {
        fs::write(config_dir.join(file_name), content).expect("source file");
    }

    materialize_module_support_files(&ModuleSupportMaterializationContext {
        storage_paths: &storage_paths,
        module_id: "humanitz",
        install_root: &install_root,
        shared_install_root: &install_root,
        config_dir: &config_dir,
        saves_dir: &saves_dir,
        instance_id: "srv-humanitz",
        instance_running: false,
        settings: &Map::new(),
    })
    .expect("materialize humanitz support files");

    for (file_name, content) in [
        (
            HUMANITZ_GAME_SERVER_SETTINGS_FILE,
            "[Host Settings]\nServerName=HumanitZ\n",
        ),
        (HUMANITZ_WELCOME_MESSAGE_FILE, "WelcomeMessage.txt\n"),
        (HUMANITZ_ADMIN_LIST_FILE, "AdminList.txt\n"),
        (HUMANITZ_RESERVED_SLOTS_FILE, "F_ReservedSlots.txt\n"),
        (HUMANITZ_BANNED_PLAYERS_FILE, "F_BannedPlayers.txt\n"),
    ] {
        assert_eq!(
            fs::read_to_string(install_root.join("HumanitZServer").join(file_name))
                .expect("target file"),
            content
        );
    }

    assert!(!install_root.join("HumanitZServer/F_MVPAccess.txt").exists());
    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn rimworld_materializes_rendered_configs_to_runtime_root() {
    let test_root = unique_test_root();
    let storage_paths = test_storage_paths(&test_root);
    let install_root = test_root.join("games").join("rimworld");
    let config_dir = test_root
        .join("instances")
        .join("srv-rimworld")
        .join("config");
    fs::create_dir_all(&install_root).expect("install root");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("ServerConfig.json"),
        "{\"Name\":\"LanGame\"}\n",
    )
    .expect("server config");
    fs::write(
        config_dir.join("WhitelistConfig.json"),
        "{\"UseWhitelist\":true}\n",
    )
    .expect("whitelist config");

    fs::create_dir_all(install_root.join("Configs")).unwrap();
    fs::write(
        install_root.join("Configs/WhitelistConfig.json"),
        b"{\"UseWhitelist\":true}\n",
    )
    .unwrap();
    materialize_module_support_files(&ModuleSupportMaterializationContext {
        storage_paths: &storage_paths,
        module_id: "rimworld",
        install_root: &install_root,
        shared_install_root: &install_root,
        config_dir: &config_dir,
        saves_dir: &install_root.join("Assets"),
        instance_id: "srv-rimworld",
        instance_running: false,
        settings: &Map::new(),
    })
    .expect("materialize rimworld support files");

    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(install_root.join("Configs").join("ServerConfig.json")).unwrap()
        )
        .unwrap(),
        json!({"Name": "LanGame"})
    );
    assert_eq!(
        fs::read_to_string(install_root.join("Configs").join("WhitelistConfig.json")).unwrap(),
        "{\"UseWhitelist\":true}\n"
    );
    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn abioticfactor_moderator_lines_deduplicate_comments_and_noop_invalid_entries() {
    let mut settings = Map::new();
    settings.insert(
            String::from("moderator_steam_ids"),
            Value::String(String::from(
                "76561198077777777\n# comment\n76561198000000000,76561198011111111\n76561198077777777\n12345\ninvalid",
            )),
        );

    assert_eq!(
        render_abioticfactor_moderator_lines(&settings),
        "Moderator=76561198077777777\nModerator=76561198000000000\nModerator=76561198011111111"
    );
}

#[test]
fn ark_roster_lines_filter_by_identity_surface() {
    let mut settings = Map::new();
    settings.insert(
            String::from("admin_account_ids"),
            Value::String(String::from(
                "76561198077777777\n12345\n76561198000000000,76561198011111111\n76561198077777777\n{{settings.server_name}}",
            )),
        );
    settings.insert(
        String::from("exclusive_join_list"),
        Value::String(String::from(
            "Account-1\nAccount_2\nAccount-1\nBad Account\nAccount|note\n// comment",
        )),
    );

    assert_eq!(
        lookup_ark_template_token(&settings, "steam64_lines admin_account_ids"),
        Some(String::from("76561198077777777"))
    );
    assert_eq!(
        lookup_ark_template_token(&settings, "account_id_lines exclusive_join_list"),
        Some(String::from("Account-1\nAccount_2"))
    );
}

#[test]
fn dst_roster_lists_filter_to_klei_user_ids() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from(
            "KU_admin_1\n# comment\nku_admin_2\nKU_admin_1\nbad\nKU bad\nKU_admin_3|note",
        )),
    );
    settings.insert(
        String::from("whitelist"),
        Value::String(String::from(
            "KU_friend_1\nKU_friend_1\n{{settings.cluster_name}}\nKU_friend_2",
        )),
    );
    settings.insert(
        String::from("blocklist"),
        Value::String(String::from(
            "KU_blocked_1\n// comment\nKU_blocked_1\nOU_blocked",
        )),
    );

    assert_eq!(
        lookup_dst_template_token_with_instance(&settings, "roster-test", "admin_list_lines"),
        Some(String::from("KU_admin_1\nKU_admin_2"))
    );
    assert_eq!(
        lookup_dst_template_token_with_instance(&settings, "roster-test", "whitelist_lines"),
        Some(String::from("KU_friend_1\nKU_friend_2"))
    );
    assert_eq!(
        lookup_dst_template_token_with_instance(&settings, "roster-test", "blocklist_lines"),
        Some(String::from("KU_blocked_1"))
    );
}

#[test]
fn terraria_banlist_lines_filter_comments_duplicates_and_template_tokens() {
    let mut settings = Map::new();
    settings.insert(
        String::from("banlist_entries"),
        Value::String(String::from(
            "Griefer\n# comment\n192.168.1.25\nGriefer\n{{settings.server_name}}\nVery Bad Player",
        )),
    );

    assert_eq!(
        lookup_terraria_template_token(&settings, "banlist_lines"),
        Some(String::from("Griefer\n192.168.1.25\nVery Bad Player"))
    );
}

#[test]
fn vrising_roster_lists_filter_to_steam64_lines() {
    let mut settings = Map::new();
    let schema_defaults = Map::new();
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from(
            "76561198077777777\n12345\n76561198000000000,76561198011111111\n76561198077777777",
        )),
    );
    settings.insert(
        String::from("ban_list"),
        Value::String(String::from(
            "76561198022222222\n// comment\nbad\n76561198022222222",
        )),
    );

    assert_eq!(
        lookup_vrising_template_token(&settings, &schema_defaults, "admin_list_lines"),
        Some(String::from(
            "76561198077777777\n76561198000000000\n76561198011111111"
        ))
    );
    assert_eq!(
        lookup_vrising_template_token(&settings, &schema_defaults, "ban_list_lines"),
        Some(String::from("76561198022222222"))
    );
}

#[test]
fn vrising_server_game_settings_render_war_event_settings() {
    let mut settings = Map::new();
    let schema_defaults = Map::new();
    settings.insert(String::from("war_event_interval"), json!(3));
    settings.insert(String::from("war_event_major_duration"), json!(2));
    settings.insert(String::from("war_event_minor_duration"), json!(1));
    settings.insert(String::from("war_event_weekday_start_hour"), json!(18));
    settings.insert(String::from("war_event_weekday_start_minute"), json!(15));
    settings.insert(String::from("war_event_weekday_end_hour"), json!(21));
    settings.insert(String::from("war_event_weekday_end_minute"), json!(45));
    settings.insert(
        String::from("war_event_scaling_players_4_points_modifier"),
        json!(0.75),
    );
    settings.insert(
        String::from("war_event_scaling_players_4_drop_modifier"),
        json!(0.5),
    );

    let rendered = render_vrising_server_game_settings_json(&settings, &schema_defaults);
    let document: Value = serde_json::from_str(&rendered).expect("rendered json");

    assert_eq!(document["WarEventGameSettings"]["Interval"], json!(3));
    assert_eq!(document["WarEventGameSettings"]["MajorDuration"], json!(2));
    assert_eq!(document["WarEventGameSettings"]["MinorDuration"], json!(1));
    assert_eq!(
        document["WarEventGameSettings"]["WeekdayTime"]["StartHour"],
        json!(18)
    );
    assert_eq!(
        document["WarEventGameSettings"]["WeekdayTime"]["StartMinute"],
        json!(15)
    );
    assert_eq!(
        document["WarEventGameSettings"]["WeekdayTime"]["EndHour"],
        json!(21)
    );
    assert_eq!(
        document["WarEventGameSettings"]["WeekdayTime"]["EndMinute"],
        json!(45)
    );
    assert_eq!(
        document["WarEventGameSettings"]["ScalingPlayers4"]["PointsModifier"],
        json!(0.75)
    );
    assert_eq!(
        document["WarEventGameSettings"]["ScalingPlayers4"]["DropModifier"],
        json!(0.5)
    );
}

#[test]
fn vrising_server_game_settings_preserves_raw_json_against_schema_defaults() {
    let mut settings = Map::new();
    let mut schema_defaults = Map::new();
    settings.insert(
        String::from("server_game_settings_json"),
        Value::String(String::from(
            r#"{
  "GameDifficulty": 2,
  "GameModeType": "PvE",
  "ClanSize": 2,
  "WarEventGameSettings": {
    "Interval": 3
  }
}"#,
        )),
    );
    settings.insert(String::from("game_mode_type"), json!("PvP"));
    schema_defaults.insert(String::from("game_difficulty"), json!("Normal"));
    schema_defaults.insert(String::from("game_mode_type"), json!("PvE"));
    schema_defaults.insert(String::from("clan_size"), json!(4));
    schema_defaults.insert(String::from("war_event_interval"), json!(0));

    let rendered = render_vrising_server_game_settings_json(&settings, &schema_defaults);
    let document: Value = serde_json::from_str(&rendered).expect("rendered json");

    assert_eq!(document["GameDifficulty"], json!(2));
    assert_eq!(document["GameModeType"], json!("PvP"));
    assert_eq!(document["ClanSize"], json!(2));
    assert_eq!(document["WarEventGameSettings"]["Interval"], json!(3));
}

#[test]
fn vrising_server_game_settings_renders_exact_nested_modifiers_and_keeps_arrays() {
    let mut settings = Map::new();
    settings.insert(
        String::from("server_game_settings_json"),
        Value::String(String::from(
            r#"{
  "VBloodUnitSettings": [{"UnitId": 42}],
  "UnlockedAchievements": [1],
  "UnlockedResearchs": [2],
  "CastleStatModifiers_Global": {"HeartLimits": {"Level5": {"FutureLimit": 9}}}
}"#,
        )),
    );
    settings.insert(String::from("vampire_max_health_modifier"), json!(1.5));
    settings.insert(String::from("global_unit_level_increase"), json!(3));
    settings.insert(
        String::from("global_equipment_spell_power_modifier"),
        json!(1.25),
    );
    settings.insert(String::from("castle_heart_level_5_floor_limit"), json!(800));

    let rendered = render_vrising_server_game_settings_json(&settings, &Map::new());
    let document: Value = serde_json::from_str(&rendered).expect("rendered json");

    assert_eq!(
        document["VampireStatModifiers"]["MaxHealthModifier"],
        json!(1.5)
    );
    assert_eq!(
        document["UnitStatModifiers_Global"]["LevelIncrease"],
        json!(3)
    );
    assert_eq!(
        document["EquipmentStatModifiers_Global"]["SpellPowerModifier"],
        json!(1.25)
    );
    assert_eq!(
        document["CastleStatModifiers_Global"]["HeartLimits"]["Level5"]["FloorLimit"],
        json!(800)
    );
    assert_eq!(
        document["CastleStatModifiers_Global"]["HeartLimits"]["Level5"]["FutureLimit"],
        json!(9)
    );
    assert_eq!(document["VBloodUnitSettings"], json!([{"UnitId": 42}]));
    assert_eq!(document["UnlockedAchievements"], json!([1]));
    assert_eq!(document["UnlockedResearchs"], json!([2]));
}

#[test]
fn barotrauma_client_permissions_render_admin_entries() {
    let mut settings = Map::new();
    settings.insert(
            String::from("admin_entries"),
            Value::String(String::from(
                "76561198077777777, Host <Lead>, Primary\nSTEAM_0:1:12345678, Backup Admin\n[U:1:24691358]\ninvalid\n76561198077777777, Duplicate",
            )),
        );

    assert_eq!(
        render_barotrauma_client_permissions_xml(&settings),
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ClientPermissions>\n  <Client name=\"Host &lt;Lead&gt;, Primary\" accountid=\"STEAM_1:1:58756024\" permissions=\"All\" />\n  <Client name=\"Backup Admin\" accountid=\"STEAM_1:1:12345678\" permissions=\"All\" />\n  <Client name=\"STEAM_1:0:12345679\" accountid=\"STEAM_1:0:12345679\" permissions=\"All\" />\n</ClientPermissions>"
    );
}

#[test]
fn squad_admins_cfg_renders_structured_admins_and_reserved_users() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_steam_ids"),
        Value::String(String::from(
            "76561198077777777\n# comment\ninvalid\n76561198000000000\n76561198077777777",
        )),
    );
    settings.insert(
        String::from("priority_join_steam_ids"),
        Value::String(String::from("76561198011111111\n76561198011111111")),
    );
    settings.insert(
        String::from("admin_permissions"),
        Value::String(String::from("kick\nban\nconfig\nkick\nbad permission")),
    );
    settings.insert(
            String::from("admins_cfg"),
            Value::String(String::from(
                "Group=Streamer:chat,KICK,kick\nAdmin=76561198022222222:Streamer\nAdmin=bad:Streamer\nAdmin=76561198022222222:Streamer\nGroup=Broken:\nGroup=Bad Name:chat\nNotCopied=true",
            )),
        );

    assert_eq!(
        render_squad_admins_cfg(&settings),
        "// Generated by LanGame Server Manager.\nGroup=LanGameAdmin:kick,ban,config\nGroup=LanGameReserved:reserve\nAdmin=76561198077777777:LanGameAdmin\nAdmin=76561198000000000:LanGameAdmin\nAdmin=76561198011111111:LanGameReserved\n// Additional Admins.cfg lines from instance settings.\nGroup=Streamer:chat,kick"
    );
}

#[test]
fn sevendaystodie_serveradmin_lists_render_structured_entries() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_users"),
        json!([
            {
                "platform": "Steam",
                "userid": "76561198077777777",
                "name": "Host Lead",
                "permission_level": 0
            },
            {
                "platform": "EOS",
                "userid": "0002604bc42244e099c1bf05145fb71f",
                "name": "Ops \"Two\"",
                "permission_level": 5
            },
            {
                "platform": "steam",
                "userid": "76561198077777777",
                "name": "Duplicate",
                "permission_level": 10
            }
        ]),
    );
    settings.insert(
        String::from("admin_groups"),
        json!([
            {
                "steam_id": "103582791434672565",
                "name": "Steam Universe",
                "permission_level_default": 1000,
                "permission_level_mod": 0
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_users"),
        json!([
            {
                "platform": "XBL",
                "userid": "2533274791234567",
                "name": "Trusted Friend"
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_groups"),
        json!([
            {
                "steam_id": "103582791434672566",
                "name": "Weekend Survivors"
            }
        ]),
    );
    settings.insert(
        String::from("blacklist_entries"),
        json!([
            {
                "platform": "PSN",
                "userid": "Raider-One_42",
                "name": "Raider",
                "unbandate": "2025-01-01",
                "reason": "Griefing & spam"
            }
        ]),
    );
    settings.insert(
        String::from("command_permissions"),
        json!([
            {
                "cmd": "help",
                "permission_level": 1000
            },
            {
                "cmd": "listplayerids",
                "permission_level": 1000
            }
        ]),
    );

    assert_eq!(
        render_sevendaystodie_admin_user_lines(&settings),
        "    <user platform=\"Steam\" userid=\"76561198077777777\" name=\"Host Lead\" permission_level=\"0\" />\n    <user platform=\"EOS\" userid=\"0002604bc42244e099c1bf05145fb71f\" name=\"Ops &quot;Two&quot;\" permission_level=\"5\" />"
    );
    assert_eq!(
        render_sevendaystodie_admin_group_lines(&settings),
        "    <group steamID=\"103582791434672565\" name=\"Steam Universe\" permission_level_default=\"1000\" permission_level_mod=\"0\" />"
    );
    assert_eq!(
        render_sevendaystodie_whitelist_user_lines(&settings),
        "    <user platform=\"XBL\" userid=\"2533274791234567\" name=\"Trusted Friend\" />"
    );
    assert_eq!(
        render_sevendaystodie_whitelist_group_lines(&settings),
        "    <group steamID=\"103582791434672566\" name=\"Weekend Survivors\" />"
    );
    assert_eq!(
        render_sevendaystodie_blacklist_lines(&settings),
        "    <blacklisted platform=\"PSN\" userid=\"Raider-One_42\" name=\"Raider\" unbandate=\"2025-01-01\" reason=\"Griefing &amp; spam\" />"
    );
    assert_eq!(
        render_sevendaystodie_permission_lines(&settings),
        "    <permission cmd=\"help\" permission_level=\"1000\" />\n    <permission cmd=\"listplayerids\" permission_level=\"1000\" />"
    );
}

#[test]
fn sevendaystodie_serveradmin_lists_skip_invalid_identity_rows() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_users"),
        json!([
            {
                "platform": "Steam",
                "userid": "",
                "name": "Blank"
            },
            {
                "platform": "Steam",
                "name": "Missing"
            },
            {
                "platform": "Steam",
                "userid": "1234567890",
                "name": "Short"
            },
            {
                "platform": "Unknown",
                "userid": "76561198044444444",
                "name": "Wrong platform"
            },
            {
                "platform": "Steam",
                "userid": "76561198044444444",
                "name": "Valid Admin",
                "permission_level": -5
            },
            {
                "platform": "steam",
                "userid": "76561198044444444",
                "name": "Duplicate",
                "permission_level": 1000
            }
        ]),
    );
    settings.insert(
        String::from("admin_groups"),
        json!([
            {
                "steam_id": "not-a-group",
                "name": "Invalid Group"
            },
            {
                "steam_id": "103582791434672567",
                "name": "Valid Group",
                "permission_level_default": 1100,
                "permission_level_mod": -10
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_users"),
        json!([
            {
                "platform": "Steam",
                "userid": true,
                "name": "Wrong Type"
            },
            {
                "platform": "EOS",
                "userid": "0002604bc42244e099c1bf05145fb71f",
                "name": "Trusted"
            }
        ]),
    );
    settings.insert(
        String::from("whitelist_groups"),
        json!([
            {
                "steam_id": "short",
                "name": "Wrong Group"
            },
            {
                "steam_id": "103582791434672568",
                "name": "Allowed Group"
            }
        ]),
    );
    settings.insert(
        String::from("blacklist_entries"),
        json!([
            {
                "platform": "Steam",
                "userid": null,
                "name": "No ID"
            },
            {
                "platform": "PSN",
                "userid": "Blocked_Player",
                "name": "Blocked",
                "unbandate": "",
                "reason": ""
            }
        ]),
    );

    assert_eq!(
        render_sevendaystodie_admin_user_lines(&settings),
        "    <user platform=\"Steam\" userid=\"76561198044444444\" name=\"Valid Admin\" permission_level=\"0\" />"
    );
    assert_eq!(
        render_sevendaystodie_admin_group_lines(&settings),
        "    <group steamID=\"103582791434672567\" name=\"Valid Group\" permission_level_default=\"1000\" permission_level_mod=\"0\" />"
    );
    assert_eq!(
        render_sevendaystodie_whitelist_user_lines(&settings),
        "    <user platform=\"EOS\" userid=\"0002604bc42244e099c1bf05145fb71f\" name=\"Trusted\" />"
    );
    assert_eq!(
        render_sevendaystodie_whitelist_group_lines(&settings),
        "    <group steamID=\"103582791434672568\" name=\"Allowed Group\" />"
    );
    assert_eq!(
        render_sevendaystodie_blacklist_lines(&settings),
        "    <blacklisted platform=\"PSN\" userid=\"Blocked_Player\" name=\"Blocked\" unbandate=\"9999-12-31\" reason=\"LanGame\" />"
    );
}

#[test]
fn corekeeper_identifier_list_deduplicates_and_filters_invalid_entries() {
    let entries = normalize_corekeeper_identifier_list(
        "76561198077777777\n# comment\n76561198000000000,76561198022222222\n76561198077777777\n12345\ninvalid\n76561198000000000",
    );

    assert_eq!(
        entries,
        vec![
            String::from("76561198077777777"),
            String::from("76561198000000000"),
            String::from("76561198022222222")
        ]
    );
}

#[test]
fn enshrouded_banned_accounts_render_exact_native_uint64_hashes() {
    let mut settings = Map::new();
    settings.insert(
        String::from("banned_player_ids"),
        Value::String(String::from(
            "76561198077777777,12345\n18446744073709551615\n# comment\n0\n76561198077777777",
        )),
    );

    let bans: Value =
        serde_json::from_str(&render_enshrouded_banned_accounts_json(&settings).unwrap()).unwrap();
    assert_eq!(bans.as_array().map(Vec::len), Some(4));
    assert_eq!(bans[0]["accountId"], json!(76561198077777777_u64));
    assert_eq!(bans[1]["accountId"], json!(12345));
    assert_eq!(bans[2]["accountId"], json!(u64::MAX));
    assert_eq!(bans[3]["accountId"], json!(0));
    assert_eq!(bans[0]["displayName"], json!(""));
    assert_eq!(bans[0]["characterName"], json!(""));
    assert_eq!(bans[0]["banDate"]["value"], json!(0));
}

#[test]
fn corekeeper_effective_game_id_uses_configured_or_derived_value() {
    let mut settings = Map::new();
    settings.insert(
        String::from("game_id"),
        Value::String(String::from("CoreKeeperRelay12345")),
    );

    assert_eq!(
        resolve_corekeeper_effective_game_id(&settings, "srv-corekeeper-1"),
        "CoreKeeperRelay12345"
    );
    assert_eq!(
        render_corekeeper_effective_game_id_json(&settings, "srv-corekeeper-1"),
        "\"CoreKeeperRelay12345\""
    );

    settings.insert(String::from("game_id"), Value::String(String::from("bad")));
    assert_eq!(
        resolve_corekeeper_effective_game_id(&settings, "srv-corekeeper-1"),
        "lgmsrvcorekeeper1corekeeper"
    );
}

#[test]
fn corekeeper_admin_and_ban_documents_render_structured_json() {
    let mut settings = Map::new();
    settings.insert(
        String::from("admin_list"),
        Value::String(String::from(
            "76561198077777777\n76561198000000000\n76561198077777777",
        )),
    );
    settings.insert(
        String::from("ban_list"),
        Value::String(String::from("76561198011111111\ninvalid")),
    );

    let admins: Value =
        serde_json::from_str(&render_corekeeper_admins_document_json(&settings)).unwrap();
    let bans: Value =
        serde_json::from_str(&render_corekeeper_bans_document_json(&settings)).unwrap();

    assert_eq!(admins["adminList"].as_array().map(Vec::len), Some(2));
    assert_eq!(
        admins["adminList"][0]["steamId"],
        json!(76561198077777777_u64)
    );
    assert_eq!(
        admins["adminList"][1]["steamId"],
        json!(76561198000000000_u64)
    );
    assert_eq!(bans["banList"].as_array().map(Vec::len), Some(1));
    assert_eq!(bans["banList"][0]["steamId"], json!(76561198011111111_u64));
    let empty_admins: Value =
        serde_json::from_str(&render_corekeeper_admins_document_json(&Map::new())).unwrap();
    assert_eq!(empty_admins, json!({ "adminList": [] }));
}

#[test]
fn rust_user_and_ban_lines_filter_invalid_steam64_entries() {
    let mut settings = Map::new();
    settings.insert(
            String::from("owner_entries"),
            Value::String(String::from(
                "76561198077777777|Host|Created by LanGame|Primary\ninvalid|Bad|Ignored\n76561198077777777|Duplicate|Ignored\n76561198000000000|Ops \"Two\"|Trusted, always",
            )),
        );
    settings.insert(
        String::from("moderator_entries"),
        Value::String(String::from(
            "76561198011111111|Mod|Weekend\n12345|Short|Ignored",
        )),
    );
    settings.insert(
            String::from("banned_entries"),
            Value::String(String::from(
                "76561198022222222|Griefing|repeated\nnot-steam|bad\n76561198022222222|Duplicate\n76561198033333333",
            )),
        );

    assert_eq!(
        render_rust_user_lines(&settings, "owner_entries", "ownerid"),
        "ownerid 76561198077777777 \"Host\" \"Created by LanGame|Primary\"\nownerid 76561198000000000 \"Ops \\\"Two\\\"\" \"Trusted, always\""
    );
    assert_eq!(
        render_rust_user_lines(&settings, "moderator_entries", "moderatorid"),
        "moderatorid 76561198011111111 \"Mod\" \"Weekend\""
    );
    assert_eq!(
        render_rust_ban_lines(&settings),
        "banid 76561198022222222 \"LanGame\" \"Griefing|repeated\"\nbanid 76561198033333333 \"LanGame\" \"Banned by server operator\""
    );
}

#[test]
fn rust_skip_queue_lines_render_optional_name_and_note() {
    let mut settings = Map::new();
    settings.insert(
            String::from("skip_queue_entries"),
            Value::String(String::from(
                "76561198077777777\ninvalid\n76561198000000000|Priority Friend\n76561198000000000|Duplicate\n76561198011111111|Streamer|Weekend event",
            )),
        );

    assert_eq!(
        render_rust_skip_queue_lines(&settings),
        "global.skipqueueid 76561198077777777\nglobal.skipqueueid 76561198000000000 \"Priority Friend\"\nglobal.skipqueueid 76561198011111111 \"Streamer\" \"Weekend event\""
    );
}

#[test]
fn rust_extra_cfg_tokens_cannot_render_managed_identity_directives() {
    let mut settings = Map::new();
    settings.insert(
        String::from("users_cfg_extra"),
        Value::String(String::from(
            "server.description custom\nownerid 76561198000000001 \"Raw owner\"\nmoderatorid 76561198000000002\nskipqueueid 76561198000000003\nglobal.skipqueueid 76561198000000004\nowneridentity remains-custom",
        )),
    );
    settings.insert(
        String::from("bans_cfg_extra"),
        Value::String(String::from(
            "server.hostname custom\nbanid 76561198000000005 \"Raw ban\"\nbanidentity remains-custom",
        )),
    );

    assert_eq!(
        lookup_rust_template_token(&settings, "users_cfg_extra_lines"),
        Some(String::from(
            "server.description custom\nowneridentity remains-custom"
        ))
    );
    assert_eq!(
        lookup_rust_template_token(&settings, "bans_cfg_extra_lines"),
        Some(String::from(
            "server.hostname custom\nbanidentity remains-custom"
        ))
    );
}

#[test]
fn valheim_roster_lists_filter_to_single_platform_id_tokens() {
    let mut settings = Map::new();
    settings.insert(
            String::from("admin_list"),
            Value::String(String::from(
                "76561198077777777\n# comment\nSteam_ABC123\nSteam_ABC123\nbad id with spaces\nxbox:player_1",
            )),
        );
    settings.insert(
        String::from("banned_list"),
        Value::String(String::from(
            "blocked-player\nblocked-player\nname|note\nname,note\n// comment\nCrossPlay-User",
        )),
    );
    settings.insert(
        String::from("permitted_list"),
        Value::String(String::from(
            "friend_one\nfriend two\nfriend_one\nFRIEND_ONE\nfriend-three",
        )),
    );

    assert_eq!(
        lookup_valheim_template_token(&settings, "admin_list_lines"),
        Some(String::from(
            "76561198077777777\nSteam_ABC123\nxbox:player_1"
        ))
    );
    assert_eq!(
        lookup_valheim_template_token(&settings, "banned_list_lines"),
        Some(String::from("blocked-player\nCrossPlay-User"))
    );
    assert_eq!(
        lookup_valheim_template_token(&settings, "permitted_list_lines"),
        Some(String::from("friend_one\nfriend-three"))
    );
}

#[test]
fn rust_custom_map_lines_skip_seed_and_size() {
    let mut settings = Map::new();
    settings.insert(
        String::from("level_url"),
        Value::String(String::from("https://maps.example.com/rust/custom.map")),
    );
    settings.insert(String::from("seed"), Value::from(12345));
    settings.insert(String::from("world_size"), Value::from(4200));
    settings.insert(String::from("wipe_unix_timestamp_override"), Value::from(0));

    assert_eq!(
        render_rust_optional_quoted_setting_line(&settings, "level_url", "server.levelurl"),
        "server.levelurl \"https://maps.example.com/rust/custom.map\""
    );
    assert_eq!(render_rust_seed_line(&settings), "");
    assert_eq!(render_rust_world_size_line(&settings), "");
    assert_eq!(render_rust_wipe_unix_override_line(&settings), "");

    settings.insert(
        String::from("wipe_unix_timestamp_override"),
        Value::from(1753372800),
    );
    assert_eq!(
        render_rust_wipe_unix_override_line(&settings),
        "wipetimer.wipeUnixTimestampOverride 1753372800"
    );
}

#[test]
fn unturned_tokens_render_admin_lines_and_browser_fallbacks() {
    let mut settings = Map::new();
    let schema_defaults = Map::new();
    settings.insert(
        String::from("welcome_message"),
        Value::String(String::from("Welcome to Unturned Ops")),
    );
    settings.insert(
        String::from("admin_steam_ids"),
        Value::String(String::from(
            "76561198077777777\ninvalid\n76561198077777777\n76561198000000000",
        )),
    );
    settings.insert(
        String::from("owner_steam_id"),
        Value::String(String::from("not-a-steam-id")),
    );
    settings.insert(
        String::from("workshop_ignore_children_file_ids"),
        Value::String(String::from("304930\ninvalid\n123456")),
    );

    assert_eq!(
        lookup_unturned_template_token(&settings, "owner_line"),
        Some(String::new())
    );
    settings.insert(
        String::from("owner_steam_id"),
        Value::String(String::from("76561198011111111")),
    );
    assert_eq!(
        lookup_unturned_template_token(&settings, "owner_line"),
        Some(String::from("Owner 76561198011111111"))
    );
    assert_eq!(
        lookup_unturned_template_token(&settings, "admin_lines"),
        Some(String::from(
            "Admin 76561198077777777\nAdmin 76561198000000000"
        ))
    );
    assert_eq!(
        render_unturned_config_text_value(
            &settings,
            &schema_defaults,
            "browser_desc_hint",
            Some("welcome_message"),
        ),
        String::from("\"Welcome to Unturned Ops\"")
    );
    assert_eq!(
        render_unturned_config_text_value(
            &settings,
            &schema_defaults,
            "browser_desc_full",
            Some("welcome_message"),
        ),
        String::from("\"Welcome to Unturned Ops\"")
    );
    assert_eq!(
        lookup_unturned_template_token(&settings, "workshop_ignore_children_file_ids_json",),
        Some(String::from("[\n  304930,\n  123456\n]"))
    );
}

#[test]
fn unturned_config_text_values_escape_quotes_and_newlines() {
    let mut settings = Map::new();
    settings.insert(
        String::from("browser_desc_full"),
        Value::String(String::from("Welcome to \"Unturned\"\r\nOperations")),
    );

    assert_eq!(
        render_unturned_config_text_value(&settings, &Map::new(), "browser_desc_full", None),
        String::from("\"Welcome to \\\"Unturned\\\"\\nOperations\"")
    );
}

#[test]
fn unturned_materializes_config_txt_into_server_id_root() {
    let test_root = unique_test_root();
    let storage_paths = test_storage_paths(&test_root);
    let install_root = test_root.join("games").join("unturned");
    let config_dir = test_root
        .join("instances")
        .join("srv-unturned")
        .join("config");
    fs::create_dir_all(&install_root).expect("install root");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(config_dir.join("Commands.dat"), "Name LanGame\n").expect("commands");
    fs::write(
        config_dir.join("Config.txt"),
        "Version 1\nServer\n{\n\tUse_FakeIP false\n}\n",
    )
    .expect("gameplay config");
    fs::write(
        config_dir.join("WorkshopDownloadConfig.json"),
        "{\"File_IDs\":[]}\n",
    )
    .expect("workshop config");

    materialize_module_support_files(&ModuleSupportMaterializationContext {
        storage_paths: &storage_paths,
        module_id: "unturned",
        install_root: &install_root,
        shared_install_root: &install_root,
        config_dir: &config_dir,
        saves_dir: &install_root.join("Servers").join("srv-unturned"),
        instance_id: "srv-unturned",
        instance_running: false,
        settings: &Map::new(),
    })
    .expect("materialize Unturned support files");

    let server_root = install_root.join("Servers").join("srv-unturned");
    assert_eq!(
        fs::read_to_string(server_root.join("Config.txt")).unwrap(),
        "Version 1\nServer\n{\n\tUse_FakeIP false\n}\n"
    );
    assert!(!server_root.join("Config.json").exists());
    assert_eq!(
        fs::read_to_string(server_root.join("Server").join("Commands.dat")).unwrap(),
        "Name LanGame\n"
    );

    let _ = fs::remove_dir_all(test_root);
}

#[test]
fn astroneer_native_template_enables_console_only_with_a_password() {
    let schema_defaults = collect_schema_defaults_from_schema_json(
        Some(include_str!("../../../modules/astroneer/schema.json")),
        SchemaDefaultContext {
            instance_id: Some("astroneer-test"),
            instance_name: Some("ASTRONEER Test"),
        },
    )
    .expect("ASTRONEER schema defaults");
    let generated_password = schema_defaults["console_password"]
        .as_str()
        .expect("generated console password");
    assert!(!generated_password.is_empty());
    let template = include_str!(
        "../../../modules/astroneer/templates/Astro/Saved/Config/WindowsServer/AstroServerSettings.ini.hbs"
    );
    let root = Path::new("C:/LanGame/tests");
    for (password, port, expected_port) in [
        (None, 1234, 1234),
        (Some("console-test-password"), 31234, 31234),
        (Some(""), 31234, 0),
        (Some("   "), 31234, 0),
    ] {
        let settings: Map<String, Value> = password
            .map(|value| {
                (
                    String::from("console_password"),
                    Value::String(value.into()),
                )
            })
            .into_iter()
            .collect();
        let ports = [PortBinding {
            name: String::from("console"),
            protocol: String::from("tcp"),
            port,
        }];
        let rendered = render_template_text(
            template,
            &TemplateRenderContext {
                instance_root: root,
                config_dir: root,
                install_root: root,
                data_dir: root,
                logs_dir: root,
                saves_dir: root,
                instance_id: "astroneer-test",
                instance_name: "ASTRONEER Test",
                module_id: "astroneer",
                bind_ip: "0.0.0.0",
                autostart: false,
                schema_defaults: &schema_defaults,
                settings: &settings,
                ports: &ports,
            },
        );
        let rendered = rendered.expect("render Astroneer template");
        let lines: Vec<_> = rendered.lines().collect();
        assert!(lines.contains(&format!("ConsolePort={expected_port}").as_str()));
        assert!(lines.contains(
            &format!("ConsolePassword={}", password.unwrap_or(generated_password)).as_str()
        ));
        assert!(!rendered.contains("{{"));
        assert!(!rendered.contains("}}"));
    }
}

#[test]
fn resolve_template_token_uses_schema_defaults_for_missing_settings() {
    let mut schema_defaults = Map::new();
    schema_defaults.insert(String::from("vac_secure"), Value::Bool(true));
    schema_defaults.insert(
        String::from("bookmark_host"),
        Value::String(String::from("server.example.com")),
    );

    let settings = Map::from_iter([(
        String::from("server_name"),
        Value::String(String::from("深海 & \"Crew\" <One>")),
    )]);
    let ports: Vec<PortBinding> = Vec::new();
    let root = Path::new("C:/LanGame/tests");
    let context = TemplateRenderContext {
        instance_root: root,
        config_dir: root,
        install_root: root,
        data_dir: root,
        logs_dir: root,
        saves_dir: root,
        instance_id: "instance-1",
        instance_name: "Instance 1",
        module_id: "unturned",
        bind_ip: "0.0.0.0",
        autostart: false,
        schema_defaults: &schema_defaults,
        settings: &settings,
        ports: &ports,
    };

    assert_eq!(
        resolve_template_token("settings.vac_secure", &context),
        Some(String::from("true"))
    );
    assert_eq!(
        resolve_template_token("json.settings.bookmark_host", &context),
        Some(String::from("\"server.example.com\""))
    );

    assert_eq!(
        resolve_template_token("xml.settings.server_name", &context),
        Some(String::from("深海 &amp; &quot;Crew&quot; &lt;One&gt;"))
    );
}

#[test]
fn ark_repeated_native_entries_end_with_a_line_separator() {
    let settings = Map::from_iter([
        (
            String::from("override_player_level_engram_points"),
            Value::String(String::from("8\n12\n16")),
        ),
        (
            String::from("auto_managed_mod_ids"),
            Value::String(String::from("731604991\n895711211")),
        ),
    ]);

    assert_eq!(
        render_ark_prefixed_lines(
            &settings,
            "override_player_level_engram_points",
            "OverridePlayerLevelEngramPoints=",
        ),
        "OverridePlayerLevelEngramPoints=8\nOverridePlayerLevelEngramPoints=12\nOverridePlayerLevelEngramPoints=16\n"
    );
    assert_eq!(
        render_ark_prefixed_lines(&settings, "auto_managed_mod_ids", "ModIDS="),
        "ModIDS=731604991\nModIDS=895711211\n"
    );
}
