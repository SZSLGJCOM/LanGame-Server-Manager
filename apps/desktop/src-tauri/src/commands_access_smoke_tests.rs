use super::*;

#[path = "commands_configuration_icons_tests.rs"]
mod configuration_icons_tests;

fn current_test_process_identity() -> ProcessIdentity {
    inspect_process_identity(std::process::id())
        .expect("inspect test process")
        .expect("test process is running")
}

#[tokio::test(flavor = "current_thread")]
async fn dst_materialized_world_overrides_execute_without_lua_globals()
-> Result<(), Box<dyn std::error::Error>> {
    fn evaluate(lua: &mlua::Lua, path: &Path) -> Result<mlua::Table, Box<dyn std::error::Error>> {
        let source = fs::read_to_string(path)?;
        Ok(lua
            .load(&source)
            .set_name(path.to_string_lossy())
            .set_environment(lua.create_table()?)
            .eval::<mlua::Table>()?)
    }

    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("dst-preset-lua");
    let env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let result = async {
        let settings = isolated_smoke_app_settings(&run_root)?;
        prepare_fake_dontstarve_install(&settings)?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::from_storage(
                &bootstrap_storage().expect("bootstrap isolated fixture storage"),
            ))
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock tauri app");
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let provisioning = create_fake_module_instance(
            app.state::<DesktopState>(),
            "dontstarve",
            "DST preset Lua",
        )
        .await?;
        let storage = bootstrap_storage()?;
        let details = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
        let mut update = dontstarve_enable_caves_update(&details)?;
        let mut values: serde_json::Map<String, Value> =
            serde_json::from_str(&update.settings_json)?;
        values.insert(
            String::from("caves_world_overrides_extra"),
            serde_json::json!("custom_rule = false,"),
        );
        update.settings_json = serde_json::to_string(&values)?;
        let details = update_instance(&storage.paths, update).await?;
        let cluster_root = dontstarve_cluster_root_from_config_file_path(&details.config_file_path);
        let master_path = cluster_root.join("Master/worldgenoverride.lua");
        let caves_path = cluster_root.join("Caves/worldgenoverride.lua");
        let lua = mlua::Lua::new();

        let master = evaluate(&lua, &master_path)?;
        let master_overrides: mlua::Table = master.get("overrides")?;
        assert_eq!(
            master.get::<String>("settings_preset")?,
            "SURVIVAL_TOGETHER"
        );
        assert_eq!(master_overrides.get::<String>("healthpenalty")?, "always");
        assert_eq!(master_overrides.get::<String>("world_size")?, "default");
        let caves = evaluate(&lua, &caves_path)?;
        let caves_overrides: mlua::Table = caves.get("overrides")?;
        assert_eq!(caves.get::<String>("settings_preset")?, "DST_CAVE");
        assert_eq!(caves_overrides.get::<String>("world_size")?, "default");
        assert!(!caves_overrides.get::<bool>("custom_rule")?);
        assert_eq!(caves_overrides.get::<String>("healthpenalty")?, "always");

        values.insert(
            String::from("master_settings_preset"),
            serde_json::json!("ENDLESS"),
        );
        values.insert(
            String::from("master_worldgen_preset"),
            serde_json::json!("LIGHTS_OUT"),
        );
        values.insert(String::from("master_day"), serde_json::json!("onlyday"));
        values.insert(
            String::from("master_world_overrides_extra"),
            serde_json::json!(
                "healthpenalty = \"always\",\nday = \"default\",\ncustom_rule = false,"
            ),
        );
        let mut update = dontstarve_enable_caves_update(&details)?;
        update.settings_json = serde_json::to_string(&values)?;
        update_instance(&storage.paths, update).await?;

        let master = evaluate(&lua, &master_path)?;
        let overrides: mlua::Table = master.get("overrides")?;
        assert_eq!(master.get::<String>("settings_preset")?, "ENDLESS");
        assert_eq!(master.get::<String>("worldgen_preset")?, "LIGHTS_OUT");
        assert_eq!(overrides.get::<String>("healthpenalty")?, "always");
        assert_eq!(overrides.get::<String>("day")?, "default");
        assert!(!overrides.get::<bool>("custom_rule")?);
        assert!(matches!(
            overrides.get::<mlua::Value>("world_size")?,
            mlua::Value::Nil
        ));
        // Master playstyle changes propagate shared rules while Caves retain
        // their own generation preset and explicit extra values.
        let caves = evaluate(&lua, &caves_path)?;
        let caves_overrides: mlua::Table = caves.get("overrides")?;
        assert_eq!(caves.get::<String>("settings_preset")?, "DST_CAVE");
        assert_eq!(caves.get::<String>("worldgen_preset")?, "DST_CAVE");
        assert_eq!(caves_overrides.get::<String>("world_size")?, "default");
        assert_eq!(
            caves_overrides.get::<String>("portalresurection")?,
            "always"
        );
        assert_eq!(caves_overrides.get::<String>("day")?, "default");
        assert!(!caves_overrides.get::<bool>("custom_rule")?);
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(env_guard);
    let cleanup = fs::remove_dir_all(&run_root);
    result?;
    cleanup?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Palworld dedicated server before exercising the Tauri access-management, launch-preview, and operator snapshot commands; optionally set LANGAME_PALWORLD_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_palworld_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = palworld_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(palworld_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("palworld")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Palworld install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\Pal\Binaries\Win64\PalServer-Win64-Shipping-Cmd.exe")
            || install_result
                .executable_path
                .ends_with("/Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"),
        "Palworld install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access Palworld"),
                module_id: String::from("palworld"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "palworld");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            palworld_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);

    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access Palworld")
    );
    assert_eq!(
        updated_settings
            .get("server_password")
            .and_then(Value::as_str),
        Some("pal-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("admin_password")
            .and_then(Value::as_str),
        Some("pal-admin-safe")
    );
    assert_eq!(
        updated_settings
            .get("community_server")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("rcon_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("rest_api_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .ok_or("palworld config root missing")?;
    let install_root = PathBuf::from(&install_result.install_root);
    let materialized_config_root = install_root
        .join("Pal")
        .join("Saved")
        .join("Config")
        .join("WindowsServer");
    let saves_root = install_root
        .join("Pal")
        .join("Saved")
        .join("SaveGames")
        .join("0")
        .join(&instance_id);
    assert_eq!(PathBuf::from(&updated_details.saves_path), saves_root);

    let game_user_settings_text = normalize_newlines(&fs::read_to_string(
        config_root.join("GameUserSettings.ini"),
    )?);
    let palworld_settings_text = normalize_newlines(&fs::read_to_string(
        config_root.join("PalWorldSettings.ini"),
    )?);
    let materialized_game_user_settings = normalize_newlines(&fs::read_to_string(
        materialized_config_root.join("GameUserSettings.ini"),
    )?);
    let materialized_palworld_settings = normalize_newlines(&fs::read_to_string(
        materialized_config_root.join("PalWorldSettings.ini"),
    )?);
    assert_eq!(
        materialized_game_user_settings, game_user_settings_text,
        "Palworld materialized GameUserSettings.ini drifted from the rendered config copy"
    );
    assert_eq!(
        materialized_palworld_settings, palworld_settings_text,
        "Palworld materialized PalWorldSettings.ini drifted from the rendered config copy"
    );
    assert!(
        game_user_settings_text.contains(&format!("DedicatedServerName={instance_id}")),
        "Palworld GameUserSettings.ini missing DedicatedServerName:\n{}",
        game_user_settings_text
    );
    for expected_fragment in [
        "ServerName=\"Command Access Palworld\"",
        "ServerDescription=\"Command-lane managed Palworld server\"",
        "ServerPlayerMaxNum=24",
        "AdminPassword=\"pal-admin-safe\"",
        "ServerPassword=\"pal-safe-pass\"",
        "PublicIP=\"203.0.113.24\"",
        "PublicPort=18211",
        "RCONEnabled=True",
        "RCONPort=28575",
        "Region=\"Asia\"",
        "bUseAuth=True",
        "BanListURL=\"https://ops.example.com/palworld/banlist.txt\"",
        "RESTAPIEnabled=True",
        "RESTAPIPort=18212",
        "bShowPlayerList=True",
        "CrossplayPlatforms=(Steam,Xbox,PS5)",
        "bIsUseBackupSaveData=True",
        "LogFormatType=Json",
        "bAllowClientMod=False",
        "bIsShowJoinLeftMessage=False",
        "ChatPostLimitPerMinute=12",
        "bEnableFastTravel=False",
        "bIsPvP=True",
        "bEnablePlayerToPlayerDamage=True",
        "bEnableFriendlyFire=True",
        "DeathPenalty=ItemAndEquipment",
        "DenyTechnologyList=(\"TechnologyA\",\"TechnologyB\")",
    ] {
        assert!(
            palworld_settings_text.contains(expected_fragment),
            "Palworld settings missing `{expected_fragment}`:\n{}",
            palworld_settings_text
        );
    }
    assert!(
        !config_root.join("launch-palworld.bat").exists(),
        "Palworld should not materialize a generated launch batch script through the command lane"
    );

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access Palworld");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert_eq!(
        preview.args,
        vec![
            String::from("-port=18211"),
            String::from("-players=24"),
            String::from("-logformat=json"),
            String::from("-publiclobby"),
            String::from("-publicip=203.0.113.24"),
            String::from("-publicport=18211"),
            String::from("-useperfthreads"),
            String::from("-NoAsyncLoadingThread"),
            String::from("-UseMultithreadForDS"),
            String::from("-NumberOfWorkerThreadsServer=12"),
        ],
        "Palworld preview should derive launch args from the updated instance intent"
    );

    let operator_snapshot = command_result(
        read_palworld_operator_snapshot(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(operator_snapshot.instance_id, instance_id);
    let game_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "game")
        .expect("Palworld game operator probe");
    assert_eq!(game_probe.transport, "udp");
    assert!(game_probe.enabled);
    assert!(game_probe.configured);
    assert_eq!(game_probe.endpoint.as_deref(), Some("127.0.0.1:18211"));
    assert_eq!(game_probe.status, "waiting_for_start");

    let rcon_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "rcon")
        .expect("Palworld RCON operator probe");
    assert_eq!(rcon_probe.transport, "tcp");
    assert!(rcon_probe.enabled);
    assert!(rcon_probe.configured);
    assert_eq!(rcon_probe.endpoint.as_deref(), Some("127.0.0.1:28575"));
    assert_eq!(rcon_probe.status, "waiting_for_start");

    let rest_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "rest_api")
        .expect("Palworld REST API operator probe");
    assert_eq!(rest_probe.transport, "http");
    assert!(rest_probe.enabled);
    assert!(rest_probe.configured);
    assert_eq!(
        rest_probe.endpoint.as_deref(),
        Some("http://127.0.0.1:18212/")
    );
    assert_eq!(rest_probe.status, "waiting_for_start");

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real 7 Days to Die dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_7DTD_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_sevendaystodie_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = sevendaystodie_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(sevendaystodie_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("sevendaystodie")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "7DTD install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\7DaysToDieServer.exe")
            || install_result
                .executable_path
                .ends_with("/7DaysToDieServer.exe"),
        "7DTD install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access 7DTD"),
                module_id: String::from("sevendaystodie"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "sevendaystodie");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            sevendaystodie_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);

    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access 7DTD")
    );
    assert_eq!(
        updated_settings
            .get("web_dashboard_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("telnet_password")
            .and_then(Value::as_str),
        Some("telnet-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("admin_users")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        updated_settings
            .get("command_permissions")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(3)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .ok_or("7DTD config root missing")?;
    let instance_root = config_root
        .parent()
        .map(PathBuf::from)
        .ok_or("7DTD instance root missing")?;
    let saves_root = PathBuf::from(&updated_details.saves_path);
    assert_eq!(saves_root, instance_root.join("saves"));

    let serverconfig_text =
        normalize_newlines(&fs::read_to_string(config_root.join("serverconfig.xml"))?);
    let normalized_serverconfig = serverconfig_text.replace('\\', "/");
    assert!(
        normalized_serverconfig
            .contains(r#"<property name="ServerName" value="Command Access 7DTD"/>"#),
        "7DTD serverconfig missing updated server name:\n{}",
        serverconfig_text
    );
    assert!(
            normalized_serverconfig.contains(
                r#"<property name="ServerDescription" value="Command-lane managed 7 Days to Die server"/>"#
            ),
            "7DTD serverconfig missing updated description:\n{}",
            serverconfig_text
        );
    assert!(
        normalized_serverconfig
            .contains(r#"<property name="ServerPassword" value="7dtd-safe-pass"/>"#),
        "7DTD serverconfig missing updated password:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="ServerVisibility" value="2"/>"#),
        "7DTD serverconfig missing updated visibility:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="ServerMaxPlayerCount" value="10"/>"#),
        "7DTD serverconfig missing updated player cap:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="ServerPort" value="27900"/>"#),
        "7DTD serverconfig missing updated game port:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="WebDashboardEnabled" value="true"/>"#),
        "7DTD serverconfig missing dashboard toggle:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="WebDashboardPort" value="18080"/>"#),
        "7DTD serverconfig missing updated dashboard port:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig
            .contains(r#"<property name="WebDashboardUrl" value="https://ops.example.com/7dtd"/>"#),
        "7DTD serverconfig missing updated dashboard URL:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="EnableMapRendering" value="true"/>"#),
        "7DTD serverconfig missing map rendering toggle:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="TelnetEnabled" value="true"/>"#),
        "7DTD serverconfig missing telnet toggle:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="TelnetPort" value="18081"/>"#),
        "7DTD serverconfig missing updated telnet port:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig
            .contains(r#"<property name="TelnetPassword" value="telnet-safe-pass"/>"#),
        "7DTD serverconfig missing updated telnet password:\n{}",
        serverconfig_text
    );
    for expected_line in [
        r#"<property name="TelnetFailedLoginLimit" value="3"/>"#,
        r#"<property name="TelnetFailedLoginsBlocktime" value="30"/>"#,
        r#"<property name="ServerAllowCrossplay" value="false"/>"#,
        r#"<property name="IgnoreEOSSanctions" value="true"/>"#,
        r#"<property name="MaxChunkAge" value="14"/>"#,
        r#"<property name="SaveDataLimit" value="2048"/>"#,
        r#"<property name="SandboxCode" value="AAAJABJACJADJARFBNC"/>"#,
        r#"<property name="AllowSpawnNearFriend" value="1"/>"#,
        r#"<property name="CameraRestrictionMode" value="1"/>"#,
        r#"<property name="MaxQueuedMeshLayers" value="750"/>"#,
    ] {
        assert!(
            normalized_serverconfig.contains(expected_line),
            "7DTD serverconfig missing `{expected_line}`:\n{}",
            serverconfig_text
        );
    }
    assert!(
        normalized_serverconfig.contains(&format!(
            r#"<property name="UserDataFolder" value="{}"/>"#,
            instance_root.to_string_lossy().replace('\\', "/")
        )),
        "7DTD serverconfig missing instance-root UserDataFolder:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig
            .contains(r#"<property name="AdminFileName" value="serveradmin.xml"/>"#),
        "7DTD serverconfig missing serveradmin pointer:\n{}",
        serverconfig_text
    );
    assert!(
        normalized_serverconfig.contains(r#"<property name="GameName" value="OpsAccess7DTD"/>"#),
        "7DTD serverconfig missing updated world name:\n{}",
        serverconfig_text
    );
    assert!(
        !saves_root.join("serverconfig.xml").exists(),
        "7DTD live save root should not gain a second serverconfig copy"
    );

    let serveradmin_text =
        normalize_newlines(&fs::read_to_string(config_root.join("serveradmin.xml"))?);
    let live_serveradmin_text =
        normalize_newlines(&fs::read_to_string(saves_root.join("serveradmin.xml"))?);
    assert_eq!(
        live_serveradmin_text, serveradmin_text,
        "7DTD live serveradmin.xml drifted from the rendered config copy"
    );
    assert!(serveradmin_text.contains("<users>"));
    assert!(serveradmin_text.contains("<commands>"));
    assert!(!serveradmin_text.contains("<admins>"));
    assert!(!serveradmin_text.contains("<permissions>"));
    for expected_line in [
        r#"<user platform="Steam" userid="76561198077777777" name="Host Lead" permission_level="0" />"#,
        r#"<user platform="Steam" userid="76561198000000000" name="Ops &quot;Two&quot;" permission_level="5" />"#,
        r#"<group steamID="103582791434672565" name="Steam Universe" permission_level_default="1000" permission_level_mod="0" />"#,
        r#"<user platform="Steam" userid="76561198011111111" name="Trusted Friend" />"#,
        r#"<group steamID="103582791434672566" name="Weekend Survivors" />"#,
        r#"<blacklisted platform="Steam" userid="76561198033333333" name="Raider" unbandate="2025-01-01" reason="Griefing &amp; spam" />"#,
        r#"<permission cmd="help" permission_level="500" />"#,
        r#"<permission cmd="listplayerids" permission_level="1000" />"#,
        r#"<permission cmd="say" permission_level="0" />"#,
    ] {
        assert!(
            serveradmin_text.contains(expected_line),
            "7DTD serveradmin.xml missing `{expected_line}`:\n{}",
            serveradmin_text
        );
    }

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access 7DTD");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedNativeWindow
    );
    let normalized_args = preview
        .args
        .iter()
        .map(|arg| arg.replace('\\', "/"))
        .collect::<Vec<_>>();
    assert_eq!(
        normalized_args,
        vec![
            String::from("-quit"),
            String::from("-batchmode"),
            String::from("-nographics"),
            format!(
                "-configfile={}",
                config_root
                    .join("serverconfig.xml")
                    .to_string_lossy()
                    .replace('\\', "/")
            ),
            String::from("-dedicated"),
        ],
        "7DTD preview should keep deriving launch args from the rendered config file"
    );

    let operator_snapshot = command_result(
        read_sevendaystodie_operator_snapshot(app.state::<DesktopState>(), instance_id.clone())
            .await,
    )?;
    assert_eq!(operator_snapshot.instance_id, instance_id);
    assert_eq!(operator_snapshot.operator_host, "127.0.0.1");

    let game_udp_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "game_udp")
        .expect("7DTD game UDP operator probe");
    assert_eq!(game_udp_probe.transport, "udp");
    assert!(game_udp_probe.enabled);
    assert!(game_udp_probe.configured);
    assert_eq!(game_udp_probe.endpoint.as_deref(), Some("127.0.0.1:27900"));
    assert_eq!(game_udp_probe.status, "waiting_for_start");

    let game_tcp_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "game_tcp")
        .expect("7DTD game TCP operator probe");
    assert_eq!(game_tcp_probe.transport, "tcp");
    assert!(game_tcp_probe.enabled);
    assert!(game_tcp_probe.configured);
    assert_eq!(game_tcp_probe.endpoint.as_deref(), Some("127.0.0.1:27900"));
    assert_eq!(game_tcp_probe.status, "waiting_for_start");

    let web_dashboard_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "web_dashboard")
        .expect("7DTD Web Dashboard operator probe");
    assert_eq!(web_dashboard_probe.transport, "http");
    assert!(web_dashboard_probe.enabled);
    assert!(web_dashboard_probe.configured);
    assert_eq!(
        web_dashboard_probe.endpoint.as_deref(),
        Some("http://127.0.0.1:18080/")
    );
    assert_eq!(web_dashboard_probe.status, "waiting_for_start");

    let telnet_probe = operator_snapshot
        .services
        .iter()
        .find(|service| service.key == "telnet")
        .expect("7DTD Telnet operator probe");
    assert_eq!(telnet_probe.transport, "tcp");
    assert!(telnet_probe.enabled);
    assert!(telnet_probe.configured);
    assert_eq!(telnet_probe.endpoint.as_deref(), Some("127.0.0.1:18081"));
    assert_eq!(telnet_probe.status, "waiting_for_start");

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real V Rising dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_VRISING_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_vrising_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = vrising_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(vrising_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("vrising")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "V Rising install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\VRisingServer.exe")
            || install_result
                .executable_path
                .ends_with("/VRisingServer.exe"),
        "V Rising install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access V Rising"),
                module_id: String::from("vrising"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "vrising");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            vrising_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);
    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access V Rising")
    );
    assert_eq!(
        updated_settings
            .get("server_password")
            .and_then(Value::as_str),
        Some("vrising-safe-pass")
    );
    assert_eq!(
        updated_settings.get("max_admins").and_then(Value::as_i64),
        Some(6)
    );
    assert_eq!(
        updated_settings
            .get("rcon_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("rcon_password")
            .and_then(Value::as_str),
        Some("vrising-rcon-safe")
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .ok_or("vrising config root missing")?;
    let instance_root = config_root
        .parent()
        .map(PathBuf::from)
        .ok_or("vrising instance root missing")?;
    let settings_root = config_root.join("Settings");
    let live_settings_root = instance_root.join("Settings");
    let saves_root = PathBuf::from(&updated_details.saves_path);
    assert_eq!(saves_root, instance_root.join("Saves"));
    assert!(
        !live_settings_root.starts_with(&saves_root),
        "V Rising live settings should remain outside the declared save root"
    );

    let host_settings_json: Value = serde_json::from_str(&fs::read_to_string(
        settings_root.join("ServerHostSettings.json"),
    )?)?;
    let live_host_settings_json: Value = serde_json::from_str(&fs::read_to_string(
        live_settings_root.join("ServerHostSettings.json"),
    )?)?;
    assert_eq!(
        host_settings_json["Name"],
        Value::String(String::from("Command Access V Rising"))
    );
    assert_eq!(
        host_settings_json["Description"],
        Value::String(String::from("Command-lane managed V Rising room."))
    );
    assert_eq!(host_settings_json["MaxConnectedUsers"], Value::from(20));
    assert_eq!(host_settings_json["MaxConnectedAdmins"], Value::from(6));
    assert_eq!(
        host_settings_json["Password"],
        Value::String(String::from("vrising-safe-pass"))
    );
    assert_eq!(host_settings_json["HideIPAddress"], Value::Bool(true));
    assert_eq!(host_settings_json["ListOnSteam"], Value::Bool(true));
    assert_eq!(host_settings_json["ListOnEOS"], Value::Bool(true));
    assert_eq!(
        host_settings_json["SaveName"],
        Value::String(String::from("OpsRealm"))
    );
    assert_eq!(host_settings_json["ServerFps"], Value::from(45));
    assert_eq!(
        host_settings_json["AdminOnlyDebugEvents"],
        Value::Bool(false)
    );
    assert_eq!(host_settings_json["API"]["Enabled"], Value::Bool(true));
    assert_eq!(host_settings_json["Rcon"]["Enabled"], Value::Bool(true));
    assert_eq!(
        host_settings_json["Rcon"]["Password"],
        Value::String(String::from("vrising-rcon-safe"))
    );
    assert_eq!(host_settings_json["Rcon"]["Port"], Value::from(25575));
    assert_eq!(live_host_settings_json, host_settings_json);

    let admin_list_text =
        normalize_newlines(&fs::read_to_string(settings_root.join("adminlist.txt"))?);
    let live_admin_list_text = normalize_newlines(&fs::read_to_string(
        live_settings_root.join("adminlist.txt"),
    )?);
    assert_eq!(
        admin_list_text.lines().collect::<Vec<_>>(),
        vec!["76561198000000001", "76561198000000002"]
    );
    assert_eq!(live_admin_list_text, admin_list_text);

    let ban_list_text = normalize_newlines(&fs::read_to_string(settings_root.join("banlist.txt"))?);
    let live_ban_list_text =
        normalize_newlines(&fs::read_to_string(live_settings_root.join("banlist.txt"))?);
    assert_eq!(
        ban_list_text.lines().collect::<Vec<_>>(),
        vec!["76561198000000003"]
    );
    assert_eq!(live_ban_list_text, ban_list_text);
    assert!(
        !saves_root.join("adminlist.txt").exists(),
        "V Rising admin list should not drift into the save root"
    );
    assert!(
        !saves_root.join("banlist.txt").exists(),
        "V Rising ban list should not drift into the save root"
    );

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access V Rising");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert_eq!(
        preview.args,
        vec![
            String::from("-persistentDataPath"),
            instance_root.to_string_lossy().into_owned(),
            String::from("-bindAddress"),
            String::from("127.0.0.1"),
        ],
        "V Rising preview should only derive the persistent data path and bind address from the updated instance intent"
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Core Keeper dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_COREKEEPER_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_corekeeper_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = corekeeper_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(corekeeper_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("corekeeper")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Core Keeper install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\CoreKeeperServer.exe")
            || install_result
                .executable_path
                .ends_with("/CoreKeeperServer.exe"),
        "Core Keeper install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access Core Keeper"),
                module_id: String::from("corekeeper"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "corekeeper");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            corekeeper_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);
    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access Core Keeper")
    );
    assert_eq!(
        updated_settings.get("game_id").and_then(Value::as_str),
        Some("CoreKeeperRelay12345")
    );
    assert_eq!(
        updated_settings.get("max_players").and_then(Value::as_i64),
        Some(16)
    );
    assert_eq!(
        updated_settings
            .get("direct_connection_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("join_password")
            .and_then(Value::as_str),
        Some("corekeeper-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("allowed_platform_code")
            .and_then(Value::as_i64),
        Some(4)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let instance_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .ok_or("corekeeper instance root missing")?;
    let config_root = instance_root.join("config");
    let data_root = instance_root.join("data");
    let worlds_root = data_root.join("worlds");
    assert_eq!(PathBuf::from(&updated_details.saves_path), worlds_root);
    assert!(
        !data_root.join("Admins.json").starts_with(&worlds_root),
        "Core Keeper access files should live in the data root, not inside the worlds root"
    );

    let config_json: Value =
        serde_json::from_str(&fs::read_to_string(config_root.join("ServerConfig.json"))?)?;
    let live_config_json: Value =
        serde_json::from_str(&fs::read_to_string(data_root.join("ServerConfig.json"))?)?;
    assert_eq!(
        config_json["worldName"],
        Value::String(String::from("Command Access Core Keeper"))
    );
    assert_eq!(
        config_json["gameId"],
        Value::String(String::from("CoreKeeperRelay12345"))
    );
    assert_eq!(config_json["world"], Value::from(3));
    assert_eq!(config_json["maxNumberPlayers"], Value::from(16));
    assert_eq!(config_json["seasonOverride"], Value::from(6));
    assert_eq!(live_config_json, config_json);

    let admins_json: Value =
        serde_json::from_str(&fs::read_to_string(config_root.join("Admins.json"))?)?;
    let live_admins_json: Value =
        serde_json::from_str(&fs::read_to_string(data_root.join("Admins.json"))?)?;
    assert_eq!(admins_json.as_array().map(Vec::len), Some(2));
    assert_eq!(admins_json[0]["steamId"], json!(76561198077777777_u64));
    assert_eq!(admins_json[1]["steamId"], json!(76561198000000000_u64));
    assert_eq!(live_admins_json, admins_json);

    let bans_json: Value =
        serde_json::from_str(&fs::read_to_string(config_root.join("PlayerBans.json"))?)?;
    let live_bans_json: Value =
        serde_json::from_str(&fs::read_to_string(data_root.join("PlayerBans.json"))?)?;
    assert_eq!(bans_json["banList"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        bans_json["banList"][0]["steamId"],
        json!(76561198011111111_u64)
    );
    assert_eq!(live_bans_json, bans_json);
    assert!(
        !worlds_root.join("Admins.json").exists(),
        "Core Keeper admin access files should not drift into the worlds save root"
    );
    assert!(
        !worlds_root.join("PlayerBans.json").exists(),
        "Core Keeper bans file should not drift into the worlds save root"
    );

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    let expected_runtime_log = instance_root
        .join("logs")
        .join("CoreKeeperServer.log")
        .to_string_lossy()
        .into_owned();
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access Core Keeper");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert_eq!(
        preview.args,
        vec![
            String::from("-batchmode"),
            String::from("-logfile"),
            expected_runtime_log,
            String::from("-datapath"),
            data_root.to_string_lossy().into_owned(),
            String::from("-ip"),
            String::from("127.0.0.1"),
            String::from("-port"),
            String::from("27015"),
            String::from("-password"),
            String::from("corekeeper-safe-pass"),
            String::from("-allowonlyplatform"),
            String::from("4"),
        ],
        "Core Keeper preview should derive direct-connect flags from the updated instance intent"
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Valheim dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_VALHEIM_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_valheim_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = valheim_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(valheim_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("valheim")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Valheim install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\valheim_server.exe")
            || install_result
                .executable_path
                .ends_with("/valheim_server.exe"),
        "Valheim install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access Valheim"),
                module_id: String::from("valheim"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "valheim");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            valheim_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access Valheim")
    );
    assert_eq!(
        updated_settings.get("world_name").and_then(Value::as_str),
        Some("OpsAccessWorld")
    );
    assert_eq!(
        updated_settings
            .get("server_password")
            .and_then(Value::as_str),
        Some("raid-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("public_server")
            .and_then(Value::as_i64),
        Some(0)
    );
    assert_eq!(
        updated_settings
            .get("crossplay_enabled")
            .and_then(Value::as_bool),
        Some(true)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .ok_or("valheim config root missing")?;
    let saves_root = PathBuf::from(&updated_details.saves_path);
    let expected_admin_list = "76561198000000001\n76561198000000002\n";
    let expected_banned_list = "76561198000000003\n";
    let expected_permitted_list = "76561198000000001\n76561198000000004\n";
    assert_eq!(
        normalize_newlines(&fs::read_to_string(config_root.join("adminlist.txt"))?),
        expected_admin_list
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(config_root.join("bannedlist.txt"))?),
        expected_banned_list
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(config_root.join("permittedlist.txt"))?),
        expected_permitted_list
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(saves_root.join("adminlist.txt"))?),
        expected_admin_list
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(saves_root.join("bannedlist.txt"))?),
        expected_banned_list
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(saves_root.join("permittedlist.txt"))?),
        expected_permitted_list
    );
    assert!(
        !config_root.join("launch-valheim.bat").exists(),
        "Valheim should not materialize a generated launch batch script through the command lane"
    );

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access Valheim");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert_eq!(
        preview.args,
        vec![
            String::from("-nographics"),
            String::from("-batchmode"),
            String::from("-name"),
            String::from("Command Access Valheim"),
            String::from("-port"),
            String::from("2456"),
            String::from("-world"),
            String::from("OpsAccessWorld"),
            String::from("-password"),
            String::from("raid-safe-pass"),
            String::from("-savedir"),
            saves_root.to_string_lossy().into_owned(),
            String::from("-public"),
            String::from("0"),
            String::from("-saveinterval"),
            String::from("1800"),
            String::from("-backups"),
            String::from("4"),
            String::from("-backupshort"),
            String::from("7200"),
            String::from("-backuplong"),
            String::from("43200"),
            String::from("-crossplay"),
        ],
        "Valheim preview should derive from the updated instance intent"
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Project Zomboid dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_PZ_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_project_zomboid_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = project_zomboid_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(project_zomboid_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("projectzomboid")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Project Zomboid install command did not resolve the bundled Java runtime"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\jre64\bin\java.exe")
            || install_result
                .executable_path
                .ends_with("/jre64/bin/java.exe"),
        "Project Zomboid install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access Project Zomboid"),
                module_id: String::from("projectzomboid"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "projectzomboid");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            project_zomboid_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);

    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("server_name").and_then(Value::as_str),
        Some("Command Access Project Zomboid")
    );
    assert_eq!(
        updated_settings.get("open_server").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        updated_settings
            .get("server_password")
            .and_then(Value::as_str),
        Some("fixture-pz-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("admin_username")
            .and_then(Value::as_str),
        Some("opsadmin")
    );
    assert_eq!(
        updated_settings
            .get("admin_password")
            .and_then(Value::as_str),
        Some("ops-admin-safe")
    );
    assert_eq!(
        updated_settings.get("memory_gb").and_then(Value::as_i64),
        Some(6)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let config_root = PathBuf::from(&updated_details.config_file_path)
        .parent()
        .map(PathBuf::from)
        .ok_or("projectzomboid config root missing")?;
    let runtime_home = config_root.join("runtime-home");
    let runtime_server_ini_path = runtime_home
        .join("Zomboid")
        .join("Server")
        .join(format!("{instance_id}.ini"));
    let saves_root = PathBuf::from(&updated_details.saves_path);
    assert_eq!(
        saves_root,
        runtime_home
            .join("Zomboid")
            .join("Saves")
            .join("Multiplayer")
            .join(&instance_id)
    );

    let server_ini_text = normalize_newlines(&fs::read_to_string(config_root.join("server.ini"))?);
    let runtime_server_ini_text = normalize_newlines(&fs::read_to_string(runtime_server_ini_path)?);
    assert_eq!(
        runtime_server_ini_text, server_ini_text,
        "Project Zomboid runtime-home server.ini drifted from rendered config copy"
    );
    for expected_line in [
        "PauseEmpty=false",
        "GlobalChat=false",
        "Open=false",
        "Public=true",
        "PublicName=Command Access Project Zomboid",
        "PublicDescription=Command-lane managed Project Zomboid room",
        "ServerWelcomeMessage=Welcome survivor <LINE> Keep the safehouse locked",
        "DefaultPort=17261",
        "UDPPort=17262",
        "PingLimit=250",
        "MaxPlayers=18",
        "Password=fixture-pz-safe-pass",
        "Map=Muldraugh, KY;RavenCreek;West Point, KY",
        "SpawnPoint=10635,9342,0",
        "AutoCreateUserInWhiteList=true",
        "DisplayUserName=false",
        "ShowFirstAndLastName=true",
        "DoLuaChecksum=false",
        "DenyLoginOnOverloadedServer=false",
        "SteamVAC=false",
        "PVP=false",
        "SafetySystem=false",
        "ShowSafety=false",
        "SleepAllowed=true",
        "SleepNeeded=true",
        "PlayerRespawnWithSelf=true",
        "PlayerRespawnWithOther=true",
        "VoiceEnable=false",
        "VoiceMinDistance=8",
        "VoiceMaxDistance=120",
        "UseTCPForMapDownloads=true",
        "WorkshopItems=1234567890;9876543210",
        "Mods=BaseMod;SafehouseTools;MapHelper",
        "RCONPort=28015",
        "RCONPassword=fixture-pz-rcon-safe",
    ] {
        assert!(
            server_ini_text.lines().any(|line| line == expected_line),
            "Project Zomboid server.ini missing `{expected_line}`:\n{}",
            server_ini_text
        );
    }
    assert!(
        !saves_root.join("server.ini").exists(),
        "Project Zomboid save root should not gain a second server.ini copy"
    );
    for legacy_name in [
        "launch-projectzomboid.bat",
        "StartServer64.lgs.bat",
        "prepare-projectzomboid.ps1",
    ] {
        assert!(
            !config_root.join(legacy_name).exists(),
            "Project Zomboid command flow should not materialize legacy `{legacy_name}`"
        );
    }

    let preview = command_result(
        preview_instance_launch(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(preview.instance_id, instance_id);
    assert_eq!(preview.instance_name, "Command Access Project Zomboid");
    assert!(preview.executable_exists);
    assert!(preview.ready_to_launch);
    assert_eq!(preview.window_policy, ProcessWindowPolicy::Background);
    assert_eq!(
        preview.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert!(!preview.uses_script_entrypoint);
    assert!(
        preview.executable_path.ends_with(r"\jre64\bin\java.exe")
            || preview.executable_path.ends_with("/jre64/bin/java.exe"),
        "Project Zomboid preview picked the wrong executable:\n{}",
        preview.executable_path
    );
    let expected_user_home = runtime_home.to_string_lossy().replace('\\', "/");
    assert!(
        preview
            .args
            .iter()
            .any(|arg| arg.replace('\\', "/") == format!("-Duser.home={expected_user_home}")),
        "Project Zomboid preview should direct Java user.home into runtime-home:\n{:#?}",
        preview.args
    );
    assert!(
        preview.args.iter().any(|arg| arg == "-Xms6g"),
        "Project Zomboid preview is missing the updated Xms memory flag:\n{:#?}",
        preview.args
    );
    assert!(
        preview.args.iter().any(|arg| arg == "-Xmx6g"),
        "Project Zomboid preview is missing the updated Xmx memory flag:\n{:#?}",
        preview.args
    );
    assert!(
        preview
            .args
            .windows(2)
            .any(|window| window == ["-servername", instance_id.as_str()]),
        "Project Zomboid preview is missing the servername bootstrap:\n{:#?}",
        preview.args
    );
    assert!(
        preview
            .args
            .windows(2)
            .any(|window| window == ["-adminusername", "opsadmin"]),
        "Project Zomboid preview is missing the updated admin username:\n{:#?}",
        preview.args
    );
    assert!(
        preview
            .args
            .windows(2)
            .any(|window| window == ["-adminpassword", "ops-admin-safe"]),
        "Project Zomboid preview is missing the updated admin password:\n{:#?}",
        preview.args
    );
    assert!(
        preview
            .args
            .windows(2)
            .any(|window| window == ["-port", "17261"]),
        "Project Zomboid preview is missing the updated game port:\n{:#?}",
        preview.args
    );
    assert!(
        preview
            .args
            .windows(2)
            .any(|window| window == ["-udpport", "17262"]),
        "Project Zomboid preview is missing the updated direct UDP port:\n{:#?}",
        preview.args
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real DST dedicated server before exercising the Tauri access-management and launch-preview commands; optionally set LANGAME_DST_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_dst_access_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = dontstarve_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(dontstarve_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("dontstarve")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "DST install command did not resolve the dedicated server executable"
    );
    assert!(
        install_result
            .executable_path
            .ends_with(r"\bin64\dontstarve_dedicated_server_nullrenderer_x64.exe")
            || install_result
                .executable_path
                .ends_with("/bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
        "DST install command resolved the wrong executable:\n{}",
        install_result.executable_path
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Access DST"),
                module_id: String::from("dontstarve"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(initial_details.summary.module_id, "dontstarve");
    assert_eq!(initial_details.summary.bind_ip, "0.0.0.0");
    assert!(!initial_details.summary.autostart);

    command_result(
        crate::commands::commands_autostart::update_instance_autostart(
            app.state::<DesktopState>(),
            instance_id.clone(),
            true,
        )
        .await,
    )?;

    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            dontstarve_access_update(&initial_details)?,
        )
        .await,
    )?;
    assert_eq!(updated_details.summary.id, instance_id);
    assert_eq!(updated_details.summary.bind_ip, "127.0.0.1");
    assert!(updated_details.summary.autostart);
    assert!(updated_details.backup_uses_declared_saves_path);

    let updated_settings =
        serde_json::from_str::<serde_json::Map<String, Value>>(&updated_details.settings_json)?;
    assert_eq!(
        updated_settings.get("cluster_name").and_then(Value::as_str),
        Some("Command Access DST")
    );
    assert_eq!(
        updated_settings
            .get("enable_caves")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        updated_settings
            .get("cluster_password")
            .and_then(Value::as_str),
        Some("fixture-dst-safe-pass")
    );
    assert_eq!(
        updated_settings
            .get("steam_group_only")
            .and_then(Value::as_bool),
        Some(true)
    );

    let tracked_instance = app
        .state::<DesktopState>()
        .app_state
        .read()
        .unwrap()
        .instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .expect("updated instance should remain in desktop state");
    assert_eq!(tracked_instance.bind_ip, "127.0.0.1");
    assert!(tracked_instance.autostart);

    let cluster_root =
        dontstarve_cluster_root_from_config_file_path(&updated_details.config_file_path);
    assert_eq!(PathBuf::from(&updated_details.saves_path), cluster_root);

    let cluster_ini = normalize_newlines(&fs::read_to_string(cluster_root.join("cluster.ini"))?);
    for expected_line in [
        "game_mode = endless",
        "max_players = 8",
        "pvp = false",
        "pause_when_empty = true",
        "vote_enabled = false",
        "cluster_name = Command Access DST",
        "cluster_description = Command-lane managed DST cluster",
        "cluster_password = fixture-dst-safe-pass",
        "offline_cluster = false",
        "lan_only_cluster = false",
        "tick_rate = 20",
        "whitelist_slots = 2",
        "cluster_intention = cooperative",
        "autosaver_enabled = true",
        "shard_enabled = true",
        "bind_ip = 127.0.0.1",
        "master_ip = 127.0.0.1",
        "master_port = 11888",
        "steam_group_only = true",
        "steam_group_id = 103582791400000000",
        "steam_group_admins = true",
    ] {
        assert!(
            cluster_ini.lines().any(|line| line == expected_line),
            "DST cluster.ini missing `{expected_line}`:\n{}",
            cluster_ini
        );
    }
    assert!(
        cluster_ini
            .lines()
            .any(|line| line == format!("cluster_key = {instance_id}")),
        "DST cluster.ini missing instance cluster_key:\n{}",
        cluster_ini
    );

    assert_eq!(
        normalize_newlines(&fs::read_to_string(cluster_root.join("cluster_token.txt"))?).trim(),
        "dst-command-token"
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(cluster_root.join("adminlist.txt"))?).trim(),
        "KU_admin_one\nKU_admin_two"
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(cluster_root.join("whitelist.txt"))?).trim(),
        "KU_white_one\nKU_white_two"
    );
    assert_eq!(
        normalize_newlines(&fs::read_to_string(cluster_root.join("blocklist.txt"))?).trim(),
        "KU_blocked_one"
    );

    let master_ini = normalize_newlines(&fs::read_to_string(join_relative(
        &cluster_root,
        "Master/server.ini",
    ))?);
    let caves_ini = normalize_newlines(&fs::read_to_string(join_relative(
        &cluster_root,
        "Caves/server.ini",
    ))?);
    for expected_line in [
        "server_port = 11999",
        "is_master = true",
        "name = Master",
        "master_server_port = 28016",
        "authentication_port = 18766",
    ] {
        assert!(
            master_ini.lines().any(|line| line == expected_line),
            "DST Master/server.ini missing `{expected_line}`:\n{}",
            master_ini
        );
    }
    for expected_line in [
        "server_port = 12000",
        "is_master = false",
        "name = Caves",
        "master_server_port = 28017",
        "authentication_port = 18767",
    ] {
        assert!(
            caves_ini.lines().any(|line| line == expected_line),
            "DST Caves/server.ini missing `{expected_line}`:\n{}",
            caves_ini
        );
    }

    let shared_mod_setup = normalize_newlines(&fs::read_to_string(
        PathBuf::from(&install_result.install_root)
            .join("mods")
            .join("dedicated_server_mods_setup.lua"),
    )?);
    for expected_line in [
        r#"ServerModSetup("1234567890")"#,
        r#"ServerModSetup("9876543210")"#,
        r#"ServerModCollectionSetup("2345678901")"#,
    ] {
        assert!(
            shared_mod_setup.lines().any(|line| line == expected_line),
            "DST shared mod setup missing `{expected_line}`:\n{}",
            shared_mod_setup
        );
    }

    let master_modoverrides = normalize_newlines(&fs::read_to_string(join_relative(
        &cluster_root,
        "Master/modoverrides.lua",
    ))?);
    let caves_modoverrides = normalize_newlines(&fs::read_to_string(join_relative(
        &cluster_root,
        "Caves/modoverrides.lua",
    ))?);
    for expected_fragment in [
        r#"["workshop-1234567890"] = {"#,
        r#"["workshop-9876543210"] = { enabled = true }"#,
        r#"["difficulty"] = "hard","#,
        r#"["enabled"] = true,"#,
        r#"["spawn_rate"] = 2,"#,
    ] {
        assert!(
            master_modoverrides.contains(expected_fragment),
            "DST Master/modoverrides.lua missing `{expected_fragment}`:\n{}",
            master_modoverrides
        );
    }
    for expected_fragment in [
        r#"["workshop-9876543210"] = {"#,
        r#"["cave_setting"] = "safe","#,
        r#"["enabled"] = true,"#,
    ] {
        assert!(
            caves_modoverrides.contains(expected_fragment),
            "DST Caves/modoverrides.lua missing `{expected_fragment}`:\n{}",
            caves_modoverrides
        );
    }

    let descriptors = discover_modules(&storage.paths.modules_root)?;
    let descriptor = command_result(find_descriptor(&descriptors, "dontstarve"))?;
    let module = map_module_details_with_install_state(&storage.settings, descriptor, None);
    let process_plans =
        build_process_launch_plans_for_instance(&storage.settings, &module, &updated_details)?;
    assert_eq!(
        process_plans.len(),
        2,
        "expected Master + Caves launch plans"
    );
    assert!(
        process_plans
            .iter()
            .all(|plan| plan.launch_plan.uses_private_runtime)
    );
    let master_plan = process_plans
        .iter()
        .find(|plan| plan.process_key == "master")
        .expect("master launch plan");
    let caves_plan = process_plans
        .iter()
        .find(|plan| plan.process_key == "caves")
        .expect("caves launch plan");
    assert!(master_plan.launch_plan.executable_exists);
    assert!(master_plan.launch_plan.ready_to_launch);
    assert!(caves_plan.launch_plan.executable_exists);
    assert!(caves_plan.launch_plan.ready_to_launch);
    assert_eq!(
        master_plan.launch_plan.window_policy,
        ProcessWindowPolicy::Background
    );
    assert_eq!(
        master_plan.launch_plan.host_surface,
        app_core::ProcessHostSurface::ManagedTerminal
    );
    assert!(
        master_plan
            .launch_plan
            .args
            .windows(2)
            .any(|window| window == ["-shard", "Master"]),
        "DST master launch args missing Master shard:\n{:#?}",
        master_plan.launch_plan.args
    );
    assert!(
        master_plan
            .launch_plan
            .args
            .windows(2)
            .any(|window| window == ["-bind_ip", "127.0.0.1"]),
        "DST master launch args missing bind IP:\n{:#?}",
        master_plan.launch_plan.args
    );
    assert!(
        master_plan
            .launch_plan
            .args
            .windows(2)
            .any(|window| window == ["-port", "11999"]),
        "DST master launch args missing updated master port:\n{:#?}",
        master_plan.launch_plan.args
    );
    assert!(
        caves_plan
            .launch_plan
            .args
            .windows(2)
            .any(|window| window == ["-shard", "Caves"]),
        "DST caves launch args missing Caves shard:\n{:#?}",
        caves_plan.launch_plan.args
    );
    assert!(
        caves_plan
            .launch_plan
            .args
            .windows(2)
            .any(|window| window == ["-port", "12000"]),
        "DST caves launch args missing updated caves port:\n{:#?}",
        caves_plan.launch_plan.args
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.create.request",
        "instance.create.success",
        "instance.update.request",
        "instance.update.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Project Zomboid dedicated server before exercising the Tauri backup commands; optionally set LANGAME_PZ_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_project_zomboid_backup_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = project_zomboid_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(project_zomboid_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("projectzomboid")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Project Zomboid install command did not resolve the bundled Java runtime"
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Backup Project Zomboid"),
                module_id: String::from("projectzomboid"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(details.summary.module_id, "projectzomboid");
    assert!(
        details.backup_uses_declared_saves_path,
        "Project Zomboid instances should declare a stable saves path"
    );
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .any(|instance| instance.id == instance_id),
        "create_instance_record should refresh desktop state"
    );

    let saves_root = PathBuf::from(&details.saves_path);
    let world_file = saves_root.join("map_t.bin");
    let players_file = saves_root.join("players.db");
    let chunk_file = saves_root.join("chunkdata").join("map_01_01.bin");
    let rogue_file = saves_root.join("temp").join("rogue.txt");
    fs::create_dir_all(chunk_file.parent().expect("chunk parent"))?;
    fs::write(&world_file, "world-v1")?;
    fs::write(&players_file, "players-v1")?;
    fs::write(&chunk_file, "chunk-v1")?;

    let created = command_result(
        create_instance_backup(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(created.instance_id, instance_id);
    assert_eq!(created.backup_kind, app_core::InstanceBackupKind::Manual);
    assert_eq!(created.file_count, 3);

    let renamed = command_result(
        rename_instance_backup(
            app.state::<DesktopState>(),
            instance_id.clone(),
            created.backup_id.clone(),
            Some(String::from("Day 1 Baseline")),
        )
        .await,
    )?;
    assert_eq!(renamed.backup_id, created.backup_id);
    assert_eq!(renamed.display_name.as_deref(), Some("Day 1 Baseline"));

    let synthetic_log_path = run_root
        .join("logs")
        .join(format!("{instance_id}-restore-guard.log"));
    let synthetic_log_path_string = synthetic_log_path.to_string_lossy().into_owned();
    let session_id = format!("{instance_id}-restore-guard");
    let process_state = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &instance_id,
            session_id: Some(&session_id),
            process_key: "main",
            display_name: "Server",
            pid: std::process::id(),
            log_path: &synthetic_log_path_string,
            is_primary: true,
        },
        None,
    )
    .await?;
    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime.insert_running(
            details.summary.clone(),
            Some(session_id.clone()),
            vec![ManagedProcess {
                run_id: process_state.run_id,
                process_key: String::from("main"),
                display_name: String::from("Server"),
                pid: std::process::id(),
                process_identity: current_test_process_identity(),
                root_process_identity: current_test_process_identity(),
                log_path: synthetic_log_path_string.clone(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: None,
                hidden_desktop: None,
            }],
        );
    }

    let blocked = restore_instance_backup(
        app.state::<DesktopState>(),
        instance_id.clone(),
        created.backup_id.clone(),
        None,
    )
    .await
    .expect_err("restore should be blocked while the instance has an active run");
    assert_eq!(blocked, "Stop the server before restoring a backup.");

    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime
            .take_running_for_stop(&instance_id)
            .expect("tracked instance should be removable after the guard check");
    }
    mark_instance_process_stopped(
        &storage.paths,
        &instance_id,
        process_state.run_id,
        None,
        false,
    )
    .await?;

    fs::write(&world_file, "world-v2")?;
    fs::remove_file(&players_file)?;
    fs::create_dir_all(rogue_file.parent().expect("rogue parent"))?;
    fs::write(&rogue_file, "rogue-v2")?;

    let restored = command_result(
        restore_instance_backup(
            app.state::<DesktopState>(),
            instance_id.clone(),
            created.backup_id.clone(),
            None,
        )
        .await,
    )?;
    assert_eq!(restored.instance_id, instance_id);
    assert_eq!(restored.backup_id, created.backup_id);
    assert_eq!(PathBuf::from(&restored.saves_path), saves_root);
    assert_eq!(restored.restored_file_count, created.file_count);
    assert_eq!(restored.restored_total_bytes, created.total_bytes);
    assert_eq!(fs::read_to_string(&world_file)?, "world-v1");
    assert_eq!(fs::read_to_string(&players_file)?, "players-v1");
    assert_eq!(fs::read_to_string(&chunk_file)?, "chunk-v1");
    assert!(
        !rogue_file.exists(),
        "restore should remove files that were not present in the chosen backup"
    );

    let safeguard_root = PathBuf::from(&restored.safeguard_backup_path).join("saves");
    assert_eq!(
        fs::read_to_string(safeguard_root.join("map_t.bin"))?,
        "world-v2"
    );
    assert!(
        !safeguard_root.join("players.db").exists(),
        "pre-restore safeguard backup should preserve the missing players file"
    );
    assert_eq!(
        fs::read_to_string(safeguard_root.join("temp").join("rogue.txt"))?,
        "rogue-v2"
    );

    let listed_after_restore = command_result(
        list_instance_backups(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(listed_after_restore.len(), 2);
    let manual_backup = listed_after_restore
        .iter()
        .find(|entry| entry.backup_id == created.backup_id)
        .expect("manual backup still listed");
    assert_eq!(
        manual_backup.display_name.as_deref(),
        Some("Day 1 Baseline")
    );
    assert_eq!(
        manual_backup.backup_kind,
        app_core::InstanceBackupKind::Manual
    );
    let safeguard_backup = listed_after_restore
        .iter()
        .find(|entry| entry.backup_id == restored.safeguard_backup_id)
        .expect("safeguard backup listed");
    assert_eq!(
        safeguard_backup.backup_kind,
        app_core::InstanceBackupKind::PreRestore
    );

    let deleted = command_result(
        delete_instance_backup(
            app.state::<DesktopState>(),
            instance_id.clone(),
            created.backup_id.clone(),
        )
        .await,
    )?;
    assert_eq!(deleted.backup_id, created.backup_id);
    assert!(
        !PathBuf::from(&deleted.backup_path).exists(),
        "delete_instance_backup should remove the backup directory"
    );

    let remaining = command_result(
        list_instance_backups(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].backup_id, restored.safeguard_backup_id);

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.backup.created",
        "instance.backup.renamed",
        "instance.backup.restored",
        "instance.backup.deleted",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real Project Zomboid dedicated server before exercising the Tauri delete-instance command; optionally set LANGAME_PZ_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_project_zomboid_delete_instance_command() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = project_zomboid_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(project_zomboid_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("projectzomboid")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "Project Zomboid install command did not resolve the bundled Java runtime"
    );

    let library = app_storage::read_library_program_install(&storage.paths, "projectzomboid")
        .await?
        .unwrap();
    let library_executable = PathBuf::from(&install_result.executable_path);
    let executable_before = fs::read(&library_executable)?;
    let clean_manifest = library.install_root.join(".langame-clean-package.json");
    let manifest_before = fs::read(&clean_manifest)?;

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Delete Project Zomboid"),
                module_id: String::from("projectzomboid"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    let instance_root = PathBuf::from(&details.config_file_path)
        .parent()
        .and_then(|path| path.parent())
        .expect("config file path should live under the instance root")
        .to_path_buf();
    let saves_root = PathBuf::from(&details.saves_path);
    let world_file = saves_root.join("map_t.bin");
    let players_file = saves_root.join("players.db");
    fs::create_dir_all(&saves_root)?;
    fs::write(&world_file, "world-delete")?;
    fs::write(&players_file, "players-delete")?;
    let backup = command_result(
        create_instance_backup(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;

    let synthetic_log_path = run_root
        .join("logs")
        .join(format!("{instance_id}-delete-guard.log"));
    let synthetic_log_path_string = synthetic_log_path.to_string_lossy().into_owned();
    let session_id = format!("{instance_id}-delete-guard");
    let process_state = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &instance_id,
            session_id: Some(&session_id),
            process_key: "main",
            display_name: "Server",
            pid: std::process::id(),
            log_path: &synthetic_log_path_string,
            is_primary: true,
        },
        None,
    )
    .await?;
    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime.insert_running(
            details.summary.clone(),
            Some(session_id.clone()),
            vec![ManagedProcess {
                run_id: process_state.run_id,
                process_key: String::from("main"),
                display_name: String::from("Server"),
                pid: std::process::id(),
                process_identity: current_test_process_identity(),
                root_process_identity: current_test_process_identity(),
                log_path: synthetic_log_path_string.clone(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: None,
                hidden_desktop: None,
            }],
        );
    }

    let blocked = delete_instance_record(app.handle().clone(), instance_id.clone())
        .await
        .expect_err("delete should be blocked while the instance has an active run");
    assert!(
        blocked.contains("Stop the server before deleting the instance."),
        "expected running guard to mention the delete block, got {blocked}"
    );

    let blocked_archive = archive_instance_record(app.handle().clone(), instance_id.clone())
        .await
        .expect_err("archive should be blocked while the instance has an active run");
    assert!(
        blocked_archive.contains("Stop the server before archiving the instance."),
        "{blocked_archive}"
    );
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .any(|instance| instance.id == instance_id)
    );

    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime
            .take_running_for_stop(&instance_id)
            .expect("tracked instance should be removable after the guard check");
    }
    mark_instance_process_stopped(
        &storage.paths,
        &instance_id,
        process_state.run_id,
        None,
        false,
    )
    .await?;

    let deleted =
        command_result(archive_instance_record(app.handle().clone(), instance_id.clone()).await)?;
    assert_eq!(deleted.instance_id, instance_id);
    assert!(deleted.saves_archived_with_instance_root);
    assert_eq!(deleted.preserved_external_saves_path, None);
    assert_eq!(
        deleted.previous_instance_root,
        instance_root.to_string_lossy().into_owned()
    );

    let archived_root = PathBuf::from(
        deleted
            .archived_instance_root
            .clone()
            .expect("archive should retain the instance data"),
    );
    let archived_saves_root = {
        let relative_saves_path = PathBuf::from(&deleted.effective_saves_path)
            .strip_prefix(&instance_root)?
            .to_path_buf();
        archived_root.join(relative_saves_path)
    };
    assert!(
        !instance_root.exists(),
        "instance root should be removed from its original location"
    );
    assert_eq!(
        fs::read_to_string(archived_saves_root.join("map_t.bin"))?,
        "world-delete"
    );
    assert_eq!(
        fs::read_to_string(archived_saves_root.join("players.db"))?,
        "players-delete"
    );
    assert!(
        archived_root
            .join("backups")
            .join(&backup.backup_id)
            .join("backup.json")
            .exists(),
        "instance archive should keep the managed backup history"
    );
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .all(|instance| instance.id != instance_id),
        "delete_instance_record should refresh desktop state"
    );

    let listed_instances =
        command_result(list_instances_from_storage(app.state::<DesktopState>()).await)?;
    assert!(
        listed_instances
            .iter()
            .all(|instance| instance.id != instance_id),
        "deleted instance should disappear from the storage-backed instance list"
    );

    let missing_details =
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone())
            .await
            .expect_err("deleted instance should no longer have readable details");
    assert!(
        missing_details.contains("was not found"),
        "expected missing-instance error after delete, got {missing_details}"
    );

    use super::commands_storage_management as management;
    management::restore_instance_archive(
        app.handle().clone(),
        management::InstanceArchiveInput {
            archive_id: deleted.archive_id.clone(),
        },
    )
    .await?;
    assert_eq!(fs::read_to_string(&world_file)?, "world-delete");
    assert_eq!(fs::read_to_string(&players_file)?, "players-delete");
    let before_delete = management::list_instance_archives(app.state::<DesktopState>()).await?;
    let archive_ids = before_delete
        .archives
        .iter()
        .map(|entry| entry.archive_id.clone())
        .collect::<Vec<_>>();
    let permanently_deleted =
        delete_instance_record(app.handle().clone(), instance_id.clone()).await?;
    assert_eq!(
        PathBuf::from(permanently_deleted.deleted_instance_root),
        instance_root
    );
    assert!(!instance_root.exists());
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .iter()
            .all(|instance| instance.id != instance_id)
    );
    let after_delete = management::list_instance_archives(app.state::<DesktopState>()).await?;
    assert_eq!(
        after_delete
            .archives
            .iter()
            .map(|entry| entry.archive_id.clone())
            .collect::<Vec<_>>(),
        archive_ids
    );
    assert!(after_delete.pending_deletions.is_empty());
    assert_eq!(fs::read(&library_executable)?, executable_before);
    assert_eq!(fs::read(clean_manifest)?, manifest_before);
    let library_after = app_storage::read_library_program_install(&storage.paths, "projectzomboid")
        .await?
        .unwrap();
    assert_eq!(library_after.id, library.id);
    assert_eq!(library_after.install_root, library.install_root);
    assert_eq!(library_after.install_state, InstallState::Installed);
    assert_eq!(library_after.current_version, library.current_version);
    assert_eq!(
        library_after.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(library_after.owner_instance_id, None);

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.archive.request",
        "instance.archive.blocked_running",
        "instance.archive.success",
        "instance.delete.request",
        "instance.delete.blocked_running",
        "instance.delete.success",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires network access and installs or updates a real DST dedicated server before exercising the Tauri DST import and backup commands; optionally set LANGAME_DST_SMOKE_GAMES_ROOT to reuse an existing games root"]
async fn smoke_dst_import_and_backup_commands() -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = dontstarve_command_smoke_run_root();
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));

    save_app_settings(dontstarve_command_smoke_settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let log_path = desktop_app_log_path(&storage);

    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");

    let install_result = command_result(
        install_module_game(app.state::<DesktopState>(), String::from("dontstarve")).await,
    )?;
    assert!(
        install_result.executable_exists,
        "DST install command did not resolve the dedicated server executable"
    );

    let provisioning = command_result(
        create_instance_record_inner(
            app.state::<DesktopState>(),
            CreateInstanceInput {
                name: String::from("Command Import DST"),
                module_id: String::from("dontstarve"),
            },
        )
        .await,
    )?;
    let instance_id = provisioning.summary.id.clone();
    let initial_details = command_result(
        read_instance_details_from_storage(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    let updated_details = command_result(
        update_instance_record(
            app.state::<DesktopState>(),
            dontstarve_enable_caves_update(&initial_details)?,
        )
        .await,
    )?;
    let target_cluster_root =
        dontstarve_cluster_root_from_config_file_path(&updated_details.config_file_path);
    assert_eq!(
        PathBuf::from(&updated_details.saves_path),
        target_cluster_root
    );
    assert!(updated_details.backup_uses_declared_saves_path);
    assert!(
        join_relative(&target_cluster_root, "Caves/server.ini").exists(),
        "enabling caves should materialize the Caves shard config"
    );

    let managed_files = [
        "cluster.ini",
        "cluster_token.txt",
        "Master/server.ini",
        "Master/worldgenoverride.lua",
        "Master/modoverrides.lua",
        "Caves/server.ini",
        "Caves/worldgenoverride.lua",
        "Caves/modoverrides.lua",
    ];
    let managed_snapshots = snapshot_text_fixture_files(&target_cluster_root, &managed_files)?;

    let target_preexisting_files = [
        ("Master/save/shardindex", "target-master-before"),
        ("Master/save/obsolete.txt", "obsolete-master"),
        ("Caves/save/shardindex", "target-caves-before"),
        ("Caves/save/session/old-session", "old-caves-session"),
    ];
    write_text_fixture_files(&target_cluster_root, &target_preexisting_files)?;

    let source_config_root = run_root.join("source-cluster").join("config");
    let source_cluster_root = source_config_root.join("clusters").join("main");
    let source_fixture_files = [
        ("Master/server.ini", "source-managed-master"),
        (
            "Master/worldgenoverride.lua",
            "source-managed-master-worldgen",
        ),
        ("Master/modoverrides.lua", "source-managed-master-mods"),
        ("Caves/server.ini", "source-managed-caves"),
        (
            "Caves/worldgenoverride.lua",
            "source-managed-caves-worldgen",
        ),
        ("Caves/modoverrides.lua", "source-managed-caves-mods"),
        ("Master/save/shardindex", "source-master-index"),
        (
            "Master/save/session/SOURCE-MASTER/0000000125",
            "source-master-session",
        ),
        (
            "Master/save/session/SOURCE-MASTER/0000000125.meta",
            "source-master-session-meta",
        ),
        ("Caves/save/shardindex", "source-caves-index"),
        (
            "Caves/save/session/SOURCE-CAVES/0000000125",
            "source-caves-session",
        ),
        (
            "Caves/save/session/SOURCE-CAVES/0000000125.meta",
            "source-caves-session-meta",
        ),
    ];
    write_text_fixture_files(&source_cluster_root, &source_fixture_files)?;
    let imported_source_files = [
        ("Master/save/shardindex", "source-master-index"),
        (
            "Master/save/session/SOURCE-MASTER/0000000125",
            "source-master-session",
        ),
        (
            "Master/save/session/SOURCE-MASTER/0000000125.meta",
            "source-master-session-meta",
        ),
        ("Caves/save/shardindex", "source-caves-index"),
        (
            "Caves/save/session/SOURCE-CAVES/0000000125",
            "source-caves-session",
        ),
        (
            "Caves/save/session/SOURCE-CAVES/0000000125.meta",
            "source-caves-session-meta",
        ),
    ];
    let imported_file_count = imported_source_files.len();
    let imported_total_bytes = imported_source_files
        .iter()
        .map(|(_, contents)| contents.len() as u64)
        .sum::<u64>();

    let source_input_path = source_config_root.to_string_lossy().into_owned();

    let synthetic_log_path = run_root
        .join("logs")
        .join(format!("{instance_id}-dst-import-guard.log"));
    let synthetic_log_path_string = synthetic_log_path.to_string_lossy().into_owned();
    let session_id = format!("{instance_id}-dst-import-guard");
    let process_state = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &instance_id,
            session_id: Some(&session_id),
            process_key: "master",
            display_name: "Master",
            pid: std::process::id(),
            log_path: &synthetic_log_path_string,
            is_primary: true,
        },
        None,
    )
    .await?;
    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime.insert_running(
            updated_details.summary.clone(),
            Some(session_id.clone()),
            vec![ManagedProcess {
                run_id: process_state.run_id,
                process_key: String::from("master"),
                display_name: String::from("Master"),
                pid: std::process::id(),
                process_identity: current_test_process_identity(),
                root_process_identity: current_test_process_identity(),
                log_path: synthetic_log_path_string.clone(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: None,
                hidden_desktop: None,
            }],
        );
    }

    let blocked = import_dontstarve_world_data(
        app.state::<DesktopState>(),
        instance_id.clone(),
        source_input_path.clone(),
        Some("zh-CN".into()),
    )
    .await
    .expect_err("DST import should be blocked while the instance is running");
    assert_eq!(blocked, "Stop the server before importing DST world data.");

    {
        let state = app.state::<DesktopState>();
        let mut runtime = state.runtime_supervisor.lock().unwrap();
        runtime
            .take_running_for_stop(&instance_id)
            .expect("tracked DST instance should be removable after the guard check");
    }
    mark_instance_process_stopped(
        &storage.paths,
        &instance_id,
        process_state.run_id,
        None,
        false,
    )
    .await?;

    let import_result = command_result(
        import_dontstarve_world_data(
            app.state::<DesktopState>(),
            instance_id.clone(),
            source_input_path,
            Some("zh-CN".into()),
        )
        .await,
    )?;
    assert_eq!(import_result.instance_id, instance_id);
    assert_eq!(
        PathBuf::from(&import_result.source_cluster_path),
        source_cluster_root
    );
    assert_eq!(
        PathBuf::from(&import_result.target_cluster_path),
        target_cluster_root
    );
    assert!(import_result.imported_master);
    assert!(import_result.imported_caves);
    assert_eq!(import_result.copied_file_count, imported_file_count);
    assert_eq!(import_result.copied_total_bytes, imported_total_bytes);

    for (relative, expected_contents) in imported_source_files {
        assert_eq!(
            read_text_fixture_file(&target_cluster_root, relative)?,
            expected_contents
        );
    }
    assert!(
        !join_relative(&target_cluster_root, "Master/save/obsolete.txt").exists(),
        "DST import should remove stale Master world files that are not present in the source"
    );
    assert!(
        !join_relative(&target_cluster_root, "Caves/save/session/old-session").exists(),
        "DST import should remove stale Caves world files that are not present in the source"
    );
    for (relative, expected_contents) in &managed_snapshots {
        assert_eq!(
            read_text_fixture_file(&target_cluster_root, relative)?,
            *expected_contents,
            "DST import should keep managed config untouched: {relative}"
        );
    }

    let import_safeguard_cluster_root = PathBuf::from(&import_result.safeguard_path).join("saves");
    for (relative, expected_contents) in target_preexisting_files {
        assert_eq!(
            read_text_fixture_file(&import_safeguard_cluster_root, relative)?,
            expected_contents
        );
    }
    for (relative, expected_contents) in &managed_snapshots {
        assert_eq!(
            read_text_fixture_file(&import_safeguard_cluster_root, relative)?,
            *expected_contents,
            "DST import safeguard should capture the managed config before import: {relative}"
        );
    }

    let listed_after_import = command_result(
        list_instance_backups(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(listed_after_import.len(), 1);
    assert_eq!(
        PathBuf::from(&listed_after_import[0].backup_path),
        PathBuf::from(&import_result.safeguard_path)
    );

    let backup = command_result(
        create_instance_backup(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(backup.instance_id, instance_id);
    assert_eq!(backup.backup_kind, app_core::InstanceBackupKind::Manual);
    assert_eq!(PathBuf::from(&backup.saves_path), target_cluster_root);
    assert!(
        backup.file_count >= imported_file_count + managed_files.len(),
        "DST backup should include imported world data and managed cluster files"
    );

    let listed_after_backup = command_result(
        list_instance_backups(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(listed_after_backup.len(), 2);
    assert!(
        listed_after_backup
            .iter()
            .any(|entry| entry.backup_id == backup.backup_id)
    );

    fs::write(
        join_relative(&target_cluster_root, "Master/save/shardindex"),
        "mutated-master-index",
    )?;
    fs::write(
        join_relative(&target_cluster_root, "cluster.ini"),
        "mutated-cluster",
    )?;
    fs::remove_file(join_relative(&target_cluster_root, "Caves/save/shardindex"))?;
    fs::write(
        join_relative(&target_cluster_root, "Caves/save/rogue.txt"),
        "rogue-caves-save",
    )?;

    let restored = command_result(
        restore_instance_backup(
            app.state::<DesktopState>(),
            instance_id.clone(),
            backup.backup_id.clone(),
            Some("zh-CN".into()),
        )
        .await,
    )?;
    assert_eq!(restored.instance_id, instance_id);
    assert_eq!(restored.backup_id, backup.backup_id);
    assert_eq!(PathBuf::from(&restored.saves_path), target_cluster_root);
    assert_eq!(restored.restored_file_count, backup.file_count);
    assert_eq!(restored.restored_total_bytes, backup.total_bytes);
    assert_eq!(
        read_text_fixture_file(&target_cluster_root, "Master/save/shardindex")?,
        "source-master-index"
    );
    assert_eq!(
        read_text_fixture_file(&target_cluster_root, "Caves/save/shardindex")?,
        "source-caves-index"
    );
    assert_eq!(
        read_text_fixture_file(&target_cluster_root, "cluster.ini")?,
        managed_snapshots
            .iter()
            .find(|(relative, _)| relative == "cluster.ini")
            .expect("cluster.ini snapshot")
            .1
    );
    assert!(
        !join_relative(&target_cluster_root, "Caves/save/rogue.txt").exists(),
        "DST restore should remove files that were added after the backup"
    );

    let restore_safeguard_root = PathBuf::from(&restored.safeguard_backup_path).join("saves");
    assert_eq!(
        read_text_fixture_file(&restore_safeguard_root, "Master/save/shardindex")?,
        "mutated-master-index"
    );
    assert_eq!(
        read_text_fixture_file(&restore_safeguard_root, "cluster.ini")?,
        "mutated-cluster"
    );
    assert!(
        !join_relative(&restore_safeguard_root, "Caves/save/shardindex").exists(),
        "restore safeguard should preserve the deleted DST world file state"
    );
    assert_eq!(
        read_text_fixture_file(&restore_safeguard_root, "Caves/save/rogue.txt")?,
        "rogue-caves-save"
    );

    let listed_after_restore = command_result(
        list_instance_backups(app.state::<DesktopState>(), instance_id.clone()).await,
    )?;
    assert_eq!(listed_after_restore.len(), 3);
    let manual_backup = listed_after_restore
        .iter()
        .find(|entry| entry.backup_id == backup.backup_id)
        .expect("manual DST backup still listed");
    assert_eq!(
        manual_backup.backup_kind,
        app_core::InstanceBackupKind::Manual
    );
    let safeguard_backup = listed_after_restore
        .iter()
        .find(|entry| entry.backup_id == restored.safeguard_backup_id)
        .expect("DST pre-restore safeguard backup listed");
    assert_eq!(
        safeguard_backup.backup_kind,
        app_core::InstanceBackupKind::PreRestore
    );

    let actions = read_desktop_log_actions(&log_path)?;
    for expected in [
        "instance.dst_world_import.request",
        "instance.dst_world_import.success",
        "instance.backup.created",
        "instance.backup.restored",
    ] {
        assert!(
            actions.iter().any(|action| action == expected),
            "desktop app log should include `{expected}`; got {:?}",
            actions
        );
    }

    Ok(())
}
