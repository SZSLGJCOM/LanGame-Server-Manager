use super::*;
use std::future::poll_fn;
use std::task::Poll;

#[test]
fn committed_exit_request_keeps_the_single_flight_claim() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let client = Client::new(endpoint);
    let request = client.try_begin_exit().unwrap();
    assert!(client.try_begin_exit().is_none());

    request.commit();
    assert!(client.try_begin_exit().is_none());
    assert!(client.is_closing());
}

#[tokio::test]
async fn final_exit_rejects_new_work_before_connecting_to_the_service() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let client = Client::new(endpoint);
    client.try_begin_exit().unwrap().commit();
    for command in [
        "read_instance_details_from_storage",
        "read_instance_runtime_overview_from_storage",
        "create_instance",
    ] {
        assert_eq!(
            client.request(command, Value::Null).await.unwrap_err(),
            "Application final exit is in progress"
        );
    }
}

#[tokio::test]
async fn final_exit_prevents_work_waiting_for_a_busy_connection_from_being_sent() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let occupied = security::create_pipe(&endpoint, true).unwrap();
    let _blocker = security::connect_verified(&endpoint).unwrap();
    occupied.connect().await.unwrap();
    let client = Client::new(endpoint.clone());
    let mut request = Box::pin(client.request("bootstrap", Value::Null));
    assert!(
        poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    client.try_begin_exit().unwrap().commit();
    let mut listener = security::create_pipe(&endpoint, false).unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error, "Application final exit is in progress");
    // A connection may have been acquired, but no operation can cross the exit fence.
    if tokio::time::timeout(Duration::from_millis(100), listener.connect())
        .await
        .is_ok()
    {
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                wire::read_frame::<wire::Request>(&mut listener)
            )
            .await
            .unwrap()
            .is_err()
        );
    }
}

#[test]
fn uncommitted_exit_request_allows_retry_after_watchdog_setup_failure() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let client = Client::new(endpoint);
    drop(client.try_begin_exit().unwrap());
    assert!(!client.is_closing());
    assert!(client.try_begin_exit().is_some());
}

#[test]
fn simultaneous_exit_requests_have_one_owner() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let client = Client::new(endpoint);
    let barrier = std::sync::Barrier::new(8);
    let owners = std::thread::scope(|scope| {
        let requests = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    let request = client.try_begin_exit();
                    barrier.wait();
                    usize::from(request.is_some())
                })
            })
            .collect::<Vec<_>>();
        requests
            .into_iter()
            .map(|request| request.join().unwrap())
            .sum::<usize>()
    });
    assert_eq!(owners, 1);
    assert!(client.try_begin_exit().is_some());
}

#[tokio::test]
async fn control_requests_use_the_dedicated_pipe_while_the_work_pipe_is_busy() {
    for command in [
        "runtime_service_status",
        "runtime_service_shutdown",
        "runtime_service_tray_exit",
    ] {
        let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
        let occupied = security::create_pipe(&endpoint, true).unwrap();
        let _blocker = security::connect_verified(&endpoint).unwrap();
        occupied.connect().await.unwrap();
        let mut control = security::create_pipe(&endpoint.control(), true).unwrap();
        let client = Client::new(endpoint);
        client.try_begin_exit().unwrap().commit();
        let receipt = if command == "runtime_service_tray_exit" {
            serde_json::json!({"shutdown_accepted":true,"pid":std::process::id()})
        } else {
            serde_json::json!({"shutdown_completed":true,"pid":std::process::id()})
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(client.request(command, Value::Null), async {
                control.connect().await.unwrap();
                let request: wire::Request = wire::read_frame(&mut control).await.unwrap();
                assert_eq!(request.command, command);
                wire::write_response(
                    &mut control,
                    &wire::Response {
                        result: Ok(receipt.clone()),
                    },
                )
                .await
                .unwrap();
                if command == "runtime_service_shutdown" {
                    assert!(wire::read_frame::<bool>(&mut control).await.unwrap());
                }
            })
        })
        .await
        .expect("Control requests must not queue behind occupied ordinary connections");
        assert_eq!(result.unwrap(), receipt);
    }
}

