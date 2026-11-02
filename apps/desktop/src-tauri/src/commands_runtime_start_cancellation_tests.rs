use super::*;

fn reserve(state: &DesktopState, instance_id: &str, source: &str) -> RuntimeStartReservationLease {
    match state
        .try_reserve_runtime_start(instance_id, source)
        .unwrap()
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected runtime start reservation: {other:?}"),
    }
}

fn reserve_attempt(
    state: &DesktopState,
    instance_id: &str,
    attempt_id: &str,
) -> RuntimeStartReservationLease {
    match state
        .try_reserve_runtime_start_for_attempt(instance_id, "manual", Some(attempt_id))
        .unwrap()
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected runtime start reservation: {other:?}"),
    }
}

#[test]
fn runtime_start_cancellation_pre_registration_work_requires_current_unready_lease() {
    let state = DesktopState::default();
    let lease = reserve(&state, "server", "manual");
    assert!(lease.ensure_current_for("server").is_ok());
    assert!(lease.ensure_current_for("another-server").is_err());
    assert!(lease.confirm_ready().unwrap());
    assert!(lease.ensure_current_for("server").is_err());
    drop(lease);
    let next = reserve(&state, "server", "manual");
    state.request_runtime_start_cancellation("server").unwrap();
    assert!(next.ensure_current_for("server").is_err());
}

#[tokio::test]
async fn runtime_start_cancellation_before_acceptance_rejects_the_late_start() {
    let state = DesktopState::default();
    let receipt = state
        .request_runtime_start_cancellation_for_attempt("server", Some("first"))
        .unwrap()
        .unwrap();
    receipt.wait_until_settled(Duration::ZERO).await.unwrap();
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .unwrap()
            .is_empty()
    );
    let error = state
        .try_reserve_runtime_start_for_attempt("server", "manual", Some("first"))
        .unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "runtime_start_cancelled");
    assert_eq!(error["instance_id"], "server");
    let replacement = reserve_attempt(&state, "server", "second");
    assert!(!replacement.is_cancelled());
    // IDs are scoped to their instance, not to every server on the host.
    let other = reserve_attempt(&state, "other", "first");
    assert!(!other.is_cancelled());
}

#[tokio::test]
async fn runtime_start_cancellation_for_another_attempt_keeps_the_current_owner() {
    let state = DesktopState::default();
    let current = reserve_attempt(&state, "server", "current");
    let unknown = state
        .request_runtime_start_cancellation_for_attempt("server", Some("earlier"))
        .unwrap()
        .unwrap();
    unknown.wait_until_settled(Duration::ZERO).await.unwrap();
    assert!(!current.is_cancelled());
    assert!(!current.cancelled_by_user());
    assert!(matches!(
        state
            .try_reserve_runtime_start_for_attempt("server", "manual", Some("next"))
            .unwrap(),
        RuntimeStartReservationAttempt::Conflict(_)
    ));
    assert!(current.confirm_ready().unwrap());
}

#[tokio::test]
async fn runtime_start_cancellation_for_the_accepted_attempt_keeps_worker_ownership() {
    let state = DesktopState::default();
    let start = reserve_attempt(&state, "server", "first");
    let worker = start.clone();
    let receipt = state
        .request_runtime_start_cancellation_for_attempt("server", Some("first"))
        .unwrap()
        .unwrap();
    assert!(start.is_cancelled());
    assert!(!start.confirm_ready().unwrap());
    drop(start);
    assert!(receipt.wait_until_settled(Duration::ZERO).await.is_err());
    drop(worker);
    receipt.wait_until_settled(Duration::ZERO).await.unwrap();
    let error = state
        .try_reserve_runtime_start_for_attempt("server", "manual", Some("first"))
        .unwrap_err();
    assert!(error.contains("runtime_start_cancelled"));
}

