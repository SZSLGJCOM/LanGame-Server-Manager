use super::*;
use std::sync::Mutex;

fn frame(value: Value) -> String {
    format!("data: {value}\r\n\r\n")
}

fn openai(delta: Value, finish: Value) -> String {
    frame(json!({"choices":[{"index":0,"delta":delta,"finish_reason":finish}]}))
}

#[test]
fn normal_token_sse_envelopes_do_not_consume_the_decoded_response_budget() {
    let expected = "先检查服务器配置，保存世界备份后再重启。\n".repeat(80);
    let mut wire = String::new();
    for character in expected.chars() {
        wire.push_str(&frame(json!({
            "id":"chatcmpl-normal-stream-fixture",
            "object":"chat.completion.chunk",
            "created":1_759_000_000,
            "model":"compatible-chat-model",
            "system_fingerprint":"compatible-provider-fingerprint",
            "choices":[{"index":0,"delta":{"content":character.to_string()},"finish_reason":null}]
        })));
    }
    wire.push_str(&openai(json!({}), json!("stop")));
    wire.push_str("data: [DONE]\n\n");
    assert!(wire.len() > ASSISTANT_MAX_RESPONSE_BYTES);
    assert!(expected.len() < ASSISTANT_MAX_RESPONSE_BYTES / 32);
    for size in [1, 997, wire.len()] {
        let (reply, public) = decode_chunks(ProviderProtocol::OpenAiCompatible, &wire, size);
        let reply =
            reply.expect("Repeated SSE envelopes are transport overhead, not retained answer text");
        assert_eq!(public, expected);
        assert_eq!(reply.content, expected);
        assert!(reply.calls.is_empty());
    }
}

fn decode_chunks(
    protocol: ProviderProtocol,
    wire: &str,
    chunk_size: usize,
) -> (Result<AssistantToolReply, String>, String) {
    let public = Mutex::new(String::new());
    let observer = |delta: &str| {
        public.lock().unwrap().push_str(delta);
        Ok(())
    };
    let mut stream = ToolStream::new(protocol);
    let result = (|| {
        for chunk in wire.as_bytes().chunks(chunk_size) {
            stream.push(chunk, &observer)?;
        }
        stream.finish(&observer)
    })();
    (result, public.into_inner().unwrap())
}

#[test]
fn openai_stream_reassembles_utf8_parallel_calls_and_keeps_reasoning_private() {
    let wire = [
        openai(json!({"role":"assistant","reasoning_content":"private chain","content":"检查"}), Value::Null),
        openai(json!({"tool_calls":[{"index":1,"id":"second","type":"function","function":{"name":"read_logs","arguments":"{\"limit\":"}},{"index":0,"id":"first","type":"function","function":{"name":"read_settings","arguments":"{"}}]}), Value::Null),
        openai(json!({"content":"完成","tool_calls":[{"index":0,"function":{"arguments":"}"}},{"index":1,"function":{"arguments":"20}"}}]}), json!("tool_calls")),
        "data: [DONE]\r\n\r\n".into(),
    ].concat();
    for size in [1, 7, wire.len()] {
        let (reply, public) = decode_chunks(ProviderProtocol::OpenAiCompatible, &wire, size);
        let reply = reply.unwrap();
        assert_eq!(public, "检查完成");
        assert_eq!(reply.content, public);
        assert_eq!(reply.calls[0].name, "read_settings");
        assert_eq!(reply.calls[1].arguments, json!({"limit":20}));
        assert_eq!(reply.raw_message["reasoning_content"], "private chain");
    }
}

#[test]
fn malformed_truncated_and_duplicate_calls_never_leave_the_stream_as_tools() {
    for wire in [
        openai(json!({"content":"partial"}), Value::Null),
        openai(json!({"content":"truncated"}), json!("length")) + "data: [DONE]\n\n",
        openai(
            json!({"tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"read_logs","arguments":"{\"limit\":"}}]}),
            json!("tool_calls"),
        ) + "data: [DONE]\n\n",
        openai(
            json!({"tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"read_logs","arguments":"{}"}},{"index":1,"id":"a","type":"function","function":{"name":"read_logs","arguments":"{}"}}]}),
            json!("tool_calls"),
        ) + "data: [DONE]\n\n",
        frame(json!({"error":{"message":"fixture upstream failure"}})),
    ] {
        assert!(
            decode_chunks(ProviderProtocol::OpenAiCompatible, &wire, 3)
                .0
                .is_err()
        );
    }
}

