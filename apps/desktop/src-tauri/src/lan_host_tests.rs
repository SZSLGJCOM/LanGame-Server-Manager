use super::*;
use base64::Engine;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Instant;

#[path = "lan_host_reliability_tests.rs"]
mod reliability;

#[test]
fn module_details_inventory_option_requires_boolean_and_preserves_full_default() {
    let decode = |args: &Value| {
        opt_arg::<bool>(
            args,
            "includePreservedProgramCounts",
            "include_preserved_program_counts",
        )
    };
    assert_eq!(decode(&json!({})).unwrap(), None);
    for key in [
        "includePreservedProgramCounts",
        "include_preserved_program_counts",
    ] {
        let mut args = json!({ "moduleId": "astroneer" });
        args[key] = Value::Null;
        assert_eq!(decode(&args).unwrap(), None);
        for include in [false, true] {
            args[key] = json!(include);
            assert_eq!(decode(&args).unwrap(), Some(include));
        }
        for invalid in [json!("false"), json!(0), json!([]), json!({})] {
            args[key] = invalid;
            assert!(decode(&args).is_err(), "invalid inventory option: {args}");
        }
    }
}

#[test]
fn start_world_confirmation_decodes_both_wire_names_without_silent_fallback() {
    use crate::commands::commands_dst_world_state::DstWorldStartPreview;

    let preview = json!({
        "instance_id": "world-a",
        "settings_json": "{\"caves_enabled\":true}",
        "shards": [
            { "shard": "Master", "state": "existing", "enabled": true },
            { "shard": "Caves", "state": "new", "enabled": true }
        ]
    });
    for key in ["expectedWorldStart", "expected_world_start"] {
        let mut args = json!({ "instanceId": "world-a" });
        args[key] = preview.clone();
        let actual =
            opt_arg::<DstWorldStartPreview>(&args, "expectedWorldStart", "expected_world_start")
                .unwrap()
                .expect("Confirmed world state must survive forwarding");
        assert_eq!(serde_json::to_value(actual).unwrap(), preview);
        args[key] = json!({ "instance_id": "world-a" });
        assert!(
            opt_arg::<DstWorldStartPreview>(&args, "expectedWorldStart", "expected_world_start")
                .is_err()
        );
    }
    for args in [json!({}), json!({ "expectedWorldStart": null })] {
        assert!(
            opt_arg::<DstWorldStartPreview>(&args, "expectedWorldStart", "expected_world_start")
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn lan_responses_protect_executable_content_and_private_management_results() {
    for content_type in [
        "application/json; charset=utf-8",
        "text/html; charset=utf-8",
    ] {
        let mut output = Vec::new();
        write_response(
            &mut output,
            ResponseSpec::new(200, content_type, b"fixture"),
            ResponseWritePolicy::bounded(Duration::from_secs(1)),
        )
        .unwrap();
        let response = String::from_utf8(output).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert_eq!(body, "fixture");
        assert!(headers.contains("script-src 'self'"));
        assert!(headers.contains("worker-src 'self' blob:"));
        assert!(headers.contains("form-action 'none'"));
        assert!(headers.contains("frame-ancestors 'none'"));
        assert!(headers.contains("Referrer-Policy: no-referrer"));
        assert_eq!(
            headers.contains("Cache-Control: no-store"),
            content_type.starts_with("application/json")
        );
    }
}

#[cfg(windows)]
#[test]
fn lan_static_requests_cannot_read_linked_private_files_or_ntfs_streams() {
    use std::os::windows::process::CommandExt;

    let root = env::temp_dir().join(format!("langame-lan-boundary-{}", uuid::Uuid::new_v4()));
    let dist = root.join("dist");
    fs::create_dir_all(&dist).unwrap();
    fs::write(dist.join("index.html"), b"<main>management</main>").unwrap();
    fs::write(root.join("private.txt"), b"private fixture").unwrap();
    fs::write(dist.join("index.html:private"), b"private stream fixture").unwrap();
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(dist.join("linked"))
        .arg(&root)
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .output()
        .unwrap();
    assert!(output.status.success(), "create isolated junction fixture");
    for path in [
        "/linked/private.txt",
        "/index.html:private",
        "/index.html%3Aprivate",
    ] {
        for head in [false, true] {
            let response = read_static_response(&dist, path, head);
            assert!(response.starts_with("HTTP/1.1 404 Not Found"));
            assert!(!response.contains("fixture"));
        }
    }
    fs::remove_dir(dist.join("linked")).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn lan_command_future_stack_budget() {
    fn future_size2<A, B, F, Fut>(_: F) -> usize
    where
        F: FnOnce(A, B) -> Fut,
        Fut: std::future::Future,
    {
        std::mem::size_of::<Fut>()
    }
    fn future_size3<A, B, C, F, Fut>(_: F) -> usize
    where
        F: FnOnce(A, B, C) -> Fut,
        Fut: std::future::Future,
    {
        std::mem::size_of::<Fut>()
    }
    let dispatch_bytes = future_size3(dispatch_command);
    let refresh_bytes = future_size2(commands::refresh_instance_live_players);
    let runtime_bytes = future_size2(commands::send_instance_runtime_command);
    let gm_bytes = future_size2(commands::send_instance_gm_command);
    eprintln!(
        "LAN dispatch future: {dispatch_bytes} bytes; player refresh future: {refresh_bytes} bytes; runtime command future: {runtime_bytes} bytes; GM command future: {gm_bytes} bytes"
    );
    assert!(
        dispatch_bytes <= 8 * 1024,
        "LAN command dispatch must not place the largest command future on the worker stack: {dispatch_bytes} bytes"
    );
    assert!(runtime_bytes <= 4 * 1024);
    assert!(gm_bytes <= 4 * 1024);
}

#[test]
fn lan_command_factory_polls_large_futures_on_a_default_worker_stack() {
    let worker = std::thread::Builder::new()
        .name(String::from("lan-command-stack-regression"))
        .spawn(|| {
            tauri::async_runtime::block_on(boxed_command(|| async {
                let state = [42_u8; 128 * 1024];
                tokio::task::yield_now().await;
                Ok(json!({ "last": std::hint::black_box(&state)[state.len() - 1] }))
            }))
        })
        .expect("spawn a worker with the same default stack as the LAN host");
    assert_eq!(worker.join().unwrap().unwrap(), json!({ "last": 42 }));
}

fn token_text(byte: u8) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([byte; 32])
}

fn access() -> LanHostAccess {
    LanHostAccess {
        management_token: LanAccessToken::parse(&token_text(7)).unwrap(),
    }
}

fn wait_until(label: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn request(method: &str, path: &str, management_header: Option<&str>) -> HttpRequestHead {
    let mut headers = HashMap::new();
    if let Some(value) = management_header {
        headers.insert("x-langame-token".to_owned(), value.to_owned());
    }
    HttpRequestHead {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        buffered_body: Vec::new(),
    }
}

#[test]
fn lan_host_config_rejects_missing_empty_and_non_32_byte_tokens() {
    assert!(required_lan_access_token(None).is_err());
    for value in ["", "0123456789abcdef0123456789abcdef", "AQ=="] {
        assert!(required_lan_access_token(Some(value)).is_err());
    }
    assert!(required_lan_access_token(Some(&token_text(7))).is_ok());
}

#[test]
fn lan_api_accepts_only_the_management_header() {
    let access = access();
    let management_token = token_text(7);
    let unrelated_token = token_text(11);

    for path in [
        format!("/__langame/api?langameToken={management_token}"),
        format!("/__langame/api?token={management_token}"),
        format!("/__langame/api?ignoredToken={unrelated_token}"),
    ] {
        assert!(!api_request_is_authorized(
            &access,
            &request("POST", &path, None),
        ));
    }
    assert!(!api_request_is_authorized(
        &access,
        &request("POST", "/__langame/api", Some(&unrelated_token)),
    ));
    assert!(api_request_is_authorized(
        &access,
        &request("POST", "/__langame/api", Some(&management_token)),
    ));
}

#[test]
fn lan_host_live_player_arguments_accept_camel_and_snake_aliases_without_a_target() {
    assert_eq!(
        arg::<String>(
            &json!({ "instanceId": "instance-camel" }),
            "instanceId",
            "instance_id",
        )
        .expect("camel instance id"),
        "instance-camel"
    );
    assert_eq!(
        arg::<String>(
            &json!({ "instance_id": "instance-snake" }),
            "instanceId",
            "instance_id",
        )
        .expect("snake instance id"),
        "instance-snake"
    );

    let input = arg::<ExecuteInstancePlayerActionInput>(
        &json!({
            "input": {
                "instance_id": "instance-a",
                "snapshot_id": "snapshot-a",
                "player_key": "player-a",
                "action_id": "kick_userid"
            }
        }),
        "input",
        "input",
    )
    .expect("safe live-player action input");
    assert_eq!(input.player_key, "player-a");

    assert!(
        arg::<ExecuteInstancePlayerActionInput>(
            &json!({
                "input": {
                    "instance_id": "instance-a",
                    "snapshot_id": "snapshot-a",
                    "player_key": "player-a",
                    "action_id": "kick_userid",
                    "target": "KU_injected"
                }
            }),
            "input",
            "input",
        )
        .is_err(),
        "LAN action input must not accept a client-supplied target"
    );
}

#[test]
fn lan_worker_pool_never_exceeds_its_fixed_concurrency() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));

    let handler_active = Arc::clone(&active);
    let handler_peak = Arc::clone(&peak);
    let handler_processed = Arc::clone(&processed);
    let handler_release = Arc::clone(&release);
    let handler: ConnectionHandler = Arc::new(move |_stream| {
        let current = handler_active.fetch_add(1, Ordering::SeqCst) + 1;
        handler_peak.fetch_max(current, Ordering::SeqCst);
        while !handler_release.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        handler_active.fetch_sub(1, Ordering::SeqCst);
        handler_processed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let server = thread::spawn(move || {
        run_lan_listener(
            listener,
            handler,
            shutdown_check,
            2,
            8,
            Duration::from_millis(5),
        )
    });

    let clients = (0..6)
        .map(|_| TcpStream::connect(address).unwrap())
        .collect::<Vec<_>>();
    wait_until("both LAN workers to become active", || {
        active.load(Ordering::SeqCst) == 2
    });
    assert_eq!(peak.load(Ordering::SeqCst), 2);

    release.store(true, Ordering::SeqCst);
    wait_until("all queued LAN requests to complete", || {
        processed.load(Ordering::SeqCst) == clients.len()
    });
    shutdown.store(true, Ordering::SeqCst);
    let summary = server.join().unwrap().unwrap();

    assert_eq!(summary.accepted_connections, clients.len());
    assert_eq!(summary.rejected_connections, 0);
    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[test]
fn lan_worker_pool_rejects_connections_when_its_queue_is_full() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));

    let handler_active = Arc::clone(&active);
    let handler_release = Arc::clone(&release);
    let handler: ConnectionHandler = Arc::new(move |_stream| {
        handler_active.fetch_add(1, Ordering::SeqCst);
        while !handler_release.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }
        handler_active.fetch_sub(1, Ordering::SeqCst);
        Ok(())
    });
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let server = thread::spawn(move || {
        run_lan_listener(
            listener,
            handler,
            shutdown_check,
            1,
            1,
            Duration::from_millis(5),
        )
    });

    let active_client = TcpStream::connect(address).unwrap();
    wait_until("the only LAN worker to become active", || {
        active.load(Ordering::SeqCst) == 1
    });
    let queued_client = TcpStream::connect(address).unwrap();
    thread::sleep(Duration::from_millis(75));
    let mut rejected_client = TcpStream::connect(address).unwrap();
    rejected_client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut response = String::new();
    rejected_client.read_to_string(&mut response).unwrap();

    assert!(response.starts_with("HTTP/1.1 503 Service Unavailable"));
    shutdown.store(true, Ordering::SeqCst);
    release.store(true, Ordering::SeqCst);
    let summary = server.join().unwrap().unwrap();
    assert!(summary.rejected_connections >= 1);

    drop(active_client);
    drop(queued_client);
}

