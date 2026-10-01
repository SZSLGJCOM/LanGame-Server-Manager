use super::*;

fn module() -> ModuleDetails {
    module_settings_tests::schema_module(json!({"properties":{
        "players":{"type":"integer","minimum":1,"maximum":64},
        "mode":{"type":"string","enum":["normal","hard"]}
    }}))
}

fn record(settings: Value) -> Value {
    json!({"settings":settings,"ports":[],"forbiddenActions":[],"unverified":[]})
}

fn setting(source: &str, key: &str, expected: Value) -> Value {
    json!({"sourceId":source,"key":key,"expected":expected})
}

#[test]
fn requirements_draft_keeps_valid_items_when_another_item_is_invalid() {
    let module = module();
    let mut draft = AssistantRequirementsDraft::new("Set 24 players, use hard mode.");
    let result = draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([
                setting("request_1", "players", json!(24)),
                setting("request_2", "invented", json!(true))
            ])),
            None,
            Some(&module),
        )
        .unwrap();
    assert_eq!(result["accepted"], json!(["settings[0]"]));
    assert_eq!(result["errors"][0]["field"], "settings[1].key/expected");
    assert_eq!(result["itemCount"], 1);
    assert!(!draft.is_ready());
    assert!(draft.requirements().is_none());
    assert!(
        draft
            .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
            .is_err()
    );
    let result = draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_2", "mode", json!("hard"))])),
            None,
            Some(&module),
        )
        .unwrap();
    assert_eq!(result["itemCount"], 2);
    assert_eq!(result["errors"], json!([]));
    draft
        .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
        .unwrap();
    let requirements = draft.requirements().unwrap();
    assert_eq!(requirements.settings[0].expected, 24);
    assert_eq!(requirements.settings[0].source_text, "Set 24 players,");
    assert_eq!(requirements.settings[0].description, "Set 24 players,");
    assert_eq!(requirements.settings[1].expected, "hard");
}

#[test]
fn requirements_draft_sources_preserve_exact_text_and_address_periods() {
    let original = "只绑定127.0.0.1本机;读取server.properties，最多4人、不要MOD。\n启动！";
    let draft = AssistantRequirementsDraft::new(original);
    let catalog = draft.source_catalog();
    let sources = catalog["sources"].as_array().unwrap();
    assert_eq!(sources[0]["text"], "只绑定127.0.0.1本机;");
    assert_eq!(sources[1]["text"], "读取server.properties，");
    for (index, source) in sources.iter().enumerate() {
        assert_eq!(source["id"], format!("request_{}", index + 1));
        assert!(original.contains(source["text"].as_str().unwrap()));
        assert_eq!(source["readable"], true);
    }
    let long = "界".repeat(250);
    let draft = AssistantRequirementsDraft::new(&long);
    let catalog = draft.source_catalog();
    let chunks = catalog["sources"].as_array().unwrap();
    assert_eq!(chunks.len(), 2);
    let combined = chunks
        .iter()
        .map(|chunk| chunk["text"].as_str().unwrap())
        .collect::<String>();
    assert_eq!(combined, long);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk["text"].as_str().unwrap().len() <= 512)
    );
}

#[test]
fn requirements_draft_normalizes_line_endings_without_hiding_plain_requests() {
    let module = module();
    for original in [
        "Set 4 players.  ",
        "Set 4 players.\n",
        "Set 4 players.\r\nUse hard mode.\r\n",
        "Set 4 players.  \r\nUse hard mode.  \r\n",
        "Set 4 players.  \nUse hard mode.  \n",
    ] {
        let mut draft = AssistantRequirementsDraft::new(original);
        let catalog = draft.source_catalog();
        assert_eq!(catalog["hasUnreadableSources"], false);
        for source in catalog["sources"].as_array().unwrap() {
            assert_eq!(source["readable"], true);
            assert!(original.contains(source["text"].as_str().unwrap()));
        }
        assert!(draft.tool_definitions(None, Some(&module)).is_ok());
        let recorded = draft
            .handle_tool(
                "record_task_requirements",
                &record(json!([setting("request_1", "players", json!(4))])),
                None,
                Some(&module),
            )
            .unwrap();
        assert_eq!(recorded["errors"], json!([]));
        draft
            .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
            .unwrap();
        assert!(draft.is_ready());
        let requirement = &draft.requirements().unwrap().settings[0];
        assert_eq!(requirement.source_text, "Set 4 players.");
        assert_eq!(requirement.expected, 4);
    }
}

