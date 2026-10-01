use super::*;

pub(super) fn schema_module(schema: Value) -> ModuleDetails {
    ModuleDetails {
        summary: ModuleSummary {
            id: String::from("schema-fixture"),
            name: String::from("Schema fixture"),
            version: String::from("1.0.0"),
            description: None,
            steam_app_id: None,
            install_state: Default::default(),
            instance_program_count: 0,
            archived_program_count: 0,
            supported_platforms: vec![],
        },
        schema_json: Some(schema.to_string()),
        default_ports: vec![],
        install: None,
        process: None,
        workshop: None,
        mods: None,
        runtime: Default::default(),
    }
}

fn delivered_module_schema(module: &ModuleDetails, tool: AssistantReadTool) -> Value {
    let data = read_assistant_module_schema(Some(module), tool).unwrap();
    let encoded = assistant_tool_result_text(&json!({"ok":true, "data":data}));
    assert!(encoded.len() <= ASSISTANT_TOOL_RESULT_BYTES);
    let delivered: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(delivered["ok"], true);
    assert_eq!(delivered["data"]["scope"], "module_schema");
    assert_eq!(delivered["data"]["moduleId"], module.summary.id);
    assert_eq!(delivered["data"]["schemaExists"], true);
    assert_eq!(delivered["data"]["instanceExists"], false);
    delivered["data"].clone()
}

#[test]
fn module_settings_catalog_distinguishes_actual_properties_and_manager_settings_without_values() {
    let module = schema_module(json!({"properties": {
        "cluster_name":{"type":"string", "default":"schema default is not a saved name"},
        "max_players":{"type":"integer", "default":16}
    }, "$defs":{"not_a_setting":{"type":"string"}}}));
    let page =
        delivered_module_schema(&module, AssistantReadTool::ListModuleSettings { offset: 0 });
    assert_eq!(
        page["keys"],
        json!(["bind_ip", "cluster_name", "max_players"])
    );
    assert_eq!(page["totalKeys"], 3);
    assert_eq!(page["managerSettings"][0]["key"], "bind_ip");
    assert_eq!(page["managerSettings"][0]["source"], "manager");
    assert_eq!(page["managerSettings"][0]["schemaExists"], false);
    assert_eq!(page["managerSettings"][0]["instanceExists"], false);
    assert!(
        !page
            .to_string()
            .contains("schema default is not a saved name")
    );
    assert!(!page.to_string().contains("not_a_setting"));
    assert!(page["note"].as_str().unwrap().contains("schema-only"));
    assert!(page["nextOffset"].is_null());
}

#[test]
fn module_settings_manager_bind_candidate_reports_real_runtime_capability_without_a_fake_schema() {
    let mut module = schema_module(json!({"properties":{}}));
    let original_schema = module.schema_json.clone();
    for mode in [
        app_core::ModuleBindAddressMode::Unsupported,
        app_core::ModuleBindAddressMode::Strict,
    ] {
        module.runtime.bind_address.mode = mode;
        module.runtime.bind_address.port_names = vec![String::from("master")];
        module.runtime.bind_address.required_setting_key = Some(String::from("allow_bind"));
        let page = delivered_module_schema(
            &module,
            AssistantReadTool::ReadModuleSettings {
                keys: vec![String::from("bind_ip")],
                offset: 0,
            },
        );
        let bind = &page["entries"][0];
        assert_eq!(bind["source"], "manager");
        assert_eq!(bind["schemaExists"], false);
        assert_eq!(bind["instanceExists"], false);
        assert_eq!(bind["declaration"]["type"], "string");
        assert!(bind.get("schema").is_none());
        assert!(bind.get("value").is_none());
        assert_eq!(
            bind["bindAddressCapability"],
            serde_json::to_value(&module.runtime.bind_address).unwrap()
        );
        let found = delivered_module_schema(
            &module,
            AssistantReadTool::SearchModuleSettings {
                query: String::from("listener address"),
                offset: 0,
            },
        );
        assert_eq!(found["totalMatches"], 1);
        assert_eq!(found["entries"][0], *bind);
    }
    assert_eq!(module.schema_json, original_schema);
}

#[test]
fn module_settings_reader_redacts_sensitive_defaults_without_claiming_current_values() {
    let credential = ["synthetic", "module", "credential"].join("-");
    let module = schema_module(json!({"properties": {
        "admin_password":{"type":"string", "title":"Admin credential",
            "default":credential, "examples":[credential], "enum":[credential], "const":credential},
        "master_world_size":{"type":"string", "default":"medium", "enum":["small","medium"]}
    }}));
    let page = delivered_module_schema(
        &module,
        AssistantReadTool::ReadModuleSettings {
            keys: vec![
                String::from("admin_password"),
                String::from("master_world_size"),
            ],
            offset: 0,
        },
    );
    assert!(!page.to_string().contains(&credential));
    let secret = &page["entries"][0];
    for keyword in ["default", "examples", "enum", "const"] {
        assert_eq!(secret["schema"][keyword], "[REDACTED]");
    }
    assert_eq!(secret["schema"]["type"], "string");
    assert_eq!(
        page["entries"][1]["schema"]["enum"],
        json!(["small", "medium"])
    );
    assert_eq!(page["entries"][1]["schema"]["default"], "medium");
    for entry in page["entries"].as_array().unwrap() {
        assert_eq!(entry["schemaExists"], true);
        assert_eq!(entry["instanceExists"], false);
        assert!(entry.get("value").is_none());
        assert!(entry.get("exists").is_none());
    }
    assert_eq!(page["unknownKeys"], json!([]));
    assert_eq!(page["partial"], false);
    assert!(page.get("hint").is_none());
}

