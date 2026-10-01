use super::*;
use crate::assistant::AssistantToolCall;

fn tools() -> Vec<AssistantToolDefinition> {
    vec![AssistantToolDefinition {
        name: "read_settings".into(),
        description: "Read current settings".into(),
        parameters: json!({"type":"object","properties":{"keys":{"type":"array","items":{"type":"string"}}},"required":["keys"],"additionalProperties":false}),
    }]
}

fn decode(protocol: ProviderProtocol, payload: Value) -> Result<AssistantToolReply, String> {
    protocol.decode_tool_response(&serde_json::to_vec(&payload).unwrap())
}

#[test]
fn ordinary_conversation_omits_tool_fields_when_no_tools_are_available() {
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::AnthropicCompatible,
        ProviderProtocol::Ollama,
    ] {
        let body = protocol
            .tool_request_body(
                "fixture",
                "System",
                &[AssistantToolMessage::User("Hello".into())],
                &[],
            )
            .unwrap();
        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
        assert_eq!(body["model"], "fixture");
        assert_eq!(
            body["messages"].as_array().unwrap().last().unwrap()["content"],
            "Hello"
        );
    }
}

#[test]
fn tool_protocol_openai_preserves_native_calls_and_malformed_arguments_for_error_feedback() {
    let raw = json!({"role":"assistant","content":null,"reasoning_content":"provider continuation state", "tool_calls":[
        {"id":"call_one","type":"function","function":{"name":"read_settings","arguments":"{\"keys\":[\"max_players\"]}"}},
        {"id":"call_two","type":"function","function":{"name":"read_settings","arguments":"{unfinished"}}
    ]});
    let reply = decode(
        ProviderProtocol::OpenAiCompatible,
        json!({"choices":[{"finish_reason":"tool_calls","message":raw}]}),
    )
    .unwrap();
    assert_eq!(reply.raw_message, raw);
    assert_eq!(reply.calls[0].arguments, json!({"keys":["max_players"]}));
    assert_eq!(reply.calls[1].arguments, json!("{unfinished"));
    let messages = vec![
        AssistantToolMessage::User("Preserve the existing mods".into()),
        AssistantToolMessage::Assistant(reply),
        AssistantToolMessage::ToolResult {
            call_id: "call_one".into(),
            name: "read_settings".into(),
            content: "{\"exists\":true}".into(),
            is_error: false,
        },
        AssistantToolMessage::ToolResult {
            call_id: "call_two".into(),
            name: "read_settings".into(),
            content: "{\"error\":\"invalid arguments\"}".into(),
            is_error: true,
        },
    ];
    let body = ProviderProtocol::OpenAiCompatible
        .tool_request_body("fixture", "trusted contract", &messages, &tools())
        .unwrap();
    assert_eq!(body["messages"][2], raw);
    assert_eq!(
        body["messages"][3],
        json!({"role":"tool","tool_call_id":"call_one","content":"{\"exists\":true}"})
    );
    assert_eq!(body["messages"][4]["tool_call_id"], "call_two");
    assert_eq!(
        body["tools"][0]["function"]["parameters"],
        tools()[0].parameters
    );
    assert!(body.get("response_format").is_none());
    assert_eq!(body["tool_choice"], "auto");
}

#[test]
fn tool_protocol_ollama_keeps_native_thinking_and_assigns_response_owned_missing_ids() {
    let raw = json!({"role":"assistant","content":"", "thinking":"retained provider state", "tool_calls":[
        {"function":{"index":0,"name":"read_settings","arguments":{"keys":[]}}},
        {"function":{"index":1,"name":"read_settings","arguments":"invalid native arguments"}}
    ]});
    let payload = json!({"done":true,"done_reason":"stop","message":raw});
    let protocol = ProviderProtocol::Ollama;
    let reply = decode(protocol, payload.clone()).unwrap();
    let first_id = reply.calls[0].id.clone();
    let second_id = reply.calls[1].id.clone();
    let nonce = first_id
        .strip_prefix("ollama_")
        .unwrap()
        .strip_suffix("_0")
        .unwrap();
    assert!(uuid::Uuid::parse_str(nonce).is_ok());
    assert_eq!(second_id, format!("ollama_{nonce}_1"));
    assert_eq!(reply.calls[1].arguments, json!("invalid native arguments"));
    assert_ne!(
        decode(protocol, payload.clone()).unwrap().calls[0].id,
        reply.calls[0].id
    );
    let body = protocol
        .tool_request_body(
            "fixture",
            "System",
            &[
                AssistantToolMessage::Assistant(reply),
                AssistantToolMessage::ToolResult {
                    call_id: first_id,
                    name: "read_settings".into(),
                    content: "one".into(),
                    is_error: false,
                },
                AssistantToolMessage::ToolResult {
                    call_id: second_id,
                    name: "read_settings".into(),
                    content: "two".into(),
                    is_error: true,
                },
            ],
            &tools(),
        )
        .unwrap();
    assert_eq!(body["messages"][1], raw);
    assert_eq!(
        body["messages"][2],
        json!({"role":"tool","tool_name":"read_settings","content":"one"})
    );
    for field in ["format", "think", "reasoning_effort", "tool_choice"] {
        assert!(body.get(field).is_none(), "{field}");
    }
    assert_eq!(body["options"], json!({"num_ctx":32768,"num_predict":8192}));
    assert_eq!(body["truncate"], false);
    assert_eq!(body["shift"], false);
    assert_eq!(
        protocol
            .tool_endpoint("https://gateway.example/TenantA/v1")
            .unwrap(),
        "https://gateway.example/TenantA/api/chat"
    );
}

