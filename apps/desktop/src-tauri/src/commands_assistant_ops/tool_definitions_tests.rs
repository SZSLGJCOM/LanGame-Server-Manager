use super::*;

fn definition<'a>(
    tools: &'a [crate::assistant::AssistantToolDefinition],
    name: &str,
) -> &'a crate::assistant::AssistantToolDefinition {
    tools.iter().find(|tool| tool.name == name).unwrap()
}

#[test]
fn investigation_tools_expose_only_the_selected_read_scope_and_explicit_operation_phase() {
    let module = module_settings_tests::schema_module(json!({"properties":{
        "cluster_name":{"type":"string"}
    }}));
    let unbound_tools = assistant_investigation_tools(None, None, false);
    assert_eq!(
        unbound_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["read_host_info"]
    );
    let module_tools = assistant_investigation_tools(None, Some(&module), false);
    assert_eq!(
        module_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        [
            "read_host_info",
            "list_module_settings",
            "read_module_settings",
            "search_module_settings",
            "search_game_docs",
            "read_game_doc"
        ]
    );
    let (_, instance) = task_tests::task_fixture(false);
    let instance_tools = assistant_investigation_tools(Some(&instance), Some(&module), false);
    assert_eq!(
        instance_tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        [
            "read_host_info",
            "list_settings",
            "read_settings",
            "search_settings",
            "search_game_docs",
            "read_game_doc",
            "list_backups",
            "read_runtime",
            "list_config_files",
            "read_config_file",
            "read_mod_state",
            "read_workshop_items",
            "inspect_installed_mods",
            "inspect_launch",
            "validate_instance_file",
            "list_instance_files",
            "search_instance_files",
            "inspect_network_endpoints",
            "read_instance_file",
        ]
    );
    let mut other_game = instance.clone();
    other_game.summary.module_id = "minecraft".into();
    let other_game_tools = assistant_investigation_tools(Some(&other_game), None, false);
    for name in [
        "list_instance_files",
        "search_instance_files",
        "read_instance_file",
        "inspect_network_endpoints",
        "list_backups",
    ] {
        assert!(
            other_game_tools.iter().any(|tool| tool.name == name),
            "generic instance capability missing: {name}"
        );
        assert!(
            module_tools.iter().all(|tool| tool.name != name),
            "instance capability leaked into module scope: {name}"
        );
    }
    assert!(
        instance_tools
            .iter()
            .chain(&module_tools)
            .all(|tool| tool.name != "read_session_history"),
        "session history requires a separate bound backend session"
    );
    assert!(
        instance_tools
            .iter()
            .all(|tool| !tool.name.contains("module"))
    );
    assert!(
        instance_tools
            .iter()
            .all(|tool| tool.name != "propose_operation")
    );
    let operation_tools = assistant_investigation_tools(Some(&instance), Some(&module), true);
    assert_eq!(operation_tools.len(), instance_tools.len() + 1);
    assert_eq!(operation_tools.last().unwrap().name, "propose_operation");
}

#[test]
fn investigation_native_parameters_match_read_tool_serde_without_a_tool_tag() {
    let module = module_settings_tests::schema_module(json!({"properties":{
        "cluster_name":{"type":"string"}
    }}));
    let (_, instance) = task_tests::task_fixture(false);
    let mut tools = assistant_investigation_tools(None, Some(&module), false);
    tools.extend(assistant_investigation_tools(
        Some(&instance),
        Some(&module),
        false,
    ));
    tools.push(assistant_session_history_tool());
    for tool in tools {
        assert_eq!(tool.parameters["type"], "object");
        assert_eq!(tool.parameters["additionalProperties"], false);
        let mut arguments = match tool.name.as_str() {
            "read_module_settings" | "read_settings" => json!({"keys":["cluster_name"],"offset":0}),
            "search_module_settings" | "search_settings" => json!({"query":"cluster", "offset":0}),
            "search_game_docs" => json!({"query":"开服", "offset":0}),
            "read_game_doc" => json!({"documentId":"0123456789abcdef0123456789abcdef","offset":0}),
            "read_runtime" => json!({"lines":80}),
            "read_config_file" => json!({"file":"server.properties", "offset":0}),
            "read_instance_file" => json!({"file":"runtime/mods/example/modmain.lua", "offset":0}),
            "validate_instance_file" => json!({"file":"data/plugins/example/config.json"}),
            "read_workshop_items" => json!({"ids":["123456"]}),
            "inspect_installed_mods" => json!({"names":["local mod"],"offset":0}),
            "list_instance_files" => json!({"directory":"data/plugins","offset":0}),
            "search_instance_files" => {
                json!({"directory":"data/plugins","query":"timeout","cursor":null})
            }
            "read_session_history" => {
                json!({"source":"messages","offset":0,"limit":4,"messageOffsetBytes":0})
            }
            "read_host_info"
            | "read_mod_state"
            | "inspect_launch"
            | "inspect_network_endpoints" => json!({}),
            "list_module_settings" | "list_settings" | "list_config_files" | "list_backups" => {
                json!({"offset":0})
            }
            name => panic!("Add a native parameter contract fixture for {name}"),
        };
        let declared = tool.parameters["properties"].as_object().unwrap();
        assert!(!declared.contains_key("tool"));
        assert_eq!(
            declared.keys().collect::<Vec<_>>(),
            arguments.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        arguments["tool"] = json!(tool.name);
        let parsed: AssistantReadTool = serde_json::from_value(arguments.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), arguments);
        for forbidden in ["instanceId", "sessionId", "root"] {
            let mut redirected = arguments.clone();
            redirected[forbidden] = json!("another-scope");
            assert!(
                serde_json::from_value::<AssistantReadTool>(redirected).is_err(),
                "{} accepted model-owned scope field {forbidden}",
                tool.name
            );
        }
        let mut minimum = json!({"tool":tool.name});
        for required in tool.parameters["required"].as_array().unwrap() {
            let key = required.as_str().unwrap();
            minimum[key] = match key {
                "query" => json!("cluster"),
                "file" => json!("server.properties"),
                "documentId" => json!("getting-started"),
                "ids" => json!(["123456"]),
                _ => panic!("Unexpected required read argument {key}"),
            };
        }
        assert!(serde_json::from_value::<AssistantReadTool>(minimum).is_ok());
        if !tool.parameters["required"].as_array().unwrap().is_empty() {
            assert!(
                serde_json::from_value::<AssistantReadTool>(json!({"tool":tool.name})).is_err()
            );
        }
    }
}

