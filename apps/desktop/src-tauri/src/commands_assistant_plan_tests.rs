use super::*;

#[test]
fn crc32_ieee_matches_standard_check_value() {
    assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
}

#[test]
fn maps_steam_review_summary_without_review_bodies() {
    let summary = map_steam_review_summary(
        1_623_730,
        SteamAppReviewsQuerySummary {
            review_score: Some(8),
            review_score_desc: Some(String::from("Very Positive")),
            total_positive: Some(8_700),
            total_negative: Some(1_300),
            total_reviews: Some(10_000),
        },
    )
    .expect("review summary");

    assert_eq!(summary.app_id, 1_623_730);
    assert_eq!(summary.review_score, Some(8));
    assert_eq!(summary.review_score_desc, "Very Positive");
    assert_eq!(summary.total_positive, 8_700);
    assert_eq!(summary.total_negative, 1_300);
    assert_eq!(summary.total_reviews, 10_000);
    assert_eq!(summary.positive_percent, 87);
    assert_eq!(
        summary.source_url,
        "https://store.steampowered.com/app/1623730/#app_reviews_hash"
    );
}

#[test]
fn battleye_rcon_packet_uses_little_endian_crc_header() {
    let packet = battleye_rcon_wrap_payload(&[0x00, b'p', b'w']);
    assert_eq!(&packet[0..2], b"BE");
    assert_eq!(packet[6], 0xFF);
    let expected = crc32_ieee(&[0x00, b'p', b'w']).to_le_bytes();
    assert_eq!(&packet[2..6], expected.as_slice());
    assert_eq!(&packet[7..], &[0x00, b'p', b'w']);
}

#[test]
fn broadcast_message_validation_rejects_unsafe_batches() {
    assert!(validate_instance_broadcast_message("").is_err());
    assert!(validate_instance_broadcast_message("one\ntwo").is_err());
    assert!(validate_instance_broadcast_message("one\ttwo").is_err());
    assert!(validate_instance_broadcast_message("Restart soon; quit").is_err());
    assert!(validate_instance_broadcast_message("Restart && quit").is_err());
    assert_eq!(
        validate_instance_broadcast_message("  Restart in five minutes.  ").unwrap(),
        "Restart in five minutes."
    );
}

