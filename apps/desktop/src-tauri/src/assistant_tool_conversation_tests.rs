use super::*;

#[test]
fn ollama_missing_ids_remain_unique_through_repeated_session_compaction() {
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let lease = store
        .begin(
            None,
            crate::assistant_sessions::AssistantSessionBinding {
                provider: "ollama".into(),
                model: "fixture".into(),
                base_url: "http://127.0.0.1:11434".into(),
                storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
            },
            true,
        )
        .unwrap();
    let session = lease.session();
    session
        .pin_context("Keep the existing Mods and investigate only.")
        .unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Keep the existing Mods and investigate only.".into(),
        )])
        .unwrap();
    let raw = json!({"role":"assistant","content":"", "thinking":"opaque native state", "tool_calls":[
        {"function":{"index":0,"name":"read_settings","arguments":{"keys":[]}}}
    ]});
    let payload =
        serde_json::to_vec(&json!({"done":true,"done_reason":"stop","message":raw})).unwrap();
    let mut all_ids = HashSet::new();
    for _ in 0..80 {
        let mut messages = session.messages().unwrap();
        let reply = ProviderProtocol::Ollama
            .decode_tool_response(&payload)
            .unwrap();
        validate_new_calls(&reply.calls, &validate_tool_history(&messages).unwrap()).unwrap();
        assert_eq!(reply.raw_message, raw);
        let id = reply.calls[0].id.clone();
        assert!(all_ids.insert(id.clone()));
        messages.push(AssistantToolMessage::Assistant(reply));
        messages.push(AssistantToolMessage::ToolResult {
            call_id: id,
            name: "read_settings".into(),
            content: "observed configuration".into(),
            is_error: false,
        });
        session.replace_messages(messages).unwrap();
        validate_tool_history(&session.messages().unwrap()).unwrap();
    }
    let page = session
        .read_history(crate::assistant_sessions::AssistantHistoryRequest {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page["totalMessages"], 161);
    assert!(page["archivedBefore"].as_u64().unwrap() > 1);
    assert_eq!(all_ids.len(), 80);
}
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn input(provider: &str, base_url: String) -> AssistantRunInput {
    AssistantRunInput {
        settings: AssistantProviderSettings {
            provider: provider.into(),
            model: "fixture-model".into(),
            base_url,
            api_key: "fixture-key".into(),
        },
        prompt_label: "test".into(),
        prompt: "Inspect only; do not start".into(),
        context: String::new(),
    }
}

fn tools() -> Vec<AssistantToolDefinition> {
    vec![AssistantToolDefinition {
        name: "read_settings".into(),
        description: "Read settings".into(),
        parameters: json!({"type":"object","properties":{}}),
    }]
}

fn reply(id: &str) -> AssistantToolReply {
    AssistantToolReply {
        content: String::new(),
        calls: vec![AssistantToolCall {
            id: id.into(),
            name: "read_settings".into(),
            arguments: json!({}),
        }],
        raw_message: Value::Null,
    }
}

fn result(id: &str) -> AssistantToolMessage {
    AssistantToolMessage::ToolResult {
        call_id: id.into(),
        name: "read_settings".into(),
        content: "{\"ok\":false,\"error\":\"invalid arguments\"}".into(),
        is_error: true,
    }
}

#[test]
fn tool_conversation_requires_unique_paired_call_results_before_continuing() {
    for messages in [
        vec![],
        vec![result("orphan")],
        vec![AssistantToolMessage::Assistant(reply("a"))],
        vec![AssistantToolMessage::Assistant(reply("a")), result("other")],
        vec![
            AssistantToolMessage::Assistant(reply("a")),
            result("a"),
            result("a"),
        ],
        vec![
            AssistantToolMessage::Assistant(reply("a")),
            result("a"),
            AssistantToolMessage::Assistant(reply("a")),
            result("a"),
        ],
        vec![
            AssistantToolMessage::Assistant(reply("a")),
            AssistantToolMessage::User("new question before result".into()),
        ],
    ] {
        assert!(validate_tool_history(&messages).is_err());
    }
    let seen = validate_tool_history(&[
        AssistantToolMessage::User("request".into()),
        AssistantToolMessage::Assistant(reply("a")),
        result("a"),
    ])
    .unwrap();
    assert!(validate_new_calls(&reply("a").calls, &seen).is_err());
    assert!(validate_new_calls(&reply("b").calls, &seen).is_ok());
    let mut batch = reply("a");
    batch.calls.extend(reply("b").calls);
    assert!(
        validate_tool_history(&[
            AssistantToolMessage::Assistant(batch),
            result("b"),
            result("a")
        ])
        .is_err()
    );
}

