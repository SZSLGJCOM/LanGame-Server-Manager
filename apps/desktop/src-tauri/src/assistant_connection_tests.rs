use super::*;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

fn input(provider: &str, endpoint: String) -> AssistantConnectionCheckInput {
    AssistantConnectionCheckInput {
        request_id: Uuid::new_v4().to_string(),
        settings: AssistantProviderSettings {
            provider: provider.into(),
            model: "fixture-model".into(),
            base_url: endpoint,
            api_key: ["fixture", "private", "key"].join("-"),
        },
    }
}

async fn read_request(stream: &mut TcpStream) -> (String, Value) {
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
        assert!(bytes.len() < 64 * 1024);
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = headers
        .lines()
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
        })
        .unwrap();
    assert!(length < 64 * 1024);
    while bytes.len() < header_end + length {
        let mut buffer = [0; 1024];
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0);
        bytes.extend_from_slice(&buffer[..count]);
    }
    (
        headers,
        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap(),
    )
}

fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn native_response(provider: &str, text: &str, calls: &[Value]) -> String {
    match provider {
        "openai-compatible" => {
            let finish = if calls.is_empty() {
                "stop"
            } else {
                "tool_calls"
            };
            let mut delta = json!({"role":"assistant","content":text,"reasoning_content":"opaque-private-reasoning"});
            if !calls.is_empty() {
                delta["tool_calls"] = json!(
                    calls
                        .iter()
                        .enumerate()
                        .map(|(index, call)| json!({
                            "index":index,"id":call["id"],"type":"function","function":{
                                "name":call["name"],"arguments":call["arguments"].to_string()
                            }
                        }))
                        .collect::<Vec<_>>()
                );
            }
            frame(json!({"choices":[{"index":0,"delta":delta,"finish_reason":null}]}))
                + &frame(json!({"choices":[{"index":0,"delta":{},"finish_reason":finish}]}))
                + "data: [DONE]\n\n"
        }
        "anthropic-compatible" => {
            let mut wire =
                frame(json!({"type":"message_start","message":{"role":"assistant","content":[]}}));
            let mut index = 0;
            wire += &frame(
                json!({"type":"content_block_start","index":index,"content_block":{
                    "type":"thinking","thinking":"opaque-private-reasoning","signature":"signed-native-state"
                }}),
            );
            wire += &frame(json!({"type":"content_block_stop","index":index}));
            index += 1;
            if !text.is_empty() {
                wire += &frame(
                    json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":text}}),
                );
                wire += &frame(json!({"type":"content_block_stop","index":index}));
                index += 1;
            }
            for call in calls {
                wire += &frame(
                    json!({"type":"content_block_start","index":index,"content_block":{
                        "type":"tool_use","id":call["id"],"name":call["name"],"input":call["arguments"]
                    }}),
                );
                wire += &frame(json!({"type":"content_block_stop","index":index}));
                index += 1;
            }
            wire += &frame(
                json!({"type":"message_delta","delta":{"stop_reason":if calls.is_empty(){"end_turn"}else{"tool_use"}}}),
            );
            wire + &frame(json!({"type":"message_stop"}))
        }
        "ollama" => {
            let mut message =
                json!({"role":"assistant","content":text,"thinking":"opaque-private-reasoning"});
            if !calls.is_empty() {
                message["tool_calls"] = json!(calls.iter().enumerate().map(|(index, call)| json!({
                    "function":{"index":index,"name":call["name"],"arguments":call["arguments"]}
                })).collect::<Vec<_>>());
            }
            json!({"message":message,"done":true,"done_reason":"stop"}).to_string() + "\n"
        }
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy)]
enum ToolScenario {
    Valid,
    NoCall,
    WrongNonce,
    DuplicateId,
    Rejected,
}

