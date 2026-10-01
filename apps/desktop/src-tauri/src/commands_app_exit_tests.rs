use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::*;

#[test]
fn app_exit_before_storage_initialization_does_not_require_database_access() {
    let state = DesktopState::default();
    state.app_state.write().expect("desktop state").storage = StorageStatus {
        database_exists: true,
        migrations_applied: false,
        ..StorageStatus::default()
    };

    assert!(
        !app_exit_requires_instance_storage(&state)
            .expect("uninitialized storage with no owned processes can close")
    );
}

#[test]
fn app_exit_after_storage_initialization_still_requires_database_access() {
    let state = DesktopState::default();
    state.app_state.write().expect("desktop state").storage = StorageStatus {
        database_exists: false,
        migrations_applied: true,
        ..StorageStatus::default()
    };

    assert!(
        app_exit_requires_instance_storage(&state)
            .expect("previous initialization still requires active-run verification")
    );
}

#[test]
fn app_exit_with_owned_process_requires_database_access_before_storage_ready() {
    let state = DesktopState::default();
    let identity = ProcessIdentity {
        creation_time: 1,
        image_path: String::from("server.exe"),
    };
    state
        .runtime_supervisor
        .lock()
        .expect("runtime supervisor")
        .insert_running(
            InstanceSummary {
                id: String::from("owned-instance"),
                name: String::from("Owned instance"),
                module_id: String::from("example"),
                status: InstanceStatus::Running,
                active_process_count: 1,
                bind_ip: String::from("127.0.0.1"),
                port_count: 0,
                autostart: false,
            },
            Some(String::from("owned-session")),
            vec![ManagedProcess {
                run_id: 1,
                process_key: String::from("main"),
                display_name: String::from("Server"),
                pid: 4242,
                process_identity: identity.clone(),
                root_process_identity: identity,
                log_path: String::new(),
                is_primary: true,
                uses_script_entrypoint: false,
                performance_policy: RuntimePerformancePolicy::default(),
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: None,
                hidden_desktop: None,
            }],
        );

    assert!(
        app_exit_requires_instance_storage(&state)
            .expect("owned processes must retain full shutdown verification")
    );
}

#[test]
fn app_exit_does_not_treat_unreadable_desktop_state_as_uninitialized_storage() {
    let state = DesktopState::default();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.app_state.write().expect("desktop state");
        panic!("poison desktop state for the shutdown regression");
    }));

    assert_eq!(
        app_exit_requires_instance_storage(&state),
        Err(String::from("desktop state lock poisoned"))
    );
}

#[test]
fn app_exit_does_not_treat_unreadable_supervisor_as_no_owned_processes() {
    let state = DesktopState::default();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.runtime_supervisor.lock().expect("runtime supervisor");
        panic!("poison runtime supervisor for the shutdown regression");
    }));

    assert_eq!(
        app_exit_requires_instance_storage(&state),
        Err(String::from("runtime supervisor lock poisoned"))
    );
}

#[test]
fn app_exit_storage_exclusive_rejects_pending_start_and_allows_retry_after_release() {
    let state = DesktopState::default();
    let reservation = match state
        .try_reserve_runtime_start("reserved-start", "manual")
        .expect("start reservation")
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        attempt => panic!("unexpected reservation attempt: {attempt:?}"),
    };
    state.shutdown_in_progress.store(true, Ordering::SeqCst);

    let error = match state.begin_storage_shutdown_exclusive() {
        Ok(_) => panic!("pending runtime start must block shutdown exclusivity"),
        Err(error) => error,
    };
    let decision = decide_app_shutdown_completion(
        &state,
        Err(format!(
            "application shutdown could not seal storage operations: {error}"
        )),
        false,
    );
    assert!(matches!(decision, AppShutdownDecision::Retry(_)));
    assert!(!state.shutdown_in_progress.load(Ordering::SeqCst));
    drop(reservation);
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let shutdown = state
        .begin_storage_shutdown_exclusive()
        .expect("released runtime start should allow shutdown retry");
    assert!(
        state
            .begin_storage_context_operation("late mutation")
            .is_err()
    );
    drop(shutdown);
}

