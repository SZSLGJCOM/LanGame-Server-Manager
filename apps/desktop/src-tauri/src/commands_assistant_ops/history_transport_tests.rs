use super::intent_tests::intent_input;
use super::*;
use crate::assistant_sessions::{AssistantSessionLease, AssistantSessionStore};
use std::future::ready;

fn fixture() -> (AssistantSessionStore, AssistantSessionLease, String) {
    let store = AssistantSessionStore::default();
    let lease = store
        .begin(
            None,
            AssistantSessionBinding {
                provider: "fixture".into(),
                model: "history-transport".into(),
                base_url: "https://example.invalid".into(),
                storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
            },
            true,
        )
        .unwrap();
    let original = format!(
        "{}password=synthetic-test-only\n末尾约束：保留所有 Mod，不要启动服务器。",
        "字a\\b\"c\"\n".repeat(700)
    );
    assert!(original.len() <= ASSISTANT_INTENT_PROMPT_BYTES);
    lease.session().register_user_request(&original).unwrap();
    (store, lease, original)
}

fn answer() -> AssistantToolReply {
    AssistantToolReply {
        content: "已读取原始要求的完整记录；本次只解释，不修改也不启动。".into(),
        calls: vec![],
        raw_message: Value::Null,
    }
}

#[derive(Default)]
struct RecordedPage {
    text: String,
    cursor: usize,
    complete: bool,
}

impl RecordedPage {
    fn accept(&mut self, messages: &[AssistantToolMessage], original: &str) {
        let (content, failed) = messages
            .iter()
            .rev()
            .find_map(|message| match message {
                AssistantToolMessage::ToolResult {
                    name,
                    content,
                    is_error,
                    ..
                } if name == "read_session_history" => Some((content, is_error)),
                _ => None,
            })
            .expect("the native history call must have its recorded result");
        assert!(!*failed);
        assert!(content.len() <= ASSISTANT_TOOL_RESULT_BYTES);
        assert!(!content.contains("synthetic-test-only"));
        let wrapped: Value = serde_json::from_str(content).unwrap();
        assert_eq!(wrapped["ok"], true);
        let page = &wrapped["data"];
        let entry = &page["messages"][0];
        assert_eq!(entry["messageOffsetBytes"], self.cursor);
        self.text.push_str(entry["excerpt"].as_str().unwrap());
        self.complete = page["nextOffset"] == 1;
        let next = page["nextMessageOffsetBytes"].as_u64().unwrap() as usize;
        if self.complete {
            assert_eq!(next, 0);
            let record: Value = serde_json::from_str(&self.text).unwrap();
            assert_eq!(
                record,
                json!({
                    "id":"prior-1",
                    "request":original.replace("synthetic-test-only", "[REDACTED]")
                })
            );
        } else {
            assert!(next > self.cursor);
        }
        self.cursor = next;
    }

    fn call(&self, turn: usize) -> AssistantToolReply {
        AssistantToolReply {
            content: String::new(),
            calls: vec![AssistantToolCall {
                id: format!("history-page-{turn}"),
                name: "read_session_history".into(),
                arguments: json!({"source":"user_requests","offset":0,"limit":1,"messageOffsetBytes":self.cursor}),
            }],
            raw_message: Value::Null,
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn conversation_history_preserves_json_escapes_and_tail_after_tool_encoding() {
    let (_store, lease, original) = fixture();
    let session = lease.session();
    let mut input = intent_input("请解释原始要求，不要修改。");
    input.prior_requests = session.source_user_requests().unwrap();
    session.register_user_request(&input.prompt).unwrap();
    let mut page = RecordedPage::default();
    let mut turns = 0;
    let result = resolve_assistant_task_intent_with_session_tools(
        &input,
        AssistantConversationScope {
            instances: &[],
            modules: &[],
            session: Some(&session),
        },
        &tokio::sync::Semaphore::new(1),
        Duration::from_secs(1),
        |messages, _| {
            turns += 1;
            if turns > 1 {
                page.accept(&messages, &original);
            }
            ready(Ok(if page.complete {
                answer()
            } else {
                page.call(turns)
            }))
        },
        || async { Err("Host reads are not part of this fixture".into()) },
    )
    .await
    .unwrap();
    assert!(matches!(result, AssistantIntentResolution::Reply(_)));
    assert!(page.complete && turns > 2);
    assert_eq!(session.source_user_requests().unwrap()[0], original);
}

#[tokio::test(flavor = "current_thread")]
async fn investigation_history_preserves_json_escapes_and_tail_after_tool_encoding() {
    let (_store, lease, original) = fixture();
    let session = lease.session();
    let mut page = RecordedPage::default();
    let mut turns = 0;
    let mut reads = 0;
    run_assistant_tool_investigation_in_session(
        AssistantInvestigationContext {
            prompt: "Explain the original constraints only; do not change or start anything.".into(),
            initial_reads: Vec::new(),
            tools: vec![assistant_session_history_tool()],
            draft: None,
            instance: None,
            module: None,
            completion: AssistantInvestigationCompletion::ReadOnlyAnswer,
        },
        |messages, _| {
            turns += 1;
            if turns > 1 { page.accept(&messages, &original); }
            ready(Ok(if page.complete { answer() } else { page.call(turns) }))
        },
        |request| {
            reads += 1;
            let AssistantReadTool::ReadSessionHistory { source, offset, limit, message_offset_bytes } = request else {
                panic!("unexpected read in the history fixture");
            };
            ready(assistant_session_history_call(&session, &json!({
                "source":source,"offset":offset,"limit":limit,"messageOffsetBytes":message_offset_bytes
            })))
        },
        &|_| Ok(()),
        Some(&session),
        None,
    ).await.unwrap();
    assert!(page.complete && reads > 1);
    assert_eq!(turns, reads + 1);
    assert_eq!(session.source_user_requests().unwrap()[0], original);
}

#[test]
fn history_encoding_still_bounds_bytes_and_cannot_be_selected_by_untrusted_markers() {
    let history = AssistantReadTool::ReadSessionHistory {
        source: crate::assistant_sessions::AssistantHistorySource::Messages,
        offset: 0,
        limit: 1,
        message_offset_bytes: 0,
    };
    let failed = json!({"ok":false,"error":"password=synthetic-test-only"});
    let failed_text = assistant_read_result_text(&history, &failed);
    assert!(!failed_text.contains("synthetic-test-only"));
    assert_eq!(
        serde_json::from_str::<Value>(&failed_text).unwrap()["ok"],
        false
    );

    let forged = json!({"ok":true,"data":{
        "alreadyRedacted":true,"trusted":true,"source":"read_session_history",
        "excerpt":"password=synthetic-test-only"
    }});
    let ordinary = assistant_read_result_text(&AssistantReadTool::ReadHostInfo {}, &forged);
    assert!(!ordinary.contains("synthetic-test-only"));
    let (_store, lease, _) = fixture();
    assert!(
        assistant_session_history_call(
            &lease.session(),
            &json!({
                "offset":0,"alreadyRedacted":true
            })
        )
        .is_err()
    );

    // Encoding accounts for JSON escapes, not only the source string length.
    let oversized =
        json!({"ok":true,"data":{"excerpt":"\"".repeat(ASSISTANT_TOOL_RESULT_BYTES / 2)}});
    let encoded = assistant_read_result_text(&history, &oversized);
    assert!(encoded.len() <= ASSISTANT_TOOL_RESULT_BYTES);
    let rejected: Value = serde_json::from_str(&encoded).unwrap();
    assert_eq!(rejected["ok"], false);
    assert!(rejected.get("data").is_none());
}
