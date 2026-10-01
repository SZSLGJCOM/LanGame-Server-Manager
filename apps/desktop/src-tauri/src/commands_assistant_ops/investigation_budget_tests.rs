use super::*;

fn messages() -> Vec<AssistantToolMessage> {
    vec![
        AssistantToolMessage::User("Keep the complete original request.".into()),
        AssistantToolMessage::Assistant(AssistantToolReply {
            content: "Read the saved settings before proposing a correction.".into(),
            calls: vec![AssistantToolCall {
                id: "read-1".into(),
                name: "read_settings".into(),
                arguments: json!({"keys":["max_players"]}),
            }],
            raw_message: json!({"role":"assistant","thinking":"protocol-only reasoning".repeat(4000),"content":"Read the saved settings before proposing a correction.",
                "tool_calls":[{"function":{"name":"read_settings","arguments":{"keys":["max_players"]}}}]}),
        }),
        AssistantToolMessage::ToolResult {
            call_id: "read-1".into(),
            name: "read_settings".into(),
            content: json!({"ok":true,"data":{"max_players":4}}).to_string(),
            is_error: false,
        },
    ]
}

#[test]
fn investigation_budget_counts_evidence_once_and_keeps_native_metadata_intact() {
    let messages = messages();
    let original = serde_json::to_value(&messages).unwrap();
    assert!(original.to_string().len() > ASSISTANT_INVESTIGATION_PROMPT_BYTES);
    assistant_check_conversation_budget(&messages, &[]).unwrap();
    assert_eq!(serde_json::to_value(&messages).unwrap(), original);
}

#[test]
fn investigation_budget_still_bounds_every_business_message_and_tool_schema() {
    for replacement in [
        AssistantToolMessage::User("x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES)),
        AssistantToolMessage::Assistant(AssistantToolReply {
            content: "x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES),
            calls: vec![],
            raw_message: Value::Null,
        }),
        AssistantToolMessage::Assistant(AssistantToolReply {
            content: String::new(),
            calls: vec![AssistantToolCall {
                id: "large-call".into(),
                name: "read_settings".into(),
                arguments: json!({"keys":["x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES)]}),
            }],
            raw_message: Value::Null,
        }),
        AssistantToolMessage::ToolResult {
            call_id: "read-1".into(),
            name: "read_settings".into(),
            content: "x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES),
            is_error: false,
        },
    ] {
        let mut messages = messages();
        messages.push(replacement);
        assert!(assistant_check_conversation_budget(&messages, &[]).is_err());
    }
    let tool = AssistantToolDefinition {
        name: "read_settings".into(),
        description: "x".repeat(ASSISTANT_INVESTIGATION_PROMPT_BYTES),
        parameters: json!({"type":"object"}),
    };
    assert!(assistant_check_conversation_budget(&messages(), &[tool]).is_err());
}