struct Request {
    headers: String,
    body: Value,
}

async fn fixture(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/TenantA/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let (end, headers, length) = loop {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0 && bytes.len() + count <= 512 * 1024);
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.split_once(':')
                                    .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                    .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        break (end + 4, headers, length);
                    }
                };
                while bytes.len() < end + length {
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0 && bytes.len() + count <= 512 * 1024);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                requests.push(Request {
                    headers,
                    body: serde_json::from_slice(&bytes[end..end + length]).unwrap(),
                });
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            requests
        })
        .await
        .expect("bounded loopback provider fixture")
    });
    (url, task)
}

fn response(body: Value) -> String {
    let body = body.to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn tool_conversation_loopback_replays_real_provider_roles_and_errors() {
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Ollama,
        ProviderProtocol::AnthropicCompatible,
    ] {
        let (first, second) = match protocol {
            ProviderProtocol::OpenAiCompatible => (
                json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"read_one","type":"function","function":{"name":"read_settings","arguments":"not-json"}}]}}]}),
                json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"No changes made."}}]}),
            ),
            ProviderProtocol::Ollama => (
                json!({"done":true,"done_reason":"stop","message":{"role":"assistant","thinking":"retained state","content":"","tool_calls":[{"function":{"name":"read_settings","arguments":"not-json"}}]}}),
                json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":"No changes made."}}),
            ),
            ProviderProtocol::AnthropicCompatible => (
                json!({"role":"assistant","stop_reason":"tool_use","content":[{"type":"thinking","thinking":"retained state","signature":"fixture-signature"},{"type":"tool_use","id":"read_one","name":"read_settings","input":"not-json"}]}),
                json!({"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"No changes made."}]}),
            ),
        };
        let (url, server) = fixture(vec![response(first), response(second)]).await;
        let input = input(protocol.as_str(), url);
        let mut messages = vec![AssistantToolMessage::User(
            "Inspect only; do not start".into(),
        )];
        let answer = run_assistant_tool_turn(&input, "Trusted tool contract", &messages, &tools())
            .await
            .unwrap();
        assert_eq!(answer.calls[0].arguments, json!("not-json"));
        let id = answer.calls[0].id.clone();
        let raw = answer.raw_message.clone();
        messages.push(AssistantToolMessage::Assistant(answer));
        messages.push(result(&id));
        let answer = run_assistant_tool_turn(&input, "Trusted tool contract", &messages, &tools())
            .await
            .unwrap();
        assert!(answer.calls.is_empty());
        assert_eq!(answer.content, "No changes made.");
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 2);
        for request in &requests {
            let system = if protocol == ProviderProtocol::AnthropicCompatible {
                &request.body["system"]
            } else {
                &request.body["messages"][0]["content"]
            };
            assert_eq!(
                system,
                &json!(persona::system_prompt("Trusted tool contract"))
            );
        }
        let expected_path = if protocol == ProviderProtocol::Ollama {
            "/TenantA/api/chat"
        } else if protocol == ProviderProtocol::AnthropicCompatible {
            "/TenantA/v1/messages"
        } else {
            "/TenantA/v1/chat/completions"
        };
        assert!(
            requests[0]
                .headers
                .starts_with(&format!("POST {expected_path} HTTP/1.1"))
        );
        let offset = usize::from(protocol != ProviderProtocol::AnthropicCompatible);
        assert_eq!(
            requests[1].body["messages"][offset]["content"],
            "Inspect only; do not start"
        );
        assert_eq!(requests[1].body["messages"][offset + 1], raw);
        let feedback = &requests[1].body["messages"][offset + 2];
        if protocol == ProviderProtocol::AnthropicCompatible {
            assert_eq!(feedback["content"][0]["is_error"], true);
            assert_eq!(feedback["content"][0]["tool_use_id"], id);
            assert!(
                requests[0]
                    .headers
                    .to_lowercase()
                    .contains("anthropic-version: 2023-06-01")
            );
        } else {
            assert_eq!(feedback["role"], "tool");
            assert!(
                feedback["content"]
                    .as_str()
                    .unwrap()
                    .contains("invalid arguments")
            );
            if protocol == ProviderProtocol::OpenAiCompatible {
                assert_eq!(feedback["tool_call_id"], id);
            } else {
                assert_eq!(feedback["tool_name"], "read_settings");
            }
        }
    }
}

