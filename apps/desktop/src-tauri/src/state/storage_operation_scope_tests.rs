use super::*;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn fixed_application_data_can_continue_and_start_during_a_path_transition() {
    let state = DesktopState::default();
    let knowledge = state
        .begin_fixed_application_data_operation("official game knowledge sync")
        .unwrap();
    let worker = knowledge.clone();
    drop(knowledge);
    let transition = state.begin_storage_context_transition().unwrap();
    let status = state
        .begin_fixed_application_data_operation("knowledge status read")
        .unwrap();
    assert!(
        state
            .begin_storage_context_operation("instance creation")
            .is_err()
    );
    assert!(!worker.cancellation_token().load(Ordering::SeqCst));
    drop(transition);
    let operation = state
        .begin_storage_context_operation("instance creation")
        .unwrap();
    drop(operation);
    drop(status);
    drop(worker);
}

#[test]
fn only_migratable_path_operations_block_the_transition_and_are_named() {
    let state = DesktopState::default();
    let _knowledge = state
        .begin_fixed_application_data_operation("official game knowledge sync")
        .unwrap();
    let writer = state
        .begin_storage_context_operation("instance backup")
        .unwrap();
    let reader = state
        .begin_storage_context_operation("system snapshot refresh")
        .unwrap();
    let error = state.begin_storage_context_transition().err().unwrap();
    assert!(error.contains("2 项操作"), "{error}");
    assert!(error.contains("instance backup"), "{error}");
    assert!(error.contains("system snapshot refresh"), "{error}");
    assert!(!error.contains("official game knowledge sync"), "{error}");
    drop(writer);
    assert!(state.begin_storage_context_transition().is_err());
    drop(reader);
    assert!(state.begin_storage_context_transition().is_ok());
}

#[test]
fn path_blocker_diagnostics_are_bounded_sorted_and_single_line() {
    let state = DesktopState::default();
    let mut leases = Vec::new();
    for index in (0..12).rev() {
        leases.push(
            state
                .begin_storage_context_operation(&format!("operation-{index:02}"))
                .unwrap(),
        );
    }
    let error = state.begin_storage_context_transition().err().unwrap();
    assert!(error.contains("12 项操作"), "{error}");
    assert!(error.contains("operation-00、operation-01"), "{error}");
    assert!(error.contains("operation-07"), "{error}");
    assert!(!error.contains("operation-08"), "{error}");
    drop(leases);

    let label = format!("{}\r\n\u{1b}worker", "文".repeat(200));
    let _lease = state.begin_storage_context_operation(&label).unwrap();
    let error = state.begin_storage_context_transition().err().unwrap();
    assert!(error.chars().all(|value| !value.is_control()), "{error}");
    assert!(error.chars().count() < 180, "{error}");
    assert!(error.contains('…'), "{error}");
}

#[tokio::test]
async fn shutdown_cancels_and_drains_fixed_data_until_its_last_worker_finishes() {
    let state = Arc::new(DesktopState::default());
    let caller = state
        .begin_fixed_application_data_operation("official game knowledge sync")
        .unwrap();
    let worker = caller.clone();
    drop(caller);
    let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
    let (settled_tx, settled_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        worker.cancelled().await;
        cancelled_tx.send(()).unwrap();
        settled_rx.await.unwrap();
        drop(worker);
    });
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let timeout = state
        .drain_storage_operations_for_shutdown(Duration::ZERO)
        .await;
    assert!(
        timeout
            .err()
            .unwrap()
            .contains("official game knowledge sync")
    );
    let shutdown_state = Arc::clone(&state);
    let shutdown = tokio::spawn(async move {
        shutdown_state
            .drain_storage_operations_for_shutdown(Duration::from_secs(2))
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), cancelled_rx)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !shutdown.is_finished(),
        "Cancellation is not worker completion"
    );
    assert!(state.begin_storage_shutdown_exclusive().is_err());
    assert!(
        state
            .begin_fixed_application_data_operation("knowledge status read")
            .is_err()
    );
    assert!(
        state
            .begin_storage_context_operation("instance creation")
            .is_err()
    );
    settled_tx.send(()).unwrap();
    task.await.unwrap();
    let exclusive = shutdown.await.unwrap().unwrap();
    state.shutdown_in_progress.store(false, Ordering::SeqCst);
    assert!(
        state
            .begin_fixed_application_data_operation("late knowledge write")
            .is_err()
    );
    drop(exclusive);
    assert!(
        state
            .begin_fixed_application_data_operation("knowledge status read")
            .is_ok()
    );
}