#[test]
fn broadcast_candidate_normalization_removes_assistant_labels() {
    assert_eq!(
        normalize_ai_broadcast_candidate("Conclusion: LanGame AI功能联调测试广播"),
        "LanGame AI功能联调测试广播"
    );
    assert_eq!(
        normalize_ai_broadcast_candidate("**Conclusion**\nLanGame AI功能联调测试广播"),
        "LanGame AI功能联调测试广播"
    );
    assert_eq!(
        normalize_ai_broadcast_candidate("\"LanGame AI功能联调测试广播\""),
        "LanGame AI功能联调测试广播"
    );
    assert_eq!(
        normalize_ai_broadcast_candidate("结论：LanGame AI功能联调测试广播"),
        "LanGame AI功能联调测试广播"
    );
    assert_eq!(
        normalize_ai_broadcast_candidate("\u{7ed3}\u{8bba}\u{ff1a}LanGame AI broadcast test"),
        "LanGame AI broadcast test"
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_fenced_json() {
    let plan = parse_assistant_operation_plan_response(
            "```json\n{\"action\":\"start_server\",\"instanceId\":\"terraria-1\",\"reason\":\"用户要求开 Terraria 服务器\"}\n```",
        )
        .expect("parse operation plan");

    assert_eq!(plan.action, AssistantOperationAction::StartServer);
    assert_eq!(plan.instance_id.as_deref(), Some("terraria-1"));
    assert_eq!(plan.reason.as_deref(), Some("用户要求开 Terraria 服务器"));
}

#[test]
fn assistant_operation_plan_parser_ignores_thinking_and_explanatory_text() {
    let plan = parse_assistant_operation_plan_response(
        r#"<think>I should explain this first: {"action":"install_server"}</think>
The safe plan follows.
```json
{"action":"none","reason":"This is only a status question."}
```
No changes were made."#,
    )
    .expect("parse the single visible operation plan");

    assert_eq!(plan.action, AssistantOperationAction::None);
    assert_eq!(
        plan.reason.as_deref(),
        Some("This is only a status question.")
    );
}

#[test]
fn assistant_operation_plan_parser_rejects_multiple_visible_plans() {
    let error = parse_assistant_operation_plan_response(
        r#"{"action":"none"}
{"action":"start_server","instanceId":"minecraft-1"}"#,
    )
    .expect_err("multiple plans must remain ambiguous");

    assert!(error.contains("multiple operation JSON objects"));
}

#[test]
fn assistant_operation_plan_parser_rejects_invalid_and_valid_visible_plans_as_ambiguous() {
    let error = parse_assistant_operation_plan_response(
        r#"{"action":"install_fun_mod","workshopItemIds":{"id":"351325790"}}
{"action":"none"}"#,
    )
    .expect_err("invalid and valid visible plans must remain ambiguous");

    assert!(error.contains("multiple operation JSON objects"), "{error}");
}

#[test]
fn assistant_operation_plan_safe_parser_downgrades_ambiguous_chat_to_none() {
    for response in [
        "I can explain the server status, but this is not an operation plan.",
        r#"{"action":"start_server"} {"action":"install_server"}"#,
        "```json\n{malformed}\n```",
    ] {
        let (plan, parse_error) = parse_assistant_operation_plan_safely(response);
        assert_eq!(plan.action, AssistantOperationAction::None);
        assert!(parse_error.is_some());
    }
}

#[test]
fn assistant_confirmation_tokens_are_single_use_and_cleared_on_context_change() {
    let _guard = command_smoke_lock().blocking_lock();
    clear_assistant_pending_operations().expect("reset confirmation fixture");
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: AssistantProviderSettings {
            provider: String::from("ollama"),
            model: String::from("qwen"),
            base_url: String::from("http://127.0.0.1:11434/v1/"),
            api_key: String::new(),
        },
        prompt: String::from("Start the selected server"),
        context: None,
        selected_instance_id: Some(String::from("minecraft-1")),
        selected_module_id: Some(String::from("minecraft")),
    };
    let plan = parse_assistant_operation_plan_response(
        r#"{"action":"start_server","instanceId":"minecraft-1","moduleId":"minecraft"}"#,
    )
    .expect("parse preview plan");
    let summary = summarize_assistant_operation_plan(&input, &plan);
    let before = current_unix_ms().min(u128::from(u64::MAX)) as u64;
    let (token, expires_at) = store_assistant_pending_operation(
        &input,
        plan,
        None,
        summary.clone(),
        0,
        std::sync::Arc::new(
            commands_assistant_ops::AssistantTaskContract::capture(&input, None)
                .expect("test task contract"),
        ),
    )
    .expect("store pending plan");
    assert_eq!(token.len(), 32);
    assert!(expires_at > before);
    assert!(expires_at <= before + 121_000);

    let pending = take_assistant_pending_operation(&token, &summary, &input.settings)
        .expect("consume matching confirmation");
    assert_eq!(pending.summary, summary);
    assert!(take_assistant_pending_operation(&token, &pending.summary, &input.settings).is_err());

    let cleared_plan = parse_assistant_operation_plan_response(
        r#"{"action":"start_server","instanceId":"minecraft-1","moduleId":"minecraft"}"#,
    )
    .expect("parse second preview plan");
    let (cleared_token, _) = store_assistant_pending_operation(
        &input,
        cleared_plan,
        None,
        summary.clone(),
        0,
        std::sync::Arc::new(
            commands_assistant_ops::AssistantTaskContract::capture(&input, None)
                .expect("test task contract"),
        ),
    )
    .expect("store confirmation before context change");
    assert_eq!(
        clear_assistant_pending_operations().expect("clear confirmations"),
        1
    );
    let error = take_assistant_pending_operation(&cleared_token, &summary, &input.settings)
        .expect_err("a context change must invalidate an earlier confirmation");
    assert!(error.contains("expired or was already used"));
}

#[test]
fn assistant_broadcast_confirmation_binds_the_exact_generated_text() {
    let _guard = command_smoke_lock().blocking_lock();
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: AssistantProviderSettings {
            provider: String::from("ollama"),
            model: String::from("qwen"),
            base_url: String::from("http://127.0.0.1:11434/v1"),
            api_key: String::new(),
        },
        prompt: String::from("Broadcast planned maintenance"),
        context: None,
        selected_instance_id: Some(String::from("minecraft-1")),
        selected_module_id: Some(String::from("minecraft")),
    };
    let plan = parse_assistant_operation_plan_response(
        r#"{"action":"broadcast","instanceId":"minecraft-1","moduleId":"minecraft","broadcastIntent":"planned maintenance"}"#,
    )
    .expect("parse broadcast plan");
    let message = String::from("Maintenance in 10 minutes: return to \"safe zone\" \\ home");
    assert_eq!(
        validate_instance_broadcast_message(&message).expect("valid exact broadcast"),
        message
    );
    let prepared = AssistantPreparedBroadcast {
        instance_id: String::from("minecraft-1"),
        module_id: String::from("minecraft"),
        message: message.clone(),
        provider: String::from("ollama"),
        model: String::from("qwen"),
    };
    let summary = summarize_assistant_operation_preview(&input, &plan, Some(&prepared));
    let encoded_message = serde_json::to_string(&message).expect("encode exact broadcast");
    assert!(summary.contains(&format!("Exact broadcast text: {encoded_message}")));

    let (token, _) = store_assistant_pending_operation(
        &input,
        plan,
        Some(prepared),
        summary.clone(),
        0,
        std::sync::Arc::new(
            commands_assistant_ops::AssistantTaskContract::capture(&input, None)
                .expect("test task contract"),
        ),
    )
    .expect("store exact broadcast preview");
    let pending = take_assistant_pending_operation(&token, &summary, &input.settings)
        .expect("consume exact broadcast preview");
    let stored = pending
        .prepared_broadcast
        .expect("pending preview should preserve generated broadcast");
    assert_eq!(stored.message, message);
}