async fn check_server(
    provider: &'static str,
    scenario: ToolScenario,
    consume_receipt: bool,
) -> (String, tokio::task::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let count = if matches!(scenario, ToolScenario::Valid) {
            3
        } else {
            2
        };
        let mut requests = Vec::new();
        for index in 0..count {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let (headers, request) = read_request(&mut socket).await;
            let path = match provider {
                "anthropic-compatible" => "/v1/messages",
                "ollama" => "/api/chat",
                _ => "/v1/chat/completions",
            };
            assert!(headers.starts_with(&format!("POST {path} HTTP/1.1")));
            assert_eq!(request["stream"], true);
            if index == 0 {
                assert!(request.get("tools").is_none());
                assert!(request.get("tool_choice").is_none());
            }
            let mut status = "200 OK";
            let wire = if index == 0 {
                native_response(provider, "Connection ready.", &[])
            } else if index == 1 {
                if matches!(scenario, ToolScenario::Rejected) {
                    status = "400 Bad Request";
                    json!({"error":{"message":"fixture-private-key opaque-private-reasoning"}})
                        .to_string()
                } else if matches!(scenario, ToolScenario::NoCall) {
                    native_response(provider, "I can chat, but will not call tools.", &[])
                } else {
                    let schema = if provider == "anthropic-compatible" {
                        &request["tools"][0]["input_schema"]
                    } else {
                        &request["tools"][0]["function"]["parameters"]
                    };
                    let nonce = if matches!(scenario, ToolScenario::WrongNonce) {
                        json!("forged-challenge")
                    } else {
                        schema["properties"]["nonce"]["const"].clone()
                    };
                    let call =
                        json!({"id":"check-call","name":CHECK_TOOL,"arguments":{"nonce":nonce}});
                    let calls = if matches!(scenario, ToolScenario::DuplicateId) {
                        vec![call.clone(), call]
                    } else {
                        vec![call]
                    };
                    native_response(provider, "", &calls)
                }
            } else {
                let messages = request["messages"].as_array().unwrap();
                let result = if provider == "anthropic-compatible" {
                    assert_eq!(
                        messages.last().unwrap()["content"][0]["tool_use_id"],
                        "check-call"
                    );
                    messages.last().unwrap()["content"][0]["content"]
                        .as_str()
                        .unwrap()
                } else {
                    let last = messages.last().unwrap();
                    assert_eq!(last["role"], "tool");
                    if provider == "openai-compatible" {
                        assert_eq!(last["tool_call_id"], "check-call");
                    } else {
                        assert_eq!(last["tool_name"], CHECK_TOOL);
                    }
                    last["content"].as_str().unwrap()
                };
                let result: Value = serde_json::from_str(result).unwrap();
                let content = result[if consume_receipt { "receipt" } else { "nonce" }]
                    .as_str()
                    .unwrap();
                assert_ne!(result["receipt"], result["nonce"]);
                assert!(request.to_string().contains("opaque-private-reasoning"));
                native_response(provider, content, &[])
            };
            let content_type = if provider == "ollama" {
                "application/x-ndjson"
            } else {
                "text/event-stream"
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{wire}",
                wire.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            requests.push(request);
        }
        requests
    });
    (endpoint, task)
}

#[tokio::test]
async fn all_three_native_streams_complete_isolated_chat_call_and_replay() {
    for provider in ["openai-compatible", "anthropic-compatible", "ollama"] {
        let (endpoint, server) = check_server(provider, ToolScenario::Valid, true).await;
        let input = input(provider, endpoint);
        let registry = CheckRegistry::default();
        let report = check_with_registry(&input, &registry, CHECK_TIMEOUT)
            .await
            .unwrap();
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(report.request_count, 3);
        assert_eq!(report.chat.status, AssistantConnectionStageStatus::Passed);
        assert_eq!(
            report.tool_call.status,
            AssistantConnectionStageStatus::Passed
        );
        assert_eq!(
            report.tool_replay.status,
            AssistantConnectionStageStatus::Passed
        );
        assert!(!report.cancelled);
        let encoded = serde_json::to_string(&report).unwrap();
        assert!(!encoded.contains(&input.settings.api_key));
        assert!(!encoded.contains("opaque-private-reasoning"));
        assert!(!encoded.contains("signed-native-state"));
        assert!(!encoded.contains("nonce"));
        assert!(registry.state.lock().unwrap().active.is_empty());
    }
}