#[test]
fn ollama_native_stream_retains_tool_objects_and_hides_thinking() {
    let wire = [
        json!({"message":{"role":"assistant","thinking":"private","content":"Ready"},"done":false}).to_string(),
        json!({"message":{"role":"assistant","tool_calls":[{"function":{"name":"read_logs","arguments":{"limit":2}}}]},"done":false}).to_string(),
        json!({"message":{"role":"assistant","content":"."},"done":true,"done_reason":"stop"}).to_string(),
    ].join("\n");
    let (reply, public) = decode_chunks(ProviderProtocol::Ollama, &wire, 1);
    let reply = reply.unwrap();
    assert_eq!(public, "Ready.");
    assert_eq!(reply.calls[0].arguments, json!({"limit":2}));
    assert_eq!(reply.raw_message["thinking"], "private");
}

#[test]
fn anthropic_stream_retains_signed_thinking_but_only_observes_text() {
    let wire = [
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":"Inspect "}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"日志"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"a","name":"read_logs","input":{}}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"limit\":"}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"2}"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
        json!({"type":"message_stop"}),
    ].into_iter().map(frame).collect::<String>();
    let (reply, public) = decode_chunks(ProviderProtocol::AnthropicCompatible, &wire, 1);
    let reply = reply.unwrap();
    assert_eq!(public, "Inspect 日志");
    assert_eq!(reply.calls[0].arguments, json!({"limit":2}));
    assert_eq!(reply.raw_message["content"][0]["signature"], "signed");
    let truncated = wire.rsplit_once("data:").unwrap().0;
    assert!(
        decode_chunks(ProviderProtocol::AnthropicCompatible, truncated, 5)
            .0
            .is_err()
    );
}

#[test]
fn observer_cancellation_and_wire_limits_stop_parsing_immediately() {
    let mut stream = ToolStream::new(ProviderProtocol::OpenAiCompatible);
    let frame = openai(json!({"content":"visible"}), Value::Null);
    assert_eq!(
        stream
            .push(frame.as_bytes(), &|_| Err("cancelled".into()))
            .unwrap_err(),
        "cancelled"
    );
    let mut stream = ToolStream::new(ProviderProtocol::Ollama);
    assert!(
        stream
            .push(&vec![b' '; ASSISTANT_MAX_RESPONSE_BYTES + 1], &|_| Ok(()))
            .is_err()
    );
}

#[test]
fn aggregate_retained_budget_includes_private_reasoning_and_native_arguments() {
    let large = "x".repeat(180 * 1024);
    let excess = "y".repeat(90 * 1024);
    let openai_wire = [
        openai(json!({"reasoning_content":large}), Value::Null),
        openai(json!({"content":excess}), Value::Null),
    ]
    .concat();
    let ollama_wire = [
        json!({"message":{"role":"assistant","tool_calls":[{"function":{"name":"read_logs","arguments":{"note":large}}}]},"done":false}).to_string(),
        json!({"message":{"role":"assistant","thinking":excess},"done":false}).to_string(),
    ].join("\n");
    let anthropic_wire = [
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":large}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":excess}}),
    ].into_iter().map(frame).collect::<String>();
    for (protocol, wire) in [
        (ProviderProtocol::OpenAiCompatible, openai_wire),
        (ProviderProtocol::Ollama, ollama_wire),
        (ProviderProtocol::AnthropicCompatible, anthropic_wire),
    ] {
        let (reply, public) = decode_chunks(protocol, &wire, 4093);
        let error = reply.unwrap_err();
        assert!(error.contains("retained response byte limit"), "{error}");
        assert!(
            error.contains("wire_bytes=") && error.contains("retained_bytes="),
            "{error}"
        );
        assert!(
            public.is_empty(),
            "Rejected public text must not reach the observer"
        );
    }
}