#[tokio::test]
async fn verified_client_pins_both_pipes_to_the_original_process_identity() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let _work = security::create_pipe(&endpoint, true).unwrap();
    let _initial_control = security::create_pipe(&endpoint.control(), true).unwrap();
    let mut client = Client::connect(endpoint.clone()).unwrap();
    assert!(client.service_target().unwrap().is_running().unwrap());
    assert_eq!(
        client.service_identity.as_ref().unwrap().pid,
        std::process::id()
    );
    let _control = security::create_pipe(&endpoint.control(), false).unwrap();
    client
        .service_identity
        .as_mut()
        .unwrap()
        .process
        .creation_time ^= 1;
    for command in ["bootstrap", "runtime_service_status"] {
        let error = client.request(command, Value::Null).await.unwrap_err();
        assert!(error.contains("replaced; no operation was sent"), "{error}");
    }
}

#[tokio::test]
async fn shutdown_receipt_remains_successful_when_acknowledgement_cannot_be_delivered() {
    for command in ["runtime_service_shutdown", "runtime_service_tray_exit"] {
        let (mut client, mut service) = tokio::io::duplex(1024);
        let receipt = if command == "runtime_service_tray_exit" {
            serde_json::json!({"shutdown_accepted":true,"pid":123})
        } else {
            serde_json::json!({"shutdown_completed":true,"pid":123})
        };
        wire::write_response(
            &mut service,
            &wire::Response {
                result: Ok(receipt.clone()),
            },
        )
        .await
        .unwrap();
        // Duplex retains the written receipt but rejects every later client write.
        drop(service);
        assert_eq!(
            receive_response(command, &mut client).await.unwrap(),
            receipt
        );
    }
}

#[tokio::test]
async fn final_exit_returns_on_owned_acceptance_while_the_service_is_still_running() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let mut control = security::create_pipe(&endpoint.control(), true).unwrap();
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let target = Arc::new(
        app_runtime::ProcessExitTarget::capture(pid, &identity)
            .unwrap()
            .unwrap(),
    );
    let client = Client::for_exit_handoff(
        endpoint,
        security::ServiceIdentity {
            pid,
            process: identity,
        },
        Arc::clone(&target),
    );
    let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(client.stop_for_tray_exit(123), async {
            control.connect().await.unwrap();
            let request: wire::Request = wire::read_frame(&mut control).await.unwrap();
            assert_eq!(request.command, "runtime_service_tray_exit");
            assert_eq!(request.args["deadline_tick_ms"], 123);
            wire::write_response(
                &mut control,
                &wire::Response {
                    result: Ok(serde_json::json!({"shutdown_accepted":true,"pid":pid})),
                },
            )
            .await
            .unwrap();
        })
    })
    .await
    .expect("Taking stop ownership must not wait for runtime exit or saving");
    assert!(result.is_ok(), "{result:?}");
    assert!(target.is_running().unwrap());
    assert!(client.is_closing());
}

#[tokio::test]
async fn shutdown_receipt_preserves_save_failure_for_application_updates() {
    let (mut client, mut service) = tokio::io::duplex(1024);
    wire::write_response(
        &mut service,
        &wire::Response {
            result: Err("Save failed".into()),
        },
    )
    .await
    .unwrap();
    drop(service);
    assert_eq!(
        receive_response("runtime_service_shutdown", &mut client)
            .await
            .unwrap_err(),
        "Save failed"
    );
}

#[tokio::test]
async fn final_exit_busy_control_recovers_after_the_ordinary_connection_budget() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let control_endpoint = endpoint.control();
    let occupied = security::create_pipe(&control_endpoint, true).unwrap();
    let _blocker = security::connect_verified(&control_endpoint).unwrap();
    occupied.connect().await.unwrap();
    let client = Client::connect(endpoint.clone());
    assert_eq!(client.err().unwrap().raw_os_error(), Some(231));
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let target = Arc::new(
        app_runtime::ProcessExitTarget::capture(pid, &identity)
            .unwrap()
            .unwrap(),
    );
    let client = Client::for_exit_handoff(
        endpoint,
        security::ServiceIdentity {
            pid,
            process: identity,
        },
        Arc::clone(&target),
    );
    let deadline = super::super::exit_deadline::deadline_tick_ms(Duration::from_secs(15));
    let mut final_exit = Box::pin(client.stop_for_tray_exit(deadline));
    assert!(
        poll_fn(|cx| Poll::Ready(final_exit.as_mut().poll(cx)))
            .await
            .is_pending()
    );

    // Observe the real ordinary request exhaust its two-second budget before
    // exposing a free listener. This is a barrier, not a timing-based release.
    let ordinary_error = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            result = &mut final_exit => panic!("Final exit abandoned its original budget: {result:?}"),
            result = client.request("runtime_service_status", Value::Null) => result.unwrap_err(),
        }
    }).await.expect("The ordinary request must retain its bounded connection wait");
    assert!(
        ordinary_error.contains("No operation was sent"),
        "{ordinary_error}"
    );
    assert_eq!(
        occupied.try_read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );

    let mut listener = security::create_pipe(&control_endpoint, false).unwrap();
    let (result, next) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(final_exit, async {
            listener.connect().await.unwrap();
            let request: wire::Request = wire::read_frame(&mut listener).await.unwrap();
            assert_eq!(request.command, "runtime_service_tray_exit");
            assert_eq!(request.args["deadline_tick_ms"], deadline);
            let next = security::create_pipe(&control_endpoint, false).unwrap();
            wire::write_response(
                &mut listener,
                &wire::Response {
                    result: Ok(serde_json::json!({"shutdown_accepted":true,"pid":pid})),
                },
            )
            .await
            .unwrap();
            next
        })
    })
    .await
    .expect("The original final-exit request must use the recovered listener");
    assert!(result.is_ok(), "{result:?}");
    assert!(client.is_closing());
    assert!(target.is_running().unwrap());
    let mut duplicate = Box::pin(next.connect());
    assert!(
        poll_fn(|cx| Poll::Ready(duplicate.as_mut().poll(cx)))
            .await
            .is_pending()
    );
}