#[test]
fn requirements_draft_rejects_unknown_sources_without_replacing_valid_values() {
    let module = module();
    let mut draft = AssistantRequirementsDraft::new("Set 24 players.");
    draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_1", "players", json!(24))])),
            None,
            Some(&module),
        )
        .unwrap();
    let result = draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_999", "players", json!(1))])),
            None,
            Some(&module),
        )
        .unwrap();
    assert_eq!(result["errors"][0]["field"], "settings[0].sourceId");
    assert_eq!(draft.draft.settings[0].expected, 24);
    let result = draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_1", "players", json!("24"))])),
            None,
            Some(&module),
        )
        .unwrap();
    assert!(!result["errors"].as_array().unwrap().is_empty());
    assert_eq!(draft.draft.settings[0].expected, 24);
    draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_1", "players", json!(4))])),
            None,
            Some(&module),
        )
        .unwrap();
    assert_eq!(draft.draft.settings.len(), 1);
    assert_eq!(draft.draft.settings[0].expected, 4);
}

#[test]
fn requirements_draft_does_not_expose_hidden_duplicate_json_values() {
    let masked = serde_json::to_string("[REDACTED]").unwrap();
    let original = format!(r#"{{"pass\u0077ord":"synthetic-test-only","password":{masked}}}"#);
    let draft = AssistantRequirementsDraft::new(&original);
    let catalog = draft.source_catalog();
    assert_eq!(catalog["hasUnreadableSources"], true);
    assert!(!catalog.to_string().contains("synthetic-test-only"));
}

#[test]
fn requirements_draft_requires_explicit_record_and_locks_finished_state() {
    let module = module();
    let mut draft = AssistantRequirementsDraft::new("Start the server.");
    assert!(
        draft
            .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
            .is_err()
    );
    draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_1", "players", json!(4))])),
            None,
            Some(&module),
        )
        .unwrap();
    let mut reset = record(json!([]));
    reset["reset"] = json!("true");
    assert!(
        draft
            .handle_tool("record_task_requirements", &reset, None, Some(&module))
            .is_err()
    );
    assert_eq!(draft.draft.settings.len(), 1);
    assert!(
        draft
            .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
            .is_err()
    );
    reset["reset"] = json!(true);
    draft
        .handle_tool("record_task_requirements", &reset, None, Some(&module))
        .unwrap();
    assert_eq!(assistant_draft_count(&draft.draft), 0);
    assert!(
        draft
            .handle_tool(
                "finish_task_requirements",
                &json!({"extra":true}),
                None,
                Some(&module)
            )
            .is_err()
    );
    draft
        .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
        .unwrap();
    assert!(draft.is_ready());
    assert!(
        draft
            .handle_tool("record_task_requirements", &reset, None, Some(&module))
            .is_err()
    );
    assert_eq!(assistant_draft_count(draft.requirements().unwrap()), 0);
}

