use super::*;
use std::future::ready;

fn context<'a>(
    request: &str,
    instance: Option<&'a InstanceDetails>,
) -> AssistantInvestigationContext<'a> {
    AssistantInvestigationContext {
        prompt: request.into(),
        initial_reads: vec![],
        tools: assistant_investigation_tools(instance, None, true),
        draft: Some(AssistantRequirementsDraft::deferred_for_lifecycle(request)),
        instance,
        module: None,
        completion: AssistantInvestigationCompletion::OperationProposal,
    }
}

fn reply(turn: usize, name: &str, arguments: Value) -> AssistantToolReply {
    AssistantToolReply {
        content: String::new(),
        raw_message: Value::Null,
        calls: vec![AssistantToolCall {
            id: format!("lifecycle-draft-{turn}"),
            name: name.into(),
            arguments,
        }],
    }
}

fn empty_requirements() -> Value {
    json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]})
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_draft_keeps_ordinary_apply_change_tools_and_single_turn_behavior() {
    let mut turn = 0;
    let operation = Box::pin(run_assistant_tool_investigation(
        context("Change the selected server.", None),
        |_, tools| {
            turn += 1;
            assert!(tools.iter().any(|tool| tool.name == "propose_operation"));
            assert!(
                !tools
                    .iter()
                    .any(|tool| tool.name == "record_task_requirements")
            );
            ready(Ok(reply(
                turn,
                "propose_operation",
                    json!({"action":"customize_config","settingsPatch":{"max_players":12},"reason":"Apply the requested setting."}),
            )))
        },
        |_| ready(Err("unexpected read".into())),
        &|_| Ok(()),
    ))
    .await
    .unwrap();
    assert_eq!(turn, 1);
    assert!(
        serde_json::from_str::<Value>(&operation)
            .unwrap()
            .get("taskRequirements")
            .is_none()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_first_proposal_requires_draft_and_preserves_original_source_constraints() {
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::CreateBackup,
        AssistantOperationAction::RestoreBackup,
    ] {
        let request = "Carry out the requested lifecycle operation. Do not create a new server.";
        let (_, instance) = task_tests::task_fixture(true);
        let mut turn = 0;
        let operation = Box::pin(run_assistant_tool_investigation(context(request, Some(&instance)),
            |messages, tools| {
                turn += 1;
                let (tool, arguments) = match turn {
                    1 | 4 => {
                        assert!(tools.iter().any(|tool| tool.name == "propose_operation"));
                        ("propose_operation", json!({"action":action}))
                    }
                    2 => {
                        assert!(!tools.iter().any(|tool| tool.name == "propose_operation"));
                        assert!(tools.iter().any(|tool| tool.name == "record_task_requirements"));
                        assert!(messages.iter().any(|message| matches!(message, AssistantToolMessage::ToolResult { call_id, is_error:true, .. } if call_id == "lifecycle-draft-1")));
                        let mut requirements = empty_requirements();
                        requirements["forbiddenActions"] = json!([{"sourceId":"request_1","action":"create_server"}]);
                        ("record_task_requirements", requirements)
                    }
                    3 => ("finish_task_requirements", json!({})),
                    _ => panic!("unexpected turn {turn}"),
                };
                ready(Ok(reply(turn, tool, arguments)))
            }, |_| ready(Err("unexpected read".into())), &|operation| {
                let plan: Value = serde_json::from_str(operation).unwrap();
                assert!(plan.get("taskRequirements").is_some(), "first proposal must not reach validation");
                Ok(())
            },
        )).await.unwrap();
        let plan: Value = serde_json::from_str(&operation).unwrap();
        assert_eq!(turn, 4);
        let forbidden = &plan["taskRequirements"]["forbiddenActions"][0];
        assert_eq!(forbidden["sourceText"], request);
        assert_eq!(forbidden["action"], "create_server");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_draft_forbidden_backup_rejects_stop_side_effect_and_restore_safeguard() {
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::RestoreBackup,
    ] {
        let request = "Do not create backups.";
        let (base, mut instance) = task_tests::task_fixture(true);
        let mut task = base.as_ref().clone();
        task.request.goal = AssistantTaskGoal::ApplyChange;
        task.original_request = request.into();
        task.requirements = None;
        instance.auto_backup_on_stop = true;
        if action != AssistantOperationAction::RestoreBackup {
            instance.summary.status = InstanceStatus::Running;
            instance.summary.active_process_count = 1;
            instance.active_run = Some(ActiveInstanceRun {
                run_id: 7,
                session_id: Some("current".into()),
                pid: Some(42),
                log_path: None,
                process_count: 1,
                processes: vec![],
            });
        }
        let mut turn = 0;
        let operation = Box::pin(run_assistant_tool_investigation(context(request, Some(&instance)),
            |messages, _| {
                turn += 1;
                let (tool, arguments) = match turn {
                    1 | 4 => ("propose_operation", json!({"action":action,"instanceId":instance.summary.id,"backupId":if action == AssistantOperationAction::RestoreBackup { json!("saves-confirmed") } else { Value::Null }})),
                    2 => ("record_task_requirements", json!({"settings":[],"ports":[],"unverified":[],"forbiddenActions":[{"sourceId":"request_1","action":"create_backup"}]})),
                    3 => ("finish_task_requirements", json!({})),
                    5 => {
                        let error = messages.iter().find_map(|message| match message { AssistantToolMessage::ToolResult {call_id,content,is_error:true,..} if call_id == "lifecycle-draft-4" => Some(content), _ => None }).unwrap();
                        assert!(error.contains("forbidden"), "{error}");
                        ("report_limitation", json!({"reason":"The requested action creates a backup, which the user excluded."}))
                    }
                    _ => panic!("unexpected turn {turn}"),
                };
                ready(Ok(reply(turn, tool, arguments)))
            }, |_| ready(Err("unexpected read".into())), &|operation| {
                let plan = parse_assistant_operation_plan_response(operation)?;
                task.bind_requirements(&plan, Some(&instance), None)?.validate_plan(&plan, Some(&instance))
            },
        )).await.unwrap();
        assert_eq!(turn, 5);
        assert_eq!(
            serde_json::from_str::<Value>(&operation).unwrap()["action"],
            "none"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_draft_activation_survives_a_paused_investigation() {
    let slot = StdMutex::new(None);
    let mut turn = 0;
    let error = Box::pin(run_assistant_tool_investigation_in_session(
        context("Stop this server.", None),
        |_, _| {
            turn += 1;
            ready(if turn == 1 {
                Ok(reply(
                    turn,
                    "propose_operation",
                    json!({"action":"stop_server"}),
                ))
            } else {
                Err("model paused".into())
            })
        },
        |_| ready(Err("unexpected read".into())),
        &|_| panic!("no completed proposal"),
        None,
        Some(&slot),
    ))
    .await
    .unwrap_err();
    assert_eq!(error, "model paused");
    let mut resumed = context("Stop this server.", None);
    resumed.draft = slot.lock().unwrap().take();
    assert!(resumed.draft_pending());
    assert!(
        !resumed
            .visible_tools()
            .unwrap()
            .iter()
            .any(|tool| tool.name == "propose_operation")
    );
    assert_eq!(
        resumed.draft.as_ref().unwrap().source_catalog()["sources"][0]["text"],
        "Stop this server."
    );
    let mut resumed_turn = 0;
    let operation = Box::pin(run_assistant_tool_investigation(
        resumed,
        |_, _| {
            resumed_turn += 1;
            let (tool, arguments) = match resumed_turn {
                1 => ("record_task_requirements", empty_requirements()),
                2 => ("finish_task_requirements", json!({})),
                3 => ("propose_operation", json!({"action":"stop_server"})),
                _ => panic!("unexpected resumed turn"),
            };
            ready(Ok(reply(resumed_turn, tool, arguments)))
        },
        |_| ready(Err("unexpected read".into())),
        &|_| Ok(()),
    ))
    .await
    .unwrap();
    assert_eq!(resumed_turn, 3);
    assert!(
        serde_json::from_str::<Value>(&operation)
            .unwrap()
            .get("taskRequirements")
            .is_some()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_draft_activation_does_not_reset_the_read_budget() {
    let mut investigation = context("Stop this server.", None);
    investigation.initial_reads =
        vec![AssistantReadTool::ReadHostInfo {}; ASSISTANT_INVESTIGATION_STEPS];
    let mut turn = 0;
    let mut reads = 0;
    let error = Box::pin(run_assistant_tool_investigation(
        investigation,
        |_, _| {
            turn += 1;
            ready(Ok(if turn == 1 {
                reply(turn, "propose_operation", json!({"action":"stop_server"}))
            } else {
                reply(turn, "read_host_info", json!({}))
            }))
        },
        |_| {
            reads += 1;
            ready(Ok(json!({"platform":"fixture"})))
        },
        &|_| panic!("no proposal"),
    ))
    .await
    .unwrap_err();
    assert!(error.contains("read limit"), "{error}");
    assert_eq!(reads, ASSISTANT_INVESTIGATION_STEPS);
}

#[test]
fn lifecycle_plan_cannot_bypass_requirements_binding() {
    let (base, instance) = task_tests::task_fixture(true);
    let mut task = base.as_ref().clone();
    task.request.goal = AssistantTaskGoal::ApplyChange;
    task.requirements = None;
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::CreateBackup,
        AssistantOperationAction::RestoreBackup,
    ] {
        let plan = AssistantOperationPlan {
            action,
            ..assistant_safe_none_plan("lifecycle request".into())
        };
        assert!(
            task.bind_requirements(&plan, Some(&instance), None)
                .is_err()
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lifecycle_source_catalog_overflow_stops_before_another_model_turn_and_retains_draft() {
    let request = format!(
        "Stop this server; keep all original constraints: {}",
        "a".repeat(8_000)
    );
    let proposal = reply(1, "propose_operation", json!({"action":"stop_server"}));
    let mut probe = context(&request, None);
    let initial_tools = probe.visible_tools().unwrap();
    let catalog = probe.draft.as_ref().unwrap().source_catalog();
    let guidance = probe
        .activate_lifecycle_requirements(&proposal.calls[0].arguments)
        .unwrap();
    let draft_tools = probe.visible_tools().unwrap();

    // Find a real context-budget boundary without assuming tool-schema sizes.
    // Two KiB reserves more than the activation error, a downgraded read gap
    // and the next control message, so the old fallback would continue.
    let messages = |padding: usize| {
        vec![
            AssistantToolMessage::User(format!(
                "{}\n{ASSISTANT_INVESTIGATION_GUIDE}",
                "x".repeat(padding)
            )),
            AssistantToolMessage::Assistant(proposal.clone()),
            AssistantToolMessage::User("r".repeat(2_048)),
        ]
    };
    let mut low = 0;
    let mut high = ASSISTANT_INVESTIGATION_PROMPT_BYTES;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = messages(middle);
        if assistant_check_conversation_budget(&candidate, &initial_tools).is_ok()
            && assistant_check_conversation_budget(&candidate, &draft_tools).is_ok()
        {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    assert!(low > ASSISTANT_INVESTIGATION_PROMPT_BYTES / 2);
    let mut full = messages(low);
    full.pop();
    full.push(AssistantToolMessage::User(guidance));
    assert!(
        assistant_check_conversation_budget(&full, &draft_tools).is_err(),
        "complete catalog must exceed the remaining budget"
    );

    let mut investigation = context(&request, None);
    investigation.prompt = "x".repeat(low);
    let slot = StdMutex::new(None);
    let mut calls = 0;
    let error = Box::pin(run_assistant_tool_investigation_in_session(
        investigation,
        |_, _| {
            calls += 1;
            assert_eq!(calls, 1, "never continue with a missing source catalog");
            ready(Ok(proposal.clone()))
        },
        |_| ready(Err("unexpected read".into())),
        &|_| panic!("source-catalog overflow must not produce a proposal"),
        None,
        Some(&slot),
    ))
    .await
    .unwrap_err();
    assert!(error.contains("context budget"), "{error}");
    assert_eq!(calls, 1);
    let saved = slot
        .lock()
        .unwrap()
        .take()
        .expect("active draft retained for recovery");
    assert!(saved.is_active() && !saved.is_ready());
    assert_eq!(saved.source_catalog(), catalog);
}