#[test]
fn lan_listener_joins_idle_workers_after_shutdown() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let handler: ConnectionHandler = Arc::new(|_stream| Ok(()));
    let server = thread::spawn(move || {
        run_lan_listener(
            listener,
            handler,
            shutdown_check,
            3,
            4,
            Duration::from_millis(5),
        )
    });

    thread::sleep(Duration::from_millis(25));
    shutdown.store(true, Ordering::SeqCst);
    let summary = server.join().unwrap().unwrap();
    assert_eq!(summary, LanHostRunSummary::default());
}

#[test]
fn lan_listener_shutdown_interrupts_an_incomplete_request_header() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let reading_header = Arc::new(AtomicBool::new(false));

    let handler_shutdown = Arc::clone(&shutdown);
    let handler_reading_header = Arc::clone(&reading_header);
    let handler: ConnectionHandler = Arc::new(move |mut stream| {
        stream
            .set_read_timeout(Some(CONNECTION_READ_POLL_INTERVAL))
            .unwrap();
        handler_reading_header.store(true, Ordering::SeqCst);
        let shutdown_check: ShutdownCheck = {
            let shutdown = Arc::clone(&handler_shutdown);
            Arc::new(move || shutdown.load(Ordering::SeqCst))
        };
        read_request_head(
            &mut stream,
            &shutdown_check,
            Instant::now() + CONNECTION_TIMEOUT,
        )
        .map(|_| ())
    });
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let (done_sender, done_receiver) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let result = run_lan_listener(
            listener,
            handler,
            shutdown_check,
            1,
            1,
            Duration::from_millis(5),
        );
        done_sender.send(result).unwrap();
    });

    let mut client = TcpStream::connect(address).unwrap();
    client.write_all(b"GET / HTTP/1.1\r\nX-Slow: ").unwrap();
    wait_until("LAN worker to start reading the request header", || {
        reading_header.load(Ordering::SeqCst)
    });
    shutdown.store(true, Ordering::SeqCst);

    let summary = done_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("LAN host shutdown must interrupt a partial request header")
        .unwrap();
    server.join().unwrap();
    assert_eq!(summary.accepted_connections, 1);
}

