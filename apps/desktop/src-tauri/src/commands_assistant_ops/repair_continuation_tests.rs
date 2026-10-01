use super::*;
use crate::assistant_sessions::AssistantHistoryRequest;
use std::sync::Arc;
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) async fn request_body(stream: &mut tokio::net::TcpStream) -> Value {
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0, "provider fixture closed before HTTP headers");
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() <= 16 * 1024);
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap();
    assert!(length <= 256 * 1024);
    while bytes.len() < header_end + length {
        let read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(read, 0, "provider fixture closed before the request body");
        bytes.extend_from_slice(&buffer[..read]);
        assert!(bytes.len() <= header_end + 256 * 1024);
    }
    serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
}

pub(super) fn complete_history(session: &AssistantSession) -> (Vec<Value>, usize) {
    let mut records = Vec::new();
    let mut partial = String::new();
    let (mut offset, mut message_offset_bytes) = (0, 0);
    for _ in 0..128 {
        let page = session
            .read_history(AssistantHistoryRequest {
                offset,
                message_offset_bytes,
                limit: 8,
                ..Default::default()
            })
            .unwrap();
        for entry in page["messages"].as_array().unwrap() {
            assert_eq!(entry["index"].as_u64().unwrap() as usize, records.len());
            if let Some(message) = entry.get("message") {
                assert!(partial.is_empty());
                records.push(message.clone());
            } else {
                assert_eq!(
                    entry["messageOffsetBytes"].as_u64().unwrap() as usize,
                    partial.len()
                );
                partial.push_str(entry["excerpt"].as_str().unwrap());
                if partial.len() == entry["recordBytes"].as_u64().unwrap() as usize {
                    records.push(serde_json::from_str(&partial).unwrap());
                    partial.clear();
                }
            }
        }
        let next = (
            page["nextOffset"].as_u64().unwrap() as usize,
            page["nextMessageOffsetBytes"].as_u64().unwrap() as usize,
        );
        if page["hasMore"] == false {
            assert!(partial.is_empty());
            assert_eq!(
                records.len(),
                page["totalRecords"].as_u64().unwrap() as usize
            );
            return (records, page["archivedBefore"].as_u64().unwrap() as usize);
        }
        assert!(
            next > (offset, message_offset_bytes),
            "history pagination did not advance"
        );
        (offset, message_offset_bytes) = next;
    }
    panic!("fixture history exceeded its bounded pagination allowance");
}