#[test]
fn module_settings_search_never_matches_schema_value_keywords() {
    let credential = ["synthetic", "search", "secret"].join("_");
    let module = schema_module(json!({"properties": {
        "admin_password":{"type":"string", "title":"Administrator access",
            "default":credential, "examples":[credential], "enum":[credential], "const":credential},
        "master_world_size":{"type":"string", "title":"World size", "default":"small"}
    }}));
    for query in [credential.as_str(), "small"] {
        let page = delivered_module_schema(
            &module,
            AssistantReadTool::SearchModuleSettings {
                query: query.into(),
                offset: 0,
            },
        );
        assert_eq!(page["entries"], json!([]));
        assert_eq!(page["totalMatches"], 0);
        assert_eq!(page["matching"]["mode"], "all_terms");
    }
    let page = delivered_module_schema(
        &module,
        AssistantReadTool::SearchModuleSettings {
            query: String::from("world size"),
            offset: 0,
        },
    );
    assert_eq!(page["entries"][0]["key"], "master_world_size");
    assert_eq!(page["totalMatches"], 1);
    assert!(page.get("hint").is_none());
}

#[test]
fn module_settings_catalog_pages_utf8_keys_without_truncating_names_or_json() {
    let properties = (0..220)
        .map(|index| {
            (
                format!("setting_{index:03}_{}", "界".repeat(40)),
                json!({"type":"string"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let mut expected = vec![String::from("bind_ip")];
    expected.extend(properties.keys().cloned());
    let module = schema_module(json!({"properties":properties}));
    let mut collected = Vec::new();
    let mut offset = 0;
    loop {
        let page =
            delivered_module_schema(&module, AssistantReadTool::ListModuleSettings { offset });
        collected.extend(
            page["keys"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key.as_str().unwrap().to_owned()),
        );
        let Some(next) = page["nextOffset"].as_u64() else {
            break;
        };
        assert!(next as usize > offset);
        assert!((next as usize) < expected.len());
        offset = next as usize;
    }
    assert!(offset > 0, "byte limit must paginate this catalog");
    assert_eq!(collected, expected);
}

#[test]
fn module_settings_read_and_search_pages_keep_schema_only_presence() {
    let properties = (0..21)
        .map(|index| {
            (
                format!("setting_{index:02}"),
                json!({"type":"integer", "title":"Public number"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let module = schema_module(json!({"properties":properties}));
    for (offset, count, next) in [(0, 20, Some(20)), (20, 2, None)] {
        let page = delivered_module_schema(
            &module,
            AssistantReadTool::ReadModuleSettings {
                keys: vec![],
                offset,
            },
        );
        assert_eq!(page["entries"].as_array().unwrap().len(), count);
        assert_eq!(page["nextOffset"], json!(next));
    }
    for (offset, count, next) in [(0, 10, Some(10)), (10, 10, Some(20)), (20, 1, None)] {
        let page = delivered_module_schema(
            &module,
            AssistantReadTool::SearchModuleSettings {
                query: "public".into(),
                offset,
            },
        );
        assert_eq!(page["entries"].as_array().unwrap().len(), count);
        assert_eq!(page["nextOffset"], json!(next));
        assert_eq!(page["totalMatches"], 21);
    }
}

#[test]
fn module_settings_tools_bind_targets_and_report_unknown_keys_without_non_schema_reads() {
    for tool in [
        "list_module_settings",
        "read_module_settings",
        "search_module_settings",
    ] {
        for extra in ["moduleId", "instanceId", "command"] {
            let mut request = json!({"tool":tool, extra:"other-target"});
            assert!(request.get(extra).is_some());
            if tool == "search_module_settings" {
                request["query"] = json!("world");
            }
            assert!(parse_assistant_investigation_response(&request.to_string()).is_err());
        }
    }
    assert!(
        read_assistant_module_schema(None, AssistantReadTool::ListModuleSettings { offset: 0 })
            .is_err()
    );
    let module = schema_module(json!({"properties":{"actual_key":{"type":"string"}}}));
    let unknown = read_assistant_module_schema(
        Some(&module),
        AssistantReadTool::ReadModuleSettings {
            keys: vec![String::from("guessed_key")],
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(unknown["entries"], json!([]));
    assert_eq!(unknown["unknownKeys"], json!(["guessed_key"]));
    assert_eq!(unknown["partial"], true);
    for tool in [
        AssistantReadTool::ReadRuntime { lines: 1 },
        AssistantReadTool::ListSettings { offset: 0 },
        AssistantReadTool::InspectLaunch {},
    ] {
        assert!(
            read_assistant_module_schema(Some(&module), tool)
                .unwrap_err()
                .contains("server instance")
        );
    }
}

#[test]
fn module_settings_missing_or_invalid_schema_is_not_reported_as_existing_schema() {
    let mut module = schema_module(json!({"properties":{}}));
    for schema in [
        None,
        Some(String::from("not-json")),
        Some(String::from("[]")),
        Some(String::from(r#"{"properties":[]}"#)),
    ] {
        module.schema_json = schema;
        assert!(
            read_assistant_module_schema(
                Some(&module),
                AssistantReadTool::ListModuleSettings { offset: 0 }
            )
            .is_err()
        );
    }
}

#[test]
fn module_settings_sensitive_names_are_never_replaced_with_invented_setting_keys() {
    let credential = ["synthetic", "name", "secret"].join("-");
    let unsafe_name = format!("password={credential}");
    let module = schema_module(json!({"properties":{
        unsafe_name.clone():{"type":"string"}, "visible_key":{"type":"integer"}
    }}));
    let catalog =
        delivered_module_schema(&module, AssistantReadTool::ListModuleSettings { offset: 0 });
    assert_eq!(catalog["keys"], json!(["bind_ip", "visible_key"]));
    assert_eq!(catalog["omittedSensitiveKeys"], 1);
    let page = delivered_module_schema(
        &module,
        AssistantReadTool::ReadModuleSettings {
            keys: vec![],
            offset: 0,
        },
    );
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["omittedSensitiveKeys"], 1);
    assert!(!page.to_string().contains(&credential));
    assert!(!page.to_string().contains("[REDACTED]"));
    let unknown = read_assistant_module_schema(
        Some(&module),
        AssistantReadTool::ReadModuleSettings {
            keys: vec![unsafe_name],
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(unknown["entries"], json!([]));
    assert_eq!(unknown["partial"], true);
    assert!(!unknown.to_string().contains(&credential));
}

#[test]
fn module_settings_mixed_read_keeps_all_valid_declarations_and_marks_unknown_keys() {
    let properties = (0..5)
        .map(|index| (format!("actual_{index}"), json!({"type":"integer"})))
        .collect::<serde_json::Map<_, _>>();
    let module = schema_module(json!({"properties":properties}));
    let mut keys = (0..5)
        .map(|index| format!("actual_{index}"))
        .collect::<Vec<_>>();
    keys.insert(2, String::from("guessed_alias"));
    let page = delivered_module_schema(
        &module,
        AssistantReadTool::ReadModuleSettings { keys, offset: 0 },
    );
    assert_eq!(page["entries"].as_array().unwrap().len(), 5);
    assert_eq!(page["unknownKeys"], json!(["guessed_alias"]));
    assert_eq!(page["partial"], true);
    assert!(page["hint"].as_str().unwrap().contains("No aliases"));
    for entry in page["entries"].as_array().unwrap() {
        assert_eq!(entry["source"], "module");
        assert_eq!(entry["schemaExists"], true);
        assert_eq!(entry["instanceExists"], false);
        assert!(entry.get("value").is_none());
    }
}

#[test]
fn module_settings_partial_unknown_labels_are_redacted_and_byte_bounded() {
    let credential = ["synthetic", "unknown", "credential"].join("-");
    let module = schema_module(json!({"properties":{"actual":{"type":"string"}}}));
    let page = delivered_module_schema(
        &module,
        AssistantReadTool::ReadModuleSettings {
            keys: vec![
                String::from("actual"),
                format!("password={credential}"),
                "超长未知键".repeat(100),
            ],
            offset: 0,
        },
    );
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["unknownKeys"].as_array().unwrap().len(), 2);
    assert_eq!(page["partial"], true);
    assert!(!page.to_string().contains(&credential));
    assert!(
        page["unknownKeys"]
            .as_array()
            .unwrap()
            .iter()
            .all(|key| key.as_str().unwrap().len() <= 256)
    );
}

#[test]
fn module_settings_oversize_schema_read_is_an_explicit_evidence_gap() {
    let module = schema_module(json!({"properties":{
        "large_setting":{"type":"string", "description":"public ".repeat(3000)}
    }}));
    let result = read_assistant_module_schema(
        Some(&module),
        AssistantReadTool::ReadModuleSettings {
            keys: vec![String::from("large_setting")],
            offset: 0,
        },
    )
    .unwrap();
    let delivered = assistant_tool_result_text(&json!({"ok":true,"data":result}));
    let delivered: Value = serde_json::from_str(&delivered).unwrap();
    assert_eq!(delivered["ok"], false);
    assert!(
        delivered["error"]
            .as_str()
            .unwrap()
            .contains("evidence budget")
    );
    assert!(delivered.get("data").is_none());
}