#[tokio::test]
async fn expired_final_exit_budget_does_not_restart_a_busy_connection_wait() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let occupied = security::create_pipe(&endpoint.control(), true).unwrap();
    let _blocker = security::connect_verified(&endpoint.control()).unwrap();
    occupied.connect().await.unwrap();
    let pid = std::process::id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let target = Arc::new(
        app_runtime::ProcessExitTarget::capture(pid, &identity)
            .unwrap()
            .unwrap(),
    );
    let client = Client::for_exit_handoff(
        endpoint,
        security::ServiceIdentity {
            pid,
            process: identity,
        },
        target,
    );
    let mut final_exit = Box::pin(client.stop_for_tray_exit(0));
    let result = poll_fn(|cx| Poll::Ready(final_exit.as_mut().poll(cx))).await;
    assert!(
        matches!(result, Poll::Ready(Err(ref error)) if error.contains("No operation was sent")),
        "{result:?}"
    );
    assert_eq!(
        occupied.try_read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[tokio::test]
async fn missing_service_fails_without_waiting_or_sending() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let client = Client::new(endpoint);
    let mut request = Box::pin(client.request("bootstrap", Value::Null));
    let first_poll = poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    match first_poll {
        Poll::Ready(Err(error)) => {
            assert!(error.contains("os error 2"), "{error}");
            assert!(error.contains("No operation was sent"), "{error}");
        }
        other => panic!("A missing service must fail on the first attempt: {other:?}"),
    }
}

#[tokio::test]
async fn persistent_pipe_busy_returns_without_sending_within_connection_budget() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let occupied = security::create_pipe(&endpoint, true).unwrap();
    let _blocker = security::connect_verified(&endpoint).unwrap();
    occupied.connect().await.unwrap();
    let client = Client::new(endpoint);
    let started = tokio::time::Instant::now();
    let error = tokio::time::timeout(
        Duration::from_secs(5),
        client.request("bootstrap", Value::Null),
    )
    .await
    .expect("A permanently occupied pipe must not wait indefinitely")
    .unwrap_err();
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert!(error.contains("os error 231"), "{error}");
    assert!(error.contains("No operation was sent"), "{error}");
    assert_eq!(
        occupied.try_read(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "Waiting for a new connection must not write to an occupied one"
    );
}

#[tokio::test]
async fn cancelling_busy_connection_does_not_send_the_cancelled_request() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let occupied = security::create_pipe(&endpoint, true).unwrap();
    let _blocker = security::connect_verified(&endpoint).unwrap();
    occupied.connect().await.unwrap();
    let client = Client::new(endpoint.clone());
    let mut cancelled =
        Box::pin(client.request("bootstrap", serde_json::json!({"cancelled": true})));
    assert!(
        poll_fn(|cx| Poll::Ready(cancelled.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    drop(cancelled);

    let mut listener = security::create_pipe(&endpoint, false).unwrap();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(client.request("bootstrap", Value::Null), async {
            listener.connect().await.unwrap();
            let request: wire::Request = wire::read_frame(&mut listener).await.unwrap();
            assert_eq!(
                request.args,
                Value::Null,
                "The cancelled request must never be sent"
            );
            wire::write_response(
                &mut listener,
                &wire::Response {
                    result: Ok(Value::Null),
                },
            )
            .await
            .unwrap();
        })
    })
    .await
    .expect("A subsequent request must retain access to the listener");
    assert_eq!(result.unwrap(), Value::Null);
}

