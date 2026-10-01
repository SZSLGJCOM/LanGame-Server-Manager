use super::*;
use crate::assistant::{AssistantToolCall, AssistantToolReply};

#[test]
fn inspection_is_read_only_and_expiration_cannot_revive_or_rebind_a_session() {
    let store = AssistantSessionStore::default();
    assert!(store.inspect("missing", &binding()).unwrap().is_none());
    assert!(!store.cancel("missing").unwrap());
    assert!(!store.remove("missing").unwrap());
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let active = store.inspect(session.id(), &binding()).unwrap().unwrap();
    assert_eq!(active.revision, 1);
    assert!(active.busy);
    drop(lease);
    let idle_since = session.lock().unwrap().last_used;
    assert!(
        !store
            .inspect(session.id(), &binding())
            .unwrap()
            .unwrap()
            .busy
    );
    assert_eq!(session.lock().unwrap().last_used, idle_since);
    let mut other_provider = binding();
    other_provider.model.push_str("-other");
    assert!(
        store
            .inspect(session.id(), &other_provider)
            .unwrap()
            .is_none()
    );
    assert!(store.inspect(session.id(), &binding()).unwrap().is_some());
    session.lock().unwrap().last_used = Instant::now() - IDLE_TTL;
    assert!(store.inspect(session.id(), &binding()).unwrap().is_none());
    assert!(session.check_active().is_err());
    assert!(!store.cancel(session.id()).unwrap());
    assert!(!store.remove(session.id()).unwrap());
    assert!(store.begin(Some(session.id()), binding(), true).is_err());
}
pub(super) fn binding() -> AssistantSessionBinding {
    AssistantSessionBinding {
        provider: "fixture".into(),
        model: "fixture-model".into(),
        base_url: "https://example.invalid".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-path-{index}")),
    }
}

pub(super) fn tool_group(id: &str, output: &str) -> Vec<AssistantToolMessage> {
    let calls = vec![AssistantToolCall {
        id: id.into(),
        name: "read_runtime".into(),
        arguments: json!({"lines":80}),
    }];
    vec![
        AssistantToolMessage::Assistant(AssistantToolReply {
            content: String::new(),
            calls,
            raw_message: json!({"role":"assistant","content":[
                {"type":"thinking","thinking":"provider payload","signature":format!("signed-{id}")},
                {"type":"tool_use","id":id,"name":"read_runtime","input":{"lines":80}}
            ]}),
        }),
        AssistantToolMessage::ToolResult {
            call_id: id.into(),
            name: "read_runtime".into(),
            content: output.into(),
            is_error: false,
        },
    ]
}

#[test]
fn native_tool_pairs_and_original_requests_survive_user_turns() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .register_user_request("Preserve all current mods.")
        .unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Preserve all current mods.".into(),
        )])
        .unwrap();
    session
        .append_messages(tool_group("first", "crash evidence"))
        .unwrap();
    let original = session.messages().unwrap();
    assert_eq!(session.revision(), 1);
    drop(lease);
    let lease = store.begin(Some(session.id()), binding(), true).unwrap();
    session
        .register_user_request("Continue investigating that crash.")
        .unwrap();
    assert_eq!(session.revision(), 2);
    assert_eq!(
        encoded(&session.messages().unwrap()).unwrap(),
        encoded(&original).unwrap()
    );
    assert_eq!(
        session.prior_user_requests().unwrap(),
        vec!["Preserve all current mods."]
    );
    let mut next = original;
    next.push(AssistantToolMessage::User(
        "Continue investigating that crash.".into(),
    ));
    session.replace_messages(next).unwrap();
    assert_eq!(
        session.revision(),
        2,
        "internal history does not revoke confirmations"
    );
    drop(lease);
    let _confirmation = store.begin(Some(session.id()), binding(), false).unwrap();
    assert_eq!(session.revision(), 2);
}

#[test]
fn binding_and_busy_checks_prevent_cross_scope_replay() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    assert!(store.begin(Some(session.id()), binding(), true).is_err());
    drop(lease);
    for key in 0..9 {
        let mut changed = binding();
        match key {
            0 => changed.provider.push_str("-other"),
            1 => changed.model.push_str("-other"),
            2 => changed.base_url.push_str("/other"),
            _ => changed.storage_identity[key - 3].push_str("-other"),
        }
        assert!(store.begin(Some(session.id()), changed, true).is_err());
    }
    assert_eq!(session.revision(), 1);
    assert!(
        session.append_messages(vec![]).is_err(),
        "mutation needs an exclusive lease"
    );
}