#[test]
fn lan_listener_shutdown_interrupts_an_authenticated_incomplete_request_body() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let reading_body = Arc::new(AtomicBool::new(false));

    let handler_shutdown = Arc::clone(&shutdown);
    let handler_reading_body = Arc::clone(&reading_body);
    let handler: ConnectionHandler = Arc::new(move |mut stream| {
        stream
            .set_read_timeout(Some(CONNECTION_READ_POLL_INTERVAL))
            .unwrap();
        let shutdown_check: ShutdownCheck = {
            let shutdown = Arc::clone(&handler_shutdown);
            Arc::new(move || shutdown.load(Ordering::SeqCst))
        };
        let deadline = Instant::now() + CONNECTION_TIMEOUT;
        let request = read_request_head(&mut stream, &shutdown_check, deadline)?;
        let content_length =
            preflight_api_request(&mut stream, &request, &access(), &shutdown_check)?
                .ok_or_else(|| String::from("authenticated request was rejected"))?;
        handler_reading_body.store(true, Ordering::SeqCst);
        read_request_body(
            &mut stream,
            &request.buffered_body,
            content_length,
            &shutdown_check,
            deadline,
        )
        .map(|_| ())
    });
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let (done_sender, done_receiver) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let result = run_lan_listener(
            listener,
            handler,
            shutdown_check,
            1,
            1,
            Duration::from_millis(5),
        );
        done_sender.send(result).unwrap();
    });

    let mut client = TcpStream::connect(address).unwrap();
    let request = format!(
        "POST /__langame/api HTTP/1.1\r\nHost: localhost\r\nX-LanGame-Token: {}\r\nContent-Length: 128\r\n\r\n{{",
        token_text(7)
    );
    client.write_all(request.as_bytes()).unwrap();
    wait_until("LAN worker to start reading the request body", || {
        reading_body.load(Ordering::SeqCst)
    });
    shutdown.store(true, Ordering::SeqCst);

    let summary = done_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("LAN host shutdown must interrupt a partial request body")
        .unwrap();
    server.join().unwrap();
    assert_eq!(summary.accepted_connections, 1);
}