#[tokio::test]
async fn connection_lost_after_send_does_not_replay_the_operation() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let mut listener = security::create_pipe(&endpoint, true).unwrap();
    let client = Client::new(endpoint.clone());
    let (result, next) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(client.request("bootstrap", Value::Null), async move {
            listener.connect().await.unwrap();
            let request: wire::Request = wire::read_frame(&mut listener).await.unwrap();
            assert_eq!(request.command, "bootstrap");
            let next = security::create_pipe(&endpoint, false).unwrap();
            // The operation has arrived; lose its response while another listener is ready.
            drop(listener);
            next
        })
    })
    .await
    .expect("Losing the response must return an error instead of replaying the operation");
    let error = result.unwrap_err();
    assert!(error.contains("after the request was sent"), "{error}");
    let mut accepted = Box::pin(next.connect());
    assert!(
        poll_fn(|cx| Poll::Ready(accepted.as_mut().poll(cx)))
            .await
            .is_pending()
    );
}

#[tokio::test]
async fn busy_pipe_waits_for_listener_and_sends_request_once() {
    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    let occupied = security::create_pipe(&endpoint, true).unwrap();
    let _blocker = security::connect_verified(&endpoint).unwrap();
    occupied.connect().await.unwrap();
    assert_eq!(
        security::connect_verified(&endpoint)
            .unwrap_err()
            .raw_os_error(),
        Some(231),
        "The only pipe instance must be occupied before the request starts"
    );

    let client = Client::new(endpoint.clone());
    let mut request = Box::pin(client.request("bootstrap", serde_json::json!({"request": 1})));
    let first_poll = poll_fn(|cx| Poll::Ready(request.as_mut().poll(cx))).await;
    assert!(
        first_poll.is_pending(),
        "A temporarily occupied pipe must wait before sending: {first_poll:?}"
    );

    // Create the listener only after observing the request wait on the real busy pipe.
    let mut listener = security::create_pipe(&endpoint, false).unwrap();
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(request, async {
            listener.connect().await.unwrap();
            let received: wire::Request = wire::read_frame(&mut listener).await.unwrap();
            assert_eq!(received.protocol, wire::PROTOCOL);
            assert_eq!(received.command, "bootstrap");
            assert_eq!(received.args, serde_json::json!({"request": 1}));
            wire::write_response(
                &mut listener,
                &wire::Response {
                    result: Ok(serde_json::json!({"received": 1})),
                },
            )
            .await
            .unwrap();
            assert!(
                wire::read_frame::<wire::Request>(&mut listener)
                    .await
                    .is_err()
            );
        })
    })
    .await
    .expect("The request must complete once a listener is available");
    assert_eq!(result.unwrap(), serde_json::json!({"received": 1}));
}

#[tokio::test]
async fn tray_exit_closes_immediately_when_the_original_process_exited_and_its_pipe_is_missing() {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
            }
            let _ = self.0.wait();
        }
    }

    let mut child = ChildGuard(
        Command::new(
            app_runtime::windows_system_directory()
                .unwrap()
                .join("cmd.exe"),
        )
        .args(["/D", "/Q", "/K"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(0x0800_0000)
        .spawn()
        .expect("Start an isolated hidden process owned by this test"),
    );
    let pid = child.0.id();
    let identity = app_runtime::inspect_process_identity(pid).unwrap().unwrap();
    let target = app_runtime::ProcessExitTarget::capture(pid, &identity)
        .unwrap()
        .expect("Capture the original process while it is alive");
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"exit\r\n")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("The isolated command process must exit naturally");
    assert!(child.0.wait().unwrap().success());
    assert!(!target.is_running().unwrap());

    let endpoint = security::Endpoint::isolated(&uuid::Uuid::new_v4().to_string()).unwrap();
    assert_eq!(
        security::connect_verified(&endpoint.control())
            .unwrap_err()
            .raw_os_error(),
        Some(2),
    );
    let mut client = Client::new(endpoint);
    client.service_identity = Some(security::ServiceIdentity {
        pid,
        process: identity,
    });
    client.service_target = Some(Arc::new(target));
    let mut shutdown = Box::pin(client.stop_for_tray_exit(0));
    let first_poll = poll_fn(|cx| Poll::Ready(shutdown.as_mut().poll(cx))).await;
    assert!(
        matches!(first_poll, Poll::Ready(Ok(()))),
        "An exited original process must close without a status request or IPC wait: {first_poll:?}"
    );
    assert!(client.is_closing());
}
