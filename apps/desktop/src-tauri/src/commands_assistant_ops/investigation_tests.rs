use super::*;

#[test]
fn parameterless_reads_reject_invented_pagination_and_launch_arguments() {
    for name in ["read_mod_state", "inspect_launch"] {
        let valid = json!({"tool": name});
        assert!(serde_json::from_value::<AssistantReadTool>(valid).is_ok());
        for field in ["offset", "instanceId", "command"] {
            let invalid = json!({"tool": name, field: 1});
            assert!(serde_json::from_value::<AssistantReadTool>(invalid).is_err());
        }
    }
}

#[tokio::test]
async fn investigation_exposes_remaining_reads_and_reserves_control_context() {
    let mut calls = 0;
    let result = run_assistant_investigation(
        String::from("Preserve the enabled mod functionality."),
        vec![AssistantReadTool::ReadRuntime { lines: 80 }],
        |prompt| {
            calls += 1;
            let remaining = ASSISTANT_INVESTIGATION_STEPS - calls;
            assert!(prompt.contains(&format!("Read budget remaining: {remaining} of 8")));
            assert_eq!(prompt.contains("No reads remain."), remaining == 0);
            assert_eq!(prompt.contains("More reads are available."), remaining > 0);
            assert!(prompt.starts_with("Preserve the enabled mod functionality."));
            assert!(prompt.len() <= ASSISTANT_INVESTIGATION_PROMPT_BYTES);
            std::future::ready(Ok(String::from(if remaining == 0 {
                r#"{"action":"none","reason":"The missing dependency is not installed; no supported offline repair."}"#
            } else {
                r#"{"tool":"read_runtime"}"#
            })))
        },
        |_| std::future::ready(Ok(json!({"log":"dependency was not loaded"}))),
    ).await.unwrap();
    assert_eq!(calls, ASSISTANT_INVESTIGATION_STEPS);
    assert!(result.contains("no supported offline repair"));
}