#[tokio::test]
async fn runtime_start_ready_receipt_cannot_stop_a_replacement_attempt() {
    let state = DesktopState::default();
    let first = reserve_attempt(&state, "server", "first");
    assert!(first.confirm_ready().unwrap());
    drop(first);
    assert!(
        state
            .request_runtime_start_cancellation_for_attempt("server", Some("first"))
            .unwrap()
            .is_none()
    );
    state
        .validate_runtime_start_attempt("server", "first")
        .unwrap();
    assert!(
        state
            .try_reserve_runtime_start_for_attempt("server", "manual", Some("first"))
            .is_err()
    );
    let replacement = reserve_attempt(&state, "server", "replacement");
    assert!(
        state
            .validate_runtime_start_attempt("server", "first")
            .is_err()
    );
    state
        .validate_runtime_start_attempt("server", "replacement")
        .unwrap();
    let late_stop = state
        .request_runtime_start_cancellation_for_attempt("server", Some("first"))
        .unwrap()
        .unwrap();
    late_stop.wait_until_settled(Duration::ZERO).await.unwrap();
    assert!(!replacement.is_cancelled());
    drop(replacement);
    let untagged = reserve(&state, "server", "manual");
    assert!(
        state
            .validate_runtime_start_attempt("server", "replacement")
            .is_err()
    );
    assert!(!untagged.is_cancelled());
}

#[tokio::test]
async fn runtime_start_cleanup_failure_survives_the_last_worker_and_missing_run_record() {
    let state = DesktopState::default();
    let start = reserve_attempt(&state, "server", "first");
    let worker = start.clone();
    let receipt = state
        .request_runtime_start_cancellation_for_attempt("server", Some("first"))
        .unwrap()
        .unwrap();
    start
        .record_cleanup_failure("Owned process tree did not confirm exit; run registration failed");
    drop(start);
    assert!(receipt.wait_until_settled(Duration::ZERO).await.is_err());
    drop(worker);
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .unwrap()
            .is_empty()
    );
    let error = receipt
        .wait_until_settled(Duration::ZERO)
        .await
        .unwrap_err();
    assert!(error.contains("Owned process tree did not confirm exit"));
    let repeated = state
        .request_runtime_start_cancellation_for_attempt("server", Some("first"))
        .unwrap()
        .unwrap();
    assert_eq!(repeated.check_cleanup_result().unwrap_err(), error);
}

#[tokio::test]
async fn runtime_start_cancellation_capacity_never_forgets_a_cancelled_attempt() {
    let state = DesktopState::default();
    for index in 0..1024 {
        let receipt = state
            .request_runtime_start_cancellation_for_attempt(
                "server",
                Some(&format!("attempt-{index}")),
            )
            .unwrap()
            .unwrap();
        receipt.wait_until_settled(Duration::ZERO).await.unwrap();
    }
    let error = state
        .request_runtime_start_cancellation_for_attempt("server", Some("overflow"))
        .unwrap_err();
    assert!(error.contains("cancellation limit"));
    assert!(
        state
            .request_runtime_start_cancellation_for_attempt("server", Some("attempt-0"))
            .is_ok()
    );
    assert!(
        state
            .try_reserve_runtime_start_for_attempt("server", "manual", Some("attempt-0"))
            .unwrap_err()
            .contains("runtime_start_cancelled")
    );
    let current = reserve_attempt(&state, "server", "accepted");
    assert!(
        state
            .request_runtime_start_cancellation_for_attempt("server", Some("accepted"))
            .is_ok()
    );
    assert!(current.is_cancelled());
}

#[test]
fn runtime_start_attempt_ids_are_bounded_before_admission_or_cancellation() {
    let state = DesktopState::default();
    for invalid in [String::new(), "x".repeat(129)] {
        assert!(
            state
                .try_reserve_runtime_start_for_attempt("server", "manual", Some(&invalid))
                .is_err()
        );
        assert!(
            state
                .request_runtime_start_cancellation_for_attempt("server", Some(&invalid))
                .is_err()
        );
    }
    let valid = reserve_attempt(&state, "server", &"x".repeat(128));
    assert!(!valid.is_cancelled());
}