#[test]
fn requirements_draft_definitions_use_real_identifiers_and_bounded_json_schema() {
    let mut module = module();
    module.default_ports = vec![PortBinding {
        name: "game".into(),
        protocol: "udp".into(),
        port: 27015,
    }];
    let draft = AssistantRequirementsDraft::new("Use requested settings.");
    let definitions = draft.tool_definitions(None, Some(&module)).unwrap();
    let tool = definitions
        .iter()
        .find(|tool| tool.name == "record_task_requirements")
        .unwrap();
    let properties = &tool.parameters["properties"];
    assert_eq!(
        properties["settings"]["items"]["properties"]["key"]["enum"],
        json!(["bind_ip", "mode", "players"])
    );
    assert_eq!(
        properties["settings"]["items"]["properties"]["sourceId"]["enum"],
        json!(["request_1"])
    );
    assert_eq!(
        properties["ports"]["items"]["properties"]["name"]["enum"],
        json!(["game"])
    );
    let actions = properties["forbiddenActions"]["items"]["properties"]["action"]["enum"]
        .as_array()
        .unwrap();
    assert_eq!(
        actions,
        json!([
            "start_server",
            "stop_server",
            "restart_server",
            "create_backup",
            "restore_backup",
            "create_server",
            "install_server",
            "validate_server",
            "apply_beginner_config",
            "customize_config",
            "patch_instance_text",
            "patch_instance_files",
            "install_fun_mod",
            "install_site_mod",
            "repair_ports",
            "run_gm_command",
            "broadcast"
        ])
        .as_array()
        .unwrap()
    );
    assert!(actions.contains(&json!("patch_instance_files")));
    assert!(actions.contains(&json!("patch_instance_text")));
    assert!(!actions.contains(&json!("none")));
    assert!(!actions.contains(&json!("shell")));
    let large = (0..400)
        .map(|index| {
            (
                format!("setting_{index}_{}", "x".repeat(48)),
                json!({"type":"string"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    module.schema_json = Some(json!({"properties":large}).to_string());
    assert!(
        draft
            .tool_definitions(None, Some(&module))
            .unwrap_err()
            .contains("16 KiB")
    );
}

#[test]
fn requirements_draft_enforces_total_items_and_input_budgets_without_dropping_earlier_items() {
    let original = (0..33)
        .map(|index| format!("Part {index},"))
        .collect::<String>();
    let mut draft = AssistantRequirementsDraft::new(&original);
    let mut input = record(json!([]));
    input["unverified"]=json!((1..=32).map(|index|json!({"sourceId":format!("request_{index}"),"reason":"No available verifier"})).collect::<Vec<_>>());
    let result = draft
        .handle_tool("record_task_requirements", &input, None, None)
        .unwrap();
    assert_eq!(result["errors"], json!([]));
    input["unverified"] = json!([{"sourceId":"request_33","reason":"No available verifier"}]);
    let result = draft
        .handle_tool("record_task_requirements", &input, None, None)
        .unwrap();
    assert_eq!(result["itemCount"], 32);
    assert!(!result["errors"].as_array().unwrap().is_empty());
    assert!(
        draft
            .handle_tool("finish_task_requirements", &json!({}), None, None)
            .is_err()
    );
    let excessive = record(json!([setting(
        "request_1",
        "players",
        json!("x".repeat(16 * 1024))
    )]));
    assert!(
        draft
            .handle_tool("record_task_requirements", &excessive, None, None)
            .is_err()
    );
    assert_eq!(assistant_draft_count(&draft.draft), 32);
    for request in [
        String::from(" "),
        "x".repeat(16 * 1024 + 1),
        "clause,".repeat(129),
    ] {
        let invalid = AssistantRequirementsDraft::new(&request);
        assert!(invalid.source_catalog().get("error").is_some());
        assert!(invalid.tool_definitions(None, None).is_err());
    }
}

#[test]
fn requirements_draft_redacted_source_gaps_block_finish_and_never_echo_secret() {
    let secret = ["draft", "credential", "fixture"].join("-");
    let sensitive_key = ["server", "password"].join("_");
    for original in [
        format!("Set 4 players,{sensitive_key}={secret},start."),
        format!("{sensitive_key}: |\n  {secret}\nStart the server."),
        format!("Set 4 players,{sensitive_key}={secret},use {secret}."),
    ] {
        let mut draft = AssistantRequirementsDraft::new(&original);
        let catalog = draft.source_catalog();
        assert!(!catalog.to_string().contains(&secret));
        assert_eq!(catalog["hasUnreadableSources"], true);
        let hidden = catalog["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["readable"] == false)
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let result = draft
            .handle_tool(
                "record_task_requirements",
                &record(json!([setting(hidden, "players", json!(4))])),
                None,
                Some(&module()),
            )
            .unwrap();
        assert!(!result.to_string().contains(&secret));
        assert_eq!(result["itemCount"], 0);
        draft
            .handle_tool(
                "record_task_requirements",
                &record(json!([])),
                None,
                Some(&module()),
            )
            .unwrap();
        let error = draft
            .handle_tool(
                "finish_task_requirements",
                &json!({}),
                None,
                Some(&module()),
            )
            .unwrap_err();
        assert!(!error.contains(&secret));
        assert!(error.contains("hidden"));
        assert!(!draft.is_ready());
    }
}

#[test]
fn requirements_draft_feedback_and_requirements_views_redact_nested_expected_secrets() {
    let sensitive_key = ["access", "token"].join("_");
    let secret = ["nested", "fixture", "credential"].join("-");
    let module =
        module_settings_tests::schema_module(json!({"properties":{"options":{"type":"object"}}}));
    let mut draft = AssistantRequirementsDraft::new("Configure the requested options.");
    let expected = json!({"items":[{sensitive_key:secret}]});
    let result = draft
        .handle_tool(
            "record_task_requirements",
            &record(json!([setting("request_1", "options", expected.clone())])),
            None,
            Some(&module),
        )
        .unwrap();
    assert!(!result.to_string().contains(&secret));
    draft
        .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
        .unwrap();
    let requirements = draft.requirements().unwrap();
    assert_eq!(requirements.settings[0].expected, expected);
    assert!(
        !serde_json::to_string(&requirements.views())
            .unwrap()
            .contains(&secret)
    );
}

#[test]
fn requirements_draft_upserts_each_kind_and_keeps_forbidden_actions_explicit() {
    let mut module = module();
    module.default_ports = vec![PortBinding {
        name: "game".into(),
        protocol: "udp".into(),
        port: 27015,
    }];
    let mut draft = AssistantRequirementsDraft::new(
        "Use the requested port,do not install files,verify the unsupported effect.",
    );
    let mut input = record(json!([]));
    input["ports"] = json!([{"sourceId":"request_1","name":"game","expected":27016}]);
    input["forbiddenActions"] = json!([{"sourceId":"request_2","action":"install_server"}]);
    input["unverified"] = json!([{"sourceId":"request_3","reason":"No effect verifier"}]);
    assert_eq!(
        draft
            .handle_tool("record_task_requirements", &input, None, Some(&module))
            .unwrap()["errors"],
        json!([])
    );
    input["ports"][0]["name"] = json!("GAME");
    input["ports"][0]["expected"] = json!(27017);
    assert_eq!(
        draft
            .handle_tool("record_task_requirements", &input, None, Some(&module))
            .unwrap()["itemCount"],
        3
    );
    draft
        .handle_tool("finish_task_requirements", &json!({}), None, Some(&module))
        .unwrap();
    let requirements = draft.requirements().unwrap();
    assert_eq!(requirements.ports[0].expected, 27017);
    assert_eq!(
        requirements.forbidden_actions[0].action,
        AssistantOperationAction::InstallServer
    );
    assert_eq!(requirements.unverified.len(), 1);
    assert!(
        requirements
            .validate_action(
                &AssistantOperationPlan {
                    action: AssistantOperationAction::CreateServer,
                    ..assistant_safe_none_plan("fixture".into())
                },
                None,
                None
            )
            .is_err()
    );
}
