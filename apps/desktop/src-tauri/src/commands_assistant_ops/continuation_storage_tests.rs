use super::continuation_tests::paused_creation_task;
use super::*;
use std::sync::Arc;
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn checkpoint(
    state: &DesktopState,
    settings: &AssistantProviderSettings,
) -> (
    Arc<AssistantSession>,
    Arc<AssistantTaskContract>,
    AssistantExecuteOperationInput,
) {
    assistant_restore_sessions(state, settings).await.unwrap();
    let lease = state
        .assistant_sessions
        .begin(
            None,
            assistant_session_binding(state, settings).unwrap(),
            true,
        )
        .unwrap();
    let session = lease.session();
    let (task, mut input) = paused_creation_task(session.clone());
    input.settings = settings.clone();
    session.register_user_request(&input.prompt).unwrap();
    assistant_pause_task(
        &input,
        &AssistantOperationMode::ResolvedPreview {
            task: task.clone(),
            target: AssistantIntentTarget::NewInstance,
            conversation_reference: None,
        },
        &task,
    )
    .unwrap();
    (session, task, input)
}

#[tokio::test(flavor = "current_thread")]
async fn storage_transition_rejects_resume_before_consuming_checkpoint_or_allowance() {
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let settings = intent_tests::intent_input("Resume fixture").settings;
    let (session, task, _) = checkpoint(&state, &settings).await;
    let transition = state.begin_storage_context_transition().unwrap();
    let error = assistant_resume_conversation_inner(
        None,
        state.clone(),
        AssistantResumeConversationInput {
            conversation_id: session.id().into(),
            settings: settings.clone(),
        },
    )
    .await
    .unwrap_err();
    assert!(error.contains("paths are being updated"));
    let saved = assistant_continuations().lock().unwrap();
    assert!(Arc::ptr_eq(&saved.get(session.id()).unwrap().task, &task));
    drop(saved);
    let pause = task.run.pause_receipt().unwrap();
    assert_eq!(pause.slices_granted, 1);
    assert_eq!(pause.calls, ASSISTANT_RUN_SLICE_CALLS);
    drop(transition);
    assert!(state.begin_storage_context_transition().is_ok());
    assert!(
        !state
            .assistant_sessions
            .inspect(
                session.id(),
                &assistant_session_binding(&state, &settings).unwrap()
            )
            .unwrap()
            .unwrap()
            .busy
    );
    invalidate_assistant_session_previews(session.id()).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn storage_transition_rejects_confirmation_before_consuming_its_token() {
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let settings = intent_tests::intent_input("Confirm fixture").settings;
    let (session, task, input) = checkpoint(&state, &settings).await;
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateServer,
        module_id: task.module_id.clone(),
        ..assistant_safe_none_plan("Create fixture after confirmation".into())
    };
    let (token, _) =
        store_assistant_pending_operation(&input, plan, None, "Bound preview".into(), 0, task)
            .unwrap();
    let confirm = || AssistantConfirmOperationInput {
        continue_task: false,
        settings: settings.clone(),
        conversation_id: Some(session.id().into()),
        confirmation_token: token.clone(),
        plan_summary: "Bound preview".into(),
    };
    let transition = state.begin_storage_context_transition().unwrap();
    let error = assistant_confirm_operation_with_verification(None, state.clone(), confirm())
        .await
        .unwrap_err();
    assert!(error.contains("paths are being updated"));
    assert!(
        assistant_pending_operations()
            .lock()
            .unwrap()
            .contains_key(&token)
    );
    drop(transition);
    // A rejected binding releases the acquired storage lease on its error path.
    let mut wrong = confirm();
    wrong.conversation_id = Some("different-session".into());
    assert!(
        assistant_confirm_operation_with_verification(None, state.clone(), wrong)
            .await
            .unwrap_err()
            .contains("different conversation")
    );
    assert!(state.begin_storage_context_transition().is_ok());
    invalidate_assistant_session_previews(session.id()).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn resume_releases_storage_lease_after_cancel_and_retained_provider_failure() {
    let (_guard, _environment, app, _) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    for cancel in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut settings = intent_tests::intent_input("Resume fixture").settings;
        settings.base_url = format!("http://{}", listener.local_addr().unwrap());
        let (session, task, _) = checkpoint(&state, &settings).await;
        let observer = async {
            let (mut stream, _) = listener.accept().await.unwrap();
            // Seeing the first HTTP byte proves the real resumed investigation
            // is active. No elapsed-time sleeps or real provider are involved.
            let mut first_byte = [0; 1];
            stream.read_exact(&mut first_byte).await.unwrap();
            assert!(state.begin_storage_context_transition().is_err());
            if cancel {
                assistant_cancel_turn(
                    state.clone(),
                    AssistantConversationControlInput {
                        conversation_id: session.id().into(),
                    },
                )
                .await
                .unwrap();
            } else {
                // A valid HTTP exchange with an invalid native model envelope.
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
                stream.shutdown().await.unwrap();
            }
        };
        let resume = assistant_resume_conversation_inner(
            None,
            state.clone(),
            AssistantResumeConversationInput {
                conversation_id: session.id().into(),
                settings: settings.clone(),
            },
        );
        let (result, ()) = tokio::time::timeout(Duration::from_secs(30), async {
            tokio::join!(resume, observer)
        })
        .await
        .expect("the isolated provider handshake or cancellation did not finish");
        if cancel {
            let error = result.unwrap_err();
            assert!(error.contains("stopped") || error.contains("cancelled"));
            assert!(session.check_active().is_err());
            assert!(
                !assistant_continuations()
                    .lock()
                    .unwrap()
                    .contains_key(session.id())
            );
        } else {
            let output = result.unwrap();
            let pause = output.continuation.unwrap();
            assert_eq!(pause.reason, AssistantRunPauseReason::InvestigationFailed);
            assert_eq!(pause.calls, ASSISTANT_RUN_SLICE_CALLS + 1);
            assert_eq!(pause.slices_granted, 2);
            assert_eq!(pause.operations, 1);
            let saved = assistant_continuations().lock().unwrap();
            assert!(Arc::ptr_eq(
                &saved.get(session.id()).unwrap().task.run,
                &task.run
            ));
        }
        assert!(state.begin_storage_context_transition().is_ok());
        assert!(
            !state
                .assistant_sessions
                .inspect(
                    session.id(),
                    &assistant_session_binding(&state, &settings).unwrap()
                )
                .unwrap()
                .unwrap()
                .busy
        );
        invalidate_assistant_session_previews(session.id()).unwrap();
    }
}
