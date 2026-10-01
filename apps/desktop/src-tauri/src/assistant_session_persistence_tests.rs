use super::*;

struct Fixture {
    root: std::path::PathBuf,
    binding: AssistantSessionBinding,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-assistant-archive-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut binding = crate::assistant_sessions::tests::binding();
        binding.storage_identity[4] = root.join("state.sqlite").to_string_lossy().into_owned();
        Self { root, binding }
    }

    fn document(&self, id: &str) -> std::path::PathBuf {
        self.root
            .join("assistant-sessions")
            .join(format!("{id}.json"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn restart_restores_original_requests_and_tool_receipts_without_opaque_provider_data() {
    let fixture = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    session
        .register_user_request("Inspect the logs; keep all installed Mods.")
        .unwrap();
    session
        .append_messages(vec![AssistantToolMessage::User(
            "Inspect the logs; keep all installed Mods.".into(),
        )])
        .unwrap();
    session
        .append_messages(crate::assistant_sessions::tests::tool_group(
            "read-1",
            "A missing library was reported.",
        ))
        .unwrap();
    session
        .set_checkpoint(Some(json!({"task":"inspect"})))
        .unwrap();
    session.flush().await.unwrap();
    let bytes = std::fs::read(fixture.document(session.id())).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("provider payload"));
    assert!(!text.contains("signed-read-1"));
    drop(lease);
    drop(store);

    let restored = AssistantSessionStore::default();
    restored.restore(&fixture.binding).await.unwrap();
    let reopened = restored
        .begin(Some(session.id()), fixture.binding.clone(), false)
        .unwrap();
    let reopened = reopened.session();
    assert!(reopened.needs_recovery());
    assert_eq!(reopened.revision(), 1);
    assert_eq!(
        reopened.source_user_requests().unwrap(),
        vec!["Inspect the logs; keep all installed Mods."]
    );
    assert!(
        encoded(&reopened.messages().unwrap())
            .unwrap()
            .windows(15)
            .any(|part| part == b"missing library")
    );
    assert_eq!(reopened.checkpoint().unwrap().unwrap()["task"], "inspect");
    let mut wrong = fixture.binding.clone();
    wrong.model.push_str("-different");
    assert!(restored.inspect(session.id(), &wrong).unwrap().is_none());
    wrong = fixture.binding.clone();
    wrong.storage_identity[0].push_str("-different");
    assert!(restored.inspect(session.id(), &wrong).unwrap().is_none());
}

#[tokio::test]
async fn interrupted_tool_results_are_unknown_and_never_reported_unexecuted_or_successful() {
    let fixture = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    let mut batch = crate::assistant_sessions::tests::tool_group("interrupted", "unused");
    batch.pop();
    session.append_messages(batch).unwrap();
    session.flush().await.unwrap();
    drop(lease);
    let restored = AssistantSessionStore::default();
    restored.restore(&fixture.binding).await.unwrap();
    let reopened = restored
        .begin(Some(session.id()), fixture.binding.clone(), false)
        .unwrap();
    let messages = reopened.session().messages().unwrap();
    let Some(AssistantToolMessage::ToolResult {
        content, is_error, ..
    }) = messages.last()
    else {
        panic!("missing recovery tool result");
    };
    assert!(*is_error);
    let evidence: Value = serde_json::from_str(content).unwrap();
    assert_eq!(evidence["outcome"], "unknown_after_restart");
    assert!(evidence.get("executed").is_none());
    assert_eq!(evidence["ok"], false);
}

#[tokio::test]
async fn cancel_and_delete_survive_restart_even_without_loading_the_conversation() {
    let fixture = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    session
        .set_checkpoint(Some(json!({"task":"pending"})))
        .unwrap();
    session.flush().await.unwrap();
    drop(lease);
    let unopened = AssistantSessionStore::default();
    unopened
        .cancel_persisted(
            session.id(),
            std::path::Path::new(&fixture.binding.storage_identity[4]),
        )
        .await
        .unwrap();
    let restored = AssistantSessionStore::default();
    restored.restore(&fixture.binding).await.unwrap();
    assert!(
        restored
            .begin(Some(session.id()), fixture.binding.clone(), false)
            .is_err()
    );
    let resumed = restored
        .begin(Some(session.id()), fixture.binding.clone(), true)
        .unwrap();
    assert!(resumed.session().checkpoint().unwrap().is_none());
    let removed_session = resumed.session();
    drop(resumed);
    restored
        .delete_persisted(
            session.id(),
            std::path::Path::new(&fixture.binding.storage_identity[4]),
        )
        .await
        .unwrap();
    removed_session.flush().await.unwrap();
    assert!(!fixture.document(session.id()).exists());
    let deleted = AssistantSessionStore::default();
    deleted.restore(&fixture.binding).await.unwrap();
    assert!(
        deleted
            .inspect(session.id(), &fixture.binding)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn archive_survives_overnight_and_isolates_a_damaged_record() {
    let fixture = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    session.flush().await.unwrap();
    let path = fixture.document(session.id());
    let mut document: SessionDocument =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    document.saved_unix_ms -= 24 * 60 * 60 * 1000;
    std::fs::write(&path, encoded(&document).unwrap()).unwrap();
    let corrupt_id = uuid::Uuid::new_v4().simple().to_string();
    std::fs::write(fixture.document(&corrupt_id), b"{broken").unwrap();
    let restored = AssistantSessionStore::default();
    restored.restore(&fixture.binding).await.unwrap();
    assert!(
        restored
            .inspect(session.id(), &fixture.binding)
            .unwrap()
            .is_some()
    );
    assert!(
        restored
            .inspect(&corrupt_id, &fixture.binding)
            .unwrap_err()
            .contains("damaged")
    );
    assert_eq!(restored.list_bound(&fixture.binding).unwrap().len(), 1);
}

#[tokio::test]
async fn archive_redacts_credentials_and_public_transcript_excludes_internal_context() {
    let fixture = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    session.register_user_request("hello").unwrap();
    let secret = format!("{}={}", "api_key", "fixture-sensitive-value");
    session
        .append_messages(vec![
            AssistantToolMessage::User("internal catalog must not appear".into()),
            AssistantToolMessage::User("hello".into()),
        ])
        .unwrap();
    session
        .append_messages(crate::assistant_sessions::tests::tool_group(
            "redacted", &secret,
        ))
        .unwrap();
    session.flush().await.unwrap();
    let text = std::fs::read_to_string(fixture.document(session.id())).unwrap();
    assert!(!text.contains("fixture-sensitive-value"));
    let (messages, truncated) = session.public_messages().unwrap();
    assert!(!truncated);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "hello");
}

#[tokio::test]
async fn switching_storage_contexts_reloads_evicted_sessions_without_mixing_history() {
    let first = Fixture::new();
    let second = Fixture::new();
    let store = AssistantSessionStore::default();
    store.restore(&first.binding).await.unwrap();
    let mut first_ids = Vec::new();
    for index in 0..SESSION_LIMIT {
        let lease = store.begin(None, first.binding.clone(), true).unwrap();
        let session = lease.session();
        session
            .register_user_request(&format!("first storage request {index}"))
            .unwrap();
        session.flush().await.unwrap();
        first_ids.push(session.id().to_owned());
    }
    store.restore(&second.binding).await.unwrap();
    let lease = store.begin(None, second.binding.clone(), true).unwrap();
    let second_id = lease.session().id().to_owned();
    lease
        .session()
        .register_user_request("second storage request")
        .unwrap();
    lease.session().flush().await.unwrap();
    drop(lease);
    store.restore(&first.binding).await.unwrap();
    let first_list = store.list_bound(&first.binding).unwrap();
    assert_eq!(first_list.len(), SESSION_LIMIT);
    assert!(
        first_list
            .iter()
            .all(|item| first_ids.contains(&item.conversation_id))
    );
    assert!(
        store
            .get_bound(&second_id, &first.binding)
            .unwrap()
            .is_none()
    );
    store.restore(&second.binding).await.unwrap();
    assert_eq!(
        store.list_bound(&second.binding).unwrap()[0].title,
        "second storage request"
    );
}
