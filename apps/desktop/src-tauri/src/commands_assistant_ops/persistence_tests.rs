use super::*;

struct ArchiveFixture {
    root: PathBuf,
    binding: crate::assistant_sessions::AssistantSessionBinding,
}

impl ArchiveFixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lgsm-task-recovery-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        Self {
            binding: crate::assistant_sessions::AssistantSessionBinding {
                provider: "openai-compatible".into(),
                model: "fixture-model".into(),
                base_url: "https://example.invalid".into(),
                storage_identity: std::array::from_fn(|index| {
                    if index == 4 {
                        root.join("state.sqlite").to_string_lossy().into_owned()
                    } else {
                        format!("fixture-{index}")
                    }
                }),
            },
            root,
        }
    }
}

impl Drop for ArchiveFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn restored_task_preserves_constraints_receipts_and_lifetime_allowance_without_replaying_a_write()
 {
    let fixture = ArchiveFixture::new();
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    store.restore(&fixture.binding).await.unwrap();
    let lease = store.begin(None, fixture.binding.clone(), true).unwrap();
    let session = lease.session();
    session
        .register_user_request("Create an empty instance without starting it.")
        .unwrap();
    let (mut task, input) = continuation_tests::paused_creation_task(session.clone());
    let original = std::sync::Arc::make_mut(&mut task);
    original
        .file_changes
        .push(app_storage::InstanceFilePatchResult {
            file: "data/mods/example/modmain.lua".into(),
            source_sha256: "a".repeat(64),
            result_sha256: "b".repeat(64),
            backup_id: "recorded-backup".into(),
            read_back_verified: true,
        });
    let expected_id = original.id.clone();
    let expected_settings = original.initial_settings.clone();
    assistant_checkpoint_target(
        &task,
        &AssistantOperationMode::ResolvedPreview {
            task: task.clone(),
            target: AssistantIntentTarget::NewInstance,
            conversation_reference: None,
        },
    )
    .unwrap();
    assistant_checkpoint_task(&task).await.unwrap();
    drop(lease);
    drop(store);
    let reopened = crate::assistant_sessions::AssistantSessionStore::default();
    reopened.restore(&fixture.binding).await.unwrap();
    let restored_lease = reopened
        .begin(Some(session.id()), fixture.binding.clone(), false)
        .unwrap();
    let restored_session = restored_lease.session();
    assistant_restore_task_checkpoint(&restored_session, &input.settings).unwrap();
    let saved = assistant_continuations()
        .lock()
        .unwrap()
        .remove(session.id())
        .unwrap();
    assert_eq!(saved.task.id, expected_id);
    assert_eq!(saved.task.initial_settings, expected_settings);
    assert_eq!(
        saved.task.original_request,
        "Create an empty instance without starting it."
    );
    assert!(saved.task.request.preserve_existing_mods);
    assert_eq!(saved.task.file_changes[0].backup_id, "recorded-backup");
    assert!(saved.input.settings.api_key.is_empty());
    assert!(matches!(
        saved.mode,
        AssistantOperationMode::ResolvedPreview {
            target: AssistantIntentTarget::NewInstance,
            ..
        }
    ));
    let before = saved.task.run.pause_receipt().unwrap();
    assert_eq!(before.calls, ASSISTANT_RUN_SLICE_CALLS);
    assert_eq!(before.operations, 1);
    saved.task.run.grant_continuation().unwrap();
    assert_eq!(
        saved
            .task
            .run
            .reserve_operation("previous-plan:previous-state")
            .unwrap_err()
            .reason,
        AssistantRunPauseReason::RepeatedOperation
    );
    drop(saved.task.run.reserve_read().unwrap());
    assert_eq!(
        saved.task.run.checkpoint().unwrap().calls,
        ASSISTANT_RUN_SLICE_CALLS
    );
    assert_eq!(saved.task.run.checkpoint().unwrap().reads, 1);
}

#[tokio::test]
async fn secret_redaction_never_weakens_a_recovered_baseline_or_requirement() {
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let binding = crate::assistant_sessions::AssistantSessionBinding {
        provider: "fixture".into(),
        model: "fixture".into(),
        base_url: "https://example.invalid".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
    };
    let lease = store.begin(None, binding, true).unwrap();
    let session = lease.session();
    let (mut task, _) = continuation_tests::paused_creation_task(session.clone());
    std::sync::Arc::make_mut(&mut task).initial_settings[format!("{}_{}", "rcon", "password")] =
        json!(format!("{}-{}", "fixture", "private-value"));
    assistant_checkpoint_task(&task).await.unwrap();
    let value = session.checkpoint().unwrap().unwrap();
    assert_eq!(value["requires_restatement"], true);
    assert!(!value.to_string().contains("fixture-private-value"));
    let saved: AssistantTaskCheckpoint = serde_json::from_value(value).unwrap();
    let restored =
        AssistantTaskRun::from_checkpoint(saved.budget, saved.requires_restatement).unwrap();
    assert!(!restored.pause_receipt().unwrap().can_resume);
    assert!(restored.grant_continuation().is_err());
}

#[test]
fn interrupted_work_cannot_regain_time_while_idle_checkpoints_do_not_charge_downtime() {
    let run = std::sync::Arc::new(AssistantTaskRun::default());
    let work = run.reserve_model().unwrap();
    let mut interrupted = run.checkpoint().unwrap();
    interrupted.saved_unix_ms = interrupted
        .saved_unix_ms
        .saturating_sub(ASSISTANT_RUN_WORK_TIME.as_millis() as u64 + 1);
    let restored = AssistantTaskRun::from_checkpoint(interrupted, false).unwrap();
    assert_eq!(
        restored.pause_receipt().unwrap().reason,
        AssistantRunPauseReason::WorkTime
    );
    assert!(restored.grant_continuation().is_err());
    drop(work);
    let mut idle = run.checkpoint().unwrap();
    idle.saved_unix_ms = idle.saved_unix_ms.saturating_sub(24 * 60 * 60 * 1000);
    let restored = AssistantTaskRun::from_checkpoint(idle, false).unwrap();
    assert!(restored.pause_receipt().unwrap().can_resume);
    restored.grant_continuation().unwrap();
    assert_eq!(restored.checkpoint().unwrap().calls, 1);
}