#[tokio::test]
async fn cancellation_wakes_waiters_and_requires_old_lease_to_finish() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    assert!(store.cancel(session.id()).unwrap());
    tokio::time::timeout(Duration::from_millis(100), session.cancelled())
        .await
        .unwrap();
    assert!(session.check_active().is_err());
    assert!(store.begin(Some(session.id()), binding(), true).is_err());
    drop(lease);
    assert!(store.begin(Some(session.id()), binding(), false).is_err());
    let _next = store.begin(Some(session.id()), binding(), true).unwrap();
    assert!(session.check_active().is_ok());
    assert_eq!(session.revision(), 2);
}

#[tokio::test]
async fn cancellation_notification_survives_a_new_turn_clearing_the_flag() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    tokio::time::timeout(Duration::from_millis(100), async {
        tokio::join!(session.cancelled(), async {
            tokio::task::yield_now().await;
            assert!(store.cancel(session.id()).unwrap());
            drop(lease);
            let _next = store.begin(Some(session.id()), binding(), true).unwrap();
            assert!(session.check_active().is_ok());
        });
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn removal_invalidates_owned_arcs_and_pending_work() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(vec![AssistantToolMessage::User("private evidence".into())])
        .unwrap();
    assert!(store.remove(session.id()).unwrap());
    tokio::time::timeout(Duration::from_millis(100), session.cancelled())
        .await
        .unwrap();
    assert!(session.check_active().is_err());
    assert!(session.messages().is_err());
    assert!(session.append_messages(vec![]).is_err());
    assert!(
        session
            .read_history(AssistantHistoryRequest {
                limit: 1,
                ..Default::default()
            })
            .is_err()
    );
    drop(lease);
    assert!(store.begin(Some(session.id()), binding(), true).is_err());
}

#[test]
fn archive_retains_complete_groups_and_exposes_absolute_pagination() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Do not remove existing mods.".into(),
        )])
        .unwrap();
    for index in 0..50 {
        session
            .append_messages(tool_group(
                &format!("call-{index}"),
                &"evidence".repeat(160),
            ))
            .unwrap();
    }
    let window = session.messages().unwrap();
    assert!(window.len() <= WINDOW_MESSAGES);
    assert!(history::business_bytes(&window).unwrap() <= WINDOW_BYTES);
    assert!(
        matches!(&window[0], AssistantToolMessage::User(text) if text.contains("read_session_history"))
    );
    assert!(
        matches!(&window[1], AssistantToolMessage::User(text) if text == "Do not remove existing mods.")
    );
    assert!(!message_groups(&window).unwrap().1);
    let page = session
        .read_history(AssistantHistoryRequest {
            offset: 1,
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page["offset"], 1);
    assert_eq!(page["nextOffset"], 3);
    assert_eq!(page["totalMessages"], 101);
    assert!(page["archivedBefore"].as_u64().unwrap() > 1);
    assert_eq!(
        page["messages"][0]["message"]["Assistant"]["calls"][0]["id"],
        "call-0"
    );
    let mut resumed = window;
    resumed.push(AssistantToolMessage::User("Next user turn".into()));
    session.replace_messages(resumed).unwrap();
    assert_eq!(
        session
            .read_history(AssistantHistoryRequest {
                offset: 1,
                limit: 2,
                ..Default::default()
            })
            .unwrap()["messages"],
        page["messages"]
    );
}

#[test]
fn partial_tool_batches_wait_for_results_and_invalid_appends_are_atomic() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let mut group = tool_group("pending", "done");
    let result = group.pop().unwrap();
    session.append_messages(group).unwrap();
    assert!(session.messages().is_err());
    assert!(
        session
            .append_messages(vec![AssistantToolMessage::User("interrupt".into())])
            .is_err()
    );
    session.append_messages(vec![result]).unwrap();
    let before = session.messages().unwrap();
    assert!(
        session
            .append_messages(tool_group("pending", "duplicate"))
            .is_err()
    );
    assert!(session.replace_messages(vec![]).is_err());
    assert!(
        session
            .append_messages(vec![AssistantToolMessage::User("x".repeat(WINDOW_BYTES))])
            .is_err()
    );
    assert_eq!(
        encoded(&session.messages().unwrap()).unwrap(),
        encoded(&before).unwrap()
    );
}