#[test]
fn assistant_confirmation_rejects_summary_changes_without_exposing_values() {
    let _guard = command_smoke_lock().blocking_lock();
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: AssistantProviderSettings {
            provider: String::from("ollama"),
            model: String::from("qwen"),
            base_url: String::from("http://127.0.0.1:11434/v1"),
            api_key: String::new(),
        },
        prompt: String::from("Apply configuration"),
        context: None,
        selected_instance_id: Some(String::from("minecraft-1")),
        selected_module_id: Some(String::from("minecraft")),
    };
    let credential = ["fixture", "credential"].join("-");
    let response = json!({
        "action": "customize_config",
        "instanceId": "minecraft-1",
        "settingsPatch": {
            "rcon_password": credential.clone(),
            "motd": "hello"
        }
    })
    .to_string();
    let plan =
        parse_assistant_operation_plan_response(&response).expect("parse configuration plan");
    let summary = summarize_assistant_operation_plan(&input, &plan);
    assert!(!summary.contains(&credential));
    assert!(summary.contains("[REDACTED]"));
    assert!(summary.contains("hello"));
    let (token, _) = store_assistant_pending_operation(
        &input,
        plan,
        None,
        summary,
        0,
        std::sync::Arc::new(
            commands_assistant_ops::AssistantTaskContract::capture(&input, None)
                .expect("test task contract"),
        ),
    )
    .expect("store pending plan");
    let error = take_assistant_pending_operation(&token, "tampered summary", &input.settings)
        .expect_err("changed request binding must be rejected");
    assert!(error.contains("does not match"));
}

