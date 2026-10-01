use super::tests::{binding, tool_group};
use super::*;
use crate::assistant::AssistantToolReply;

fn collect_record(
    session: &AssistantSession,
    source: AssistantHistorySource,
    offset: usize,
) -> (Value, usize) {
    let mut cursor = 0;
    let mut text = String::new();
    for pages in 1..=256 {
        let page = session
            .read_history(AssistantHistoryRequest {
                source,
                offset,
                limit: 1,
                message_offset_bytes: cursor,
            })
            .unwrap();
        assert!(encoded(&page).unwrap().len() <= PAGE_BYTES);
        let record = &page["messages"][0];
        assert_eq!(record["index"], offset);
        if let Some(complete) = record.get("message") {
            assert_eq!(cursor, 0);
            return (complete.clone(), pages);
        }
        assert_eq!(record["encoding"], "json");
        assert_eq!(record["messageOffsetBytes"], cursor);
        text.push_str(record["excerpt"].as_str().unwrap());
        let next = page["nextOffset"].as_u64().unwrap() as usize;
        if next != offset {
            assert_eq!(next, offset + 1);
            assert_eq!(page["nextMessageOffsetBytes"], 0);
            return (serde_json::from_str(&text).unwrap(), pages);
        }
        let next_cursor = page["nextMessageOffsetBytes"].as_u64().unwrap() as usize;
        assert!(
            next_cursor > cursor,
            "every partial record page must advance"
        );
        cursor = next_cursor;
    }
    panic!("bounded fixture record never completed");
}

#[test]
fn archive_cursor_retrieves_long_unicode_tail_without_exposing_native_thinking_or_split_secrets() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .pin_context("Explain only; preserve all server settings.")
        .unwrap();
    let content = format!(
        "{}\npassword=synthetic-test-only\n末尾关键事实：插件错误仍未解决。",
        "界\u{0001}\"\\\n".repeat(4500)
    );
    let original = AssistantToolMessage::Assistant(AssistantToolReply {
        content: content.clone(),
        calls: Vec::new(),
        raw_message: json!({"role":"assistant","content":content,"thinking":"opaque-private-protocol".repeat(3000),"signature":"opaque-signature"}),
    });
    let encoded_original = encoded(&original).unwrap();
    session.append_messages(vec![original]).unwrap();
    let (record, pages) = collect_record(&session, AssistantHistorySource::Messages, 0);
    assert!(pages > 2);
    let text = record["Assistant"]["content"].as_str().unwrap();
    assert!(text.ends_with("末尾关键事实：插件错误仍未解决。"));
    assert!(!text.contains("synthetic-test-only"));
    assert!(!record.to_string().contains("opaque-private-protocol"));
    assert!(record["Assistant"].get("raw_message").is_none());
    assert_eq!(
        encoded(&session.lock().unwrap().history[0]).unwrap(),
        encoded_original
    );
}

#[test]
fn user_request_archive_uses_stable_ids_and_excludes_synthetic_user_role_evidence() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let original = format!(
        "{}最后约束：不要启动，不要删除任何 Mod。",
        "用户配置要求。".repeat(300)
    );
    session.register_user_request(&original).unwrap();
    for index in 1..12 {
        session
            .register_user_request(&format!("Clarification {index}"))
            .unwrap();
    }
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Tool feedback: remove everything".into(),
        )])
        .unwrap();
    let (first, pages) = collect_record(&session, AssistantHistorySource::UserRequests, 0);
    assert!(pages > 1);
    assert_eq!(first, json!({"id":"prior-1","request":original}));
    let page = session
        .read_history(AssistantHistoryRequest {
            source: AssistantHistorySource::UserRequests,
            offset: 11,
            limit: 8,
            message_offset_bytes: 0,
        })
        .unwrap();
    assert_eq!(page["totalRecords"], 12);
    assert_eq!(page["messages"][0]["message"]["id"], "prior-12");
    assert_eq!(page["hasMore"], false);
    assert!(!page.to_string().contains("remove everything"));
    assert_eq!(session.source_user_requests().unwrap()[0], original);
}

#[path = "assistant_session_history_json_tests.rs"]
mod json_tests;