fn native_event(records: &[Value], name: &str) -> (usize, Value, Value) {
    let index = records
        .iter()
        .position(|record| {
            record["Assistant"]["calls"]
                .as_array()
                .is_some_and(|calls| calls.iter().any(|call| call["name"] == name))
        })
        .unwrap_or_else(|| panic!("full backend history lost native event {name}"));
    let calls = records[index]["Assistant"]["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    let result = &records[index + 1]["ToolResult"];
    assert_eq!(result["call_id"], calls[0]["id"]);
    assert_eq!(result["name"], name);
    assert_eq!(result["is_error"], false);
    let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
    assert_eq!(
        content["ok"], true,
        "native event must retain actual evidence"
    );
    (
        index,
        json!([records[index], records[index + 1]]),
        content["data"].clone(),
    )
}

fn assert_event_available(
    request: &Value,
    name: &str,
    index: usize,
    archived_before: usize,
    round: usize,
) {
    if request.to_string().contains(name) {
        return;
    }
    assert!(
        index + 2 <= archived_before,
        "round {round}: native event {name} is absent from both the active window and declared archive range {archived_before}"
    );
    let notice = format!("earlier messages [0, {archived_before}) are archived");
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains(&notice))
            }),
        "round {round}: archived evidence must have an explicit retrieval notice"
    );
    assert!(
        request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| { tool["function"]["name"] == "read_session_history" }),
        "round {round}: archived evidence must remain retrievable by the model"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn first_post_write_investigation_failure_resumes_without_replaying_the_write() {
    exercise_failed_follow_up(false).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_first_post_write_investigation_cannot_publish_a_checkpoint() {
    exercise_failed_follow_up(true).await;
}

async fn exercise_failed_follow_up(cancel: bool) {
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let storage = bootstrap_storage().unwrap();
    let install = storage.paths.games_root.join("minecraft");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("server.jar"), b"fixture jar; never executed").unwrap();
    crate::commands::tests::record_fake_program_baseline(&storage.settings, "minecraft").unwrap();
    sync_modules_to_storage(state.clone()).await.unwrap();
    let created = create_instance_record_inner(
        state.clone(),
        CreateInstanceInput {
            name: "Retained repair".into(),
            module_id: "minecraft".into(),
        },
    )
    .await
    .unwrap();
    let before = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&before.settings_json).unwrap()["max_players"],
        20
    );
    assert!(!install.join("jre/bin/java.exe").exists());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut provider = intent_tests::intent_input("Follow-up fixture").settings;
    provider.base_url = format!("http://{}", listener.local_addr().unwrap());
    let lease = state
        .assistant_sessions
        .begin(
            None,
            assistant_session_binding(&state, &provider).unwrap(),
            true,
        )
        .unwrap();
    let session = lease.session();
    let input = AssistantExecuteOperationInput {
        task: AssistantTaskRequest {
            goal: AssistantTaskGoal::RestoreService,
            preserve_existing_mods: true,
        },
        settings: provider.clone(),
        prompt: "Set max_players to 24 and inspect the startup failure.".into(),
        context: None,
        selected_instance_id: Some(before.summary.id.clone()),
        selected_module_id: Some("minecraft".into()),
    };
    session.register_user_request(&input.prompt).unwrap();
    let mut task = AssistantTaskContract::capture(&input, Some(&before)).unwrap();
    task.session = Some(session.clone());
    task.requirements = Some(AssistantTaskRequirements {
        settings: vec![AssistantSettingRequirement {
            key: "max_players".into(),
            expected: json!(24),
            description: "Use 24 player slots".into(),
            source_text: input.prompt.clone(),
        }],
        ports: Vec::new(),
        forbidden_actions: Vec::new(),
        unverified: Vec::new(),
    });
    let task = Arc::new(task);
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: Some(before.summary.id.clone()),
        module_id: Some("minecraft".into()),
        settings_patch: Some(json!({"max_players":24})),
        ..assistant_safe_none_plan("Apply the reviewed player limit".into())
    };
    let (token, _) = store_assistant_pending_operation(
        &input,
        plan,
        None,
        "Reviewed configuration change".into(),
        0,
        task.clone(),
    )
    .unwrap();
    {
        let mut pending = assistant_pending_operations().lock().unwrap();
        let saved = pending.get_mut(&token).unwrap();
        saved.expected_instance = Some(before.clone());
        saved.precondition = Some(AssistantOperationPrecondition::from_details(&before));
    }
    drop(lease);
    let observer = async {
        let mut original_verification = None;
        for index in 0..if cancel { 1 } else { 2 } {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = request_body(&mut stream).await;
            let (records, archived_before) = complete_history(&session);
            let (event_index, pair, evidence) = native_event(&records, "operation_verification");
            if let Some(original) = &original_verification {
                assert_eq!(
                    &pair, original,
                    "resume changed the original native verification pair"
                );
            } else {
                original_verification = Some(pair);
            }
            assert_event_available(
                &request,
                "operation_verification",
                event_index,
                archived_before,
                index,
            );
            let packet = json!({"instanceId":before.summary.id,"moduleId":"minecraft",
                "previousAction":"customize_config","step":1,"verification":evidence["verification"]});
            let current_evidence = assistant_repair_evidence_text(&packet).unwrap();
            assert!(
                request["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| {
                        message["role"] == "user"
                            && message["content"].as_str().is_some_and(|text| {
                                text.contains("Previous operation verification")
                                    && text.contains(&current_evidence)
                            })
                    }),
                "round {index}: current follow-up lost its bound verification packet"
            );
            let saved = read_instance_details(&storage.paths, &before.summary.id)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&saved.settings_json).unwrap()["max_players"],
                24
            );
            if cancel {
                assistant_cancel_turn(
                    state.clone(),
                    AssistantConversationControlInput {
                        conversation_id: session.id().into(),
                    },
                )
                .await
                .unwrap();
                break;
            }
            let (status, body) = if index == 0 {
                (
                    "503 Service Unavailable",
                    json!({"error":"fixture follow-up unavailable"}),
                )
            } else {
                let (event_index, _, receipt) = native_event(&records, "confirmed_operation");
                assert_eq!(receipt["action"], "customize_config");
                assert_event_available(
                    &request,
                    "confirmed_operation",
                    event_index,
                    archived_before,
                    index,
                );
                (
                    "200 OK",
                    json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":"","tool_calls":[
                        {"function":{"name":"propose_operation","arguments":{"action":"none","reason":"The saved limit is retained; Java is missing and no further change is justified."}}}
                    ]}}),
                )
            };
            let body = body.to_string();
            stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    };
    let exercise = async {
        // Use the same boxed confirmation boundary as the desktop command.
        let output = assistant_confirm_operation_inner(
            state.clone(),
            AssistantConfirmOperationInput {
                continue_task: false,
                settings: provider.clone(),
                conversation_id: Some(session.id().into()),
                confirmation_token: token.clone(),
                plan_summary: "Reviewed configuration change".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(output.action, AssistantOperationAction::CustomizeConfig);
        assert_eq!(output.applied_settings_keys, ["max_players"]);
        assert!(!output.requires_confirmation);
        assert!(output.follow_up.is_none());
        assert!(
            !assistant_pending_operations()
                .lock()
                .unwrap()
                .contains_key(&token)
        );
        let verification = output.verification.as_ref().unwrap();
        assert_eq!(verification.status, AssistantVerificationStatus::Failed);
        assert!(!verification.can_continue);
        assert!(verification.evidence["investigationError"].is_string());
        if cancel {
            assert!(output.continuation.is_none());
            assert!(
                !assistant_continuations()
                    .lock()
                    .unwrap()
                    .contains_key(session.id())
            );
            assert!(session.check_active().is_err());
            return;
        }
        let pause = output.continuation.unwrap();
        assert_eq!(pause.reason, AssistantRunPauseReason::InvestigationFailed);
        assert_eq!(
            (pause.calls, pause.operations, pause.slices_granted),
            (1, 1, 1)
        );
        {
            let saved = assistant_continuations().lock().unwrap();
            let checkpoint = saved.get(session.id()).unwrap();
            assert!(Arc::ptr_eq(&checkpoint.task.run, &task.run));
            assert!(matches!(
                checkpoint.mode,
                AssistantOperationMode::FollowUp { step: 1, .. }
            ));
        }
        let resumed = assistant_resume_conversation_inner(
            None,
            state.clone(),
            AssistantResumeConversationInput {
                conversation_id: session.id().into(),
                settings: provider.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(resumed.action, AssistantOperationAction::None);
        assert!(!resumed.requires_confirmation);
        assert!(resumed.continuation.is_none(), "{}", resumed.message);
        task.run.pause_after_investigation_failure().unwrap();
        let after = task.run.pause_receipt().unwrap();
        assert_eq!(
            (after.calls, after.operations, after.slices_granted),
            (2, 1, 1)
        );
        assert!(
            !assistant_continuations()
                .lock()
                .unwrap()
                .contains_key(session.id())
        );
        let (records, _) = complete_history(&session);
        let (_, _, receipt) = native_event(&records, "confirmed_operation");
        assert_eq!(receipt["action"], "customize_config");
        assert!(receipt.to_string().contains("max_players"));
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(exercise, observer)
    })
    .await
    .unwrap();
    let saved = read_instance_details(&storage.paths, &before.summary.id)
        .await
        .unwrap();
    let mut expected: Value = serde_json::from_str(&before.settings_json).unwrap();
    expected["max_players"] = json!(24);
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap(),
        expected
    );
    assert!(saved.active_run.is_none());
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .unwrap()
            .is_tracked(&before.summary.id)
    );
    assert!(state.begin_storage_context_transition().is_ok());
    invalidate_assistant_session_previews(session.id()).unwrap();
}