#[test]
fn assistant_operation_plan_parser_accepts_site_mod_references() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_site_mod",
                "instanceId": "ark-survival-ascended-dedicated-server-1",
                "moduleId": "arksurvivalascended",
                "modReferences": [
                    "https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting"
                ],
                "sourcePaths": [
                    "D:/LanGame/downloads/ExampleMod.zip"
                ],
                "reason": "install a CurseForge mod"
            }"#,
    )
    .expect("parse site mod plan");

    assert_eq!(plan.action, AssistantOperationAction::InstallSiteMod);
    assert_eq!(
        plan.mod_references,
        vec!["https://www.curseforge.com/ark-survival-ascended/mods/devkitlivemodtesting"]
    );
    assert_eq!(
        plan.source_paths,
        vec!["D:/LanGame/downloads/ExampleMod.zip"]
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_port_patch() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "repair_ports",
                "instanceId": "project-zomboid-1",
                "moduleId": "projectzomboid",
                "portPatch": {
                    "game": 16271,
                    "direct": { "port": 16272 }
                },
                "reason": "move occupied Project Zomboid ports"
            }"#,
    )
    .expect("parse port repair plan");

    assert_eq!(plan.action, AssistantOperationAction::RepairPorts);
    let patch = plan.port_patch.as_ref().expect("port patch");
    assert_eq!(patch.get("game").and_then(Value::as_u64), Some(16271));
    assert_eq!(
        patch
            .get("direct")
            .and_then(Value::as_object)
            .and_then(|entry| entry.get("port"))
            .and_then(Value::as_u64),
        Some(16272)
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_gm_runtime_command() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "run_gm_command",
                "instanceId": "asa-1",
                "moduleId": "arksurvivalascended",
                "runtimeCommands": ["GMSummon \"Rex_Character_BP_C\" 150"],
                "transport": "source_rcon",
                "portName": "rcon",
                "passwordSettingKey": "admin_password",
                "enabledSettingKey": "rcon_enabled",
                "reason": "spawn a tamed Rex for the admin event"
            }"#,
    )
    .expect("parse GM runtime command plan");

    assert_eq!(plan.action, AssistantOperationAction::RunGmCommand);
    assert_eq!(
        plan.runtime_commands,
        vec![String::from("GMSummon \"Rex_Character_BP_C\" 150")]
    );
    assert_eq!(plan.transport.as_deref(), Some("source_rcon"));
    assert_eq!(plan.port_name.as_deref(), Some("rcon"));
    assert_eq!(plan.password_setting_key.as_deref(), Some("admin_password"));
    assert_eq!(plan.enabled_setting_key.as_deref(), Some("rcon_enabled"));
}

#[test]
fn assistant_operation_plan_parser_unknown_action_defaults_to_none() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "unsupported_action",
                "instanceId": "minecraft-1",
                "reason": "unsupported action request"
            }"#,
    )
    .expect("parse unknown action plan");

    assert_eq!(plan.action, AssistantOperationAction::None);
    assert_eq!(plan.instance_id.as_deref(), Some("minecraft-1"));
    assert_eq!(plan.reason.as_deref(), Some("unsupported action request"));
}

#[test]
fn assistant_operation_plan_parser_accepts_camel_case_action_aliases() {
    let cases = [
        ("start", AssistantOperationAction::StartServer),
        ("install", AssistantOperationAction::InstallServer),
        ("startServer", AssistantOperationAction::StartServer),
        ("installServer", AssistantOperationAction::InstallServer),
        (
            "applyBeginnerConfig",
            AssistantOperationAction::ApplyBeginnerConfig,
        ),
        ("customizeConfig", AssistantOperationAction::CustomizeConfig),
        ("installFunMod", AssistantOperationAction::InstallFunMod),
        ("installSiteMod", AssistantOperationAction::InstallSiteMod),
        ("repairPorts", AssistantOperationAction::RepairPorts),
        ("runGmCommand", AssistantOperationAction::RunGmCommand),
        ("broadcast", AssistantOperationAction::Broadcast),
    ];

    for (action, expected) in cases {
        let plan = parse_assistant_operation_plan_response(&format!(
            r#"{{"action":"{action}","instanceId":"minecraft-1","reason":"alias validation test"}}"#
        ))
        .expect("parse action alias");

        assert_eq!(plan.action, expected);
        assert_eq!(plan.instance_id.as_deref(), Some("minecraft-1"));
        assert_eq!(plan.reason.as_deref(), Some("alias validation test"));
    }
}

#[test]
fn assistant_operation_plan_parser_accepts_snake_case_field_aliases() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "start_server",
                "instance_id": "minecraft-1",
                "module_id": "minecraft",
                "workshop_item_ids": ["123", "456"],
                "mod_references": ["https://example.com/mod.zip"],
                "source_paths": ["D:/mods/example.zip"],
                "runtime_commands": ["say Hello"],
                "process_key": "admin",
                "port_name": "rcon",
                "password_setting_key": "rcon_password",
                "enabled_setting_key": "rcon_enabled"
            }"#,
    )
    .expect("parse snake case fields");

    assert_eq!(plan.action, AssistantOperationAction::StartServer);
    assert_eq!(plan.instance_id.as_deref(), Some("minecraft-1"));
    assert_eq!(plan.module_id.as_deref(), Some("minecraft"));
    assert_eq!(plan.workshop_item_ids, vec!["123", "456"]);
    assert_eq!(
        plan.mod_references,
        vec![String::from("https://example.com/mod.zip")]
    );
    assert_eq!(plan.source_paths, vec![String::from("D:/mods/example.zip")]);
    assert_eq!(plan.runtime_commands, vec![String::from("say Hello")]);
    assert_eq!(plan.process_key.as_deref(), Some("admin"));
    assert_eq!(plan.port_name.as_deref(), Some("rcon"));
    assert_eq!(plan.password_setting_key.as_deref(), Some("rcon_password"));
    assert_eq!(plan.enabled_setting_key.as_deref(), Some("rcon_enabled"));
}