#[tokio::test]
async fn runtime_start_cancellation_is_scoped_and_waits_for_all_worker_leases() {
    let state = DesktopState::default();
    let first = reserve(&state, "first", "manual");
    let worker = first.clone();
    let other = reserve(&state, "other", "manual");
    let receipt = state
        .request_runtime_start_cancellation("first")
        .unwrap()
        .unwrap();
    assert!(first.cancelled_by_user());
    assert!(first.is_cancelled());
    assert!(!other.is_cancelled());
    assert!(!first.confirm_ready().unwrap());
    assert!(receipt.wait_until_settled(Duration::ZERO).await.is_err());
    drop(first);
    assert!(receipt.wait_until_settled(Duration::ZERO).await.is_err());
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .unwrap()
            .contains(&"first".into())
    );
    drop(worker);
    receipt.wait_until_settled(Duration::ZERO).await.unwrap();
    let replacement = reserve(&state, "first", "manual");
    assert!(!replacement.is_cancelled());
    assert!(!replacement.cancelled_by_user());
    drop((replacement, other));
}

#[test]
fn runtime_start_cancellation_after_ready_cannot_force_the_running_attempt() {
    let state = DesktopState::default();
    let ready = reserve(&state, "ready", "manual");
    assert!(ready.confirm_ready().unwrap());
    assert!(
        state
            .request_runtime_start_cancellation("ready")
            .unwrap()
            .is_none()
    );
    assert!(!ready.is_cancelled());
    assert!(!ready.cancelled_by_user());
}

#[tokio::test]
async fn runtime_start_cancellation_interrupts_a_scheduled_delay() {
    let state = DesktopState::default();
    let reservation = reserve(&state, "queued-start", "manual");
    let mut delay = Box::pin(wait_for_runtime_start_delay(
        &reservation,
        Duration::from_secs(300),
    ));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(delay.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .is_err()
    );
    tokio::time::timeout(Duration::from_secs(1), &mut delay)
        .await
        .expect("shutdown must interrupt the queued delay");
    drop(delay);
    assert!(reservation.is_cancelled());
    assert!(state.begin_storage_shutdown_exclusive().is_err());
    drop(reservation);
    state
        .drain_storage_operations_for_shutdown(Duration::ZERO)
        .await
        .unwrap();
}

#[tokio::test]
async fn runtime_start_cancellation_survives_failed_shutdown_without_blocking_new_starts() {
    let root = std::env::temp_dir().join(format!(
        "langame-start-cancellation-{}",
        uuid::Uuid::new_v4().simple(),
    ));
    let paths = app_storage::StoragePaths {
        app_data_root: root.clone(),
        settings_path: root.join("settings.json"),
        database_path: root.join("db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    };
    let storage = StorageBootstrap {
        settings: paths.settings(),
        storage_status: paths.probe_status(),
        paths,
    };
    let state = DesktopState::default();
    let pending = [
        ("manual-start", "manual"),
        ("restart-start", "auto_restart"),
    ]
    .map(|(id, source)| (id, source, reserve(&state, id, source)));
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let error = state
        .drain_storage_operations_for_shutdown(Duration::ZERO)
        .await
        .err()
        .expect("pending starts retain their leases");
    assert!(matches!(
        decide_app_shutdown_completion(&state, Err(error), false),
        AppShutdownDecision::Retry(_),
    ));
    assert!(!state.shutdown_in_progress.load(Ordering::SeqCst));
    for (id, source, reservation) in &pending {
        let error = ensure_runtime_start_allowed(&state, &storage, id, source, reservation)
            .expect_err("failed shutdown must not revive an old start");
        assert!(error.contains("application shutdown"), "{error}");
    }
    drop(pending);
    let next = reserve(&state, "manual-start", "manual");
    ensure_runtime_start_allowed(&state, &storage, "manual-start", "manual", &next)
        .expect("a new request after failed shutdown should be admitted");
    drop(next);
    std::fs::remove_dir_all(root).unwrap();
}