#[test]
fn anthropic_partial_tool_json_is_bounded_before_a_block_can_close() {
    let wire = [
        json!({"type":"message_start","message":{"role":"assistant","content":[]}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"a","name":"read_logs","input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"x".repeat(140 * 1024)}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"y".repeat(140 * 1024)}}),
    ].into_iter().map(frame).collect::<String>();
    let error = decode_chunks(ProviderProtocol::AnthropicCompatible, &wire, 4093)
        .0
        .unwrap_err();
    assert!(error.contains("retained response byte limit"), "{error}");
}

#[test]
fn completed_json_still_obeys_the_nonstreaming_response_size_limit() {
    let escaped = "\0".repeat(32 * 1024);
    let wire = [
        openai(json!({"content":escaped}), Value::Null),
        openai(json!({"content":escaped}), json!("stop")),
        "data: [DONE]\n\n".into(),
    ]
    .concat();
    let error = decode_chunks(ProviderProtocol::OpenAiCompatible, &wire, 4093)
        .0
        .unwrap_err();
    assert!(error.contains("retained response byte limit"), "{error}");
    assert!(error.contains("response_bytes="), "{error}");
}

#[test]
fn transport_event_and_pending_frame_limits_are_independent() {
    let observer = |_: &str| Ok(());
    let mut stream = ToolStream::new(ProviderProtocol::OpenAiCompatible);
    let comment = format!(":{}\n\n", ".".repeat(1021));
    assert_eq!(comment.len(), 1024);
    for _ in 0..MAX_STREAM_WIRE_BYTES / comment.len() {
        stream.push(comment.as_bytes(), &observer).unwrap();
    }
    assert_eq!(stream.events, 0);
    assert_eq!(stream.accumulator.retained_bytes(), 0);
    let error = stream.push(b":\n\n", &observer).unwrap_err();
    assert!(
        error.contains("transport byte limit") && error.contains("wire_bytes="),
        "{error}"
    );

    let mut stream = ToolStream::new(ProviderProtocol::AnthropicCompatible);
    let ping = frame(json!({"type":"ping"}));
    for _ in 0..MAX_STREAM_EVENTS {
        stream.push(ping.as_bytes(), &observer).unwrap();
    }
    let error = stream.push(ping.as_bytes(), &observer).unwrap_err();
    assert!(error.contains("event count limit"), "{error}");

    let mut stream = ToolStream::new(ProviderProtocol::Ollama);
    let error = stream
        .push(&vec![b' '; MAX_STREAM_FRAME_BYTES + 1], &observer)
        .unwrap_err();
    assert!(error.contains("frame byte limit"), "{error}");
    let mut stream = ToolStream::new(ProviderProtocol::OpenAiCompatible);
    let data_line = format!("data: {}\n", "a".repeat(1023));
    for _ in 0..MAX_STREAM_FRAME_BYTES / 1024 {
        stream.push(data_line.as_bytes(), &observer).unwrap();
    }
    let error = stream.push(data_line.as_bytes(), &observer).unwrap_err();
    assert!(error.contains("frame byte limit"), "{error}");
}

#[tokio::test]
async fn transport_delivers_public_text_before_the_provider_finishes() {
    use crate::assistant::{
        AssistantProviderSettings, AssistantRunInput, AssistantToolMessage,
        run_assistant_tool_turn_streaming,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let first = openai(json!({"role":"assistant","content":"early"}), Value::Null);
    let tail = openai(json!({"content":" reply"}), json!("stop")) + "data: [DONE]\n\n";
    let (observed, wait) = tokio::sync::oneshot::channel();
    let observed = Mutex::new(Some(observed));
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let count = socket.read(&mut buf).await.unwrap();
            assert!(count > 0 && bytes.len() < 64 * 1024);
            bytes.extend_from_slice(&buf[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|value| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    let body: Value =
                        serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
                    assert_eq!(body["stream"], true);
                    break;
                }
            }
        }
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            first.len() + tail.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(first.as_bytes()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), wait)
            .await
            .unwrap()
            .unwrap();
        socket.write_all(tail.as_bytes()).await.unwrap();
        socket.shutdown().await.unwrap();
    });
    let input = AssistantRunInput {
        settings: AssistantProviderSettings {
            provider: "openai-compatible".into(),
            model: "fixture".into(),
            base_url: endpoint,
            api_key: "fixture".into(),
        },
        prompt_label: "fixture".into(),
        prompt: String::new(),
        context: String::new(),
    };
    let observer = |text: &str| {
        if text == "early"
            && let Some(sender) = observed.lock().unwrap().take()
        {
            sender.send(()).unwrap();
        }
        Ok(())
    };
    let reply = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        run_assistant_tool_turn_streaming(
            &input,
            "fixture",
            &[AssistantToolMessage::User("hello".into())],
            &[],
            &observer,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(reply.content, "early reply");
    server.await.unwrap();
}

#[test]
fn openai_stream_observes_refusal_text_without_creating_calls() {
    let wire = [
        openai(
            json!({"role":"assistant","content":null,"refusal":"我无法"}),
            Value::Null,
        ),
        openai(json!({"refusal":"协助这项请求。"}), json!("stop")),
        "data: [DONE]\n\n".into(),
    ]
    .concat();
    for size in [1, 7, wire.len()] {
        let (reply, public) = decode_chunks(ProviderProtocol::OpenAiCompatible, &wire, size);
        let reply = reply.unwrap();
        assert_eq!(public, "我无法协助这项请求。");
        assert_eq!(reply.content, public);
        assert_eq!(reply.raw_message["refusal"], public);
        assert!(reply.calls.is_empty());
    }
    let refused_call = openai(
        json!({"refusal":"I cannot do that.","tool_calls":[{"index":0,"id":"a","type":"function","function":{"name":"read_settings","arguments":"{}"}}]}),
        json!("tool_calls"),
    ) + "data: [DONE]\n\n";
    assert!(
        decode_chunks(ProviderProtocol::OpenAiCompatible, &refused_call, 3)
            .0
            .is_err()
    );
}