#[test]
fn unauthorized_large_api_request_is_rejected_before_the_body_is_read() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let shutdown_check: ShutdownCheck = Arc::new(|| false);
        let request = read_request_head(
            &mut stream,
            &shutdown_check,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert!(
            preflight_api_request(&mut stream, &request, &access(), &shutdown_check)
                .unwrap()
                .is_none()
        );
    });

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let request = format!(
        "POST /__langame/api HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
        MAX_REQUEST_BODY_BYTES + 1
    );
    client.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();

    assert!(response.starts_with("HTTP/1.1 401 Unauthorized"));
    server.join().unwrap();
}

#[test]
fn authorized_oversized_api_request_is_rejected_before_the_body_is_read() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let shutdown_check: ShutdownCheck = Arc::new(|| false);
        let request = read_request_head(
            &mut stream,
            &shutdown_check,
            Instant::now() + Duration::from_secs(2),
        )
        .unwrap();
        assert!(
            preflight_api_request(&mut stream, &request, &access(), &shutdown_check)
                .unwrap()
                .is_none()
        );
    });

    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let request = format!(
        "POST /__langame/api HTTP/1.1\r\nHost: localhost\r\nX-LanGame-Token: {}\r\nContent-Length: {}\r\n\r\n",
        token_text(7),
        MAX_REQUEST_BODY_BYTES + 1
    );
    client.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();

    assert!(response.starts_with("HTTP/1.1 413 Payload Too Large"));
    server.join().unwrap();
}

