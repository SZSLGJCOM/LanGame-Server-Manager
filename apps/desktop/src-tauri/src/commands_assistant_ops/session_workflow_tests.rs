use super::intent_tests::{intent_arguments, intent_catalog_data, intent_input};
use super::*;
use crate::assistant_sessions::AssistantSessionStore;
use std::future::ready;
use tauri::Manager;

fn binding() -> AssistantSessionBinding {
    AssistantSessionBinding {
        provider: "ollama".into(),
        model: "fixture".into(),
        base_url: "http://127.0.0.1:11434".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-path-{index}")),
    }
}

fn native_call(id: &str, name: &str, arguments: Value) -> AssistantToolReply {
    AssistantToolReply {
        content: String::new(),
        calls: vec![AssistantToolCall {
            id: id.into(),
            name: name.into(),
            arguments: arguments.clone(),
        }],
        raw_message: json!({"role":"assistant","content":[
            {"type":"thinking","thinking":"synthetic protocol state","signature":format!("signed-{id}")},
            {"type":"tool_use","id":id,"name":name,"input":arguments}
        ]}),
    }
}

fn answer(content: &str) -> AssistantToolReply {
    AssistantToolReply {
        content: content.into(),
        calls: Vec::new(),
        raw_message: Value::Null,
    }
}

fn context(
    prompt: &str,
    completion: AssistantInvestigationCompletion,
) -> AssistantInvestigationContext<'_> {
    let mut tools = assistant_fixture_tools();
    tools.push(assistant_native_tool(
        "read_instance_file",
        "Read bound instance text.",
        json!({
            "file":{"type":"string"},"offset":{"type":"integer","minimum":0}
        }),
        &["file"],
    ));
    AssistantInvestigationContext {
        prompt: prompt.into(),
        initial_reads: Vec::new(),
        tools,
        draft: None,
        instance: None,
        module: None,
        completion,
    }
}

