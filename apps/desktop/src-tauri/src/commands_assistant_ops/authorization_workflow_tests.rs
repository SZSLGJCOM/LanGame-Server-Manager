use super::repair_continuation_tests::{complete_history, request_body};
use super::*;
use std::sync::Arc;
use tauri::Manager;
use tokio::io::AsyncWriteExt;

const REPAIRED_MOTD: &str = "Reviewed continuation fixture";

fn session_events(session: &AssistantSession, name: &str) -> Vec<Value> {
    let (records, _) = complete_history(session);
    records
        .iter()
        .filter_map(|record| {
            let result = record.get("ToolResult")?;
            (result["name"] == name).then(|| {
                assert_eq!(result["is_error"], false);
                let content: Value =
                    serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
                assert_eq!(content["ok"], true);
                content["data"].clone()
            })
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_continuation_authorization_executes_two_repairs_after_one_confirmation() {
    exercise_authorized_repair(true).await;
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_continuation_authorization_keeps_file_validation_for_separate_confirmation() {
    exercise_authorized_repair(false).await;
}

async fn exercise_authorized_repair(continue_configuration: bool) {
    // Scripted native-provider integration tests verify application authorization
    // and receipts, not model intelligence or a running Minecraft service.
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let storage = bootstrap_storage().unwrap();
    let install = storage.paths.games_root.join("minecraft");
    std::fs::create_dir_all(&install).unwrap();
    let jar = b"fixture jar; never executed";
    std::fs::write(install.join("server.jar"), jar).unwrap();
    crate::commands::tests::record_fake_program_baseline(&storage.settings, "minecraft").unwrap();
    sync_modules_to_storage(state.clone()).await.unwrap();
    assert!(!install.join("jre/bin/java.exe").exists());
    let created = create_instance_record_inner(
        state.clone(),
        CreateInstanceInput {
            name: "Scoped continuation".into(),
            module_id: "minecraft".into(),
        },
    )
    .await
    .unwrap();
    let before = read_instance_details(&storage.paths, &created.summary.id)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut provider = intent_tests::intent_input("Scoped continuation fixture").settings;
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
        prompt: format!(
            "Set max_players to 24 and motd to {REPAIRED_MOTD}, then inspect the startup failure."
        ),
        context: None,
        selected_instance_id: Some(before.summary.id.clone()),
        selected_module_id: Some("minecraft".into()),
    };
    session.register_user_request(&input.prompt).unwrap();
    let mut task = AssistantTaskContract::capture(&input, Some(&before)).unwrap();
    task.session = Some(session.clone());
    task.requirements = Some(AssistantTaskRequirements {
        settings: [("max_players", json!(24)), ("motd", json!(REPAIRED_MOTD))]
            .into_iter()
            .map(|(key, expected)| AssistantSettingRequirement {
                key: key.into(),
                expected,
                description: format!("Save the requested {key}"),
                source_text: input.prompt.clone(),
            })
            .collect(),
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
        ..assistant_safe_none_plan("Apply the reviewed first configuration change".into())
    };
    let summary = "Reviewed player limit with scoped continuation";
    let (token, _) =
        store_assistant_pending_operation(&input, plan, None, summary.into(), 0, task.clone())
            .unwrap();
    {
        let mut operations = assistant_pending_operations().lock().unwrap();
        let pending = operations.get_mut(&token).unwrap();
        pending.expected_instance = Some(before.clone());
        pending.precondition = Some(AssistantOperationPrecondition::from_details(&before));
    }
    drop(lease);

    let serve = async {
        for round in 0..if continue_configuration { 3 } else { 2 } {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = request_body(&mut stream).await;
            let current = read_instance_details(&storage.paths, &before.summary.id)
                .await
                .unwrap();
            let settings: Value = serde_json::from_str(&current.settings_json).unwrap();
            assert_eq!(
                settings["max_players"], 24,
                "first save precedes follow-up planning"
            );
            assert!(current.active_run.is_none());
            if round == 2 {
                assert_eq!(
                    settings["motd"], REPAIRED_MOTD,
                    "second save precedes its verification"
                );
                assert_eq!(session_events(&session, "confirmed_operation").len(), 1);
            }
            if round == 1 {
                assert!(
                    request["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|message| {
                            message["role"] == "tool"
                                && message["content"].as_str().is_some_and(|content| {
                                    let value: Value = serde_json::from_str(content).unwrap();
                                    value["ok"] == true && content.contains("max_players")
                                })
                        }),
                    "planner must receive the real read_settings result"
                );
            }
            let (name, arguments) = match round {
                0 => ("read_settings", json!({"keys":["max_players","motd"]})),
                1 if continue_configuration => (
                    "propose_operation",
                    json!({
                        "action":"customize_config", "settingsPatch":{"motd":REPAIRED_MOTD},
                        "reason":"Save the remaining requested message after reading current settings."
                    }),
                ),
                1 => (
                    "propose_operation",
                    json!({
                        "action":"validate_server", "reason":"Propose checking installation files separately."
                    }),
                ),
                _ => (
                    "propose_operation",
                    json!({
                        "action":"none", "reason":"Both requested settings are saved; Java is missing, so runtime recovery remains unverified."
                    }),
                ),
            };
            let body = json!({"done":true,"done_reason":"stop","message":{
                "role":"assistant","content":"","tool_calls":[
                    {"function":{"name":name,"arguments":arguments}}
                ]
            }})
            .to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    };
    let confirm = async {
        // Use the same boxed confirmation boundary as the desktop command.
        let output = assistant_confirm_operation_inner(
            state.clone(),
            AssistantConfirmOperationInput {
                continue_task: true,
                settings: provider.clone(),
                conversation_id: Some(session.id().into()),
                confirmation_token: token.clone(),
                plan_summary: summary.into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(output.action, AssistantOperationAction::CustomizeConfig);
        assert_eq!(output.conversation_id.as_deref(), Some(session.id()));
        let receipt = output.task.as_ref().unwrap();
        assert_eq!(receipt.id, task.id);
        assert_ne!(
            receipt.status,
            AssistantTaskStatus::Completed,
            "saved configuration cannot prove runtime recovery"
        );
        if continue_configuration {
            assert_eq!(output.applied_settings_keys, ["motd"]);
            assert!(output.follow_up.is_none());
            assert_eq!(output.completed_operations.len(), 1);
            let first = &output.completed_operations[0];
            assert_eq!(first.action, AssistantOperationAction::CustomizeConfig);
            assert_eq!(
                first.instance_id.as_deref(),
                Some(before.summary.id.as_str())
            );
            assert_eq!(first.task.as_ref().unwrap().id, task.id);
            assert!(first.verification.is_some());
        } else {
            assert_eq!(output.applied_settings_keys, ["max_players"]);
            assert!(output.completed_operations.is_empty());
            let follow_up = output
                .follow_up
                .as_ref()
                .expect("out-of-scope proposal must remain reviewable");
            assert_eq!(follow_up.action, AssistantOperationAction::ValidateServer);
            assert!(follow_up.requires_confirmation);
            assert_eq!(follow_up.task.as_ref().unwrap().id, task.id);
            let next_token = follow_up.confirmation_token.as_ref().unwrap();
            let pending = assistant_pending_operations().lock().unwrap();
            assert_eq!(
                pending.get(next_token).unwrap().plan.action,
                AssistantOperationAction::ValidateServer
            );
        }
        let confirmations = session_events(&session, "user_confirmation");
        assert_eq!(
            confirmations.len(),
            1,
            "one user confirmation covers only the allowed chain"
        );
        assert_eq!(confirmations[0]["continueTask"], true);
        let completed = session_events(&session, "confirmed_operation");
        assert_eq!(completed.len(), if continue_configuration { 2 } else { 1 });
        for event in &completed {
            assert_eq!(event["action"], "customize_config");
            assert_eq!(event["instanceId"], before.summary.id);
            assert_eq!(event["task"]["id"], task.id);
            assert!(event["verification"].is_object());
        }
        assert!(
            !assistant_pending_operations()
                .lock()
                .unwrap()
                .contains_key(&token)
        );
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::join!(serve, confirm)
    })
    .await
    .unwrap();
    let saved = read_instance_details(&storage.paths, &before.summary.id)
        .await
        .unwrap();
    let mut expected: Value = serde_json::from_str(&before.settings_json).unwrap();
    expected["max_players"] = json!(24);
    if continue_configuration {
        expected["motd"] = json!(REPAIRED_MOTD);
    }
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json).unwrap(),
        expected
    );
    assert_eq!(saved.summary.active_process_count, 0);
    assert!(saved.active_run.is_none());
    assert_eq!(std::fs::read(install.join("server.jar")).unwrap(), jar);
    invalidate_assistant_session_previews(session.id()).unwrap();
}
