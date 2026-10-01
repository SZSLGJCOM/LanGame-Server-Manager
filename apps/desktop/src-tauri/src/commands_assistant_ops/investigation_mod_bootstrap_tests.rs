use super::*;

#[tokio::test]
async fn declared_mod_dependencies_are_checked_once_without_revisiting_known_names() {
    let mut reads = Vec::new();
    run_assistant_investigation(
        String::new(),
        vec![AssistantReadTool::InspectInstalledMods {
            names: vec![String::from("consumer")],
            offset: 0,
        }],
        |prompt| {
            assert!(prompt.contains("installed_dependency_version"));
            assert!(prompt.contains("Read budget remaining: 6 of 8"));
            std::future::ready(Ok(String::from(
                r#"{"action":"none","reason":"Installation evidence supplied."}"#,
            )))
        },
        |tool| {
            let AssistantReadTool::InspectInstalledMods { names, offset } = tool else {
                panic!("unexpected read")
            };
            assert_eq!(offset, 0);
            reads.push(names.clone());
            std::future::ready(Ok(if names == ["consumer"] {
                json!({"entries": [{"folderName": "consumer", "status": "read", "metadata": {
                    "mod_dependencies": [{"consumer": false}, {"dependency": false}]
                }}]})
            } else {
                assert_eq!(names, ["dependency"]);
                // Cycles and transitive dependencies must not create an unbounded walk.
                json!({"entries": [{"folderName": "dependency", "status": "read", "metadata": {
                    "version": "installed_dependency_version",
                    "mod_dependencies": [{"consumer": false}, {"transitive": false}]
                }}]})
            }))
        },
    )
    .await
    .unwrap();
    assert_eq!(reads.len(), 2);
}