#[test]
fn limits_reject_growth_without_deleting_evidence_or_evicting_active_sessions() {
    let store = AssistantSessionStore::default();
    let leases: Vec<_> = (0..SESSION_LIMIT)
        .map(|_| store.begin(None, binding(), true).unwrap())
        .collect();
    assert!(store.begin(None, binding(), true).is_err());
    let session = leases[0].session();
    session.register_user_request("retained request").unwrap();
    assert!(
        session
            .register_user_request(&"x".repeat(HISTORY_BYTES))
            .is_err()
    );
    assert_eq!(
        session.source_user_requests().unwrap(),
        vec!["retained request"]
    );
    let idle = leases.into_iter().next().unwrap();
    let old = idle.session();
    drop(idle);
    // All dropped leases are idle; expiration invalidates every old Arc.
    old.lock().unwrap().last_used = Instant::now() - IDLE_TTL;
    let _new = store.begin(None, binding(), true).unwrap();
    assert!(old.check_active().is_err());
    assert!(store.begin(Some(old.id()), binding(), true).is_err());
}

#[test]
fn large_archive_reads_explicitly_mark_excerpts_and_keep_stored_payload_intact() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    let text = "配置证据".repeat(900);
    session
        .append_messages(vec![AssistantToolMessage::User(text.clone())])
        .unwrap();
    let page = session
        .read_history(AssistantHistoryRequest {
            limit: 8,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page["messages"][0]["truncated"], true);
    assert_eq!(page["nativeReplay"], false);
    assert!(encoded(&page).unwrap().len() <= PAGE_BYTES);
    assert!(
        matches!(&session.messages().unwrap()[0], AssistantToolMessage::User(actual) if actual == &text)
    );
}

#[test]
fn message_and_user_source_limits_preserve_the_previous_snapshot() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    session
        .append_messages(
            (0..HISTORY_MESSAGES)
                .map(|index| AssistantToolMessage::User(format!("request-{index}")))
                .collect(),
        )
        .unwrap();
    let previous = session.messages().unwrap();
    assert!(
        session
            .append_messages(vec![AssistantToolMessage::User("overflow".into())])
            .is_err()
    );
    assert_eq!(
        session
            .read_history(AssistantHistoryRequest {
                limit: 1,
                ..Default::default()
            })
            .unwrap()["totalMessages"],
        HISTORY_MESSAGES
    );
    assert_eq!(
        encoded(&session.messages().unwrap()).unwrap(),
        encoded(&previous).unwrap()
    );
    let other = store.begin(None, binding(), true).unwrap().session();
    // An Arc does not itself extend a mutation lease.
    assert!(other.register_user_request("no lease").is_err());
    let _lease = store.begin(Some(other.id()), binding(), true).unwrap();
    for index in 0..128 {
        other
            .register_user_request(&format!("source-{index}"))
            .unwrap();
    }
    assert!(other.register_user_request("source overflow").is_err());
    assert_eq!(other.source_user_requests().unwrap().len(), 128);
}

#[test]
fn capacity_eviction_invalidates_only_an_idle_session() {
    let store = AssistantSessionStore::default();
    let mut leases: Vec<_> = (0..SESSION_LIMIT)
        .map(|_| store.begin(None, binding(), true).unwrap())
        .collect();
    let idle = leases.pop().unwrap();
    let evicted = idle.session();
    drop(idle);
    let _replacement = store.begin(None, binding(), true).unwrap();
    assert!(evicted.check_active().is_err());
    assert!(
        leases
            .iter()
            .all(|lease| lease.session().check_active().is_ok())
    );
}

#[test]
fn cancellation_keeps_committed_write_results_but_removal_rejects_late_evidence() {
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, binding(), true).unwrap();
    let session = lease.session();
    store.cancel(session.id()).unwrap();
    assert!(
        session
            .append_messages(tool_group("ordinary", "must not execute"))
            .is_err()
    );
    let group = tool_group("committed", "saved and readback verified");
    assert!(
        session
            .append_completed_operation(vec![group[0].clone()])
            .is_err()
    );
    session.append_completed_operation(group.clone()).unwrap();
    assert!(
        session.check_active().is_err(),
        "recording a result does not resume execution"
    );
    drop(lease);
    assert!(session.append_completed_operation(vec![]).is_err());
    let _next = store.begin(Some(session.id()), binding(), true).unwrap();
    assert_eq!(
        encoded(&session.messages().unwrap()).unwrap(),
        encoded(&group).unwrap()
    );
    assert!(store.remove(session.id()).unwrap());
    assert!(
        session
            .append_completed_operation(tool_group("late", "must remain cleared"))
            .is_err()
    );
    assert!(session.lock().unwrap().history.is_empty());
}
