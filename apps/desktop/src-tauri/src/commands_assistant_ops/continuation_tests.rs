use super::*;
use crate::assistant_sessions::AssistantSessionStore;
use std::sync::Arc;

pub(super) fn paused_creation_task(
    session: Arc<AssistantSession>,
) -> (Arc<AssistantTaskContract>, AssistantExecuteOperationInput) {
    let (mut task, _) = task_tests::task_fixture(true);
    let contract = Arc::make_mut(&mut task);
    contract.session = Some(session);
    contract.instance_id = None;
    contract.request.goal = AssistantTaskGoal::ApplyChange;
    contract.original_request = "Create an empty instance without starting it.".into();
    let input = AssistantExecuteOperationInput {
        settings: intent_tests::intent_input("fixture").settings,
        prompt: contract.original_request.clone(),
        task: contract.request.clone(),
        context: None,
        selected_instance_id: None,
        selected_module_id: contract.module_id.clone(),
    };
    // A prior attempted write must remain remembered after a slice is renewed.
    drop(
        task.run
            .reserve_operation("previous-plan:previous-state")
            .unwrap(),
    );
    for _ in 0..ASSISTANT_RUN_SLICE_CALLS {
        drop(task.run.reserve_model().unwrap());
    }
    assert_eq!(
        task.run.reserve_model().unwrap_err().reason,
        AssistantRunPauseReason::ModelSlice
    );
    (task, input)
}

#[tokio::test(flavor = "current_thread")]
async fn evicted_sessions_release_checkpoint_capacity_for_the_next_paused_task() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    let store = AssistantSessionStore::default();
    let mut ids = Vec::new();
    let mut first_session = None;
    for index in 0..17 {
        let lease = store.begin(None, session_binding(), true).unwrap();
        let session = lease.session();
        if index == 0 {
            first_session = Some(session.clone());
        }
        let (task, input) = paused_creation_task(session.clone());
        let mode = AssistantOperationMode::ResolvedPreview {
            task: task.clone(),
            target: AssistantIntentTarget::NewInstance,
            conversation_reference: None,
        };
        let output = assistant_pause_task(&input, &mode, &task).unwrap();
        assert!(output.continuation.unwrap().can_resume);
        ids.push(session.id().to_owned());
    }
    assert!(first_session.unwrap().check_active().is_err());
    let mut checkpoints = assistant_continuations().lock().unwrap();
    assert!(!checkpoints.contains_key(&ids[0]));
    assert_eq!(
        ids.iter()
            .filter(|id| checkpoints.contains_key(*id))
            .count(),
        16
    );
    assert!(checkpoints.get(&ids[16]).unwrap().is_current());
    let next_turn = store
        .begin(Some(&ids[15]), session_binding(), true)
        .unwrap();
    assert!(!checkpoints.get(&ids[15]).unwrap().is_current());
    drop(next_turn);
    store.cancel(&ids[16]).unwrap();
    assert!(!checkpoints.get(&ids[16]).unwrap().is_current());
    for id in ids {
        checkpoints.remove(&id);
    }
}

fn session_binding() -> AssistantSessionBinding {
    AssistantSessionBinding {
        provider: "ollama".into(),
        model: "fixture".into(),
        base_url: "http://127.0.0.1:11434".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-path-{index}")),
    }
}

fn assert_creation_checkpoint(
    session: &AssistantSession,
    original: &Arc<AssistantTaskContract>,
    output: &AssistantExecuteOperationOutput,
) -> AssistantTaskContinuation {
    let pause = output.continuation.as_ref().unwrap();
    assert_eq!(pause.reason, AssistantRunPauseReason::ModelSlice);
    assert!(pause.can_resume);
    assert_eq!(pause.calls, ASSISTANT_RUN_SLICE_CALLS);
    assert_eq!(pause.operations, 1);
    let saved = assistant_continuations()
        .lock()
        .unwrap()
        .remove(session.id())
        .unwrap();
    assert_eq!(saved.revision, session.revision());
    assert!(Arc::ptr_eq(&saved.task, original));
    assert!(Arc::ptr_eq(&saved.task.run, &original.run));
    assert!(Arc::ptr_eq(
        saved.task.session.as_ref().unwrap(),
        original.session.as_ref().unwrap()
    ));
    assert!(saved.input.settings.api_key.is_empty());
    assert_eq!(saved.input.prompt, original.original_request);
    let AssistantOperationMode::ResolvedPreview { task, target, .. } = &saved.mode else {
        panic!("paused creation must return to a preview, never replay a consumed confirmation");
    };
    assert_eq!(*target, AssistantIntentTarget::NewInstance);
    assert!(Arc::ptr_eq(task, original));
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateServer,
        module_id: original.module_id.clone(),
        ..assistant_safe_none_plan("Create the requested empty instance.".into())
    };
    validate_assistant_resolved_target(task, *target, &plan).unwrap();
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
    for _ in 0..ASSISTANT_RUN_SLICE_CALLS {
        drop(saved.task.run.reserve_model().unwrap());
    }
    let next = saved.task.run.reserve_model().unwrap_err();
    assert_eq!(next.calls, ASSISTANT_RUN_SLICE_CALLS * 2);
    assert_eq!(next.operations, 1);
    assert_eq!(next.slices_granted, 2);
    saved
}