#[test]
fn tool_protocol_anthropic_replays_signed_blocks_and_batches_results() {
    let blocks = json!([
        {"type":"thinking","thinking":"opaque provider state","signature":"fixture-signature"},
        {"type":"redacted_thinking","data":"fixture-opaque-data"},
        {"type":"text","text":"I will inspect both."},
        {"type":"tool_use","id":"read_a","name":"read_settings","input":{"keys":["max_players"]}},
        {"type":"tool_use","id":"read_b","name":"read_settings","input":"invalid input"}
    ]);
    let protocol = ProviderProtocol::AnthropicCompatible;
    let reply = decode(
        protocol,
        json!({"role":"assistant","stop_reason":"tool_use","content":blocks}),
    )
    .unwrap();
    assert_eq!(reply.content, "I will inspect both.");
    assert_eq!(reply.calls.len(), 2);
    let body = protocol
        .tool_request_body(
            "fixture",
            "System",
            &[
                AssistantToolMessage::User("Request".into()),
                AssistantToolMessage::Assistant(reply),
                AssistantToolMessage::ToolResult {
                    call_id: "read_a".into(),
                    name: "read_settings".into(),
                    content: "first result".into(),
                    is_error: false,
                },
                AssistantToolMessage::ToolResult {
                    call_id: "read_b".into(),
                    name: "read_settings".into(),
                    content: "invalid arguments".into(),
                    is_error: true,
                },
            ],
            &tools(),
        )
        .unwrap();
    assert_eq!(body["messages"][1]["content"], blocks);
    assert_eq!(
        body["messages"][2],
        json!({"role":"user","content":[
            {"type":"tool_result","tool_use_id":"read_a","content":"first result","is_error":false},
            {"type":"tool_result","tool_use_id":"read_b","content":"invalid arguments","is_error":true}
        ]})
    );
    assert_eq!(body["tools"][0]["input_schema"], tools()[0].parameters);
    assert_eq!(body["system"], "System");
    assert_eq!(body["max_tokens"], 4096);
    assert_eq!(body["tool_choice"], json!({"type":"auto"}));
    assert!(body.get("thinking").is_none());
}

#[test]
fn tool_protocol_never_extracts_calls_from_text_or_incomplete_response() {
    let text = "{\"action\":\"start_server\"}";
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Ollama,
        ProviderProtocol::AnthropicCompatible,
    ] {
        let response = match protocol {
            ProviderProtocol::OpenAiCompatible => {
                json!({"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":text}}]})
            }
            ProviderProtocol::Ollama => {
                json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":text}})
            }
            ProviderProtocol::AnthropicCompatible => {
                json!({"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":text}]})
            }
        };
        let reply = decode(protocol, response.clone()).unwrap();
        assert_eq!(reply.content, text);
        assert!(reply.calls.is_empty());
        let raw = reply.raw_message.clone();
        let body = protocol
            .tool_request_body(
                "fixture",
                "System",
                &[
                    AssistantToolMessage::User("Explain this example".into()),
                    AssistantToolMessage::Assistant(reply),
                    AssistantToolMessage::User("Continue the explanation".into()),
                ],
                &tools(),
            )
            .unwrap();
        let first_user = usize::from(protocol != ProviderProtocol::AnthropicCompatible);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), first_user + 3);
        assert_eq!(
            messages[first_user],
            json!({"role":"user","content":"Explain this example"})
        );
        assert_eq!(messages[first_user + 1], raw);
        assert_eq!(
            messages[first_user + 2],
            json!({"role":"user","content":"Continue the explanation"})
        );
        for reason in [
            "length",
            "max_tokens",
            "error",
            "content_filter",
            "pause_turn",
            "",
        ] {
            let mut incomplete = response.clone();
            match protocol {
                ProviderProtocol::OpenAiCompatible => {
                    incomplete["choices"][0]["finish_reason"] = json!(reason)
                }
                ProviderProtocol::Ollama => incomplete["done_reason"] = json!(reason),
                ProviderProtocol::AnthropicCompatible => incomplete["stop_reason"] = json!(reason),
            }
            assert!(
                decode(protocol, incomplete).is_err(),
                "{protocol:?} {reason}"
            );
        }
    }
}

