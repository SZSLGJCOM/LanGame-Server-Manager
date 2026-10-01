use super::*;

#[test]
fn provider_protocol_rejects_removed_brand_aliases() {
    for value in ["", "openai", "deepseek", "anthropic", "unknown"] {
        assert!(ProviderProtocol::parse(value).is_err(), "{value}");
    }
    assert_eq!(
        ProviderProtocol::parse(" Anthropic-Compatible ").unwrap(),
        ProviderProtocol::AnthropicCompatible,
    );
}

#[test]
fn anthropic_request_uses_the_messages_contract() {
    let protocol = ProviderProtocol::AnthropicCompatible;
    let endpoint = protocol.endpoint("https://api.anthropic.com/v1/").unwrap();
    assert_eq!(endpoint, "https://api.anthropic.com/v1/messages");
    assert_eq!(protocol.endpoint(&endpoint).unwrap(), endpoint);
    let body = protocol.request_body("fixture-model", "System instructions", "User question");
    let request = protocol
        .request(&Client::new(), &endpoint, &body, "fixture-key")
        .build()
        .unwrap();
    assert_eq!(request.headers()["x-api-key"], "fixture-key");
    assert_eq!(request.headers()["anthropic-version"], "2023-06-01");
    assert!(request.headers().get("authorization").is_none());
    assert_eq!(body["system"], "System instructions");
    assert_eq!(
        body["messages"],
        json!([{ "role": "user", "content": "User question" }])
    );
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["max_tokens"], 4096);
    assert!(body.get("temperature").is_none());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn openai_and_ollama_requests_use_chat_completions_without_anthropic_headers() {
    for protocol in [ProviderProtocol::OpenAiCompatible, ProviderProtocol::Ollama] {
        let endpoint = protocol.endpoint("https://gateway.example/v1").unwrap();
        assert_eq!(endpoint, "https://gateway.example/v1/chat/completions");
        assert_eq!(
            protocol.endpoint(&format!("{endpoint}/")).unwrap(),
            endpoint
        );
        let body = protocol.request_body("fixture-model", "System", "User");
        let request = protocol
            .request(&Client::new(), &endpoint, &body, "fixture-key")
            .build()
            .unwrap();
        assert_eq!(request.headers()["authorization"], "Bearer fixture-key");
        assert!(request.headers().get("x-api-key").is_none());
        assert!(request.headers().get("anthropic-version").is_none());
        assert_eq!(body["messages"][0]["role"], "system");
        assert!(body.get("system").is_none());
        assert!(body.get("max_tokens").is_none());
    }
}

#[test]
fn ollama_tool_endpoint_preserves_custom_prefixes_and_normalizes_known_suffixes() {
    let protocol = ProviderProtocol::Ollama;
    for (base, expected) in [
        ("http://localhost:11434", "http://localhost:11434/api/chat"),
        ("http://localhost:11434/", "http://localhost:11434/api/chat"),
        (
            "http://localhost:11434/v1/",
            "http://localhost:11434/api/chat",
        ),
        (
            "http://localhost:11434/v1/chat/completions/",
            "http://localhost:11434/api/chat",
        ),
        (
            "https://proxy.example/TenantA",
            "https://proxy.example/TenantA/api/chat",
        ),
        (
            "https://proxy.example/TenantA/v1",
            "https://proxy.example/TenantA/api/chat",
        ),
        (
            "https://proxy.example/TenantA/v1/chat/completions",
            "https://proxy.example/TenantA/api/chat",
        ),
        (
            "https://proxy.example/TenantA/chat/completions",
            "https://proxy.example/TenantA/api/chat",
        ),
        (
            "https://proxy.example/TenantA/api/chat",
            "https://proxy.example/TenantA/api/chat",
        ),
        (
            "https://proxy.example/v1/TenantA",
            "https://proxy.example/v1/TenantA/api/chat",
        ),
        ("https://v1", "https://v1/api/chat"),
    ] {
        assert_eq!(protocol.tool_endpoint(base).unwrap(), expected, "{base}");
    }
    for base in [
        "file:///v1",
        "https://user:pass@example.test/v1",
        "https://example.test/v1?key=secret",
    ] {
        assert!(protocol.tool_endpoint(base).is_err());
    }
}

#[test]
fn anthropic_response_decodes_only_text_and_rejects_truncated_output() {
    let protocol = ProviderProtocol::AnthropicCompatible;
    let body = json!({
        "stop_reason": "end_turn",
        "content": [
            { "type": "thinking", "thinking": "Internal reasoning" },
            { "type": "text", "text": " First " },
            { "type": "text", "text": "Second" }
        ]
    });
    assert_eq!(
        protocol
            .decode_response(&serde_json::to_vec(&body).unwrap())
            .unwrap(),
        "First\n\nSecond"
    );
    for body in [
        json!({ "content": [] }),
        json!({ "content": [{ "type": "text", "text": " " }] }),
        json!({ "content": [{ "type": "text", "text": "partial" }], "stop_reason": "max_tokens" }),
        json!({ "choices": [{ "message": { "content": "Wrong protocol" } }] }),
    ] {
        assert!(
            protocol
                .decode_response(&serde_json::to_vec(&body).unwrap())
                .is_err()
        );
    }
    assert!(protocol.decode_response(b"invalid JSON").is_err());
}