#[tokio::test(flavor = "current_thread")]
async fn continuation_preserves_explicit_creation_scope_in_apply_change_preview() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, session_binding(), true).unwrap();
    let session = lease.session();
    let (task, input) = paused_creation_task(session.clone());
    let mode = AssistantOperationMode::ResolvedPreview {
        task: task.clone(),
        target: AssistantIntentTarget::NewInstance,
        conversation_reference: Some("Earlier clarification remains evidence.".into()),
    };
    let output = assistant_pause_task(&input, &mode, &task).unwrap();
    let saved = assert_creation_checkpoint(&session, &task, &output);
    let AssistantOperationMode::ResolvedPreview {
        conversation_reference,
        ..
    } = saved.mode
    else {
        unreachable!()
    };
    assert_eq!(
        conversation_reference.as_deref(),
        Some("Earlier clarification remains evidence.")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn continuation_of_confirmed_creation_repreviews_same_target_with_shared_budget() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, session_binding(), true).unwrap();
    let session = lease.session();
    let (task, input) = paused_creation_task(session.clone());
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateServer,
        module_id: task.module_id.clone(),
        ..assistant_safe_none_plan("Create the requested empty instance.".into())
    };
    let (token, _) = store_assistant_pending_operation(
        &input,
        plan,
        None,
        "Create preview".into(),
        0,
        task.clone(),
    )
    .unwrap();
    let pending =
        take_assistant_pending_operation(&token, "Create preview", &input.settings).unwrap();
    let mode = AssistantOperationMode::Confirmed(Box::new(pending));
    let output = assistant_pause_task(&input, &mode, &task).unwrap();
    assert_creation_checkpoint(&session, &task, &output);
    assert!(!output.requires_confirmation);
    assert!(output.confirmation_token.is_none());
    assert!(take_assistant_pending_operation(&token, "Create preview", &input.settings).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_failed_continuation_retains_task_evidence_and_budget_for_explicit_retry() {
    let _guard = crate::commands::tests::command_smoke_lock().lock().await;
    let store = AssistantSessionStore::default();
    let lease = store.begin(None, session_binding(), true).unwrap();
    let session = lease.session();
    let (task, mut input) = paused_creation_task(session.clone());
    task.run.grant_continuation().unwrap();
    drop(task.run.reserve_model().unwrap());
    session
        .append_messages(vec![crate::assistant::AssistantToolMessage::User(
            "Retained native investigation evidence.".into(),
        )])
        .unwrap();
    input.settings.api_key = "synthetic-test-only".into();
    let mode = AssistantOperationMode::ResolvedPreview {
        task: task.clone(),
        target: AssistantIntentTarget::NewInstance,
        conversation_reference: Some("Keep earlier constraints.".into()),
    };
    let output =
        assistant_retain_failed_continuation(&input, &mode, &task, "Provider timed out.").unwrap();
    assert!(!output.requires_confirmation);
    assert!(output.confirmation_token.is_none());
    assert!(output.message.contains("Provider timed out."));
    let pause = output.continuation.unwrap();
    assert_eq!(pause.reason, AssistantRunPauseReason::InvestigationFailed);
    assert_eq!(pause.calls, ASSISTANT_RUN_SLICE_CALLS + 1);
    assert_eq!(pause.slice_calls, 1);
    assert_eq!(pause.operations, 1);
    let running = assistant_conversation_state(
        session.id(),
        Some(crate::assistant_sessions::AssistantSessionSnapshot {
            revision: session.revision(),
            busy: true,
        }),
    )
    .unwrap();
    assert_eq!(running.status, AssistantConversationStatus::Running);
    let paused = assistant_conversation_state(
        session.id(),
        Some(crate::assistant_sessions::AssistantSessionSnapshot {
            revision: session.revision(),
            busy: false,
        }),
    )
    .unwrap();
    assert_eq!(paused.status, AssistantConversationStatus::Paused);
    assert!(paused.continuation.unwrap().can_resume);
    let stale = assistant_conversation_state(
        session.id(),
        Some(crate::assistant_sessions::AssistantSessionSnapshot {
            revision: session.revision() + 1,
            busy: false,
        }),
    )
    .unwrap();
    assert_eq!(stale.status, AssistantConversationStatus::Idle);
    let saved = assistant_continuations()
        .lock()
        .unwrap()
        .remove(session.id())
        .unwrap();
    assert!(Arc::ptr_eq(&saved.task, &task));
    assert!(saved.input.settings.api_key.is_empty());
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
    assert!(
        session
            .messages()
            .unwrap()
            .iter()
            .any(|message| matches!(message,
        crate::assistant::AssistantToolMessage::User(text) if text.contains("Retained native")))
    );
    store.cancel(session.id()).unwrap();
    assert!(assistant_retain_failed_continuation(&input, &mode, &task, "Stopped.").is_err());
    assert!(
        !assistant_continuations()
            .lock()
            .unwrap()
            .contains_key(session.id())
    );
}