#[tokio::test]
async fn aborted_runtime_start_caller_leaves_owned_worker_to_finish_handoff() {
    let state = Arc::new(DesktopState::default());
    let reservation = match state
        .try_reserve_runtime_start("detached-start", "manual")
        .expect("start reservation")
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        attempt => panic!("unexpected reservation attempt: {attempt:?}"),
    };
    let supervisor_owned = Arc::new(AtomicBool::new(false));
    let worker_owned = Arc::clone(&supervisor_owned);
    let (spawned_tx, spawned_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let caller = tokio::spawn(async move {
        run_runtime_start_worker_to_completion(&reservation, async move {
            spawned_tx.send(()).expect("spawn signal");
            release_rx.await.expect("handoff release");
            worker_owned.store(true, Ordering::SeqCst);
            completed_tx.send(()).expect("completion signal");
            Ok::<_, String>(())
        })
        .await
    });

    spawned_rx.await.expect("spawn signal");
    caller.abort();
    let _ = caller.await;
    assert!(state.begin_storage_context_transition().is_err());
    assert_eq!(
        state
            .pending_runtime_start_instance_ids()
            .expect("pending starts"),
        vec![String::from("detached-start")]
    );

    release_tx.send(()).expect("release handoff");
    completed_rx.await.expect("completion signal");
    tokio::task::yield_now().await;
    assert!(supervisor_owned.load(Ordering::SeqCst));
    assert!(
        state
            .pending_runtime_start_instance_ids()
            .expect("pending starts after handoff")
            .is_empty()
    );
    state
        .begin_storage_context_transition()
        .expect("completed handoff should release the worker lease");
}

#[test]
fn app_exit_aggregates_every_instance_shutdown_failure() {
    let failures = vec![
        (
            String::from("alpha"),
            String::from("process identity is missing"),
        ),
        (String::from("beta"), String::from("access denied")),
    ];

    let error = aggregate_app_exit_shutdown_failures(
        &failures,
        Some("old run remains"),
        Some("state lock poisoned"),
    )
    .expect_err("any failed instance must keep application shutdown incomplete");

    assert!(error.contains("failed to stop 2 managed instance(s)"));
    assert!(error.contains("alpha: process identity is missing"));
    assert!(error.contains("beta: access denied"));
    assert!(error.contains("active runtime records remain: old run remains"));
    assert!(error.contains("desktop state refresh failed: state lock poisoned"));
}

#[test]
fn failed_app_shutdown_is_not_completed_and_can_be_retried() {
    let state = DesktopState::default();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);

    let attempt_result = aggregate_app_shutdown_phase_results(
        Err(String::from("directory join failed")),
        Err(String::from("join failed")),
        Err(String::from("alpha remained running")),
    );
    let decision = decide_app_shutdown_completion(&state, attempt_result, false);

    let AppShutdownDecision::Retry(error) = decision else {
        panic!("failed cleanup must not complete app shutdown");
    };
    assert!(error.contains("LanGame LAN directory shutdown: directory join failed"));
    assert!(error.contains("LAN host shutdown: join failed"));
    assert!(error.contains("managed instance shutdown: alpha remained running"));
    assert!(!state.shutdown_completed.load(Ordering::SeqCst));
    assert!(!state.shutdown_in_progress.load(Ordering::SeqCst));
}

#[tokio::test]
async fn app_exit_preserves_discovery_panic_without_blocking_confirmed_shutdown() {
    let state = DesktopState::default();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker = std::thread::spawn(|| panic!("discovery diagnostic pipe closed"));
    state
        .register_lan_directory_worker(crate::state::LanDirectoryWorker::new(cancel, worker))
        .expect("register discovery worker");

    let result = join_lan_directory_for_app_exit(&state).await;
    assert!(
        result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("discovery diagnostic pipe closed"),
        "the original panic must remain available for diagnostics"
    );
    let completion = aggregate_app_shutdown_phase_results(result.map(|_| ()), Ok(()), Ok(()));
    assert_eq!(
        decide_app_shutdown_completion(&state, completion, false),
        AppShutdownDecision::Complete
    );
    assert!(state.shutdown_completed.load(Ordering::SeqCst));
    assert!(state.take_lan_directory_worker().unwrap().is_none());
}

#[test]
fn final_tray_exit_failure_keeps_admission_closed_until_watchdog_finishes() {
    let state = DesktopState::default();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    let decision = decide_app_shutdown_completion(
        &state,
        Err(String::from("save transport disconnected")),
        true,
    );
    assert_eq!(
        decision,
        AppShutdownDecision::AwaitDeadline(String::from("save transport disconnected"))
    );
    assert!(state.shutdown_in_progress.load(Ordering::SeqCst));
    assert!(!state.shutdown_completed.load(Ordering::SeqCst));
    assert!(state.begin_storage_context_operation("late start").is_err());
}

