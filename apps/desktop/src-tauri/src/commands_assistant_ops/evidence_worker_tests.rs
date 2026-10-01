use super::*;

#[tokio::test]
async fn assistant_evidence_timeout_retains_the_blocking_read_slot() {
    let state = DesktopState::default();
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let result =
        run_assistant_evidence_read_with(&state, slots.clone(), Duration::ZERO, move || {
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        })
        .await;
    assert!(result.unwrap_err().contains("timed out"));
    assert!(slots.try_acquire().is_err());
    assert!(state.begin_storage_context_transition().is_err());
    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_secs(5), slots.acquire())
        .await
        .expect("timed-out read eventually releases its slot")
        .unwrap();
    drop(permit);
}

#[tokio::test]
async fn assistant_evidence_cancellation_retains_worker_capacity_until_io_exits() {
    let state = std::sync::Arc::new(DesktopState::default());
    let slots = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker_state = state.clone();
    let worker_slots = slots.clone();
    let waiter = tokio::spawn(async move {
        run_assistant_evidence_read_with(
            &worker_state,
            worker_slots,
            Duration::from_secs(30),
            move || {
                let _ = started_tx.send(());
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            },
        )
        .await
    });
    started_rx.await.unwrap();
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    assert!(state.begin_storage_context_transition().is_err());
    let rejected: Result<(), String> =
        run_assistant_evidence_read_with(&state, slots.clone(), Duration::from_secs(1), || {
            panic!("a cancelled waiter must not admit another blocking read")
        })
        .await;
    assert!(rejected.unwrap_err().contains("still busy"));
    release_tx.send(()).unwrap();
    let released = tokio::time::timeout(Duration::from_secs(5), slots.acquire())
        .await
        .expect("the completed worker releases capacity")
        .unwrap();
    drop(released);
    assert_eq!(
        run_assistant_evidence_read_with(&state, slots, Duration::from_secs(1), || Ok(42))
            .await
            .unwrap(),
        42
    );
}