#[test]
fn assistant_operation_plan_parser_accepts_numeric_workshop_item_ids() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": [351325790, "351325791"]
            }"#,
    )
    .expect("parse numeric workshop item ids");

    assert_eq!(
        plan.workshop_item_ids,
        vec![String::from("351325790"), String::from("351325791")]
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_string_workshop_item_ids() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": "351325790"
            }"#,
    )
    .expect("parse string workshop item ids");

    assert_eq!(plan.workshop_item_ids, vec![String::from("351325790")]);
}

#[test]
fn assistant_operation_plan_parser_accepts_comma_separated_workshop_item_ids() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": "351325790, 351325791,351325792"
            }"#,
    )
    .expect("parse comma separated workshop item ids");

    assert_eq!(
        plan.workshop_item_ids,
        vec![
            String::from("351325790"),
            String::from("351325791"),
            String::from("351325792")
        ]
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_sparse_comma_workshop_item_ids() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": "351325790,  ,351325791,, 351325792 ,"
            }"#,
    )
    .expect("parse sparse comma workshop IDs");

    assert_eq!(
        plan.workshop_item_ids,
        vec![
            String::from("351325790"),
            String::from("351325791"),
            String::from("351325792")
        ]
    );
}

#[test]
fn assistant_operation_plan_parser_accepts_empty_string_workshop_item_ids_as_empty() {
    let plan = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": "   "
            }"#,
    )
    .expect("parse empty workshop string");

    assert_eq!(plan.workshop_item_ids, Vec::<String>::new());
}

#[test]
fn assistant_operation_plan_parser_rejects_mixed_invalid_workshop_item_ids() {
    let error = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": ["351325790", {"id": 42}, true]
            }"#,
    )
    .unwrap_err();

    assert!(
        error.contains("workshopItemIds entries must be string or number"),
        "{error}"
    );
}

#[test]
fn assistant_operation_plan_parser_rejects_non_array_workshop_item_object() {
    let error = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": {"ids": ["351325790"]}
            }"#,
    )
    .unwrap_err();

    assert!(
        error.contains("workshopItemIds must be an array or string ID list"),
        "{error}"
    );
}

