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