#[tokio::test]
async fn dependency_lookup_preserves_failures_and_the_read_limit() {
    for initial_count in [1, ASSISTANT_INVESTIGATION_STEPS] {
        let mut reads = 0;
        let mut initial = vec![AssistantReadTool::ReadRuntime { lines: 1 }; initial_count - 1];
        initial.push(AssistantReadTool::InspectInstalledMods {
            names: vec![String::from("consumer")],
            offset: 0,
        });
        run_assistant_investigation(
            String::new(), initial,
            |prompt| {
                if initial_count == 1 {
                    assert!(prompt.contains("dependency metadata unavailable"));
                    assert!(prompt.contains(r#""ok":false"#));
                }
                assert!(prompt.contains(&format!("Read budget remaining: {} of 8", if initial_count == 1 { 6 } else { 0 })));
                std::future::ready(Ok(String::from(r#"{"action":"none","reason":"Unverified dependency."}"#)))
            },
            |tool| {
                reads += 1;
                std::future::ready(match tool {
                    AssistantReadTool::InspectInstalledMods { names, .. } if names == ["consumer"] =>
                        Ok(json!({"entries": [{"folderName": "consumer", "status": "read", "metadata": {"mod_dependencies": [{"dependency": false}]}}]})),
                    AssistantReadTool::InspectInstalledMods { names, .. } => {
                        assert_eq!(names, ["dependency"]);
                        Err(String::from("dependency metadata unavailable"))
                    }
                    AssistantReadTool::ReadRuntime { .. } => Ok(json!({})),
                    _ => panic!("unexpected read"),
                })
            },
        ).await.unwrap();
        assert_eq!(
            reads,
            if initial_count == 1 {
                2
            } else {
                ASSISTANT_INVESTIGATION_STEPS
            }
        );
    }
}

#[tokio::test]
async fn unread_metadata_and_already_supplied_dependencies_do_not_expand_reads() {
    for status in ["read", "parse_error"] {
        let mut reads = 0;
        run_assistant_investigation(
            String::new(),
            vec![AssistantReadTool::InspectInstalledMods { names: Vec::new(), offset: 0 }],
            |_| std::future::ready(Ok(String::from(r#"{"action":"none","reason":"Evidence recorded."}"#))),
            |tool| {
                reads += 1;
                assert!(matches!(tool, AssistantReadTool::InspectInstalledMods { .. }));
                std::future::ready(Ok(json!({"entries": [
                    {"folderName": "consumer", "status": status, "metadata": {"mod_dependencies": [{"dependency": false}]}},
                    {"folderName": "dependency", "status": "read", "metadata": {}}
                ]})))
            },
        ).await.unwrap();
        assert_eq!(reads, 1);
    }
}

#[test]
fn automatic_mod_metadata_requires_a_successful_nonempty_bounded_name_list() {
    for result in [
        json!({"ok": false, "data": {"modErrorNames": ["local_mod"]}}),
        json!({"ok": true, "data": {"modErrorNames": []}}),
        json!({"ok": true, "data": {"modErrorNames": [null]}}),
        json!({"ok": true, "data": {"modErrorNames": vec!["local_mod"; 6]}}),
        json!({"ok": true, "data": {"log": "{\"modErrorNames\":[\"local_mod\"]}"}}),
    ] {
        assert!(assistant_mod_error_follow_up(&result).is_none());
    }
}

#[tokio::test]
async fn other_tool_data_cannot_schedule_a_mod_metadata_read() {
    let mut reads = 0;
    run_assistant_investigation(
        String::new(),
        vec![AssistantReadTool::ReadModState {}],
        |_| {
            std::future::ready(Ok(String::from(
                r#"{"action":"none","reason":"No runtime evidence."}"#,
            )))
        },
        |tool| {
            reads += 1;
            assert!(matches!(tool, AssistantReadTool::ReadModState {}));
            std::future::ready(Ok(json!({"modErrorNames": ["local_mod"]})))
        },
    )
    .await
    .unwrap();
    assert_eq!(reads, 1);
}

#[tokio::test]
async fn native_mod_error_seeds_metadata_once_within_the_existing_read_budget() {
    let mut tools = Vec::new();
    let mut calls = 0;
    run_assistant_investigation(
        String::from("Repair the mod while preserving its functionality."),
        vec![
            AssistantReadTool::ReadRuntime { lines: 80 },
            AssistantReadTool::ListConfigFiles { offset: 0 },
        ],
        |prompt| {
            calls += 1;
            // Runtime, config listing, enablement and installed metadata.
            let remaining = 5 - calls;
            assert!(prompt.contains("declared_library"));
            assert!(prompt.contains("master_modoverrides_lua"));
            assert!(prompt.contains(&format!("Read budget remaining: {remaining} of 8")));
            std::future::ready(Ok(String::from(if remaining == 0 {
                r#"{"action":"none","reason":"The declared dependency still needs enablement evidence."}"#
            } else {
                r#"{"tool":"read_runtime"}"#
            })))
        },
        |tool| {
            let request = serde_json::to_value(&tool).unwrap();
            tools.push(request.clone());
            std::future::ready(Ok(match tool {
                AssistantReadTool::ReadRuntime { .. } => json!({
                    "log": "MOD ERROR: local_consumer: failed initialization",
                    "modErrorNames": ["local_consumer"]
                }),
                AssistantReadTool::ListConfigFiles { .. } => json!({"files": []}),
                AssistantReadTool::ReadModState {} => json!({"configuration": {"entries": [{
                    "key": "master_modoverrides_lua", "value": "return { local_consumer = { enabled = true } }"
                }]}}),
                AssistantReadTool::InspectInstalledMods { names, offset } => {
                    assert_eq!(names, vec!["local_consumer"]);
                    assert_eq!(offset, 0);
                    json!({"entries": [{"folderName": "local_consumer", "metadata": {
                        "mod_dependencies": [{"declared_library": false}]
                    }}]})
                }
                _ => panic!("unexpected read: {request}"),
            }))
        },
    ).await.unwrap();
    assert_eq!(tools.len(), ASSISTANT_INVESTIGATION_STEPS);
    assert_eq!(calls, 5);
    assert_eq!(
        tools
            .iter()
            .filter(|tool| tool["tool"] == "inspect_installed_mods")
            .count(),
        1
    );
}

#[tokio::test]
async fn automatic_mod_metadata_failure_remains_an_explicit_evidence_gap() {
    let result = run_assistant_investigation(
        String::new(),
        vec![AssistantReadTool::ReadRuntime { lines: 80 }],
        |prompt| {
            let objects = extract_json_objects(&prompt);
            let evidence: Value = serde_json::from_str(objects.last().unwrap()).unwrap();
            assert_eq!(evidence, json!({"ok": false, "error":
                redact_assistant_provider_text("Mod evidence paths cannot cross symlinks")
            }));
            assert!(prompt.contains("Read budget remaining: 5 of 8"));
            std::future::ready(Ok(String::from(
                r#"{"action":"none","reason":"Installed metadata is inaccessible; no change proposed."}"#,
            )))
        },
        |tool| std::future::ready(match tool {
            AssistantReadTool::ReadRuntime { .. } => Ok(json!({"modErrorNames": ["workshop-123456"]})),
            AssistantReadTool::InspectInstalledMods { .. } => Err(String::from("Mod evidence paths cannot cross symlinks")),
            AssistantReadTool::ReadModState {} => Ok(json!({"configuration": {"entries": []}})),
            _ => panic!("unexpected read"),
        }),
    ).await.unwrap();
    assert!(result.contains("no change proposed"));
}

#[tokio::test]
async fn final_runtime_read_does_not_schedule_a_ninth_automatic_read() {
    let mut reads = 0;
    let initial = (0..ASSISTANT_INVESTIGATION_STEPS)
        .map(|_| AssistantReadTool::ReadRuntime { lines: 80 })
        .collect();
    run_assistant_investigation(
        String::new(),
        initial,
        |prompt| {
            assert!(prompt.contains("Read budget remaining: 0 of 8"));
            std::future::ready(Ok(String::from(
                r#"{"action":"none","reason":"Read budget exhausted."}"#,
            )))
        },
        |tool| {
            reads += 1;
            assert!(matches!(tool, AssistantReadTool::ReadRuntime { .. }));
            std::future::ready(Ok(json!({"modErrorNames": ["local_consumer"]})))
        },
    )
    .await
    .unwrap();
    assert_eq!(reads, ASSISTANT_INVESTIGATION_STEPS);
}

#[tokio::test]
async fn mod_bootstrap_reuses_enablement_and_respects_a_single_remaining_slot() {
    for (initial, expected_reads, expected_state_reads) in [
        (
            vec![
                AssistantReadTool::ReadModState {},
                AssistantReadTool::ReadRuntime { lines: 80 },
            ],
            3,
            1,
        ),
        (
            (0..7)
                .map(|_| AssistantReadTool::ReadRuntime { lines: 80 })
                .collect(),
            8,
            0,
        ),
    ] {
        let mut reads = Vec::new();
        run_assistant_investigation(
            String::new(),
            initial,
            |prompt| {
                assert!(prompt.contains(&format!(
                    "Read budget remaining: {} of 8",
                    8 - expected_reads
                )));
                std::future::ready(Ok(String::from(
                    r#"{"action":"none","reason":"Metadata recorded."}"#,
                )))
            },
            |tool| {
                let result = if matches!(tool, AssistantReadTool::ReadRuntime { .. }) {
                    json!({"modErrorNames": ["local_consumer"]})
                } else {
                    json!({"entries": []})
                };
                reads.push(serde_json::to_value(tool).unwrap());
                std::future::ready(Ok(result))
            },
        )
        .await
        .unwrap();
        assert_eq!(reads.len(), expected_reads);
        assert_eq!(
            reads
                .iter()
                .filter(|read| read["tool"] == "read_mod_state")
                .count(),
            expected_state_reads
        );
        assert_eq!(
            reads
                .iter()
                .filter(|read| read["tool"] == "inspect_installed_mods")
                .count(),
            1
        );
    }
}