#[test]
fn investigation_workspace_search_cursor_roundtrips_without_model_owned_scope() {
    let arguments = json!({"tool":"search_instance_files","directory":"data/plugins","query":"timeout",
        "cursor":{"fileOffset":64,"lineOffset":25,"sourceSha256":"a".repeat(64),
            "listingSha256":"b".repeat(64),"query":"timeout"}});
    let parsed: AssistantReadTool = serde_json::from_value(arguments.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), arguments);
    let mut redirected = arguments;
    redirected["cursor"]["root"] = json!("different-instance");
    assert!(serde_json::from_value::<AssistantReadTool>(redirected).is_err());
}

#[test]
fn investigation_read_key_enum_uses_actual_scope_keys_without_values_or_aliases() {
    let credential = ["synthetic", "tool", "secret"].join("-");
    let unsafe_key = format!("password={credential}");
    let module = module_settings_tests::schema_module(json!({"properties":{
        "schema_only":{"type":"string", "default":credential},
        unsafe_key:{"type":"string"}
    }}));
    let (_, mut instance) = task_tests::task_fixture(false);
    instance.settings_json = json!({"actual_key":credential,"null_key":null}).to_string();
    let module_tools = assistant_investigation_tools(None, Some(&module), false);
    assert_eq!(
        definition(&module_tools, "read_module_settings").parameters["properties"]["keys"]["items"]
            ["enum"],
        json!(["bind_ip", "schema_only"])
    );
    let instance_tools = assistant_investigation_tools(Some(&instance), Some(&module), false);
    assert_eq!(
        definition(&instance_tools, "read_settings").parameters["properties"]["keys"]["items"]["enum"],
        json!(["actual_key", "null_key"])
    );
    for tool in module_tools.iter().chain(&instance_tools) {
        assert!(!tool.parameters.to_string().contains(&credential));
        assert!(!tool.description.contains(&credential));
    }
}