#[test]
fn assistant_operation_plan_parser_rejects_blank_workshop_item_array_entry() {
    let error = parse_assistant_operation_plan_response(
        r#"{
                "action": "install_fun_mod",
                "instanceId": "dontstarve-1",
                "moduleId": "dontstarve",
                "workshopItemIds": ["351325790", "  ", "351325791"]
            }"#,
    )
    .unwrap_err();

    assert!(error.contains("non-empty string or number"), "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_config_action_without_settings_patch() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-config-missing-patch");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Missing Settings Patch")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from("Please update settings for the selected Minecraft server."),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("config action should require settingsPatch");

    assert_eq!(
        error,
        String::from("AI did not provide a settingsPatch to apply.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_config_action_with_unknown_settings_keys() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-config-unknown-keys");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Unknown Settings Keys")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: String::from(
                    "Update settings using a mock-settings-patch: {\"no_such_setting\": true, \"another_unknown\": \"value\"}.",
                ),
                context: None,
                selected_instance_id: Some(provisioning.summary.id.clone()),
                selected_module_id: Some(String::from("minecraft")),
            },
        )
        .await
        .expect_err("config action should reject unknown settings keys");

    assert_eq!(
        error,
        String::from("AI settingsPatch did not contain any known setting keys.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_rejects_config_action_with_non_object_settings_patch() {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-config-non-object-patch");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake minecraft install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Non-Object Settings Patch")
            .await
            .expect("seed test instance");

    let error = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: String::from(
                "Update settings using a mock-settings-patch: [\"no-such\", 123].",
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await
    .expect_err("config action should require object settingsPatch");

    assert_eq!(
        error,
        String::from("assistant settingsPatch must be a JSON object")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_applies_config_patch_and_reports_unknown_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-config-success-with-unknown-keys");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning =
        create_fake_minecraft_instance(app.state::<DesktopState>(), "AI Config Success")
            .await
            .expect("seed test instance");
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let baseline = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let baseline_settings: Value = serde_json::from_str(&baseline.settings_json)?;
    let baseline_object = baseline_settings
        .as_object()
        .ok_or_else(|| std::io::Error::other("minecraft settings should be a JSON object"))?;

    let (known_key, known_replacement) = baseline_object
        .iter()
        .next()
        .map(|(key, value)| {
            let updated_value = match value {
                Value::Bool(flag) => Value::Bool(!flag),
                Value::Number(number) if number.as_u64().is_some() => {
                    Value::from(number.as_u64().unwrap_or(0).saturating_add(1))
                }
                Value::Number(number) if number.as_i64().is_some() => {
                    Value::from(number.as_i64().unwrap_or(0).saturating_add(1))
                }
                Value::Number(number) => Value::from(number.as_f64().unwrap_or(1.0) + 1.0),
                Value::String(text) => Value::String(format!("{text}-ai")),
                _ => Value::String(String::from("ai-updated")),
            };
            (key.clone(), updated_value)
        })
        .ok_or_else(|| std::io::Error::other("baseline settings should not be empty"))?;

    let mut patch = serde_json::Map::new();
    patch.insert(known_key.clone(), known_replacement.clone());
    patch.insert(
        String::from("ghost_setting"),
        Value::String(String::from("ignored")),
    );
    let patch = Value::Object(patch);

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: format!("Update settings using a mock-settings-patch:{patch}"),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await?;

    assert!(output.handled);
    assert!(matches!(
        output.action,
        AssistantOperationAction::CustomizeConfig | AssistantOperationAction::ApplyBeginnerConfig
    ));
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));
    assert!(
        output
            .applied_settings_keys
            .iter()
            .any(|key| key == &known_key)
    );
    assert!(
        output
            .rejected_settings_keys
            .iter()
            .any(|key| key == "ghost_setting")
    );

    let updated = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let updated_settings: Value = serde_json::from_str(&updated.settings_json)?;
    assert_eq!(
        updated_settings
            .as_object()
            .and_then(|settings| settings.get(&known_key))
            .expect("known setting should be persisted"),
        &known_replacement
    );
    assert!(
        !updated_settings
            .as_object()
            .expect("updated settings should be object")
            .contains_key("ghost_setting")
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_applies_beginner_config_alias_patch()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-beginner-config-alias");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_minecraft_instance(
        app.state::<DesktopState>(),
        "AI Apply Beginner Config Alias",
    )
    .await
    .expect("seed test instance");
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let baseline = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let baseline_settings: Value = serde_json::from_str(&baseline.settings_json)?;
    let baseline_object = baseline_settings
        .as_object()
        .ok_or_else(|| std::io::Error::other("minecraft settings should be a JSON object"))?;

    let (known_key, known_replacement) = baseline_object
        .iter()
        .next()
        .map(|(key, value)| {
            let updated_value = match value {
                Value::Bool(flag) => Value::Bool(!flag),
                Value::Number(number) if number.as_u64().is_some() => {
                    Value::from(number.as_u64().unwrap_or(0).saturating_add(1))
                }
                Value::Number(number) if number.as_i64().is_some() => {
                    Value::from(number.as_i64().unwrap_or(0).saturating_add(1))
                }
                Value::Number(number) => Value::from(number.as_f64().unwrap_or(1.0) + 1.0),
                Value::String(text) => Value::String(format!("{text}-ai")),
                _ => Value::String(String::from("ai-updated")),
            };
            (key.clone(), updated_value)
        })
        .ok_or_else(|| std::io::Error::other("baseline settings should not be empty"))?;

    let mut patch = serde_json::Map::new();
    patch.insert(known_key.clone(), known_replacement.clone());
    let patch = Value::Object(patch);

    let output = assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: format!(
                    "Update beginner settings for the selected server.\nmock-action:apply_beginner_config\nmock-settings-patch:{patch}"
                ),
                context: Some(String::from(
                    "Smoke requirement: parse mock apply_beginner_config action and apply patch.",
                )),
                selected_instance_id: Some(provisioning.summary.id.clone()),
                selected_module_id: Some(String::from("minecraft")),
            },
        )
        .await?;

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::ApplyBeginnerConfig);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));

    let updated = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let updated_settings: Value = serde_json::from_str(&updated.settings_json)?;
    assert_eq!(
        updated_settings
            .as_object()
            .and_then(|settings| settings.get(&known_key))
            .expect("known setting should be persisted"),
        &known_replacement
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_returns_none_for_guidance_prompt()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-guidance-none");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let _settings = isolated_smoke_app_settings(&run_root)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");

    let output = command_result(
        assistant_execute_operation_inner(
            None,
            app.state::<DesktopState>(),
            AssistantExecuteOperationInput {
                task: Default::default(),
                settings: stored_openai_compatible_ai_mock_settings(),
                prompt: String::from("Can you explain what AI operations are supported?"),
                context: Some(String::from(
                    "Smoke requirement: return none and explain no action was chosen.",
                )),
                selected_instance_id: None,
                selected_module_id: None,
            },
        )
        .await,
    )?;

    assert!(!output.handled);
    assert_eq!(output.action, AssistantOperationAction::None);
    assert_eq!(output.message, String::from("mocked assistant response"));
    assert!(output.instance_id.is_none());
    assert!(output.module_id.is_none());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_execute_operation_inner_repairs_ports_with_mixed_entries()
-> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let run_root = temp_test_dir("assistant-ai-repair-ports-mixed-entries");
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let settings = isolated_smoke_app_settings(&run_root).expect("isolate app settings");
    prepare_fake_minecraft_install(&settings).expect("prepare fake install");
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app");
    sync_modules_to_storage(app.state::<DesktopState>())
        .await
        .expect("sync modules");
    let provisioning = create_fake_minecraft_instance(
        app.state::<DesktopState>(),
        "AI Repair Ports Mixed Entries",
    )
    .await
    .expect("seed test instance");
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let baseline = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let game_port_name = baseline
        .ports
        .first()
        .map(|entry| entry.name.clone())
        .ok_or_else(|| std::io::Error::other("instance should include at least one port"))?;
    let reject_port_name = baseline
        .ports
        .get(1)
        .map(|entry| entry.name.clone())
        .ok_or_else(|| std::io::Error::other("instance should include at least two ports"))?;
    let game_target = baseline
        .ports
        .first()
        .map(|entry| entry.port)
        .ok_or_else(|| std::io::Error::other("instance should include a game port"))?
        .saturating_add(101);

    let mut patch = serde_json::Map::new();
    patch.insert(game_port_name.clone(), Value::from(u64::from(game_target)));
    patch.insert(reject_port_name.clone(), Value::from(0));
    patch.insert(String::from("ghost_port"), Value::from(28999));
    let patch = Value::Object(patch);

    let output = assistant_execute_operation_inner(
        None,
        app.state::<DesktopState>(),
        AssistantExecuteOperationInput {
            task: Default::default(),
            settings: stored_openai_compatible_ai_mock_settings(),
            prompt: format!(
                "Update ports with mock-action:repairPorts and mock-port-patch:{patch}."
            ),
            context: None,
            selected_instance_id: Some(provisioning.summary.id.clone()),
            selected_module_id: Some(String::from("minecraft")),
        },
    )
    .await?;

    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::RepairPorts);
    assert_eq!(
        output.instance_id.as_deref(),
        Some(provisioning.summary.id.as_str())
    );
    assert_eq!(output.module_id.as_deref(), Some("minecraft"));
    assert!(
        output
            .applied_port_names
            .iter()
            .any(|name| name == &game_port_name)
    );
    assert!(
        output
            .applied_port_names
            .iter()
            .all(|name| name != &reject_port_name)
    );
    assert!(
        output
            .rejected_port_names
            .iter()
            .any(|name| name == &reject_port_name)
    );
    assert!(
        output
            .rejected_port_names
            .iter()
            .any(|name| name == "ghost_port")
    );

    let updated = read_instance_details(&storage.paths, &provisioning.summary.id).await?;
    let updated_game_port = updated
        .ports
        .iter()
        .find(|port| port.name == game_port_name)
        .map(|port| port.port);
    let updated_reject_port = updated
        .ports
        .iter()
        .find(|port| port.name == reject_port_name)
        .map(|port| port.port);
    let rejected_old_port = baseline
        .ports
        .iter()
        .find(|port| port.name == reject_port_name)
        .map(|port| port.port);

    assert_eq!(updated_game_port, Some(game_target));
    assert_eq!(updated_reject_port, rejected_old_port);
    Ok(())
}