#[test]
fn tool_protocol_rejects_invalid_or_missing_native_envelopes() {
    for payload in [
        json!({"done":false,"done_reason":"stop","message":{"role":"assistant","content":"partial"}}),
        json!({"done":true,"message":{"role":"assistant","content":"partial"}}),
        json!({"done":true,"done_reason":"stop","message":{"role":"user","content":"wrong role"}}),
        json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":5}}),
        json!({"done":true,"done_reason":"stop","message":{"role":"assistant","thinking":"only thinking"}}),
        json!({"done":true,"done_reason":"stop","message":{"role":"assistant","content":" "}}),
    ] {
        assert!(decode(ProviderProtocol::Ollama, payload).is_err());
    }
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Ollama,
        ProviderProtocol::AnthropicCompatible,
    ] {
        assert!(protocol.decode_tool_response(b"invalid JSON").is_err());
        assert!(decode(protocol, json!({"error":"provider failure"})).is_err());
    }
    assert!(decode(ProviderProtocol::OpenAiCompatible, json!({"choices":[{"finish_reason":"tool_calls","message":{
        "role":"assistant","tool_calls":[{"id":"a","type":"function","function":{"name":"read_settings","arguments":{}}}]
    }}]})).is_err(), "Chat Completions arguments must be a JSON-encoded string");
}

#[test]
fn tool_protocol_rejects_duplicate_or_invalid_call_ids_and_unsigned_thinking() {
    let call =
        json!({"id":"same","type":"function","function":{"name":"read_settings","arguments":"{}"}});
    for ids in [
        vec!["same".to_string(), "same".to_string()],
        vec![String::new()],
        vec!["x".repeat(129)],
        vec!["space id".to_string()],
    ] {
        let calls: Vec<_> = ids
            .iter()
            .map(|id| {
                let mut item = call.clone();
                item["id"] = json!(id);
                item
            })
            .collect();
        assert!(decode(ProviderProtocol::OpenAiCompatible,json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":calls}}]})).is_err());
    }
    assert!(decode(ProviderProtocol::AnthropicCompatible,json!({"role":"assistant","stop_reason":"tool_use","content":[
        {"type":"thinking","thinking":"missing signature"},{"type":"tool_use","id":"a","name":"read_settings","input":{}}
    ]})).is_err());
    let calls = vec![call; 33];
    assert!(decode(ProviderProtocol::OpenAiCompatible,json!({"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","tool_calls":calls}}]})).is_err());
}

#[test]
fn tool_protocol_canonical_fixture_message_preserves_pairing_without_raw_blocks() {
    let reply = AssistantToolReply {
        content: "fixture".into(),
        calls: vec![AssistantToolCall {
            id: "fixture_call".into(),
            name: "read_settings".into(),
            arguments: json!({"keys":[]}),
        }],
        raw_message: Value::Null,
    };
    for protocol in [
        ProviderProtocol::OpenAiCompatible,
        ProviderProtocol::Ollama,
        ProviderProtocol::AnthropicCompatible,
    ] {
        let body = protocol
            .tool_request_body(
                "fixture",
                "System",
                &[AssistantToolMessage::Assistant(reply.clone())],
                &tools(),
            )
            .unwrap();
        let message = &body["messages"][if protocol == ProviderProtocol::AnthropicCompatible {
            0
        } else {
            1
        }];
        assert_eq!(message["role"], "assistant");
        if protocol == ProviderProtocol::AnthropicCompatible {
            assert_eq!(message["content"][1]["id"], "fixture_call");
        } else {
            assert_eq!(
                message["tool_calls"][0]["function"]["name"],
                "read_settings"
            );
        }
    }
}

#[test]
fn openai_refusal_is_public_text_and_keeps_its_native_history() {
    let raw = json!({"role":"assistant","content":null,"refusal":"我无法协助这项请求。"});
    let reply = decode(
        ProviderProtocol::OpenAiCompatible,
        json!({"choices":[{"finish_reason":"stop","message":raw}]}),
    )
    .unwrap();
    assert_eq!(reply.content, "我无法协助这项请求。");
    assert!(reply.calls.is_empty());
    assert_eq!(reply.raw_message, raw);
    let body = ProviderProtocol::OpenAiCompatible
        .tool_request_body(
            "fixture",
            "System",
            &[AssistantToolMessage::Assistant(reply)],
            &tools(),
        )
        .unwrap();
    assert_eq!(body["messages"][1], raw);
}

#[test]
fn openai_refusal_cannot_also_schedule_a_tool_call() {
    let payload = json!({"choices":[{"finish_reason":"tool_calls","message":{
        "role":"assistant","content":null,"refusal":"I cannot do that.",
        "tool_calls":[{"id":"call_one","type":"function","function":{"name":"read_settings","arguments":"{}"}}]
    }}]});
    assert!(decode(ProviderProtocol::OpenAiCompatible, payload).is_err());
}