#[test]
fn lan_listener_shutdown_interrupts_a_blocked_large_response() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let shutdown = Arc::new(AtomicBool::new(false));
    let writing_response = Arc::new(AtomicBool::new(false));
    let response_body = Arc::new(vec![b'x'; 32 * 1024 * 1024]);

    let handler_shutdown = Arc::clone(&shutdown);
    let handler_writing_response = Arc::clone(&writing_response);
    let handler_response_body = Arc::clone(&response_body);
    let handler: ConnectionHandler = Arc::new(move |mut stream| {
        stream
            .set_write_timeout(Some(CONNECTION_READ_POLL_INTERVAL))
            .unwrap();
        let shutdown_check: ShutdownCheck = {
            let shutdown = Arc::clone(&handler_shutdown);
            Arc::new(move || shutdown.load(Ordering::SeqCst))
        };
        handler_writing_response.store(true, Ordering::SeqCst);
        write_response(
            &mut stream,
            ResponseSpec::new(
                200,
                "application/octet-stream",
                handler_response_body.as_slice(),
            ),
            ResponseWritePolicy::connection(&shutdown_check),
        )
    });
    let worker_shutdown = Arc::clone(&shutdown);
    let shutdown_check: ShutdownCheck = Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
    let (done_sender, done_receiver) = std::sync::mpsc::channel();
    let server = thread::spawn(move || {
        let result = run_lan_listener(
            listener,
            handler,
            shutdown_check,
            1,
            1,
            Duration::from_millis(5),
        );
        done_sender.send(result).unwrap();
    });

    let client = TcpStream::connect(address).unwrap();
    wait_until("LAN worker to start writing the response", || {
        writing_response.load(Ordering::SeqCst)
    });
    thread::sleep(Duration::from_millis(50));
    assert!(
        matches!(
            done_receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ),
        "large response unexpectedly completed before shutdown"
    );
    shutdown.store(true, Ordering::SeqCst);

    let summary = done_receiver
        .recv_timeout(Duration::from_secs(2))
        .expect("LAN host shutdown must interrupt a blocked response write")
        .unwrap();
    server.join().unwrap();
    assert_eq!(summary.accepted_connections, 1);
    drop(client);
}

