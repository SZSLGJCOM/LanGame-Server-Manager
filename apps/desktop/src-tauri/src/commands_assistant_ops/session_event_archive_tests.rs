use super::*;

#[test]
fn complete_write_receipt_remains_retrievable_beyond_read_and_active_window_limits() {
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let binding = crate::assistant_sessions::AssistantSessionBinding {
        provider: "fixture".into(),
        model: "fixture".into(),
        base_url: "https://example.invalid".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
    };
    let lease = store.begin(None, binding, true).unwrap();
    let session = lease.session();
    session
        .pin_context("Repair the private extension files and preserve all Mods.")
        .unwrap();
    let files: Vec<_> = (0..8)
        .map(|index| {
            json!({
                "file":format!("data/mods/{}/{index}.lua", "a".repeat(980)),
                "sourceSha256":"a".repeat(64), "resultSha256":"b".repeat(64),
                "backupId":format!("backup-{index}"), "readBackVerified":true,
            })
        })
        .collect();
    let receipt = json!({"fileChanges":files,"task":{"checks":[{"evidence":{"fileChanges":files}},{"evidence":{"fileChanges":files}}]}});
    assert!(receipt.to_string().len() > 24 * 1024);
    assistant_record_session_event(&session, "confirmed_operation", receipt.clone()).unwrap();
    let mut offset_bytes = 0;
    let mut reconstructed = String::new();
    loop {
        let page = session
            .read_history(crate::assistant_sessions::AssistantHistoryRequest {
                offset: 1,
                limit: 1,
                message_offset_bytes: offset_bytes,
                ..Default::default()
            })
            .unwrap();
        let entry = &page["messages"][0];
        if let Some(excerpt) = entry["excerpt"].as_str() {
            reconstructed.push_str(excerpt);
        } else {
            reconstructed = entry["message"].to_string();
        }
        if page["nextOffset"].as_u64().unwrap() > 1 {
            break;
        }
        offset_bytes = page["nextMessageOffsetBytes"].as_u64().unwrap() as usize;
    }
    let message: Value = serde_json::from_str(&reconstructed).unwrap();
    let content: Value =
        serde_json::from_str(message["ToolResult"]["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["ok"], true);
    assert_eq!(content["data"], receipt);
    assert!(session.messages().unwrap().iter().any(|message| matches!(message, crate::assistant::AssistantToolMessage::User(text) if text.contains("archiv"))));
}

#[test]
fn oversized_write_receipt_fails_without_replacing_it_with_a_successful_empty_record() {
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let binding = crate::assistant_sessions::AssistantSessionBinding {
        provider: "fixture".into(),
        model: "fixture".into(),
        base_url: "https://example.invalid".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
    };
    let lease = store.begin(None, binding, true).unwrap();
    let session = lease.session();
    assistant_record_session_event(
        &session,
        "confirmed_operation",
        json!({"message":"first receipt"}),
    )
    .unwrap();
    let before = serde_json::to_value(session.messages().unwrap()).unwrap();
    let error = assistant_record_session_event(
        &session,
        "confirmed_operation",
        json!({"message":"x".repeat(2 * 1024 * 1024)}),
    )
    .unwrap_err();
    assert!(error.contains("capacity"));
    assert_eq!(
        serde_json::to_value(session.messages().unwrap()).unwrap(),
        before
    );
}