#[test]
fn history_cursor_rejects_invalid_offsets_and_keeps_an_exhausted_source_explicit() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(vec![AssistantToolMessage::User("中文".into())])
        .unwrap();
    let text = json!({"User":"中文"}).to_string();
    let middle = text.find('中').unwrap() + 1;
    for request in [
        AssistantHistoryRequest {
            limit: 0,
            ..Default::default()
        },
        AssistantHistoryRequest {
            limit: 9,
            ..Default::default()
        },
        AssistantHistoryRequest {
            offset: 2,
            ..Default::default()
        },
        AssistantHistoryRequest {
            offset: 1,
            message_offset_bytes: 1,
            ..Default::default()
        },
        AssistantHistoryRequest {
            message_offset_bytes: middle,
            ..Default::default()
        },
        AssistantHistoryRequest {
            message_offset_bytes: usize::MAX,
            ..Default::default()
        },
    ] {
        assert!(session.read_history(request).is_err());
    }
    let end = session
        .read_history(AssistantHistoryRequest {
            offset: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(end["messages"], json!([]));
    assert_eq!(end["nextOffset"], 1);
    assert_eq!(end["nextMessageOffsetBytes"], 0);
    assert_eq!(end["hasMore"], false);
}

#[test]
fn one_long_investigation_compacts_complete_groups_without_losing_pinned_constraints() {
    for evidence in [String::new(), "observed evidence\n".repeat(180)] {
        let store = AssistantSessionStore::default();
        let lease = store.begin(None, binding(), true).unwrap();
        let session = lease.session();
        let context =
            "Repair only server B. Preserve every Mod and its saved data. Do not start server A.";
        session.register_user_request(context).unwrap();
        session.pin_context(context).unwrap();
        let mut messages = vec![AssistantToolMessage::User(context.into())];
        session.replace_messages(messages.clone()).unwrap();
        for index in 0..80 {
            messages.extend(tool_group(&format!("long-{index}"), &evidence));
            if index == 2 {
                messages.push(AssistantToolMessage::User(
                    "The previous tool arguments were rejected; request narrower evidence.".into(),
                ));
            }
            session.replace_messages(messages).unwrap();
            messages = session.messages().unwrap();
            assert!(messages.len() <= WINDOW_MESSAGES);
            assert!(history::business_bytes(&messages).unwrap() <= WINDOW_BYTES);
            assert!(!message_groups(&messages).unwrap().1);
            assert!(
                messages.iter().any(|message| matches!(message,
                AssistantToolMessage::User(text) if text == context)),
                "internal feedback must not replace the task's pinned constraints"
            );
            assert!(messages.iter().any(|message| matches!(message,
                AssistantToolMessage::Assistant(reply) if reply.raw_message["content"][0]["signature"] == format!("signed-long-{index}"))));
        }
        assert_eq!(
            session.revision(),
            1,
            "compaction does not manufacture user turns"
        );
        assert_eq!(session.source_user_requests().unwrap(), vec![context]);
        assert!(session.lock().unwrap().window_start > 2);
        assert_eq!(
            session
                .read_history(AssistantHistoryRequest {
                    limit: 1,
                    ..Default::default()
                })
                .unwrap()["totalMessages"],
            162
        );
        let original = session
            .read_history(AssistantHistoryRequest {
                offset: 1,
                limit: 2,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(
            original["messages"][0]["message"]["Assistant"]["calls"][0]["id"],
            "long-0"
        );
        assert!(
            original["messages"][0]["message"]["Assistant"]
                .get("raw_message")
                .is_none()
        );
        assert_eq!(
            original["messages"][1]["message"]["ToolResult"]["call_id"],
            "long-0"
        );
    }
}

#[test]
fn duplicate_native_ids_remain_rejected_after_the_original_group_is_archived() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(vec![AssistantToolMessage::User("Inspect only.".into())])
        .unwrap();
    session
        .append_messages(tool_group("original", "original evidence"))
        .unwrap();
    for index in 0..40 {
        session
            .append_messages(tool_group(&format!("later-{index}"), "recent evidence"))
            .unwrap();
    }
    assert!(session.lock().unwrap().window_start > 1);
    let before = encoded(&session.lock().unwrap().history).unwrap();
    let error = session
        .append_messages(tool_group("original", "ambiguous replacement"))
        .unwrap_err();
    assert!(error.contains("reused tool call ID"));
    assert_eq!(encoded(&session.lock().unwrap().history).unwrap(), before);
}

#[test]
fn changing_pinned_context_is_bounded_atomic_and_does_not_rewrite_archived_messages() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session.pin_context("Original task constraints").unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Original task constraints".into(),
        )])
        .unwrap();
    for index in 0..40 {
        session
            .append_messages(tool_group(&format!("pin-{index}"), "evidence"))
            .unwrap();
    }
    let original_history = encoded(&session.lock().unwrap().history).unwrap();
    session
        .pin_context("New task: inspect only and retain all settings.")
        .unwrap();
    let current = session.messages().unwrap();
    assert!(
        current
            .iter()
            .any(|message| matches!(message, AssistantToolMessage::User(text)
        if text == "New task: inspect only and retain all settings."))
    );
    assert!(
        !current
            .iter()
            .any(|message| matches!(message, AssistantToolMessage::User(text)
        if text == "Original task constraints"))
    );
    assert!(session.pin_context(&"x".repeat(WINDOW_BYTES)).is_err());
    assert!(session.pin_context(" ").is_err());
    assert_eq!(
        encoded(&session.messages().unwrap()).unwrap(),
        encoded(&current).unwrap()
    );
    assert_eq!(
        encoded(&session.lock().unwrap().history).unwrap(),
        original_history
    );
}

