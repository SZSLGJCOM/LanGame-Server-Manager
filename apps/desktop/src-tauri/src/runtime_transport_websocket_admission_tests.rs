use super::super::{
    DeadlineTcpStream, websocket_rcon_exchange, websocket_rcon_exec, websocket_read_frame,
    websocket_read_http_headers,
};
use super::*;
use std::io::{self, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread::{self, JoinHandle};

fn advance(at: AttemptTime, elapsed: Duration) -> AttemptTime {
    AttemptTime {
        monotonic: at.monotonic + elapsed,
        utc: at.utc + elapsed,
    }
}

#[test]
fn websocket_admission_requires_strict_expiry_on_both_clocks() {
    let completed = AttemptTime::now();
    assert!(
        completed
            .remaining(advance(completed, ATTEMPT_WINDOW))
            .is_some()
    );
    let expired = advance(completed, ATTEMPT_WINDOW + Duration::from_millis(1));
    assert!(completed.remaining(expired).is_none());
    assert!(
        completed
            .remaining(AttemptTime {
                monotonic: expired.monotonic,
                utc: completed.utc - Duration::from_secs(1),
            })
            .is_some()
    );
    assert!(
        completed
            .remaining(AttemptTime {
                monotonic: completed.monotonic,
                utc: expired.utc,
            })
            .is_some()
    );
}

#[test]
fn websocket_admission_bounds_pending_and_failed_attempts_without_blocking_other_endpoints() {
    let admission = ConnectionAdmission::default();
    let endpoint = "127.0.0.1:28016".parse().unwrap();
    let other_endpoint = "127.0.0.1:28017".parse().unwrap();
    let attempts = (0..ATTEMPTS_PER_WINDOW)
        .map(|_| admission.acquire(endpoint, Duration::from_secs(1)).unwrap())
        .collect::<Vec<_>>();
    // An unfinished connection cannot expire even after the native window.
    {
        let mut endpoints = admission.endpoints.lock().unwrap();
        endpoints
            .get_mut(&endpoint)
            .unwrap()
            .release_expired(advance(AttemptTime::now(), Duration::from_secs(60)));
    }
    let started = Instant::now();
    let error = admission
        .acquire(endpoint, Duration::from_millis(20))
        .err()
        .expect("all five attempts are still in flight");
    assert!(error.contains("before connecting; no command was sent"));
    assert!(started.elapsed() >= Duration::from_millis(20));
    assert!(started.elapsed() < Duration::from_secs(1));
    let independent = admission
        .acquire(other_endpoint, Duration::from_secs(1))
        .unwrap();
    let finished_after = AttemptTime::now();
    drop(attempts); // Failed attempts use the same guard drop as successful ones.
    {
        let endpoints = admission.endpoints.lock().unwrap();
        for slot in &endpoints[&endpoint].slots {
            let Slot::Finished(at) = slot else {
                panic!("a completed attempt must retain its allowance");
            };
            assert!(at.monotonic >= finished_after.monotonic);
        }
    }
    assert!(
        admission
            .acquire(endpoint, Duration::from_millis(20))
            .is_err()
    );
    drop(independent);
}

#[derive(Default)]
struct PeerReport {
    arrivals: Vec<Instant>,
    commands: Vec<String>,
    limited: usize,
    rejected_upgrade: usize,
}

// This peer models the native fixed window, not the client's conservative
// completion-based sliding window. Native OnClientConnect counts every TCP
// acceptance before authentication, and closes the sixth without an HTTP reply.
struct NativeLimitedPeer {
    endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<PeerReport>>,
}

impl NativeLimitedPeer {
    fn start(reject_first_upgrade: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut report = PeerReport::default();
            let mut window_start = None;
            let mut attempts = 0;
            while report.arrivals.len() < 6
                && !stopping.load(Ordering::Acquire)
                && Instant::now() < deadline
            {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                };
                report.arrivals.push(Instant::now());
                let now = SystemTime::now();
                let new_window = window_start.is_none_or(|start| {
                    now.duration_since(start).unwrap_or_default() > Duration::from_secs(5)
                });
                if new_window {
                    window_start = Some(now);
                    attempts = 1;
                } else {
                    attempts += 1;
                }
                if attempts > 5 {
                    report.limited += 1;
                    continue;
                }
                if reject_first_upgrade && report.arrivals.len() == 1 {
                    report.rejected_upgrade += 1;
                    continue;
                }
                serve_command(&mut stream, &mut report);
            }
            report
        });
        Self {
            endpoint,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) -> PeerReport {
        self.worker.take().unwrap().join().unwrap()
    }
}