#[tokio::test]
async fn assistant_focused_generation_keeps_shared_voice_and_exact_artifact_instructions() {
    let task = "Generate exactly one in-game server broadcast line. Return only that line.";
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::AnthropicCompatible,
        ProviderProtocol::Ollama,
    ] {
        let body = if protocol == ProviderProtocol::AnthropicCompatible {
            json!({"stop_reason":"end_turn","content":[{"type":"text","text":"Server restarts in ten minutes."}]})
        } else {
            json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Server restarts in ten minutes."}}]})
        };
        let (url, server) = fixture(vec![response(body)]).await;
        let output = run_assistant_with_system_prompt(&input(protocol.as_str(), url), task)
            .await
            .unwrap();
        assert_eq!(output.content, "Server restarts in ten minutes.");
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        let system = if protocol == ProviderProtocol::AnthropicCompatible {
            &requests[0].body["system"]
        } else {
            &requests[0].body["messages"][0]["content"]
        };
        assert_eq!(system, &json!(persona::system_prompt(task)));
    }
}

#[tokio::test]
async fn tool_conversation_keeps_response_byte_limits_and_http_error_redaction() {
    let responses = [
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            ASSISTANT_MAX_RESPONSE_BYTES + 1
        ),
        {
            let body = json!({"api_key":"fixture-client-secret"}).to_string();
            format!(
                "HTTP/1.1 429 Too Many Requests\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        },
    ];
    for (index, response) in responses.into_iter().enumerate() {
        let (url, server) = fixture(vec![response]).await;
        let error = run_assistant_tool_turn(
            &input("openai-compatible", url),
            "System",
            &[AssistantToolMessage::User("request".into())],
            &tools(),
        )
        .await
        .unwrap_err();
        if index == 0 {
            assert!(error.contains("byte limit"));
        } else {
            assert!(error.contains("429"));
            assert!(!error.contains("fixture-client-secret"));
        }
        server.await.unwrap();
    }
}

#[tokio::test]
async fn tool_conversation_rejects_native_history_over_transport_limit_before_network() {
    for protocol in [
        ProviderProtocol::Ollama,
        ProviderProtocol::AnthropicCompatible,
    ] {
        let mut messages = vec![AssistantToolMessage::User("Inspect only".into())];
        for id in ["first", "second"] {
            let thinking = "x".repeat(ASSISTANT_MAX_RESPONSE_BYTES / 2);
            let payload = if protocol == ProviderProtocol::Ollama {
                json!({"done":true,"done_reason":"stop","message":{
                    "role":"assistant","content":"","thinking":thinking,
                    "tool_calls":[{"id":id,"function":{"name":"read_settings","arguments":{}}}]
                }})
            } else {
                json!({"role":"assistant","stop_reason":"tool_use","content":[
                    {"type":"thinking","thinking":thinking,"signature":"fixture-signature"},
                    {"type":"tool_use","id":id,"name":"read_settings","input":{}}
                ]})
            };
            let encoded = serde_json::to_vec(&payload).unwrap();
            assert!(encoded.len() < ASSISTANT_MAX_RESPONSE_BYTES);
            let answer = protocol.decode_tool_response(&encoded).unwrap();
            assert!(
                serde_json::to_vec(&(&answer.content, &answer.calls))
                    .unwrap()
                    .len()
                    < 1024
            );
            messages.push(AssistantToolMessage::Assistant(answer));
            messages.push(result(id));
        }
        validate_tool_history(&messages).unwrap();
        let body = protocol
            .tool_request_body("fixture-model", "System", &messages, &tools())
            .unwrap();
        assert!(serde_json::to_vec(&body).unwrap().len() > ASSISTANT_MAX_RESPONSE_BYTES);
        let offset = usize::from(protocol != ProviderProtocol::AnthropicCompatible);
        for index in [1, 3] {
            let AssistantToolMessage::Assistant(answer) = &messages[index] else {
                panic!("fixture must preserve paired assistant messages");
            };
            assert_eq!(body["messages"][offset + index], answer.raw_message);
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let error = run_assistant_tool_turn(
            &input(protocol.as_str(), url),
            "System",
            &messages,
            &tools(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error,
            "assistant tool request exceeded the transport byte limit"
        );
    }
}

#[tokio::test]
async fn tool_conversation_mock_maps_fixture_plans_to_calls_only_in_tests() {
    let mut input = input("ollama", "mock://assistant".into());
    input.prompt =
        "User request:\nmock-action:customizeConfig\nmock-settings-patch:{\"max_players\":4}"
            .into();
    let answer = run_assistant_tool_turn(
        &input,
        "System",
        &[AssistantToolMessage::User(input.prompt.clone())],
        &tools(),
    )
    .await
    .unwrap();
    assert_eq!(answer.calls[0].name, "propose_operation");
    assert_eq!(
        answer.calls[0].arguments["settingsPatch"],
        json!({"max_players":4})
    );
    assert!(answer.content.is_empty());
}

#[tokio::test]
async fn tool_conversation_mock_drafts_explicit_fixture_requirements_before_proposing() {
    for with_requirement in [false, true] {
        let requirements = json!({"settings":[{"key":"max_players","expected":4,"description":"Four players","sourceText":"Use four players"}],"ports":[],"forbiddenActions":[],"unverified":[]});
        let mut input = input("ollama", "mock://assistant".into());
        input.prompt = String::from(
            "User request:\nUse four players\nmock-action:customizeConfig\nmock-settings-patch:{\"max_players\":4}",
        );
        if with_requirement {
            input
                .prompt
                .push_str(&format!("\nmock-task-requirements:{requirements}"));
        }
        let catalog =
            json!({"sources":[{"id":"request_2","readable":true,"text":"Use four players"}]});
        let mut messages = vec![AssistantToolMessage::User(format!(
            "{}\nOriginal request references:\n{catalog}",
            input.prompt
        ))];
        let draft_tools = vec![
            AssistantToolDefinition {
                name: "record_task_requirements".into(),
                description: "Record".into(),
                parameters: json!({}),
            },
            AssistantToolDefinition {
                name: "finish_task_requirements".into(),
                description: "Finish".into(),
                parameters: json!({}),
            },
        ];
        let record = run_assistant_tool_turn(&input, "System", &messages, &draft_tools)
            .await
            .unwrap();
        assert_eq!(record.calls[0].name, "record_task_requirements");
        if with_requirement {
            assert_eq!(
                record.calls[0].arguments["settings"],
                json!([{"sourceId":"request_2","key":"max_players","expected":4}])
            );
        } else {
            assert_eq!(
                record.calls[0].arguments,
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]})
            );
        }
        let id = record.calls[0].id.clone();
        messages.push(AssistantToolMessage::Assistant(record));
        messages.push(AssistantToolMessage::ToolResult {
            call_id: id,
            name: "record_task_requirements".into(),
            content: json!({"ok":true,"data":{"errors":[]}}).to_string(),
            is_error: false,
        });
        let finish = run_assistant_tool_turn(&input, "System", &messages, &draft_tools)
            .await
            .unwrap();
        assert_eq!(finish.calls[0].name, "finish_task_requirements");
        assert_eq!(finish.calls[0].arguments, json!({}));
        let id = finish.calls[0].id.clone();
        messages.push(AssistantToolMessage::Assistant(finish));
        messages.push(AssistantToolMessage::ToolResult {
            call_id: id,
            name: "finish_task_requirements".into(),
            content: json!({"ok":true,"data":{"ready":true}}).to_string(),
            is_error: false,
        });
        let proposal = run_assistant_tool_turn(&input, "System", &messages, &tools())
            .await
            .unwrap();
        assert_eq!(proposal.calls[0].name, "propose_operation");
        assert!(
            proposal.calls[0]
                .arguments
                .get("taskRequirements")
                .is_none()
        );
    }
}

#[test]
fn tool_conversation_mock_does_not_guess_missing_or_ambiguous_source_ids() {
    let requirements = json!({"settings":[{"key":"max_players","expected":4,"sourceText":"four players"}],"ports":[],"forbiddenActions":[],"unverified":[]});
    for sources in [
        json!([]),
        json!([{"id":"request_1","text":"four players","readable":false}]),
        json!([{"id":"request_1","text":"Use four players now","readable":true},{"id":"request_2","text":"Use four players later","readable":true}]),
    ] {
        let prompt = format!(
            "Original request references:\n{}",
            json!({"sources":sources})
        );
        assert!(fixture_protocol::map_requirements(&requirements, &prompt).is_err());
    }
}
