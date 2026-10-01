use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{assert_native_tool_available, openai_tool_response};
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type ConversationTestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test(flavor = "current_thread")]
async fn assistant_request_rejects_storage_context_change_while_the_model_is_replying()
-> ConversationTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-conversation-context-change");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let _settings = isolated_smoke_app_settings(&root)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    state.app_state.write().map_err(|_| "state lock")?.modules = vec![ModuleSummary {
        id: "dontstarve".into(),
        name: "Don't Starve Together".into(),
        version: "fixture".into(),
        description: None,
        steam_app_id: None,
        install_state: InstallState::Installed,
        instance_program_count: 0,
        archived_program_count: 0,
        supported_platforms: vec!["windows".into()],
    }];
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        let (mut stream, _) = listener.accept().await?;
        read_repair_model_request(&mut stream).await?;
        {
            let _transition = state.begin_storage_context_transition()?;
            let mut current = state.app_state.write().map_err(|_| "state lock")?;
            current.settings.servers_root = root
                .join("different-instances")
                .to_string_lossy()
                .into_owned();
            // Keep the same catalog IDs, as a copied database could do.
        }
        let body = openai_tool_response(
            "resolve-1",
            "resolve_task",
            json!({
                "goal":"inspect","target":"module","instanceId":null,"moduleId":"dontstarve",
                "preserveExistingMods":true,"priorRequestIds":[],"clarification":null
            }),
        )
        .to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        stream.shutdown().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let error = assistant_request_operation_inner(
            None,
            state.clone(),
            AssistantRequestInput {
                conversation_id: None,
                settings: provider,
                prompt: "查看当前服务状态".into(),
                prior_requests: Vec::new(),
                conversation_messages: Vec::new(),
                context: None,
                selected_instance_id: None,
                selected_module_id: None,
            },
        )
        .await
        .expect_err("old catalog must not bind a new storage root");
        assert!(error.contains("storage paths changed"));
        assert!(read_background_jobs(state.clone())?.is_empty());
        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::try_join!(serve, exercise)
    })
    .await??;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_host_tool_and_follow_up_need_no_server_or_storage_lease()
-> ConversationTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-host-conversation");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let _settings = isolated_smoke_app_settings(&root)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    let _transition = state.begin_storage_context_transition()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let answer = "这台管理端电脑使用 Fixture CPU，内存为 32 GiB。显卡和系统版本尚未采集。";
    let serve = async {
        let first = respond(
            &listener,
            openai_tool_response(
                "resolve-without-target",
                "resolve_task",
                json!({
                    "goal":"inspect","target":"none","instanceId":null,"moduleId":null,
                    "preserveExistingMods":true,"priorRequestIds":[],"clarification":null
                }),
            ),
            "/v1/chat/completions",
        )
        .await?;
        assert_native_tool_available(&first, "read_host_info");
        let corrected = respond(
            &listener,
            openai_tool_response("host-1", "read_host_info", json!({})),
            "/v1/chat/completions",
        )
        .await?;
        let feedback = corrected["messages"]
            .as_array()
            .ok_or("missing correction messages")?
            .last()
            .ok_or("missing correction tool result")?;
        assert_eq!(feedback["role"], "tool");
        assert_eq!(feedback["tool_call_id"], "resolve-without-target");
        let error: Value = serde_json::from_str(
            feedback["content"]
                .as_str()
                .ok_or("missing correction body")?,
        )?;
        assert_eq!(error["ok"], false);
        assert!(
            error["error"]
                .as_str()
                .ok_or("missing target error")?
                .contains("target=none")
        );
        let second = respond(&listener, text_response(answer), "/v1/chat/completions").await?;
        let messages = second["messages"].as_array().ok_or("missing messages")?;
        let feedback = messages
            .iter()
            .find(|message| message["role"] == "tool" && message["tool_call_id"] == "host-1")
            .ok_or("missing host tool result")?;
        assert_eq!(feedback["tool_call_id"], "host-1");
        let evidence: Value =
            serde_json::from_str(feedback["content"].as_str().ok_or("missing result body")?)?;
        assert_eq!(evidence["ok"], true);
        assert_eq!(evidence["data"]["scope"], "manager_host");
        assert_eq!(evidence["data"]["cpu"]["name"], "Fixture CPU");
        assert_eq!(
            evidence["data"]["memory"]["totalBytes"],
            32_u64 * 1024 * 1024 * 1024
        );
        assert!(evidence["data"]["os"]["version"].is_null());
        assert!(!evidence.to_string().contains("PRIVATE_"));
        let third = respond(
            &listener,
            text_response("简单说，CPU 是处理任务的部件，内存是程序运行时的工作空间。"),
            "/v1/chat/completions",
        )
        .await?;
        let messages = third["messages"]
            .as_array()
            .ok_or("missing follow-up messages")?;
        assert!(
            messages
                .iter()
                .any(|message| message["role"] == "assistant" && message["content"] == answer)
        );
        let remembered = messages
            .iter()
            .find(|message| message["role"] == "tool" && message["tool_call_id"] == "host-1")
            .ok_or("backend host evidence missing from follow-up")?;
        let remembered: Value = serde_json::from_str(
            remembered["content"]
                .as_str()
                .ok_or("remembered host evidence missing")?,
        )?;
        assert_eq!(remembered["data"]["cpu"]["name"], "Fixture CPU");
        assert_eq!(
            messages.last().ok_or("missing follow-up")?["content"],
            "没懂"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        state
            .system_snapshot_cache
            .lock()
            .map_err(|_| "cache lock")?
            .store(SystemSnapshot {
                cpu_name: "Fixture CPU".into(),
                cpu_physical_cores: 8,
                cpu_logical_cores: 16,
                memory_total_bytes: 32 * 1024 * 1024 * 1024,
                memory_available_bytes: 12 * 1024 * 1024 * 1024,
                disk_label: "PRIVATE_PATH".into(),
                disk_volume_name: "PRIVATE_VOLUME".into(),
                ..SystemSnapshot::default()
            });
        let mut input = AssistantRequestInput {
            conversation_id: Some(seed_assistant_session(
                &state,
                &provider,
                "你是谁啊",
                "我是 LAN。",
            )?),
            settings: provider,
            prompt: "我们本机配置是什么".into(),
            prior_requests: Vec::new(),
            conversation_messages: Vec::new(),
            context: Some(r#"{"interfaceLanguage":"zh-CN"}"#.into()),
            selected_instance_id: None,
            selected_module_id: None,
        };
        let output = assistant_request_operation_inner(None, state.clone(), input.clone()).await?;
        assert_eq!(output.message, answer);
        assert!(output.task.is_none());
        assert!(!output.requires_confirmation);
        assert_eq!(output.conversation_id, input.conversation_id);
        input.conversation_id = output.conversation_id;
        input.prompt = "没懂".into();
        let output = assistant_request_operation_inner(None, state.clone(), input).await?;
        assert!(output.message.starts_with("简单说"));
        assert_eq!(output.action, AssistantOperationAction::None);
        assert!(output.task.is_none());
        assert!(read_background_jobs(state.clone())?.is_empty());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::try_join!(serve, exercise)
    })
    .await??;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConversationScenario {
    Capabilities,
    SmallTalk,
    TextualOperation,
    CorrectedConversation,
    UnknownTool,
    MultipleResolutions,
    OllamaCorrection,
    RepeatedMalformedMutation,
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_answers_capabilities_without_a_task_or_second_provider_call()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::Capabilities).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_answers_small_talk_without_an_operation() -> ConversationTestResult
{
    exercise_conversation(ConversationScenario::SmallTalk).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_never_executes_an_operation_encoded_as_plain_text()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::TextualOperation).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_recovers_from_missing_required_fields_with_one_text_reply()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::CorrectedConversation).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_corrects_an_unknown_native_tool_into_an_identity_reply()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::UnknownTool).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_rejects_every_call_in_a_batch_before_answering_identity()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::MultipleResolutions).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_ollama_corrects_missing_fields_with_generated_call_identity()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::OllamaCorrection).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_conversation_rejects_a_twice_malformed_mutation_without_writing()
-> ConversationTestResult {
    exercise_conversation(ConversationScenario::RepeatedMalformedMutation).await
}

fn text_response(content: &str) -> Value {
    json!({"choices":[{"finish_reason":"stop","message":{
        "role":"assistant","content":content
    }}]})
}