#[tokio::test]
async fn investigation_reads_installed_mod_declarations_before_proposing_enablement() {
    let mut calls = 0;
    let mut reads = 0;
    let result = run_assistant_investigation(
        String::from("Diagnose a local mod dependency failure"),
        Vec::new(),
        |prompt| {
            calls += 1;
            std::future::ready(Ok(if calls == 1 {
                String::from(r#"{"tool":"inspect_installed_mods","names":["local_consumer","local_library"]}"#)
            } else {
                let objects = extract_json_objects(&prompt);
                let evidence: Value = serde_json::from_str(objects.last().unwrap()).unwrap();
                assert_eq!(evidence["data"]["entries"][1]["metadata"]["priority"], 10);
                assert_eq!(evidence["data"]["entries"][1]["folderName"], "local_library");
                String::from(r#"{"action":"none","reason":"Installed declarations read; inspect the current setting before proposing a patch."}"#)
            }))
        },
        |tool| {
            reads += 1;
            let request = serde_json::to_value(tool).unwrap();
            assert_eq!(request["tool"], "inspect_installed_mods");
            assert_eq!(request["names"], json!(["local_consumer", "local_library"]));
            assert_eq!(request["offset"], 0);
            std::future::ready(Ok(json!({"entries": [
                {"folderName": "local_consumer", "metadata": {"priority": 0}},
                {"folderName": "local_library", "metadata": {"priority": 10}}
            ]})))
        },
    ).await.unwrap();
    assert_eq!(reads, 1);
    assert_eq!(calls, 2);
    assert!(result.contains("Installed declarations read"));
}

#[test]
fn tool_evidence_budget_counts_redacted_json_without_formatting_overhead() {
    let dependencies = (0..30)
        .map(|index| json!({format!("library_{index:02}"): false}))
        .collect::<Vec<_>>();
    let entries = (0..5).map(|index| json!({
        "folderName": format!("local_{index}"), "status": "read",
        "source": format!("install/mods/local_{index}"),
        "files": {"modinfo": "present", "modmain": "present"},
        "metadata": {"name": "n".repeat(200), "version": "v".repeat(200), "mod_dependencies": dependencies},
    })).collect::<Vec<_>>();
    let result = json!({"ok": true, "data": {"entries": entries}});
    assert!(result.to_string().len() < 10 * 1024);
    assert!(serde_json::to_string_pretty(&result).unwrap().len() > ASSISTANT_TOOL_RESULT_BYTES);
    let delivered: Value = serde_json::from_str(&assistant_tool_result_text(&result)).unwrap();
    assert_eq!(delivered, result);
}

#[test]
fn installed_mod_identifiers_survive_redaction_without_exposing_private_paths() {
    let result = json!({"ok": true, "data": {
        "entries": [{"folderName": "local_library", "source": "install/mods/local_library",
            "metadata": {"name": "Library", "private_path": "C:/private/mod.lua", "api_key": "fixture-client-secret"}}],
        "sourceLabels": {"install": "Selected instance runtime installation"}
    }});
    let text = assistant_tool_result_text(&result);
    let delivered: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        delivered["data"]["entries"][0]["folderName"],
        "local_library"
    );
    assert_eq!(
        delivered["data"]["sourceLabels"]["install"],
        "Selected instance runtime installation"
    );
    assert!(!text.contains("C:/private"));
    assert!(!text.contains("fixture-client-secret"));
}

#[test]
fn installed_mod_tool_cannot_select_another_instance_or_arbitrary_root() {
    for request in [
        r#"{"tool":"inspect_installed_mods","names":[],"instanceId":"another"}"#,
        r#"{"tool":"inspect_installed_mods","names":[],"root":"C:/"}"#,
    ] {
        assert!(serde_json::from_str::<AssistantReadTool>(request).is_err());
    }
}

#[tokio::test]
async fn investigation_returns_read_evidence_to_the_model_before_a_repair() {
    let mut prompts = Vec::new();
    let mut calls = 0;
    let response = run_assistant_investigation(String::from("Fix mod order"), Vec::new(), |prompt| {
        prompts.push(prompt);
        calls += 1;
        std::future::ready(Ok(if calls == 1 {
            String::from(r#"{"tool":"read_mod_state"}"#)
        } else {
            String::from(r#"{"action":"customize_config","settingsPatch":{"mods":"dependency\naddon"}}"#)
        }))
    }, |tool| {
        assert!(matches!(tool, AssistantReadTool::ReadModState {}));
        std::future::ready(Ok(json!({"error": "addon requires dependency before it", "mods": "addon\ndependency"})))
    }).await.expect("investigation");
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].contains("addon requires dependency"));
    assert!(response.contains("customize_config"));
}

#[tokio::test]
async fn investigation_reports_read_failures_and_does_not_invent_a_success() {
    let mut calls = 0;
    let response = run_assistant_investigation(String::new(), Vec::new(), |prompt| {
        calls += 1;
        if calls == 2 { assert!(prompt.contains("access denied")); assert!(prompt.contains("false")); }
        std::future::ready(Ok(if calls == 1 { String::from(r#"{"tool":"read_config_file","file":"server.ini"}"#) }
            else { String::from(r#"{"action":"none","reason":"Configuration could not be read; no repair was made."}"#) }))
    }, |_| std::future::ready(Err(String::from("access denied")))).await.expect("diagnosis");
    assert!(response.contains("no repair was made"));
}

#[tokio::test]
async fn investigation_has_a_hard_tool_budget_and_rejects_shell_or_mixed_actions() {
    let mut reads = 0;
    let error = run_assistant_investigation(
        String::new(),
        Vec::new(),
        |_| std::future::ready(Ok(String::from(r#"{"tool":"read_runtime"}"#))),
        |_| {
            reads += 1;
            std::future::ready(Ok(json!({})))
        },
    )
    .await
    .expect_err("bounded investigation");
    assert_eq!(reads, ASSISTANT_INVESTIGATION_STEPS);
    assert!(error.contains("read limit"));
    for request in [
        r#"{"tool":"shell","command":"cmd.exe"}"#,
        r#"{"tool":"read_runtime","action":"start_server"}"#,
    ] {
        assert!(serde_json::from_str::<AssistantReadTool>(request).is_err());
    }
}

#[tokio::test]
async fn diagnostic_investigation_reads_real_runtime_before_the_first_model_call() {
    let response = run_assistant_investigation(
        String::from("Investigate startup failure"),
        vec![
            AssistantReadTool::ReadRuntime { lines: 80 },
            AssistantReadTool::ListConfigFiles { offset: 0 },
        ],
        |prompt| {
            assert!(prompt.contains("native dependency load failed"));
            assert!(prompt.contains("Master/modoverrides.lua"));
            std::future::ready(Ok(String::from(
                r#"{"action":"none","reason":"unsupported repair"}"#,
            )))
        },
        |tool| {
            std::future::ready(Ok(match tool {
                AssistantReadTool::ReadRuntime { lines: 80 } => {
                    json!({"log": "native dependency load failed"})
                }
                AssistantReadTool::ListConfigFiles { offset: 0 } => {
                    json!({"files": ["Master/modoverrides.lua"]})
                }
                _ => panic!("unexpected bootstrap read"),
            }))
        },
    )
    .await
    .unwrap();
    assert!(response.contains("unsupported repair"));
}

#[tokio::test]
async fn unknown_read_action_is_corrected_instead_of_silently_finishing() {
    let mut calls = 0;
    let mut reads = 0;
    let result = run_assistant_investigation(
        String::new(),
        Vec::new(),
        |prompt| {
            calls += 1;
            std::future::ready(Ok(match calls {
                1 => String::from(
                    r#"{"action":"read_config_file","file":"Master/modoverrides.lua"}"#,
                ),
                2 => {
                    assert!(prompt.contains("Invoke the matching native read tool"));
                    String::from(r#"{"tool":"read_config_file","file":"Master/modoverrides.lua"}"#)
                }
                _ => String::from(
                    r#"{"action":"none","reason":"Read completed; unsupported repair."}"#,
                ),
            }))
        },
        |tool| {
            assert!(matches!(tool, AssistantReadTool::ReadConfigFile { .. }));
            reads += 1;
            std::future::ready(Ok(json!({"content":"return {}"})))
        },
    )
    .await
    .unwrap();
    assert!(result.contains("Read completed"));
    assert_eq!(calls, 3);
    assert_eq!(reads, 1);
}

#[tokio::test]
async fn malformed_model_output_can_be_corrected_without_executing_it() {
    let mut calls = 0;
    let mut reads = 0;
    let response = run_assistant_investigation(String::new(), Vec::new(), |prompt| {
        calls += 1;
        std::future::ready(Ok(match calls {
            1 => String::from(r#"{"action":"customize_config","reason":[],"settingsPatch":{"mods":"bad"}}"#),
            2 => {
                assert!(prompt.contains("Response rejected; no operation was executed"));
                String::from(r#"{"tool":"read_settings","keys":["mods"]}"#)
            }
            _ => String::from(r#"{"action":"customize_config","settingsPatch":{"mods":"dependency\naddon"},"reason":"read configuration"}"#),
        }))
    }, |_| {
        reads += 1;
        std::future::ready(Ok(json!({"mods":"addon"})))
    }).await.unwrap();
    assert_eq!(calls, 3);
    assert_eq!(reads, 1);
    assert!(response.contains("dependency"));
}

#[tokio::test]
async fn repeated_invalid_responses_stop_with_a_bounded_error() {
    let mut calls = 0;
    let mut reads = 0;
    let error = run_assistant_investigation(
        String::new(),
        Vec::new(),
        |_| {
            calls += 1;
            std::future::ready(Ok(String::from("not JSON")))
        },
        |_| {
            reads += 1;
            std::future::ready(Ok(json!({})))
        },
    )
    .await
    .unwrap_err();
    assert_eq!(calls, 3);
    assert_eq!(reads, 0);
    assert!(error.contains("exactly one"));
}

#[test]
fn settings_evidence_reads_keys_after_the_initial_prompt_budget() {
    let settings = json!({"description": "a".repeat(2000), "mod_ids_csv": "200,100"});
    let schema = json!({"properties": {"mod_ids_csv": {"type": "string", "description": "Ordered mod IDs"}}});
    let result = assistant_settings_evidence(
        &settings.to_string(),
        Some(&schema.to_string()),
        &[String::from("mod_ids_csv")],
        0,
    )
    .unwrap();
    assert_eq!(result["entries"][0]["value"], "200,100");
    assert_eq!(
        result["entries"][0]["schema"]["description"],
        "Ordered mod IDs"
    );
}

#[tokio::test]
async fn investigation_context_budget_preserves_instructions_and_reports_omitted_reads() {
    let mut calls = 0;
    let mut reads = 0;
    let constraint = "operator-constraint ";
    let repetitions =
        (ASSISTANT_INVESTIGATION_PROMPT_BYTES - ASSISTANT_INVESTIGATION_GUIDE.len() - 11_000)
            / constraint.len();
    let response = run_assistant_investigation(constraint.repeat(repetitions), Vec::new(), |prompt| {
        calls += 1;
        assert!(prompt.len() <= ASSISTANT_INVESTIGATION_PROMPT_BYTES);
        assert!(prompt.starts_with("operator-constraint"));
        if calls == 3 {
            assert!(prompt.contains("first-read-evidence"));
            assert!(prompt.contains("remaining investigation context budget"));
            assert!(!prompt.contains("second-read-evidence"));
        }
        std::future::ready(Ok(if calls < 3 { String::from(r#"{"tool":"read_runtime"}"#) }
            else { String::from(r#"{"action":"none","reason":"More targeted evidence is needed."}"#) }))
    }, |_| {
        reads += 1;
        std::future::ready(Ok(json!({"marker": if reads == 1 { "first-read-evidence" } else { "second-read-evidence" }, "log": "x".repeat(7000)})))
    }).await.unwrap();
    assert_eq!(reads, 2);
    assert!(response.contains("More targeted"));
}

#[tokio::test]
async fn oversized_initial_context_never_calls_the_model() {
    let mut calls = 0;
    let result = run_assistant_investigation(
        "x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES),
        Vec::new(),
        |_| {
            calls += 1;
            std::future::ready(Ok(String::from(r#"{"action":"none"}"#)))
        },
        |_| std::future::ready(Ok(json!({}))),
    )
    .await;
    assert!(result.unwrap_err().contains("initial investigation"));
    assert_eq!(calls, 0);
}

#[test]
fn settings_evidence_redacts_secrets_before_restructuring_entries() {
    let credential = ["synthetic", "provider", "credential"].join("-");
    let settings =
        json!({"admin_password": credential, "rcon_password": credential, "max_players": 8});
    let result = assistant_settings_evidence(&settings.to_string(), None, &[], 0).unwrap();
    assert!(!result.to_string().contains(&credential));
    assert!(result.to_string().contains("REDACTED"));
}

#[test]
fn tool_result_budget_preserves_json_and_configuration_page_cursors() {
    let page =
        json!({"ok": true, "data": {"content": "\n\\\"".repeat(300), "nextOffsetBytes": 900}});
    let result: Value = serde_json::from_str(&assistant_tool_result_text(&page)).unwrap();
    assert_eq!(result["data"]["nextOffsetBytes"], 900);
    let oversized = json!({"ok": true, "data": "a".repeat(ASSISTANT_TOOL_RESULT_BYTES)});
    let result: Value = serde_json::from_str(&assistant_tool_result_text(&oversized)).unwrap();
    assert_eq!(result["ok"], false);
}

#[test]
fn investigation_file_identifiers_survive_redaction_without_exposing_secrets() {
    let request = r#"{"tool":"read_config_file","file":"Master/modoverrides.lua","offset":0}"#;
    let (tool, _) = parse_assistant_investigation_response(request).unwrap();
    let encoded = serde_json::to_string(&tool.unwrap()).unwrap();
    let sanitized: Value = serde_json::from_str(&redact_assistant_provider_text(&encoded)).unwrap();
    assert_eq!(sanitized["file"], "Master/modoverrides.lua");
    let credential = ["example", "secret"].join("-");
    let page = AssistantConfigFileSlice {
        path: String::from("Master/modoverrides.lua"),
        content: format!("password={credential}\nlogfile=C:\\private\\runtime.log"),
        offset_bytes: 0,
        next_offset_bytes: Some(1024),
        truncated: true,
    };
    let text = assistant_tool_result_text(&json!({"ok": true, "data": page}));
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["data"]["file"], "Master/modoverrides.lua");
    assert_eq!(parsed["data"]["nextOffsetBytes"], 1024);
    assert!(!text.contains(&credential));
    assert!(!text.contains("private"));
}

#[test]
fn native_file_evidence_exposes_matching_editable_settings_without_values() {
    let schema = json!({"properties": {
        "master_worldgenoverride_lua": {"type": "string", "title": "Master worldgenoverride.lua",
            "description": "Complete Lua content", "default": "default-value-not-needed",
            "x-lsgm-source-key": "worldgenoverride.lua:raw_lua"},
        "port": {"type": "integer", "title": "Network port"}
    }});
    let result = assistant_file_setting_candidates(
        Some(&schema.to_string()),
        "clusters/main/Master/worldgenoverride.lua",
    )
    .unwrap();
    assert_eq!(result["totalMatches"], 1);
    assert_eq!(result["entries"][0]["key"], "master_worldgenoverride_lua");
    assert_eq!(result["entries"][0]["type"], "string");
    assert_eq!(
        result["entries"][0]["sourceKey"],
        "worldgenoverride.lua:raw_lua"
    );
    assert!(!result.to_string().contains("default-value-not-needed"));
    assert_eq!(
        assistant_file_setting_candidates(None, "server.ini").unwrap()["entries"],
        json!([])
    );
}

#[test]
fn initial_log_excerpt_keeps_the_latest_failure_with_utf8_boundaries() {
    let text = format!(
        "{}\nError: 模组 addon requires dependency",
        "normal startup\n".repeat(80)
    );
    let excerpt = truncate_assistant_log_tail(&text, 600);
    assert!(excerpt.len() <= 600);
    assert!(excerpt.ends_with("addon requires dependency"));
}

#[test]
fn assistant_settings_never_persists_a_redacted_placeholder() {
    let placeholder = "[REDACTED]";
    assert!(
        merge_assistant_settings_patch(
            &json!({"rcon_password": ""}),
            &json!({"rcon_password": placeholder})
        )
        .is_err()
    );
}

#[test]
fn mod_evidence_does_not_confuse_game_modes_and_modifiers_with_mods() {
    for key in [
        "mod_ids_csv",
        "mods",
        "workshop_items",
        "master_enabled_workshop_mod_ids",
    ] {
        assert!(assistant_setting_describes_mods(key));
    }
    for key in [
        "game_mode",
        "UnitStatModifiers_Global",
        "PlayerDamageModifiers",
    ] {
        assert!(!assistant_setting_describes_mods(key));
    }
}