#[test]
fn final_tray_exit_upgrades_an_ordinary_failure_without_reopening_or_recovery() {
    let state = DesktopState::default();
    assert!(!state.is_final_exit_requested());
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    state.request_final_exit();
    assert!(
        state
            .shutdown_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err(),
        "the ordinary shutdown keeps ownership while final intent is accepted"
    );
    let decision = decide_app_shutdown_completion(
        &state,
        Err(String::from("ordinary save failed after final exit")),
        false,
    );
    assert_eq!(
        decision,
        AppShutdownDecision::AwaitDeadline(String::from("ordinary save failed after final exit"))
    );
    assert!(state.is_final_exit_requested());
    assert!(state.shutdown_in_progress.load(Ordering::SeqCst));
    assert!(!state.shutdown_completed.load(Ordering::SeqCst));
    assert!(state.begin_storage_context_operation("late write").is_err());
    assert!(state.begin_storage_context_transition().is_err());
    let recovered = std::cell::Cell::new(false);
    let error = recover_lan_directory_after_failed_shutdown(
        &state,
        String::from("ordinary save failed after final exit"),
        || {
            recovered.set(true);
            Ok(())
        },
    );
    assert_eq!(error, "ordinary save failed after final exit");
    assert!(!recovered.get(), "final exit must not restart discovery");
}

#[test]
fn final_intent_seals_admission_even_during_an_ordinary_failure_reset() {
    let state = DesktopState::default();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    state.request_final_exit();
    // Model the old attempt resetting its flag after the final-intent check.
    state.shutdown_in_progress.store(false, Ordering::SeqCst);
    assert!(
        state
            .begin_storage_context_operation("racing write")
            .is_err()
    );
    assert!(state.begin_storage_context_transition().is_err());
    state.request_final_exit();
    assert!(state.is_final_exit_requested());
}

#[tokio::test]
async fn failed_app_shutdown_re_registers_the_lan_directory_worker() {
    let state = DesktopState::default();
    state.shutdown_in_progress.store(true, Ordering::SeqCst);

    let cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&cancel);
    let directory_thread = std::thread::spawn(move || {
        while !thread_cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    state
        .register_lan_directory_worker(crate::state::LanDirectoryWorker::new(
            cancel,
            directory_thread,
        ))
        .expect("register initial directory worker");
    join_lan_directory_for_app_exit(&state)
        .await
        .expect("cancel and join initial directory worker");

    let replacement_cancel = Arc::new(AtomicBool::new(false));
    let thread_cancel = Arc::clone(&replacement_cancel);
    let error = recover_lan_directory_after_failed_shutdown(
        &state,
        String::from("managed instance shutdown failed"),
        || {
            let replacement_thread = std::thread::spawn(move || {
                while !thread_cancel.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(1));
                }
            });
            state
                .register_lan_directory_worker(crate::state::LanDirectoryWorker::new(
                    replacement_cancel,
                    replacement_thread,
                ))
                .map_err(|(error, worker)| {
                    let _ = worker.cancel_and_take_thread().join();
                    error
                })
        },
    );

    assert_eq!(error, "managed instance shutdown failed");
    let replacement = state
        .take_lan_directory_worker()
        .expect("read replacement directory worker")
        .expect("failed shutdown must restore directory worker");
    replacement
        .cancel_and_take_thread()
        .join()
        .expect("join replacement directory worker");
}

#[tokio::test]
async fn untracked_running_process_without_pid_or_identity_is_rejected_before_stop() {
    let root = std::env::temp_dir().join(format!(
        "langame-app-exit-missing-identity-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let paths = app_storage::StoragePaths {
        app_data_root: root.clone(),
        settings_path: root.join("settings.json"),
        database_path: root.join("db").join("lgs.db"),
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
    let storage_context_operation = state
        .begin_storage_context_operation("missing identity test")
        .expect("storage operation");
    for (pid, process_identity, expected_error) in [
        (
            Some(4242),
            None,
            "PID 4242 has no recorded process identity",
        ),
        (
            None,
            Some(ProcessIdentity {
                creation_time: 1,
                image_path: String::from("server.exe"),
            }),
            "has no recorded PID",
        ),
    ] {
        let active_run = ActiveInstanceRun {
            run_id: 7,
            session_id: Some(String::from("session")),
            pid,
            log_path: None,
            process_count: 1,
            processes: vec![app_core::InstanceProcessState {
                run_id: 7,
                session_id: Some(String::from("session")),
                process_key: String::from("server"),
                display_name: String::from("Dedicated server"),
                pid,
                process_identity,
                status: String::from("running"),
                started_at: None,
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path: None,
                is_primary: true,
            }],
        };

        let error = stop_active_instance_processes(
            &state,
            &storage,
            &storage_context_operation,
            "missing-safety-metadata",
            &active_run,
            InstanceShutdownSource::AppExit,
        )
        .await
        .expect_err("incomplete process identity metadata must fail closed");

        assert!(error.contains(expected_error), "unexpected error: {error}");
        assert!(error.contains("active runtime record was preserved"));
        assert_eq!(active_run.processes[0].status, "running");
    }
    assert!(
        !root.exists(),
        "identity preflight must fail before any storage mutation"
    );
}