async fn respond(
    listener: &TcpListener,
    response: Value,
    expected_path: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let (mut stream, _) = listener.accept().await?;
    let mut request_line = Vec::new();
    while !request_line.ends_with(b"\r\n") {
        assert!(
            request_line.len() < 1024,
            "fixture request line exceeded its limit"
        );
        request_line.push(stream.read_u8().await?);
    }
    assert_eq!(
        std::str::from_utf8(&request_line)?,
        format!("POST {expected_path} HTTP/1.1\r\n")
    );
    // The shared reader only needs the remaining headers and bounded JSON body.
    let request = read_repair_model_request(&mut stream).await?;
    let body = response.to_string();
    stream.write_all(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ).as_bytes()).await?;
    stream.shutdown().await?;
    Ok(request)
}

async fn exercise_conversation(scenario: ConversationScenario) -> ConversationTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-conversation");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_module_instance(state.clone(), "dontstarve", "Conversation fixture").await?;
    let storage = bootstrap_storage()?;
    let before = read_instance_details(&storage.paths, &created.summary.id).await?;
    let native_before = fs::read(&before.config_file_path)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let ollama = scenario == ConversationScenario::OllamaCorrection;
    if ollama {
        provider.provider = String::from("ollama");
    }
    let prompt = match scenario {
        ConversationScenario::Capabilities | ConversationScenario::CorrectedConversation => {
            "你能干啥？"
        }
        ConversationScenario::SmallTalk => "你好，今天怎么样？",
        ConversationScenario::UnknownTool
        | ConversationScenario::MultipleResolutions
        | ConversationScenario::OllamaCorrection => "你是谁？",
        ConversationScenario::TextualOperation => "解释一下修改配置的工具格式，不要执行。",
        ConversationScenario::RepeatedMalformedMutation => {
            "把选中服务器的描述改成 Conversation updated。"
        }
    };
    let answer = match scenario {
        ConversationScenario::SmallTalk => String::from("你好，我可以帮你查看和管理游戏服务器。"),
        ConversationScenario::UnknownTool
        | ConversationScenario::MultipleResolutions
        | ConversationScenario::OllamaCorrection => {
            String::from("我是 LAN，可以帮助你管理游戏服务器。")
        }
        ConversationScenario::TextualOperation => json!({
            "action":"customize_config","instanceId":created.summary.id,"moduleId":"dontstarve",
            "settingsPatch":{"cluster_description":"This text must never be executed"}
        })
        .to_string(),
        _ => {
            String::from("我可以查看服务器日志、解释配置，并在你提出操作请求后帮助修改和验证结果。")
        }
    };
    let malformed = if scenario == ConversationScenario::RepeatedMalformedMutation {
        json!({"goal":"apply_change","target":"existing_instance", "instanceId":created.summary.id,
            "moduleId":"dontstarve","priorRequestIds":[]})
    } else {
        json!({"goal":"inspect","target":"none","priorRequestIds":[]})
    };
    let second_turn = matches!(
        scenario,
        ConversationScenario::CorrectedConversation
            | ConversationScenario::RepeatedMalformedMutation
            | ConversationScenario::UnknownTool
            | ConversationScenario::MultipleResolutions
            | ConversationScenario::OllamaCorrection
    );
    let first_reply = match scenario {
        ConversationScenario::UnknownTool => {
            openai_tool_response("unknown-1", "identify_assistant", json!({}))
        }
        ConversationScenario::MultipleResolutions => {
            let arguments = json!({"goal":"inspect","target":"none","preserveExistingMods":true,"priorRequestIds":[]});
            let mut batch =
                openai_tool_response("resolve-batch-1", "resolve_task", arguments.clone());
            let second = openai_tool_response("resolve-batch-2", "resolve_task", arguments);
            batch["choices"][0]["message"]["tool_calls"]
                .as_array_mut()
                .ok_or("batch calls missing")?
                .push(second["choices"][0]["message"]["tool_calls"][0].clone());
            batch
        }
        ConversationScenario::OllamaCorrection => {
            json!({"done":true,"done_reason":"stop","message":{
                "role":"assistant","thinking":"Fixture native protocol state.","content":"",
                "tool_calls":[{"function":{"name":"resolve_task","arguments":malformed.clone()}}]
            }})
        }
        _ if second_turn => {
            openai_tool_response("resolve-incomplete-1", "resolve_task", malformed.clone())
        }
        _ => text_response(&answer),
    };
    let native_reply = if ollama {
        first_reply["message"].clone()
    } else {
        first_reply["choices"][0]["message"].clone()
    };
    let second_reply = if scenario == ConversationScenario::RepeatedMalformedMutation {
        openai_tool_response("resolve-incomplete-2", "resolve_task", malformed)
    } else if ollama {
        json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":answer}})
    } else {
        text_response(&answer)
    };
    let serve = async move {
        let path = if ollama {
            "/api/chat"
        } else {
            "/v1/chat/completions"
        };
        let first = respond(&listener, first_reply, path).await?;
        assert_native_tool_available(&first, "resolve_task");
        assert!(first.to_string().contains(prompt));
        if second_turn {
            let second = respond(&listener, second_reply, path).await?;
            let messages = second["messages"]
                .as_array()
                .ok_or("correction messages missing")?;
            let replay = messages
                .iter()
                .position(|message| message["role"] == "assistant")
                .ok_or("native rejected reply missing from history")?;
            assert_eq!(messages[replay], native_reply);
            let expected_ids = match scenario {
                ConversationScenario::UnknownTool => vec!["unknown-1"],
                ConversationScenario::MultipleResolutions => {
                    vec!["resolve-batch-1", "resolve-batch-2"]
                }
                _ => vec!["resolve-incomplete-1"],
            };
            let results: Vec<_> = messages
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect();
            assert_eq!(
                results.len(),
                expected_ids.len(),
                "every rejected call needs exactly one result"
            );
            for (offset, (feedback, id)) in results.iter().zip(expected_ids).enumerate() {
                assert_eq!(
                    messages[replay + offset + 1],
                    **feedback,
                    "native tool results must immediately follow their rejected batch"
                );
                if ollama {
                    // Ollama omits call IDs on the wire. Reaching this second
                    // request proves the core paired its generated ID before
                    // serializing native results by tool_name.
                    assert!(native_reply["tool_calls"][0].get("id").is_none());
                    assert!(native_reply["tool_calls"][0]["function"]["arguments"].is_object());
                    assert_eq!(feedback["tool_name"], "resolve_task");
                    assert!(feedback.get("tool_call_id").is_none());
                } else {
                    assert_eq!(feedback["tool_call_id"], id);
                }
                let feedback: Value = serde_json::from_str(
                    feedback["content"]
                        .as_str()
                        .ok_or("tool feedback missing")?,
                )?;
                assert_eq!(feedback["ok"], false);
                let error = feedback["error"].as_str().ok_or("tool error missing")?;
                assert!(!error.is_empty());
                if !matches!(
                    scenario,
                    ConversationScenario::UnknownTool | ConversationScenario::MultipleResolutions
                ) {
                    assert!(error.contains("preserveExistingMods"));
                }
            }
        }
        // Dropping the only listener prevents an unnoticed third interpretation
        // or investigation from obtaining another model response.
        drop(listener);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let result = assistant_request_operation_inner(
            None,
            state.clone(),
            AssistantRequestInput {
                conversation_id: None,
                settings: provider,
                prompt: prompt.into(),
                prior_requests: Vec::new(),
                conversation_messages: Vec::new(),
                context: None,
                selected_instance_id: Some(created.summary.id.clone()),
                selected_module_id: Some("dontstarve".into()),
            },
        )
        .await;
        if scenario == ConversationScenario::RepeatedMalformedMutation {
            let error =
                result.expect_err("a second malformed interpretation cannot authorize changes");
            assert!(error.contains("assistant_request_interpretation_failed"));
            assert!(
                !error.contains(prompt),
                "structured protocol errors must not echo user requests"
            );
        } else {
            let output = result?;
            assert_eq!(output.action, AssistantOperationAction::None);
            if scenario == ConversationScenario::TextualOperation {
                assert_eq!(
                    serde_json::from_str::<Value>(&output.message)?,
                    serde_json::from_str::<Value>(&answer)?
                );
            } else {
                assert_eq!(output.message, answer);
            }
            assert!(output.task.is_none());
            assert!(!output.requires_confirmation);
            assert!(output.confirmation_token.is_none());
            assert!(output.plan_summary.is_none());
            assert!(output.follow_up.is_none());
            assert!(output.runtime_start.is_none());
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        tokio::try_join!(Box::pin(serve), Box::pin(exercise))
    })
    .await??;
    let after = read_instance_details(&storage.paths, &created.summary.id).await?;
    assert_eq!(after.settings_json, before.settings_json);
    assert_eq!(fs::read(&before.config_file_path)?, native_before);
    assert!(after.active_run.is_none());
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    Ok(())
}