#[tokio::test]
async fn a_chat_capable_provider_can_fail_tools_without_losing_chat_result() {
    for scenario in [
        ToolScenario::NoCall,
        ToolScenario::WrongNonce,
        ToolScenario::DuplicateId,
        ToolScenario::Rejected,
    ] {
        let (endpoint, server) = check_server("openai-compatible", scenario, true).await;
        let input = input("openai-compatible", endpoint);
        let report = check_with_registry(&input, &CheckRegistry::default(), CHECK_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(server.await.unwrap().len(), 2);
        assert_eq!(report.chat.status, AssistantConnectionStageStatus::Passed);
        assert_eq!(
            report.tool_call.status,
            AssistantConnectionStageStatus::Failed
        );
        assert_eq!(
            report.tool_replay.status,
            AssistantConnectionStageStatus::Skipped
        );
        assert_eq!(report.request_count, 2);
        let expected = match scenario {
            ToolScenario::NoCall => "tool_not_called",
            ToolScenario::WrongNonce => "tool_call_invalid",
            ToolScenario::DuplicateId => "invalid_response",
            ToolScenario::Rejected => "request_failed",
            ToolScenario::Valid => unreachable!(),
        };
        assert_eq!(report.tool_call.diagnostic.as_deref(), Some(expected));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains(&input.settings.api_key)
        );
    }
}

#[tokio::test]
async fn echoing_the_call_nonce_does_not_prove_tool_result_consumption() {
    let (endpoint, server) = check_server("anthropic-compatible", ToolScenario::Valid, false).await;
    let report = check_with_registry(
        &input("anthropic-compatible", endpoint),
        &CheckRegistry::default(),
        CHECK_TIMEOUT,
    )
    .await
    .unwrap();
    assert_eq!(server.await.unwrap().len(), 3);
    assert_eq!(report.chat.status, AssistantConnectionStageStatus::Passed);
    assert_eq!(
        report.tool_call.status,
        AssistantConnectionStageStatus::Passed
    );
    assert_eq!(
        report.tool_replay.status,
        AssistantConnectionStageStatus::Failed
    );
    assert_eq!(
        report.tool_replay.diagnostic.as_deref(),
        Some("tool_result_not_consumed")
    );
}

async fn stalled_server() -> (String, oneshot::Receiver<()>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sender, accepted) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), read_request(&mut socket))
            .await
            .unwrap();
        sender.send(()).unwrap();
        let mut byte = [0];
        let count = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut byte))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(count, 0, "cancel/timeout must close the in-flight request");
    });
    (endpoint, accepted, server)
}

#[tokio::test]
async fn cancellation_owns_only_the_current_check_and_releases_admission() {
    let (endpoint, accepted, server) = stalled_server().await;
    let input = input("openai-compatible", endpoint);
    let id = parse_request_id(&input.request_id).unwrap();
    let registry = CheckRegistry::default();
    let report = tokio::join!(
        check_with_registry(&input, &registry, CHECK_TIMEOUT),
        async {
            accepted.await.unwrap();
            assert!(registry.cancel(Uuid::new_v4()).unwrap());
            assert!(registry.cancel(id).unwrap());
        }
    )
    .0
    .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.chat.diagnostic.as_deref(), Some("request_cancelled"));
    assert_eq!(
        report.tool_call.status,
        AssistantConnectionStageStatus::Skipped
    );
    assert_eq!(report.request_count, 1);
    assert!(registry.state.lock().unwrap().active.is_empty());
    assert!(!*registry.begin(id).unwrap().receiver.borrow());
    server.await.unwrap();
}