#[test]
fn investigation_tool_schemas_bound_unicode_key_enums_and_keep_pagination_available() {
    let properties = (0..1500)
        .map(|index| {
            (
                format!("setting_{index:04}_{}", "中文".repeat(30)),
                json!({"type":"string"}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let (_, mut instance) = task_tests::task_fixture(false);
    instance.settings_json = Value::Object(
        properties
            .keys()
            .map(|key| (key.clone(), Value::Null))
            .collect(),
    )
    .to_string();
    instance.summary.module_id = "dontstarve".into();
    let mut minecraft = instance.clone();
    minecraft.summary.module_id = "minecraft".into();
    let module = module_settings_tests::schema_module(json!({"properties":properties}));
    for (scope, instance, read_name) in [
        ("module", None, "read_module_settings"),
        ("minecraft", Some(&minecraft), "read_settings"),
        ("dontstarve", Some(&instance), "read_settings"),
    ] {
        let tools = assistant_investigation_tools(instance, Some(&module), true);
        assert_eq!(
            tools
                .iter()
                .any(|tool| tool.name == "inspect_installed_mods"),
            scope == "dontstarve",
            "{scope} must retain its actual fixed tool catalog"
        );
        let read = definition(&tools, read_name);
        let keys = &read.parameters["properties"]["keys"]["items"]["enum"];
        assert!(keys.as_array().unwrap().len() < 1501);
        assert!(keys.to_string().len() <= ASSISTANT_TOOL_KEY_ENUM_BYTES);
        assert_eq!(read.parameters["properties"]["keys"]["default"], json!([]));
        assert!(read.description.contains("page"));
        let bytes = tools
            .iter()
            .map(|tool| {
                tool.parameters.to_string().len() + tool.description.len() + tool.name.len()
            })
            .sum::<usize>();
        assert!(bytes < 16 * 1024, "{scope} bounded definitions: {bytes}");
        if instance.is_none() {
            let page = read_assistant_module_schema(
                Some(&module),
                AssistantReadTool::ReadModuleSettings {
                    keys: Vec::new(),
                    offset: keys.as_array().unwrap().len(),
                },
            )
            .unwrap();
            assert!(
                page["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|entry| !keys.as_array().unwrap().contains(&entry["key"])),
                "empty-key pagination must still expose keys beyond the shortcut enum"
            );
        }
    }
}

#[test]
fn investigation_read_schema_bounds_match_the_existing_readers() {
    let (_, instance) = task_tests::task_fixture(false);
    let tools = assistant_investigation_tools(Some(&instance), None, false);
    assert_eq!(
        definition(&tools, "read_runtime").parameters["properties"]["lines"]["maximum"],
        400
    );
    assert_eq!(
        definition(&tools, "read_settings").parameters["properties"]["keys"]["maxItems"],
        40
    );
    assert_eq!(
        definition(&tools, "search_settings").parameters["properties"]["query"]["maxLength"],
        128
    );
    assert_eq!(
        definition(&tools, "search_instance_files").parameters["properties"]["query"]["maxLength"],
        128
    );
    for name in ["list_instance_files", "search_instance_files"] {
        assert_eq!(
            definition(&tools, name).parameters["properties"]["directory"]["maxLength"],
            1024
        );
    }
    assert_eq!(
        definition(&tools, "read_instance_file").parameters["properties"]["file"]["maxLength"],
        1024
    );
    assert_eq!(
        definition(&tools, "read_config_file").parameters["properties"]["file"]["maxLength"],
        1024
    );
    assert_eq!(
        definition(&tools, "read_workshop_items").parameters["properties"]["ids"]["maxItems"],
        20
    );
    assert_eq!(
        definition(&tools, "inspect_installed_mods").parameters["properties"]["names"]["maxItems"],
        10
    );
    for name in [
        "read_host_info",
        "read_mod_state",
        "inspect_launch",
        "inspect_network_endpoints",
    ] {
        assert_eq!(definition(&tools, name).parameters["properties"], json!({}));
        assert_eq!(
            definition(&tools, name).parameters["additionalProperties"],
            false
        );
    }
}

#[test]
fn investigation_operation_schema_has_only_supported_fields_and_never_model_owned_requirements() {
    let tools = assistant_investigation_tools(None, None, true);
    let operation = definition(&tools, "propose_operation");
    assert_eq!(operation.parameters["required"], json!(["action"]));
    assert_eq!(operation.parameters["additionalProperties"], false);
    let properties = operation.parameters["properties"].as_object().unwrap();
    assert!(!properties.contains_key("taskRequirements"));
    for action in properties["action"]["enum"].as_array().unwrap() {
        let plan: AssistantOperationPlan =
            serde_json::from_value(json!({"action":action})).unwrap();
        assert_eq!(serde_json::to_value(plan.action).unwrap(), *action);
        let mut actual = serde_json::to_value(plan).unwrap();
        actual.as_object_mut().unwrap().remove("taskRequirements");
        assert_eq!(
            properties.keys().collect::<Vec<_>>(),
            actual.as_object().unwrap().keys().collect::<Vec<_>>()
        );
    }
    assert_eq!(
        properties["action"]["enum"],
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
            "install_fun_mod",
            "install_site_mod",
            "repair_ports",
            "patch_instance_text",
            "patch_instance_files",
            "run_gm_command",
            "broadcast",
            "none"
        ])
    );
    assert_eq!(properties["backupId"]["minLength"], 1);
    assert_eq!(properties["backupId"]["maxLength"], 256);
    assert_eq!(properties["filePatches"]["maxItems"], 8);
    assert_eq!(
        properties["filePatches"]["items"]["required"],
        json!(["file", "sourceSha256", "edits"])
    );
    assert_eq!(
        properties["textPatch"]["required"],
        json!(["file", "sourceSha256", "before", "after"])
    );
    assert_eq!(properties["textPatch"]["additionalProperties"], false);
    assert_eq!(properties["runtimeCommands"]["maxItems"], 1);
    assert_eq!(properties["settingsPatch"]["type"], "object");
    assert_eq!(properties["portPatch"]["type"], "object");
}