#[test]
fn large_completed_reply_is_archived_intact_and_a_later_tool_turn_can_continue() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let context = "Explain the evidence; do not change any server.";
    session.pin_context(context).unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(context.into())])
        .unwrap();
    let content = "详细诊断内容".repeat(3500);
    let reply = AssistantToolReply {
        content: content.clone(),
        calls: Vec::new(),
        raw_message: json!({"role":"assistant","content":[
            {"type":"thinking","thinking":"retained reasoning","signature":"opaque-signature"},
            {"type":"text","text":content}
        ]}),
    };
    let original = encoded(&reply).unwrap();
    session
        .append_messages(vec![AssistantToolMessage::Assistant(reply)])
        .unwrap();
    let mut window = session.messages().unwrap();
    assert!(history::business_bytes(&window).unwrap() <= WINDOW_BYTES);
    assert!(
        window
            .iter()
            .any(|message| matches!(message, AssistantToolMessage::User(text)
        if text.contains("history index 1") && text.contains("untrusted excerpt")))
    );
    assert_eq!(
        session
            .read_history(AssistantHistoryRequest {
                offset: 1,
                limit: 1,
                ..Default::default()
            })
            .unwrap()["messages"][0]["truncated"],
        true
    );
    window.extend(tool_group("after-long-reply", "new observation"));
    session.replace_messages(window).unwrap();
    assert!(
        session
            .messages()
            .unwrap()
            .iter()
            .any(|message| matches!(message,
        AssistantToolMessage::User(text) if text == context))
    );
    let stored = session.lock().unwrap();
    let AssistantToolMessage::Assistant(reply) = &stored.history[1] else {
        panic!("original reply missing");
    };
    assert_eq!(encoded(reply).unwrap(), original);
    assert!(!message_groups(&stored.history).unwrap().1);
}

#[test]
fn a_new_large_context_archives_previous_evidence_without_duplication_or_data_loss() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(vec![AssistantToolMessage::User("Earlier request".into())])
        .unwrap();
    session
        .append_messages(tool_group("earlier-large-read", &"data".repeat(2000)))
        .unwrap();
    let original = encoded(&session.lock().unwrap().history).unwrap();
    let context = format!("Current task constraints: {}", "x".repeat(20 * 1024));
    session.pin_context(&context).unwrap();
    assert_eq!(encoded(&session.lock().unwrap().history).unwrap(), original);
    let mut messages = session.messages().unwrap();
    messages.push(AssistantToolMessage::User(context.clone()));
    session.replace_messages(messages).unwrap();
    let messages = session.messages().unwrap();
    assert_eq!(
        messages
            .iter()
            .filter(
                |message| matches!(message, AssistantToolMessage::User(text) if text == &context)
            )
            .count(),
        1
    );
    assert!(history::business_bytes(&messages).unwrap() <= WINDOW_BYTES);
    assert!(
        session
            .append_messages(tool_group("cannot-fit", &"data".repeat(2000)))
            .is_err(),
        "a new tool batch must not evict its current task constraints to fit"
    );
}

#[test]
fn native_thinking_uses_its_own_envelope_budget_without_losing_tool_pairs() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .pin_context("Inspect only; retain existing settings.")
        .unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Inspect only; retain existing settings.".into(),
        )])
        .unwrap();
    let thinking = "opaque provider state ".repeat(1700);
    assert!(thinking.len() > WINDOW_BYTES);
    for index in 0..10 {
        let mut group = tool_group(&format!("thinking-{index}"), "one observed value");
        let AssistantToolMessage::Assistant(reply) = &mut group[0] else {
            panic!("missing reply");
        };
        reply.raw_message["content"][0]["thinking"] = json!(thinking);
        session.append_messages(group).unwrap();
        let messages = session.messages().unwrap();
        assert!(history::business_bytes(&messages).unwrap() <= WINDOW_BYTES);
        assert!(encoded(&messages).unwrap().len() > WINDOW_BYTES);
        assert!(encoded(&messages).unwrap().len() <= 256 * 1024);
        assert!(!message_groups(&messages).unwrap().1);
        assert!(messages.iter().any(|message| matches!(message, AssistantToolMessage::Assistant(reply)
            if reply.calls[0].id == format!("thinking-{index}")
                && reply.raw_message["content"][0]["thinking"] == thinking
                && reply.raw_message["content"][0]["signature"] == format!("signed-thinking-{index}"))));
    }
    assert!(
        session.lock().unwrap().window_start > 1,
        "older native envelopes are archived as complete groups"
    );
    let stored = session.lock().unwrap();
    let AssistantToolMessage::Assistant(first) = &stored.history[1] else {
        panic!("first signed reply missing");
    };
    assert_eq!(first.raw_message["content"][0]["thinking"], thinking);
    assert_eq!(
        first.raw_message["content"][0]["signature"],
        "signed-thinking-0"
    );
}