impl Drop for NativeLimitedPeer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve_command(stream: &mut TcpStream, report: &mut PeerReport) {
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let headers = websocket_read_http_headers(stream).unwrap();
    assert!(headers.starts_with("GET /fixture-password HTTP/1.1\r\n"));
    stream
        .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n")
        .unwrap();
    let frame = websocket_read_frame(stream).unwrap();
    let request: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
    let command = request["Message"].as_str().unwrap().to_string();
    report.commands.push(command.clone());
    let response = serde_json::json!({
        "Identifier": request["Identifier"],
        "Message": format!("accepted {command}"),
        "Type": "Generic",
    })
    .to_string();
    assert!(response.len() < 126);
    stream.write_all(&[0x81, response.len() as u8]).unwrap();
    stream.write_all(response.as_bytes()).unwrap();
}

#[test]
fn websocket_admission_control_unpaced_sixth_connection_is_rejected() {
    let peer = NativeLimitedPeer::start(false);
    for index in 0..6 {
        // The old entry point connected directly, using this same wire codec.
        let mut stream =
            DeadlineTcpStream::connect(&peer.endpoint, Duration::from_secs(3)).unwrap();
        let result = websocket_rcon_exchange(
            &mut stream,
            &peer.endpoint,
            "fixture-password",
            &format!("query-{index}"),
        );
        if index < 5 {
            assert_eq!(result.unwrap(), format!("accepted query-{index}"));
        } else {
            assert!(result.unwrap_err().contains("handshake"));
        }
    }
    let report = peer.finish();
    assert_eq!(report.arrivals.len(), 6);
    assert_eq!(report.limited, 1);
    assert_eq!(
        report.commands,
        (0..5)
            .map(|index| format!("query-{index}"))
            .collect::<Vec<_>>()
    );
}

#[test]
fn websocket_admission_six_command_burst_sends_each_once() {
    let peer = NativeLimitedPeer::start(false);
    let commands = (0..6)
        .map(|index| format!("query-{index}"))
        .collect::<Vec<_>>();
    for command in &commands {
        assert_eq!(
            websocket_rcon_exec(&peer.endpoint, "fixture-password", command).unwrap(),
            format!("accepted {command}")
        );
    }
    let report = peer.finish();
    assert_eq!(report.arrivals.len(), 6);
    assert_eq!(report.limited, 0);
    assert_eq!(report.commands, commands);
    assert!(report.arrivals[5].duration_since(report.arrivals[0]) > Duration::from_secs(5));
}

#[test]
fn websocket_admission_concurrent_failed_upgrade_is_counted_and_never_retried() {
    let peer = NativeLimitedPeer::start(true);
    let ready = Arc::new(Barrier::new(7));
    let workers = (0..6)
        .map(|index| {
            let endpoint = peer.endpoint.clone();
            let ready = Arc::clone(&ready);
            thread::spawn(move || {
                let command = format!("query-{index}");
                ready.wait();
                let result = websocket_rcon_exec(&endpoint, "fixture-password", &command);
                (command, result)
            })
        })
        .collect::<Vec<_>>();
    ready.wait();
    let mut succeeded = Vec::new();
    let mut errors = Vec::new();
    for worker in workers {
        let (command, result) = worker.join().unwrap();
        match result {
            Ok(response) => {
                assert_eq!(response, format!("accepted {command}"));
                succeeded.push(command);
            }
            Err(error) => errors.push(error),
        }
    }
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("handshake"));
    let mut report = peer.finish();
    assert_eq!(report.arrivals.len(), 6);
    assert_eq!(report.rejected_upgrade, 1);
    assert_eq!(report.limited, 0);
    succeeded.sort();
    report.commands.sort();
    assert_eq!(report.commands, succeeded);
    assert_eq!(report.commands.len(), 5);
}

#[test]
fn websocket_admission_queue_deadline_prevents_late_connect_or_send() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = listener.local_addr().unwrap().to_string();
    let attempts = (0..ATTEMPTS_PER_WINDOW)
        .map(|_| admit(&endpoint).unwrap())
        .collect::<Vec<_>>();
    let started = Instant::now();
    let error = websocket_rcon_exec(&endpoint, "fixture-password", "must-not-send").unwrap_err();
    assert!(error.contains("before connecting; no command was sent"));
    assert!(started.elapsed() >= ADMISSION_BUDGET);
    assert!(started.elapsed() < ADMISSION_BUDGET + Duration::from_secs(1));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    drop(attempts);
}