#[tokio::test]
async fn dropping_a_check_future_closes_its_socket_and_releases_its_request_id() {
    let (endpoint, accepted, server) = stalled_server().await;
    let input = input("openai-compatible", endpoint);
    let id = parse_request_id(&input.request_id).unwrap();
    let registry = std::sync::Arc::new(CheckRegistry::default());
    let owned_registry = registry.clone();
    let task =
        tokio::spawn(
            async move { check_with_registry(&input, &owned_registry, CHECK_TIMEOUT).await },
        );
    accepted.await.unwrap();
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    assert!(registry.state.lock().unwrap().active.is_empty());
    assert!(!*registry.begin(id).unwrap().receiver.borrow());
    server.await.unwrap();
}

#[tokio::test]
async fn an_overall_deadline_closes_the_request_and_reports_remaining_stages_skipped() {
    let (endpoint, accepted, server) = stalled_server().await;
    let registry = CheckRegistry::default();
    let report = check_with_registry(
        &input("openai-compatible", endpoint),
        &registry,
        Duration::from_millis(150),
    )
    .await
    .unwrap();
    accepted.await.unwrap();
    assert_eq!(report.chat.diagnostic.as_deref(), Some("overall_timeout"));
    assert_eq!(
        report.tool_call.diagnostic.as_deref(),
        Some("overall_timeout")
    );
    assert_eq!(
        report.tool_replay.status,
        AssistantConnectionStageStatus::Skipped
    );
    assert!(!report.cancelled);
    assert!(registry.state.lock().unwrap().active.is_empty());
    server.await.unwrap();
}

#[test]
fn request_identity_and_concurrency_admission_are_bounded_and_raii_owned() {
    for invalid in [
        "",
        "not-a-uuid",
        "00000000-0000-0000-0000-000000000000",
        "00000000000040008000000000000001",
        " 00000000-0000-4000-8000-000000000001",
    ] {
        assert!(parse_request_id(invalid).is_err());
    }
    let registry = CheckRegistry::default();
    let first_id = Uuid::new_v4();
    let first = registry.begin(first_id).unwrap();
    assert!(registry.begin(first_id).is_err());
    let second = registry.begin(Uuid::new_v4()).unwrap();
    assert!(registry.begin(Uuid::new_v4()).is_err());
    assert!(registry.cancel(first_id).unwrap());
    assert!(*first.receiver.borrow());
    assert!(!*second.receiver.borrow());
    drop(first);
    let replacement = registry.begin(first_id).unwrap();
    assert!(!*replacement.receiver.borrow());
    drop(second);
    drop(replacement);
    assert!(registry.state.lock().unwrap().active.is_empty());
}

#[tokio::test]
async fn cancellation_before_start_skips_credentials_and_all_model_requests() {
    let mut input = input("openai-compatible", "http://127.0.0.1:9/v1".into());
    input.settings.api_key.clear();
    let registry = CheckRegistry::default();
    let id = parse_request_id(&input.request_id).unwrap();
    assert!(registry.cancel(id).unwrap());
    let report = check_with_registry(&input, &registry, CHECK_TIMEOUT)
        .await
        .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.request_count, 0);
    assert_eq!(report.chat.diagnostic.as_deref(), Some("request_cancelled"));
    assert_eq!(
        report.tool_call.status,
        AssistantConnectionStageStatus::Skipped
    );
    assert_eq!(
        report.tool_replay.status,
        AssistantConnectionStageStatus::Skipped
    );
    let state = registry.state.lock().unwrap();
    assert!(state.active.is_empty());
    assert!(state.pending_cancellations.is_empty());
}

