use super::*;
use crate::assistant_sessions::AssistantSessionSnapshot;
use tauri::Manager;

struct StateArchiveDirectory(PathBuf);

impl Drop for StateArchiveDirectory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn assistant_state_does_not_infer_completion_from_an_idle_or_missing_session() {
    let id = uuid::Uuid::new_v4().to_string();
    let missing = assistant_conversation_state(&id, None).unwrap();
    assert_eq!(missing.status, AssistantConversationStatus::Unavailable);
    assert_eq!(missing.revision, None);
    assert!(missing.continuation.is_none());
    let idle = assistant_conversation_state(
        &id,
        Some(AssistantSessionSnapshot {
            revision: 3,
            busy: false,
        }),
    )
    .unwrap();
    assert_eq!(idle.status, AssistantConversationStatus::Idle);
    assert_eq!(idle.revision, Some(3));
    assert!(idle.continuation.is_none());
    let running = assistant_conversation_state(
        &id,
        Some(AssistantSessionSnapshot {
            revision: 3,
            busy: true,
        }),
    )
    .unwrap();
    assert_eq!(running.status, AssistantConversationStatus::Running);
    assert!(running.continuation.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_state_command_observes_identity_and_does_not_start_or_renew_a_turn() {
    let archive_directory = StateArchiveDirectory(std::env::temp_dir().join(format!(
        "lgsm-session-state-{}",
        uuid::Uuid::new_v4().simple()
    )));
    std::fs::create_dir_all(&archive_directory.0).unwrap();
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let state = app.state::<DesktopState>();
    state.app_state.write().unwrap().storage.database_path = archive_directory
        .0
        .join("state.sqlite")
        .to_string_lossy()
        .into_owned();
    let settings = intent_tests::intent_input("State observation").settings;
    let created = assistant_create_conversation(
        state.clone(),
        AssistantConversationCreateInput {
            settings: settings.clone(),
        },
    )
    .await
    .unwrap();
    let input = || AssistantConversationStateInput {
        conversation_id: created.conversation_id.clone(),
        settings: settings.clone(),
        after_cursor: None,
    };
    let observed = assistant_get_conversation_state(state.clone(), input())
        .await
        .unwrap();
    assert_eq!(observed.status, AssistantConversationStatus::Idle);
    assert_eq!(observed.revision, Some(0));
    let mut changed = input();
    changed.settings.model.push_str("-different");
    assert_eq!(
        assistant_get_conversation_state(state.clone(), changed)
            .await
            .unwrap()
            .status,
        AssistantConversationStatus::Unavailable
    );
    let lease = state
        .assistant_sessions
        .begin(
            Some(&created.conversation_id),
            assistant_session_binding(&state, &settings).unwrap(),
            true,
        )
        .unwrap();
    let observed = assistant_get_conversation_state(state.clone(), input())
        .await
        .unwrap();
    assert_eq!(observed.status, AssistantConversationStatus::Running);
    assert_eq!(observed.revision, Some(1));
    drop(lease);
    assert_eq!(
        assistant_get_conversation_state(state.clone(), input())
            .await
            .unwrap()
            .status,
        AssistantConversationStatus::Idle
    );
    for _ in 0..2 {
        let deleted = assistant_delete_conversation(
            state.clone(),
            AssistantConversationControlInput {
                conversation_id: created.conversation_id.clone(),
            },
        )
        .await
        .unwrap();
        assert!(!deleted.stopping);
    }
    let gone = assistant_get_conversation_state(state.clone(), input())
        .await
        .unwrap();
    assert_eq!(gone.status, AssistantConversationStatus::Unavailable);
    let stopped = assistant_cancel_turn(
        state,
        AssistantConversationControlInput {
            conversation_id: created.conversation_id,
        },
    )
    .await
    .unwrap();
    assert!(!stopped.stopping);
}