#[test]
fn chat_response_accepts_plain_text_and_content_parts() {
    let protocol = ProviderProtocol::OpenAiCompatible;
    for content in [json!(" First "), json!([{ "text": " First " }])] {
        let body =
            json!({ "choices": [{ "finish_reason": "stop", "message": { "content": content } }] });
        assert_eq!(
            protocol
                .decode_response(&serde_json::to_vec(&body).unwrap())
                .unwrap(),
            "First"
        );
    }
    assert!(
        protocol
            .decode_response(br#"{"content":[{"type":"text","text":"Wrong protocol"}]}"#,)
            .is_err()
    );
}

#[test]
fn credential_urls_preserve_case_sensitive_gateway_paths() {
    assert_eq!(
        normalize_service_url(" HTTPS://Gateway.Example/TenantA/v1/ ").unwrap(),
        "https://gateway.example/TenantA/v1"
    );
    assert_ne!(
        normalize_service_url("https://gateway.example/TenantA/v1").unwrap(),
        normalize_service_url("https://gateway.example/tenanta/v1").unwrap()
    );
    for url in [
        "/v1",
        "file:///v1",
        "mock://assistant",
        "https://user:pass@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#fragment",
    ] {
        assert!(normalize_service_url(url).is_err(), "{url}");
    }
}

#[test]
fn ollama_all_request_paths_share_the_same_service_root() {
    for root in [
        "http://localhost:11434",
        "https://proxy.example/TenantA",
        "https://proxy.example/v1/TenantA",
    ] {
        for suffix in [
            "",
            "/",
            "/v1/",
            "/api/chat",
            "/api/tags/",
            "/chat/completions",
            "/v1/chat/completions/",
        ] {
            let base = format!("{root}{suffix}");
            assert_eq!(
                ProviderProtocol::Ollama.tool_endpoint(&base).unwrap(),
                format!("{root}/api/chat"),
                "{base}"
            );
            assert_eq!(
                ProviderProtocol::Ollama.endpoint(&base).unwrap(),
                format!("{root}/v1/chat/completions"),
                "{base}"
            );
            assert_eq!(
                crate::assistant::build_ollama_tags_endpoint(Some(&base)).unwrap(),
                format!("{root}/api/tags"),
                "{base}"
            );
        }
    }
}

#[test]
fn chat_text_generation_rejects_truncated_and_abnormal_completions() {
    for protocol in [ProviderProtocol::OpenAiCompatible, ProviderProtocol::Ollama] {
        for reason in [
            json!("length"),
            json!("content_filter"),
            json!("tool_calls"),
            json!("error"),
            json!(""),
            Value::Null,
        ] {
            let body = json!({"choices":[{"finish_reason":reason,"message":{"content":"partial output"}}]});
            assert!(
                protocol
                    .decode_response(&serde_json::to_vec(&body).unwrap())
                    .is_err(),
                "{protocol:?} {reason}"
            );
        }
    }
}

#[test]
fn anthropic_text_generation_rejects_nonfinal_stop_reasons() {
    let protocol = ProviderProtocol::AnthropicCompatible;
    for reason in [
        Some(json!("pause_turn")),
        Some(json!("tool_use")),
        Some(json!("stop_sequence")),
        Some(json!("refusal")),
        Some(json!("unknown")),
        Some(Value::Null),
        None,
    ] {
        let mut body = json!({"content":[{"type":"text","text":"unfinished candidate"}]});
        if let Some(reason) = &reason {
            body["stop_reason"] = reason.clone();
        }
        let error = protocol
            .decode_response(&serde_json::to_vec(&body).unwrap())
            .expect_err("a nonfinal turn must not become a generated artifact");
        assert!(error.contains("did not complete"), "{reason:?}: {error}");
    }
}

#[test]
fn chat_text_generation_reports_a_pure_refusal_explicitly() {
    let body = json!({"choices":[{
        "finish_reason":"stop",
        "message":{"content":null,"refusal":"I cannot generate this content."}
    }]});
    let error = ProviderProtocol::OpenAiCompatible
        .decode_response(&serde_json::to_vec(&body).unwrap())
        .expect_err("a refusal must not become a generated artifact");
    assert!(error.contains("refus"), "{error}");
}

#[test]
fn chat_text_generation_rejects_content_mixed_with_refusal() {
    for content in [
        json!("partial candidate"),
        json!([{"text":"partial candidate"}]),
    ] {
        let body = json!({"choices":[{
            "finish_reason":"stop",
            "message":{"content":content,"refusal":"I cannot complete this content."}
        }]});
        let error = ProviderProtocol::OpenAiCompatible
            .decode_response(&serde_json::to_vec(&body).unwrap())
            .expect_err("a mixed refusal must not publish its partial content");
        assert!(error.contains("refus"), "{error}");
    }
}