#[test]
fn pending_cancellations_are_uuid_scoped_capacity_bounded_and_expire_without_sleep() {
    let registry = CheckRegistry::default();
    let ids = (0..MAX_PENDING_CANCELLATIONS)
        .map(|_| Uuid::new_v4())
        .collect::<Vec<_>>();
    for &id in &ids {
        assert!(registry.cancel(id).unwrap());
    }
    assert!(registry.cancel(ids[0]).unwrap());
    assert_eq!(
        registry.state.lock().unwrap().pending_cancellations.len(),
        MAX_PENDING_CANCELLATIONS
    );
    let extra = Uuid::new_v4();
    assert!(registry.cancel(extra).is_err());
    let consumed = registry.begin(ids[0]).unwrap();
    assert!(*consumed.receiver.borrow());
    assert!(registry.cancel(extra).unwrap());
    let other = registry.begin(Uuid::new_v4()).unwrap();
    assert!(!*other.receiver.borrow());
    assert!(
        registry.cancel(ids[0]).unwrap(),
        "active cancellation ignores pending capacity"
    );
    assert!(!*other.receiver.borrow());
    drop(consumed);
    drop(other);
    let expiry = Instant::now() - Duration::from_secs(1);
    registry
        .state
        .lock()
        .unwrap()
        .pending_cancellations
        .values_mut()
        .for_each(|deadline| *deadline = expiry);
    let expired = registry.begin(ids[1]).unwrap();
    assert!(!*expired.receiver.borrow());
    assert!(
        registry
            .state
            .lock()
            .unwrap()
            .pending_cancellations
            .is_empty()
    );
}

#[test]
fn provider_diagnostics_never_echo_errors_or_private_payloads() {
    let secret = ["fixture", "private", "key"].join("-");
    for (prefix, expected) in [
        (
            "assistant provider returned 401 Unauthorized:",
            "authentication_rejected",
        ),
        (
            "assistant provider returned 403 Forbidden:",
            "authentication_rejected",
        ),
        (
            "assistant provider returned 404 Not Found:",
            "endpoint_not_found",
        ),
        (
            "assistant provider returned 429 Too Many Requests:",
            "rate_limited",
        ),
        (
            "assistant provider returned 503 Service Unavailable:",
            "provider_unavailable",
        ),
        ("assistant request failed: timed out:", "request_timeout"),
        (
            "failed to read assistant credential:",
            "credential_unavailable",
        ),
        (
            "failed to decode assistant tool response:",
            "invalid_response",
        ),
    ] {
        assert_eq!(safe_diagnostic(&format!("{prefix} {secret}")), expected);
    }
}

#[test]
fn connection_input_debug_never_formats_provider_credentials() {
    let input = input("openai-compatible", "https://example.invalid/v1".into());
    let debug = format!("{input:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains(&input.settings.api_key));
    assert!(!debug.contains(&input.settings.base_url));
}

#[tokio::test]
#[ignore = "requires LANGAME_ASSISTANT_LIVE=1 and the saved DeepSeek credential"]
async fn saved_deepseek_connection_probe() {
    assert_eq!(
        std::env::var("LANGAME_ASSISTANT_LIVE").ok().as_deref(),
        Some("1"),
        "Live provider access requires LANGAME_ASSISTANT_LIVE=1"
    );
    let settings = AssistantProviderSettings {
        provider: "openai-compatible".into(),
        base_url: "https://api.deepseek.com".into(),
        model: "deepseek-flash".into(),
        api_key: String::new(),
    };
    let descriptor = crate::assistant::AssistantSecretDescriptor {
        provider: settings.provider.clone(),
        base_url: settings.base_url.clone(),
    };
    let stored = crate::assistant::read_secret_status(&descriptor)
        .map(|status| status.stored)
        .unwrap_or(false);
    assert!(stored, "The saved DeepSeek credential is unavailable");
    let input = AssistantConnectionCheckInput {
        request_id: Uuid::new_v4().to_string(),
        settings,
    };
    let report = check_assistant_connection(&input)
        .await
        .expect("Live check could not be admitted");
    println!("chat={:?}", report.chat);
    println!("toolCall={:?}", report.tool_call);
    println!("toolReplay={:?}", report.tool_replay);
    assert_eq!(report.chat.status, AssistantConnectionStageStatus::Passed);
    assert_eq!(
        report.tool_call.status,
        AssistantConnectionStageStatus::Passed
    );
    assert_eq!(
        report.tool_replay.status,
        AssistantConnectionStageStatus::Passed
    );
    assert_eq!(report.request_count, 3);
    assert!(!report.cancelled);
}
