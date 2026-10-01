use super::*;
use std::future::ready;
use std::sync::atomic::{AtomicUsize, Ordering};

fn context<'a>() -> AssistantInvestigationContext<'a> {
    AssistantInvestigationContext {
        prompt: String::from("Investigate the selected server before proposing a correction."),
        initial_reads: Vec::new(),
        tools: assistant_fixture_tools(),
        draft: None,
        instance: None,
        module: None,
        completion: AssistantInvestigationCompletion::OperationProposal,
    }
}

fn call(id: &str, name: &str, arguments: Value) -> AssistantToolCall {
    AssistantToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}

fn reply(calls: Vec<AssistantToolCall>) -> AssistantToolReply {
    let raw_message = json!({"role":"assistant", "content":null, "tool_calls":calls.iter().map(|call| {
        json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments.to_string()}})
    }).collect::<Vec<_>>()});
    AssistantToolReply {
        content: String::new(),
        calls,
        raw_message,
    }
}

fn tool_result(messages: &[AssistantToolMessage], id: &str, name: &str) -> (Value, bool) {
    let matching = messages
        .iter()
        .filter_map(|message| match message {
            AssistantToolMessage::ToolResult {
                call_id,
                name: actual_name,
                content,
                is_error,
            } if call_id == id => {
                assert_eq!(actual_name, name);
                Some((serde_json::from_str(content).unwrap(), *is_error))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "one result must match each native call ID"
    );
    matching.into_iter().next().unwrap()
}

fn diagnosis(id: &str) -> AssistantToolReply {
    reply(vec![call(
        id,
        "report_limitation",
        json!({"reason":"Evidence inspected; no mutation requested."}),
    )])
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_retains_rejected_proposal_and_paired_error_before_correction() {
    let rejected = reply(vec![call(
        "bad-plan",
        "propose_operation",
        json!({
            "action":"customize_config","settingsPatch":{"max_players":0},"reason":"Rejected proposal"
        }),
    )]);
    let corrected = json!({"action":"customize_config","settingsPatch":{"max_players":24}});
    let validations = AtomicUsize::new(0);
    let reads = AtomicUsize::new(0);
    let mut turns = 0;
    let output = Box::pin(run_assistant_tool_investigation(context(), |messages, _| {
        turns += 1;
        ready(Ok(if turns == 1 { rejected.clone() } else {
            let assistant_index = messages.iter().position(|message| matches!(message, AssistantToolMessage::Assistant(_))).unwrap();
            let AssistantToolMessage::Assistant(retained) = &messages[assistant_index] else { unreachable!() };
            assert_eq!(serde_json::to_value(retained).unwrap(), serde_json::to_value(&rejected).unwrap());
            assert!(matches!(&messages[assistant_index+1], AssistantToolMessage::ToolResult { call_id, .. } if call_id == "bad-plan"));
            let (error, failed) = tool_result(&messages, "bad-plan", "propose_operation");
            assert!(failed);
            assert_eq!(error["ok"], false);
            assert!(error["error"].as_str().unwrap().contains("max_players must remain positive"));
            reply(vec![call("corrected-plan", "propose_operation", corrected.clone())])
        }))
    }, |_| {
        reads.fetch_add(1,Ordering::SeqCst);
        ready(Ok(json!({})))
    }, &|operation| {
        validations.fetch_add(1, Ordering::SeqCst);
        let operation: Value = serde_json::from_str(operation).unwrap();
        if operation["settingsPatch"]["max_players"] == 0 {
            Err(String::from("max_players must remain positive"))
        } else { Ok(()) }
    })).await.unwrap();
    assert_eq!(serde_json::from_str::<Value>(&output).unwrap(), corrected);
    assert_eq!(turns, 2);
    assert_eq!(validations.load(Ordering::SeqCst), 2);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_pairs_every_result_in_a_multiple_read_turn() {
    let mut turns = 0;
    let reads = AtomicUsize::new(0);
    let output = Box::pin(run_assistant_tool_investigation(
        context(),
        |messages, _| {
            turns += 1;
            ready(Ok(if turns == 1 {
                reply(vec![
                    call(
                        "settings-read",
                        "read_settings",
                        json!({"keys":["cluster_name"]}),
                    ),
                    call("runtime-read", "read_runtime", json!({"lines":1})),
                ])
            } else {
                assert_eq!(reads.load(Ordering::SeqCst), 2);
                assert_eq!(
                    tool_result(&messages, "settings-read", "read_settings"),
                    (
                        json!({"ok":true,"data":{"cluster_name":"quoted \"name\"\n中文"}}),
                        false
                    )
                );
                assert_eq!(
                    tool_result(&messages, "runtime-read", "read_runtime"),
                    (
                        json!({"ok":true,"data":{"running":false,"lastExit":7}}),
                        false
                    )
                );
                let ids = messages
                    .iter()
                    .filter_map(|message| match message {
                        AssistantToolMessage::ToolResult { call_id, .. } => Some(call_id.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(ids, ["settings-read", "runtime-read"]);
                diagnosis("reads-complete")
            }))
        },
        |request| {
            reads.fetch_add(1, Ordering::SeqCst);
            ready(Ok(match request {
                AssistantReadTool::ReadSettings { keys, offset } => {
                    assert_eq!(keys, ["cluster_name"]);
                    assert_eq!(offset, 0);
                    json!({"cluster_name":"quoted \"name\"\n中文"})
                }
                AssistantReadTool::ReadRuntime { lines } => {
                    assert_eq!(lines, 1);
                    json!({"running":false,"lastExit":7})
                }
                _ => panic!("Unexpected read"),
            }))
        },
        &|_| Ok(()),
    ))
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["action"],
        "none"
    );
    assert_eq!(turns, 2);
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_rejects_mixed_final_and_read_atomically_before_dispatch() {
    let reads = AtomicUsize::new(0);
    let validations = AtomicUsize::new(0);
    let mut turns = 0;
    Box::pin(run_assistant_tool_investigation(
        context(),
        |messages, _| {
            turns += 1;
            ready(Ok(if turns == 1 {
                reply(vec![
                    call("mixed-read", "read_runtime", json!({"lines":1})),
                    call(
                        "mixed-plan",
                        "propose_operation",
                        json!({"action":"start_server"}),
                    ),
                ])
            } else {
                assert_eq!(reads.load(Ordering::SeqCst), 0);
                assert_eq!(validations.load(Ordering::SeqCst), 0);
                for (id, name) in [
                    ("mixed-read", "read_runtime"),
                    ("mixed-plan", "propose_operation"),
                ] {
                    let (result, failed) = tool_result(&messages, id, name);
                    assert!(failed);
                    assert_eq!(result["ok"], false);
                    assert!(result["error"].as_str().unwrap().contains("only call"));
                }
                diagnosis("mixed-rejected")
            }))
        },
        |_| {
            reads.fetch_add(1, Ordering::SeqCst);
            ready(Ok(json!({})))
        },
        &|operation| {
            validations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                serde_json::from_str::<Value>(operation).unwrap()["action"],
                "none"
            );
            Ok(())
        },
    ))
    .await
    .unwrap();
    assert_eq!(turns, 2);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_rejects_duplicate_ids_before_reexecuting_a_read() {
    for same_turn in [true, false] {
        let reads = AtomicUsize::new(0);
        let validations = AtomicUsize::new(0);
        let mut turns = 0;
        let error = Box::pin(run_assistant_tool_investigation(
            context(),
            |_, _| {
                turns += 1;
                let mut calls = vec![call("reused-id", "read_runtime", json!({"lines":1}))];
                if same_turn {
                    calls.push(call("reused-id", "read_settings", json!({"keys":[]})));
                }
                ready(Ok(reply(calls)))
            },
            |_| {
                reads.fetch_add(1, Ordering::SeqCst);
                ready(Ok(json!({})))
            },
            &|_| {
                validations.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        ))
        .await
        .unwrap_err();
        assert!(error.contains("tool call ID"), "{error}");
        assert_eq!(reads.load(Ordering::SeqCst), usize::from(!same_turn));
        assert_eq!(turns, if same_turn { 1 } else { 2 });
        assert_eq!(validations.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_finishes_draft_and_injects_only_original_requirements() {
    let original = "将人数上限设为24。";
    let module = module_settings_tests::schema_module(json!({"properties":{
        "max_players":{"type":"integer","minimum":1,"maximum":64}
    }}));
    let mut context = context();
    context.prompt = original.into();
    context.module = Some(&module);
    context.draft = Some(AssistantRequirementsDraft::new(original));
    context.tools = assistant_investigation_tools(None, Some(&module), true);
    let validations = AtomicUsize::new(0);
    let reads = AtomicUsize::new(0);
    let mut turns = 0;
    let output=Box::pin(run_assistant_tool_investigation(context,|messages,tools| {
        turns+=1;
        let proposal_available=tools.iter().any(|tool|tool.name=="propose_operation");
        let calls=match turns {
            1 => {
                assert!(!proposal_available);
                assert!(assistant_fixture_transcript(&messages).contains(original));
                vec![call("record","record_task_requirements",json!({"settings":[{
                    "sourceId":"request_1","key":"max_players","expected":24
                }],"ports":[],"forbiddenActions":[],"unverified":[]}))]
            }
            2 => {
                assert!(!proposal_available);
                let (result,failed)=tool_result(&messages,"record","record_task_requirements");
                assert!(!failed); assert_eq!(result["data"]["errors"],json!([]));
                assert_eq!(result["data"]["itemCount"],1);
                vec![call("finish","finish_task_requirements",json!({}))]
            }
            3 => {
                assert!(proposal_available);
                assert!(tools.iter().all(|tool|tool.name!="record_task_requirements"));
                assert_eq!(tool_result(&messages,"finish","finish_task_requirements").0["data"]["ready"],true);
                vec![call("override","propose_operation",json!({"action":"create_server",
                    "taskRequirements":{"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}}))]
            }
            4 => {
                let (error,failed)=tool_result(&messages,"override","propose_operation");
                assert!(failed); assert_eq!(error["ok"],false);
                assert!(error["error"].as_str().unwrap().contains("fixed task requirements"));
                assert_eq!(validations.load(Ordering::SeqCst),0);
                vec![call("create","propose_operation",json!({"action":"create_server","moduleId":module.summary.id}))]
            }
            _=>panic!("Unexpected drafting turn"),
        };
        ready(Ok(reply(calls)))
    }, |_| { reads.fetch_add(1,Ordering::SeqCst); ready(Ok(json!({}))) },
    &|operation| {
        validations.fetch_add(1,Ordering::SeqCst);
        let operation:Value=serde_json::from_str(operation).unwrap();
        assert_eq!(operation["taskRequirements"],json!({"settings":[{
            "key":"max_players","expected":24,"description":original,"sourceText":original
        }],"ports":[],"forbiddenActions":[],"unverified":[]})); Ok(())
    })).await.unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&output).unwrap()["action"],
        "create_server"
    );
    assert_eq!(turns, 4);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_scope_rejection_never_invokes_the_reader() {
    let module = module_settings_tests::schema_module(json!({"properties":{}}));
    let (_, instance) = task_tests::task_fixture(false);
    for instance_scope in [false, true] {
        let mut context = context();
        context.instance = instance_scope.then_some(&instance);
        context.module = Some(&module);
        context.tools = assistant_investigation_tools(context.instance, context.module, true);
        let forbidden = if instance_scope {
            "read_module_settings"
        } else {
            "read_runtime"
        };
        let reads = AtomicUsize::new(0);
        let mut turns = 0;
        Box::pin(run_assistant_tool_investigation(
            context,
            |messages, tools| {
                turns += 1;
                assert!(tools.iter().all(|tool| tool.name != forbidden));
                ready(Ok(if turns == 1 {
                    reply(vec![call("wrong-scope", forbidden, json!({}))])
                } else {
                    let (error, failed) = tool_result(&messages, "wrong-scope", forbidden);
                    assert!(failed);
                    assert_eq!(error["ok"], false);
                    assert!(error["error"].as_str().unwrap().contains("target scope"));
                    diagnosis("scope-rejected")
                }))
            },
            |_| {
                reads.fetch_add(1, Ordering::SeqCst);
                ready(Ok(json!({})))
            },
            &|_| Ok(()),
        ))
        .await
        .unwrap();
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(turns, 2);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_propagates_provider_truncation_without_a_proposal_or_retry() {
    let expected = "assistant response exceeded its output token limit";
    let reads = AtomicUsize::new(0);
    let validations = AtomicUsize::new(0);
    let mut turns = 0;
    let error = Box::pin(run_assistant_tool_investigation(
        context(),
        |_, _| {
            turns += 1;
            ready(if turns == 1 {
                Ok(reply(vec![call(
                    "initial-read",
                    "read_runtime",
                    json!({"lines":1}),
                )]))
            } else {
                Err(expected.into())
            })
        },
        |_| {
            reads.fetch_add(1, Ordering::SeqCst);
            ready(Ok(json!({"running":false})))
        },
        &|_| {
            validations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    ))
    .await
    .unwrap_err();
    assert_eq!(error, expected);
    assert_eq!(turns, 2);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(validations.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn native_investigation_corrects_malformed_final_calls_without_ignoring_fields() {
    let mut turns = 0;
    let validations = AtomicUsize::new(0);
    Box::pin(run_assistant_tool_investigation(
        context(),
        |messages, _| {
            turns += 1;
            ready(Ok(match turns {
                1 => reply(vec![call(
                    "invalid-reason",
                    "report_limitation",
                    json!({"reason":42}),
                )]),
                2 => {
                    assert!(tool_result(&messages, "invalid-reason", "report_limitation").1);
                    reply(vec![call(
                        "invalid-field",
                        "propose_operation",
                        json!({"action":"start_server","inventedField":true}),
                    )])
                }
                _ => {
                    let (result, failed) =
                        tool_result(&messages, "invalid-field", "propose_operation");
                    assert!(failed);
                    assert!(
                        result["error"]
                            .as_str()
                            .unwrap()
                            .contains("Unknown operation field")
                    );
                    diagnosis("corrected-limit")
                }
            }))
        },
        |_| ready(Err(String::from("unexpected read"))),
        &|_| {
            validations.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    ))
    .await
    .unwrap();
    assert_eq!(turns, 3);
    assert_eq!(validations.load(Ordering::SeqCst), 1);
}