#[test]
fn lan_responses_do_not_enable_cross_origin_requests() {
    use std::net::{Shutdown, TcpListener, TcpStream};

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        write_response(
            &mut stream,
            ResponseSpec::new(405, "text/plain", b""),
            ResponseWritePolicy::bounded(CONNECTION_TIMEOUT),
        )
        .unwrap();
    });
    let mut client = TcpStream::connect(address).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap();

    let lower = response.to_ascii_lowercase();
    assert!(!lower.contains("access-control-allow-origin"));
    assert!(!lower.contains("access-control-allow-headers"));
    assert!(!lower.contains("access-control-allow-methods"));
}

fn read_static_response(dist_dir: &Path, request_path: &str, head_only: bool) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let dist_dir = dist_dir.to_owned();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let shutdown_check: ShutdownCheck = Arc::new(|| false);
        let request = read_request_head(
            &mut stream,
            &shutdown_check,
            Instant::now() + Duration::from_secs(2),
        )?;
        serve_static_file(
            &mut stream,
            &dist_dir,
            &request.path,
            request.method == "HEAD",
            &shutdown_check,
        )
    });
    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let method = if head_only { "HEAD" } else { "GET" };
    write!(
        client,
        "{method} {request_path} HTTP/1.1\r\nHost: localhost\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    server.join().unwrap().unwrap();
    response
}

#[test]
fn lan_static_head_preserves_get_metadata_without_sending_a_body() {
    let fixture = env::temp_dir().join(format!("langame-lan-head-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&fixture).unwrap();
    fs::write(fixture.join("index.html"), b"<main>LAN management</main>").unwrap();
    fs::create_dir(fixture.join("assets")).unwrap();
    let get = read_static_response(&fixture, "/", false);
    let head = read_static_response(&fixture, "/", true);
    assert_eq!(read_static_response(&fixture, "/assets/", false), get);
    assert_eq!(read_static_response(&fixture, "/missing-route", false), get);
    fs::remove_dir_all(&fixture).unwrap();

    let (get_headers, get_body) = get.split_once("\r\n\r\n").unwrap();
    let (head_headers, head_body) = head.split_once("\r\n\r\n").unwrap();
    assert_eq!(
        head_headers, get_headers,
        "HEAD describes the same representation as GET"
    );
    assert_eq!(get_body, "<main>LAN management</main>");
    assert!(
        head_body.is_empty(),
        "HEAD must never send the representation body"
    );
}

#[test]
fn lan_static_management_page_denies_framing() {
    let fixture = env::temp_dir().join(format!("langame-lan-framing-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&fixture).unwrap();
    fs::write(fixture.join("index.html"), b"<main>LAN management</main>").unwrap();
    let response = read_static_response(&fixture, "/", false);
    fs::remove_dir_all(&fixture).unwrap();

    let headers = response
        .split_once("\r\n\r\n")
        .unwrap()
        .0
        .to_ascii_lowercase();
    let content_security_policy = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-security-policy: "))
        .expect("management page content policy");
    assert!(
        content_security_policy
            .split(';')
            .any(|directive| directive.trim() == "frame-ancestors 'none'")
    );
    assert!(headers.contains("\r\nx-frame-options: deny"));
}
