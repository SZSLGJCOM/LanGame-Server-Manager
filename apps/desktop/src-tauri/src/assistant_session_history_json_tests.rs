use super::*;

#[test]
fn archived_native_receipt_keeps_nested_json_valid_and_redacts_only_its_values() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .pin_context("Explain the completed write; do not repeat it.")
        .unwrap();
    let tail = format!(
        "{}Keep every Mod; do not start again.",
        "保留证据。".repeat(600)
    );
    let receipt = json!({"ok":true,"data":{
        "summary":"The password is unavailable; continue diagnosis without repeating the write.",
        "credentials":{"password":"synthetic-test-only"},
        "literal":"a\\b and \"quoted\" text",
        "verification":{"status":"failed","tail":tail}
    }})
    .to_string();
    session
        .append_messages(tool_group("native-receipt", &receipt))
        .unwrap();
    let original = encoded(&session.lock().unwrap().history).unwrap();
    let (record, pages) = collect_record(&session, AssistantHistorySource::Messages, 1);
    assert!(pages > 2);
    assert_eq!(record["ToolResult"]["call_id"], "native-receipt");
    assert_eq!(record["ToolResult"]["is_error"], false);
    let content = record["ToolResult"]["content"].as_str().unwrap();
    let retrieved: Value = serde_json::from_str(content).unwrap();
    assert_eq!(retrieved["ok"], true);
    assert_eq!(retrieved["data"]["credentials"], "[REDACTED]");
    assert_eq!(retrieved["data"]["literal"], "a\\b and \"quoted\" text");
    assert_eq!(retrieved["data"]["verification"]["tail"], tail);
    assert!(!content.contains("synthetic-test-only"));
    assert!(!record.to_string().contains("signed-native-receipt"));
    assert_eq!(encoded(&session.lock().unwrap().history).unwrap(), original);
}

#[test]
fn user_and_assistant_json_messages_keep_their_document_boundary_in_history() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let text = json!({"summary":"The password is missing; explain only.",
        "password":"synthetic-test-only","literal":"a\\b", "tail":"preserve every Mod"})
    .to_string();
    session.register_user_request(&text).unwrap();
    session
        .append_messages(vec![
            AssistantToolMessage::User(text.clone()),
            AssistantToolMessage::Assistant(AssistantToolReply {
                content: text,
                calls: Vec::new(),
                raw_message: json!({"thinking":"opaque must stay private"}),
            }),
        ])
        .unwrap();
    for (source, offset, pointer) in [
        (AssistantHistorySource::Messages, 0, "/User"),
        (AssistantHistorySource::Messages, 1, "/Assistant/content"),
        (AssistantHistorySource::UserRequests, 0, "/request"),
    ] {
        let (record, _) = collect_record(&session, source, offset);
        let text = record.pointer(pointer).unwrap().as_str().unwrap();
        let document: Value = serde_json::from_str(text).unwrap();
        assert_eq!(document["password"], "[REDACTED]");
        assert_eq!(document["literal"], "a\\b");
        assert_eq!(document["tail"], "preserve every Mod");
        assert!(!record.to_string().contains("opaque must stay private"));
    }
}