fn assert_pair(messages: &[AssistantToolMessage], original: &AssistantToolReply) -> (Value, bool) {
    let call = &original.calls[0];
    let indices: Vec<_> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| match message {
            AssistantToolMessage::Assistant(reply)
                if reply.calls.iter().any(|item| item.id == call.id) =>
            {
                assert_eq!(
                    serde_json::to_value(reply).unwrap(),
                    serde_json::to_value(original).unwrap()
                );
                Some(index)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        indices.len(),
        1,
        "each native call must be retained exactly once"
    );
    match &messages[indices[0] + 1] {
        AssistantToolMessage::ToolResult {
            call_id,
            name,
            content,
            is_error,
        } => {
            assert_eq!(call_id, &call.id);
            assert_eq!(name, &call.name);
            (serde_json::from_str(content).unwrap(), *is_error)
        }
        _ => panic!("native result must immediately follow its single-call turn"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_native_host_and_file_evidence_cross_scope_and_next_user_turn() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let input = intent_input("请读取机器信息和选中实例的插件，先解释，不要修改。");
    session.register_user_request(&input.prompt).unwrap();
    let (instances, modules) = intent_catalog_data();
    let slots = tokio::sync::Semaphore::new(1);
    let host = native_call("host-evidence", "read_host_info", json!({}));
    let resolved = native_call(
        "resolved-scope",
        "resolve_task",
        intent_arguments(
            "inspect",
            "existing_instance",
            Some("server-a"),
            Some("dontstarve"),
        ),
    );
    let facts = json!({"scope":"manager_host","cpu":{"name":"Fixture CPU"},"memory":{"totalBytes":34359738368_u64}});
    let mut model_calls = 0;
    let mut host_reads = 0;
    let resolution = resolve_assistant_task_intent_with_session_tools(
        &input,
        AssistantConversationScope {
            instances: &instances,
            modules: &modules,
            session: Some(&session),
        },
        &slots,
        Duration::from_secs(1),
        |messages, tools| {
            model_calls += 1;
            assert!(tools.iter().any(|tool| tool.name == "read_host_info"));
            ready(Ok(match model_calls {
                1 => host.clone(),
                2 => {
                    let (result, failed) = assert_pair(&messages, &host);
                    assert!(!failed);
                    assert_eq!(result["data"], facts);
                    resolved.clone()
                }
                _ => panic!("unexpected intent model round"),
            }))
        },
        || {
            host_reads += 1;
            ready(Ok(facts.clone()))
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        resolution,
        AssistantIntentResolution::Resolved { .. }
    ));
    assert_eq!((model_calls, host_reads), (2, 1));

    let file = native_call(
        "plugin-source",
        "read_instance_file",
        json!({"file":"data/plugins/example.lua","offset":0}),
    );
    let source = json!({"file":"data/plugins/example.lua","sourceSha256":"a".repeat(64),"content":"return { timeout = 45 }","editable":true});
    let mut model_calls = 0;
    let mut file_reads = 0;
    run_assistant_tool_investigation_in_session(
        context("Read the bound instance plugin and explain the evidence.", AssistantInvestigationCompletion::ReadOnlyAnswer),
        |messages, tools| {
            model_calls += 1;
            assert_eq!(assert_pair(&messages, &host).0["data"], facts);
            let (scope, failed) = assert_pair(&messages, &resolved);
            assert!(!failed);
            assert_eq!(scope["status"], "scope_resolved");
            assert_eq!(scope["executed"], false);
            assert!(tools.iter().any(|tool| tool.name == "read_instance_file"));
            ready(Ok(match model_calls {
                1 => file.clone(),
                2 => {
                    let (result, failed) = assert_pair(&messages, &file);
                    assert!(!failed);
                    assert_eq!(result["data"], source);
                    answer("已读取机器和插件；插件超时配置为 45，未做修改。")
                }
                _ => panic!("unexpected investigation model round"),
            }))
        },
        |request| {
            file_reads += 1;
            assert!(matches!(request, AssistantReadTool::ReadInstanceFile { file, offset: 0 } if file == "data/plugins/example.lua"));
            ready(Ok(source.clone()))
        }, &|_| Ok(()), Some(&session), None,
    ).await.unwrap();
    assert_eq!((model_calls, file_reads), (2, 1));
    drop(lease);

    let _lease = store.begin(Some(session.id()), binding(), true).unwrap();
    let mut follow_up = intent_input("刚才那个数值是什么意思？只解释。");
    // IPC history is not authoritative and must not replace native evidence.
    follow_up.conversation_messages = vec![AssistantConversationMessage::Assistant(
        "UNTRUSTED_CLIENT_HISTORY".into(),
    )];
    session.register_user_request(&follow_up.prompt).unwrap();
    let mut turns = 0;
    let result = resolve_assistant_task_intent_with_session_tools(
        &follow_up, AssistantConversationScope { instances: &instances, modules: &modules, session: Some(&session) }, &slots, Duration::from_secs(1),
        |messages, _| {
            turns += 1;
            assert_eq!(assert_pair(&messages, &host).0["data"], facts);
            assert_eq!(assert_pair(&messages, &file).0["data"], source);
            assert_pair(&messages, &resolved);
            assert!(matches!(messages.last(), Some(AssistantToolMessage::User(text)) if text == &follow_up.prompt));
            assert!(!serde_json::to_string(&messages).unwrap().contains("UNTRUSTED_CLIENT_HISTORY"));
            ready(Ok(answer("它是插件中读取到的 timeout 值；具体单位仍需查插件定义。")))
        }, || -> std::future::Ready<Result<Value, String>> { panic!("follow-up must use retained evidence in this fixture") },
    ).await.unwrap();
    assert!(matches!(result, AssistantIntentResolution::Reply(_)));
    assert_eq!(turns, 1);
    assert_eq!(
        session.source_user_requests().unwrap(),
        vec![input.prompt, follow_up.prompt]
    );
    assert_eq!(session.revision(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn session_reinvestigation_retains_failed_read_proposal_and_application_verification() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .register_user_request("修复插件，保留现有功能。")
        .unwrap();
    let failed_read = native_call(
        "failed-source-read",
        "read_instance_file",
        json!({"file":"data/plugins/unavailable.lua","offset":0}),
    );
    let proposal = native_call(
        "proposed-setting",
        "propose_operation",
        json!({"action":"customize_config","settingsPatch":{"max_players":12},"reason":"Apply the requested setting."}),
    );
    let mut rounds = 0;
    run_assistant_tool_investigation_in_session(
        context(
            "Prepare the next correction using the fixed task scope.",
            AssistantInvestigationCompletion::OperationProposal,
        ),
        |messages, _| {
            rounds += 1;
            ready(Ok(match rounds {
                1 => failed_read.clone(),
                2 => {
                    let (result, failed) = assert_pair(&messages, &failed_read);
                    assert!(failed);
                    assert_eq!(result["error"], "File is no longer present.");
                    proposal.clone()
                }
                _ => panic!("unexpected first investigation round"),
            }))
        },
        |_| ready(Err(String::from("File is no longer present."))),
        &|_| Ok(()),
        Some(&session),
        None,
    )
    .await
    .unwrap();
    let (pending, failed) = assert_pair(&session.messages().unwrap(), &proposal);
    assert!(!failed);
    assert_eq!(pending["data"]["status"], "awaiting_confirmation");
    assert_eq!(pending["data"]["executed"], false);
    let verification = json!({"action":"customize_config","instanceId":"server-a","status":"failed","evidence":{"readbackError":"Applied value differs from the requested value."},"canContinue":true});
    assistant_record_session_event(&session, "operation_verification", verification.clone())
        .unwrap();
    let saved = session.messages().unwrap();
    let event = saved
        .iter()
        .find_map(|message| match message {
            AssistantToolMessage::Assistant(reply)
                if reply
                    .calls
                    .iter()
                    .any(|call| call.name == "operation_verification") =>
            {
                Some(reply.clone())
            }
            _ => None,
        })
        .unwrap();
    let next_read = native_call(
        "settings-after-failure",
        "read_settings",
        json!({"keys":["max_players"]}),
    );
    let mut rounds = 0;
    let mut reads = 0;
    run_assistant_tool_investigation_in_session(
        context(
            "Continue the same repair after application verification failed.",
            AssistantInvestigationCompletion::ReadOnlyAnswer,
        ),
        |messages, _| {
            rounds += 1;
            assert!(assert_pair(&messages, &failed_read).1);
            assert_eq!(
                assert_pair(&messages, &proposal).0["data"]["executed"],
                false
            );
            let (result, transport_error) = assert_pair(&messages, &event);
            assert!(
                !transport_error,
                "a recorded failure is successful evidence transport"
            );
            assert_eq!(result["data"], verification);
            assert!(result["observedAtUnixMs"].as_u64().is_some());
            ready(Ok(match rounds {
                1 => next_read.clone(),
                2 => {
                    assert_eq!(
                        assert_pair(&messages, &next_read).0["data"]["max_players"],
                        8
                    );
                    answer("核验失败后重新读取，实际值仍为 8；没有声称修复完成。")
                }
                _ => panic!("unexpected continuation round"),
            }))
        },
        |request| {
            reads += 1;
            assert!(matches!(request, AssistantReadTool::ReadSettings { .. }));
            ready(Ok(json!({"max_players":8})))
        },
        &|_| Ok(()),
        Some(&session),
        None,
    )
    .await
    .unwrap();
    assert_eq!((rounds, reads), (2, 1));
}

#[tokio::test(flavor = "current_thread")]
async fn session_confirmation_cannot_cross_conversation_provider_revision_or_cancellation() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    for boundary in [
        "conversation",
        "provider",
        "revision",
        "cancel",
        "removed",
        "storage",
    ] {
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .unwrap();
        let state = app.state::<DesktopState>();
        let request = intent_input("Use only this preview and this conversation.");
        let binding = assistant_session_binding(&state, &request.settings).unwrap();
        let lease = state
            .assistant_sessions
            .begin(None, binding.clone(), true)
            .unwrap();
        let session = lease.session();
        let (mut task, _) = task_tests::task_fixture(true);
        std::sync::Arc::make_mut(&mut task).session = Some(session.clone());
        let operation_input = AssistantExecuteOperationInput {
            settings: request.settings.clone(),
            prompt: request.prompt,
            task: task.request.clone(),
            context: None,
            selected_instance_id: task.instance_id.clone(),
            selected_module_id: task.module_id.clone(),
        };
        // A non-mutating plan makes a guard regression safe while still testing
        // the real confirmation entry point before any storage initialization.
        let (token, _) = store_assistant_pending_operation(
            &operation_input,
            assistant_safe_none_plan("Fixture preview".into()),
            None,
            "Fixture preview".into(),
            0,
            task,
        )
        .unwrap();
        drop(lease);
        let mut confirmation = AssistantConfirmOperationInput {
            continue_task: false,
            settings: request.settings,
            conversation_id: Some(session.id().into()),
            confirmation_token: token.clone(),
            plan_summary: "Fixture preview".into(),
        };
        let expected = match boundary {
            "conversation" => {
                let other = state
                    .assistant_sessions
                    .begin(None, binding, false)
                    .unwrap();
                confirmation.conversation_id = Some(other.session().id().into());
                assert!(other.session().messages().unwrap().is_empty());
                "different conversation"
            }
            "provider" => {
                confirmation.settings.model.push_str("-different");
                "does not match"
            }
            "revision" => {
                drop(
                    state
                        .assistant_sessions
                        .begin(Some(session.id()), binding, true)
                        .unwrap(),
                );
                "earlier user request"
            }
            "cancel" => {
                assistant_cancel_turn(
                    state.clone(),
                    AssistantConversationControlInput {
                        conversation_id: session.id().into(),
                    },
                )
                .await
                .unwrap();
                "expired or was already used"
            }
            "removed" => {
                assistant_delete_conversation(
                    state.clone(),
                    AssistantConversationControlInput {
                        conversation_id: session.id().into(),
                    },
                )
                .await
                .unwrap();
                "expired or was already used"
            }
            "storage" => {
                state
                    .app_state
                    .write()
                    .unwrap()
                    .storage
                    .database_path
                    .push_str("-different");
                "provider, model or storage changed"
            }
            _ => unreachable!(),
        };
        let error =
            assistant_confirm_operation_with_verification(None, state, confirmation.clone())
                .await
                .unwrap_err();
        assert!(error.contains(expected), "{boundary}: {error}");
        assert!(
            take_assistant_pending_operation(
                &token,
                &confirmation.plan_summary,
                &confirmation.settings
            )
            .is_err(),
            "a rejected or cancelled confirmation must not remain reusable"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn session_stopped_after_work_cannot_publish_another_preview() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let (mut task, _) = task_tests::task_fixture(true);
    std::sync::Arc::make_mut(&mut task).session = Some(session.clone());
    let input = AssistantExecuteOperationInput {
        settings: intent_input("fixture").settings,
        prompt: "Continue the bound task.".into(),
        task: task.request.clone(),
        context: None,
        selected_instance_id: task.instance_id.clone(),
        selected_module_id: task.module_id.clone(),
    };
    assert!(
        store.cancel(session.id()).unwrap(),
        "the cancelled work still holds its lease"
    );
    let error = store_assistant_pending_operation(
        &input,
        assistant_safe_none_plan("No new preview after stop".into()),
        None,
        "Stopped preview".into(),
        0,
        task,
    )
    .unwrap_err();
    assert!(error.contains("cancelled"));
    assert!(
        !assistant_pending_operations()
            .lock()
            .unwrap()
            .values()
            .any(|pending| {
                pending
                    .task
                    .session
                    .as_ref()
                    .is_some_and(|owner| owner.id() == session.id())
            })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn session_confirmation_failure_records_unknown_operation_for_next_turn() {
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let request = intent_input("Install the selected plugin.");
    let binding = assistant_session_binding(&state, &request.settings).unwrap();
    let lease = state
        .assistant_sessions
        .begin(None, binding.clone(), true)
        .unwrap();
    let session = lease.session();
    let (mut task, _) = task_tests::task_fixture(true);
    let contract = std::sync::Arc::make_mut(&mut task);
    contract.session = Some(session.clone());
    contract.request.goal = AssistantTaskGoal::ApplyChange;
    let input = AssistantExecuteOperationInput {
        settings: request.settings.clone(),
        prompt: request.prompt,
        task: task.request.clone(),
        context: None,
        selected_instance_id: task.instance_id.clone(),
        selected_module_id: task.module_id.clone(),
    };
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::InstallSiteMod,
        instance_id: task.instance_id.clone(),
        module_id: task.module_id.clone(),
        ..assistant_safe_none_plan("Install plugin fixture".into())
    };
    let (token, _) =
        store_assistant_pending_operation(&input, plan, None, "Plugin preview".into(), 0, task)
            .unwrap();
    drop(lease);
    // The accepted confirmation targets a missing instance in the isolated
    // database. Its generic error path cannot establish an execution outcome.
    let error = assistant_confirm_operation_with_verification(
        None,
        state.clone(),
        AssistantConfirmOperationInput {
            continue_task: false,
            settings: request.settings,
            conversation_id: Some(session.id().into()),
            confirmation_token: token,
            plan_summary: "Plugin preview".into(),
        },
    )
    .await
    .unwrap_err();
    assert!(!error.is_empty());
    assert!(state.begin_storage_context_transition().is_ok());
    let messages = session.messages().unwrap();
    let event = messages
        .iter()
        .find_map(|message| match message {
            AssistantToolMessage::Assistant(reply)
                if reply
                    .calls
                    .iter()
                    .any(|call| call.name == "operation_error") =>
            {
                Some(reply.clone())
            }
            _ => None,
        })
        .expect("confirmation errors must remain native conversation evidence");
    let (result, failed_transport) = assert_pair(&messages, &event);
    assert!(!failed_transport);
    assert_eq!(result["data"]["attempted"]["action"], "install_site_mod");
    assert_eq!(result["data"]["attempted"]["instanceId"], "task-instance");
    assert_eq!(result["data"]["executionStatus"], "unknown");
    assert!(result["data"].get("executed").is_none());
    assert!(!result["data"]["error"].as_str().unwrap().is_empty());
    let _lease = state
        .assistant_sessions
        .begin(Some(session.id()), binding, true)
        .unwrap();
    let follow_up = intent_input("刚才成功了吗？");
    session.register_user_request(&follow_up.prompt).unwrap();
    let slots = tokio::sync::Semaphore::new(1);
    resolve_assistant_task_intent_with_session_tools(
        &follow_up, AssistantConversationScope { instances: &[], modules: &[], session: Some(&session) }, &slots, Duration::from_secs(1),
        |messages, _| {
            assert_eq!(assert_pair(&messages, &event).0, result);
            assert!(matches!(messages.last(), Some(AssistantToolMessage::User(text)) if text == &follow_up.prompt));
            ready(Ok(answer("没有完整执行结果，不能确认成功；需要先检查当前状态。")))
        }, || -> std::future::Ready<Result<Value, String>> { panic!("no host read was requested") },
    ).await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn session_cancel_drops_pending_read_but_preserves_completed_operation_receipt() {
    struct MarkDropped<'a>(&'a std::cell::Cell<bool>);
    impl Drop for MarkDropped<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let dropped = std::cell::Cell::new(false);
    let (started, waiting) = tokio::sync::oneshot::channel();
    let work = async {
        let _guard = MarkDropped(&dropped);
        started.send(()).unwrap();
        std::future::pending::<Result<Value, String>>().await
    };
    let cancel = async {
        waiting.await.unwrap();
        assert!(store.cancel(session.id()).unwrap());
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(assistant_cancellable_read(Some(&session), work), cancel)
    })
    .await
    .expect("cancel must release a pending read without waiting for its provider");
    assert!(result.unwrap_err().contains("stopped"));
    assert!(dropped.get(), "the cancelled read still owns its resources");
    assert!(
        store.begin(Some(session.id()), binding(), true).is_err(),
        "cancelling a read must not release the enclosing confirmed-operation lease"
    );
    let receipt = json!({"action":"customize_config","instanceId":"server-a","executionStatus":"completed","settingsReadback":{"max_players":12}});
    assistant_record_session_event(&session, "confirmed_operation", receipt.clone()).unwrap();
    assert!(
        session
            .append_messages(vec![AssistantToolMessage::User(
                "Unwanted next step".into()
            )])
            .is_err()
    );
    drop(lease);

    let _next_turn = store.begin(Some(session.id()), binding(), true).unwrap();
    let messages = session.messages().unwrap();
    let event = messages
        .iter()
        .find_map(|message| match message {
            AssistantToolMessage::Assistant(reply)
                if reply
                    .calls
                    .iter()
                    .any(|call| call.name == "confirmed_operation") =>
            {
                Some(reply.clone())
            }
            _ => None,
        })
        .expect("the completed write receipt must survive the stopped investigation");
    let (result, failed_transport) = assert_pair(&messages, &event);
    assert!(!failed_transport);
    assert_eq!(result["data"], receipt);
}

#[tokio::test(flavor = "current_thread")]
async fn session_already_cancelled_never_polls_new_read_work() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    assert!(store.cancel(session.id()).unwrap());
    let polled = std::cell::Cell::new(false);
    let result = assistant_cancellable_read(Some(&session), async {
        polled.set(true);
        Ok::<_, String>(json!({"unexpected":"read was started"}))
    })
    .await;
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(
        !polled.get(),
        "pre-cancelled work must not even start an I/O request"
    );
}
