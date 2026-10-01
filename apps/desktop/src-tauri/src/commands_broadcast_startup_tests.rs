use super::*;
use std::io::Read;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize};

#[test]
fn startup_broadcast_wait_is_limited_to_automatic_startup() {
    for initiator in ["auto", "lifecycle", "system"] {
        assert!(should_wait_for_broadcast_channel("startup", initiator));
    }
    assert!(!should_wait_for_broadcast_channel("startup", "manual"));
    for source in ["manual", "shutdown", "periodic", "runtime_health"] {
        assert!(!should_wait_for_broadcast_channel(source, "lifecycle"));
    }
}

#[tokio::test]
async fn startup_broadcast_waits_for_delayed_listener_and_sends_once() {
    verify_delayed_startup_broadcast(false).await;
}

#[tokio::test]
async fn startup_broadcast_does_not_retry_a_command_after_its_response_is_lost() {
    verify_delayed_startup_broadcast(true).await;
}

async fn verify_delayed_startup_broadcast(drop_response: bool) {
    let reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = reservation.local_addr().unwrap();
    drop(reservation);
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let executions = Arc::new(AtomicUsize::new(0));
    let server_executions = executions.clone();
    let server = std::thread::spawn(move || {
        start_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let listener = TcpListener::bind(endpoint).unwrap();
        listener.set_nonblocking(true).unwrap();
        let accept = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Accepted sockets inherit the listener mode on Windows.
                        stream.set_nonblocking(false).unwrap();
                        return stream;
                    }
                    Err(error)
                        if error.kind() == ErrorKind::WouldBlock && Instant::now() < deadline =>
                    {
                        std::thread::yield_now()
                    }
                    Err(error) => panic!("listener accept failed: {error}"),
                }
            }
        };
        let mut probe = accept();
        probe
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        // A readiness check must not authenticate or send any command bytes.
        assert_eq!(probe.read(&mut [0; 1]).unwrap(), 0);
        let mut stream = accept();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let auth = source_rcon_read_packet(&mut stream).unwrap();
        assert_eq!(auth.packet_type, 3);
        source_rcon_write_packet(&mut stream, auth.id, 2, "").unwrap();
        let command = source_rcon_read_packet(&mut stream).unwrap();
        assert_eq!(command.body, b"say startup fixture");
        server_executions.fetch_add(1, Ordering::SeqCst);
        if !drop_response {
            source_rcon_write_packet(&mut stream, command.id, 0, "accepted").unwrap();
            let marker = source_rcon_read_packet(&mut stream).unwrap();
            source_rcon_write_packet(&mut stream, marker.id, 0, "").unwrap();
        }
        drop(stream);
        listener
    });
    let mut checks = 0;
    await_startup_broadcast_readiness(
        Duration::from_secs(4),
        || {
            checks += 1;
            // Release the listener only after readiness monitoring has started.
            if checks == 3 {
                start_tx.send(()).unwrap();
            }
            Ok(())
        },
        wait_for_startup_broadcast_listener(endpoint),
    )
    .await
    .unwrap();
    let result = tokio::task::spawn_blocking(move || {
        source_rcon_exec(
            &endpoint.to_string(),
            "fixture-only-password",
            "say startup fixture",
        )
    })
    .await
    .unwrap();
    if drop_response {
        assert!(
            result
                .unwrap_err()
                .contains("failed to read RCON packet size")
        );
    } else {
        assert_eq!(result.unwrap(), "accepted");
    }
    let listener = server.join().unwrap();
    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert_eq!(listener.accept().unwrap_err().kind(), ErrorKind::WouldBlock);
}

#[tokio::test]
async fn startup_broadcast_times_out_without_sending_when_channel_never_becomes_ready() {
    let error = await_startup_broadcast_readiness(
        Duration::from_millis(20),
        || Ok(()),
        std::future::pending::<Result<(), String>>(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("within 20 ms"));
}

#[tokio::test]
async fn startup_broadcast_cancels_while_readiness_or_instance_lock_is_pending() {
    for cause in [
        "application shutdown",
        "instance stopped",
        "server run changed",
    ] {
        let mut checks = 0;
        let error = await_startup_broadcast_readiness(
            Duration::from_secs(3),
            || {
                checks += 1;
                if checks >= 3 {
                    Err(cause.to_string())
                } else {
                    Ok(())
                }
            },
            std::future::pending::<Result<(), String>>(),
        )
        .await
        .unwrap_err();
        assert_eq!(error, cause);
    }
}

#[tokio::test]
async fn startup_broadcast_rechecks_cancellation_after_channel_becomes_ready() {
    let cancelled = AtomicBool::new(false);
    let error = await_startup_broadcast_readiness(
        Duration::from_secs(1),
        || {
            if cancelled.load(Ordering::SeqCst) {
                Err(String::from("instance stopped"))
            } else {
                Ok(())
            }
        },
        async {
            cancelled.store(true, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error, "instance stopped");
}

#[test]
fn startup_broadcast_cancels_for_application_exit_stopped_and_replaced_server_runs() {
    let state = DesktopState::default();
    let run = ActiveInstanceRun {
        run_id: 91,
        session_id: Some(String::from("startup-fixture")),
        pid: Some(std::process::id()),
        log_path: None,
        process_count: 1,
        processes: vec![],
    };
    assert!(
        ensure_startup_broadcast_run(&state, "fixture", &run)
            .unwrap_err()
            .contains("stopped")
    );
    let summary = InstanceSummary {
        id: String::from("fixture"),
        name: String::from("Startup fixture"),
        module_id: String::from("minecraft"),
        status: InstanceStatus::Running,
        active_process_count: 1,
        bind_ip: String::from("127.0.0.1"),
        port_count: 1,
        autostart: false,
    };
    let identity = inspect_process_identity(std::process::id())
        .unwrap()
        .unwrap();
    state.runtime_supervisor.lock().unwrap().insert_running(
        summary,
        run.session_id.clone(),
        vec![ManagedProcess {
            run_id: run.run_id,
            process_key: String::from("main"),
            display_name: String::from("fixture"),
            pid: std::process::id(),
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
    ensure_startup_broadcast_run(&state, "fixture", &run).unwrap();
    let mut stopped = state
        .runtime_supervisor
        .lock()
        .unwrap()
        .take_running_for_stop("fixture")
        .unwrap();
    assert!(
        ensure_startup_broadcast_run(&state, "fixture", &run)
            .unwrap_err()
            .contains("stopped")
    );
    // Retaining the same live PID must not make a different run eligible.
    stopped.processes[0].run_id += 1;
    assert!(
        state
            .runtime_supervisor
            .lock()
            .unwrap()
            .restore_running_after_failed_stop(stopped)
    );
    assert!(
        ensure_startup_broadcast_run(&state, "fixture", &run)
            .unwrap_err()
            .contains("run changed")
    );
    state.shutdown_in_progress.store(true, Ordering::SeqCst);
    assert!(
        ensure_startup_broadcast_run(&state, "fixture", &run)
            .unwrap_err()
            .contains("application shutdown")
    );
}
