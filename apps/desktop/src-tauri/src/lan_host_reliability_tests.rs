use super::*;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Receiver};

// One worker makes a leaked slot observable: the next ordinary request cannot
// succeed unless the previous handler returned. All traffic stays on loopback.
struct ReliabilityHost {
    address: SocketAddr,
    shutdown: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    completed: Receiver<Result<(), String>>,
    done: Receiver<Result<LanHostRunSummary, String>>,
    server: Option<thread::JoinHandle<()>>,
}

impl ReliabilityHost {
    fn start(
        operation: impl Fn(&mut TcpStream, &ShutdownCheck) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let shutdown_check: ShutdownCheck =
            Arc::new(move || worker_shutdown.load(Ordering::SeqCst));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let handler_active = Arc::clone(&active);
        let handler_peak = Arc::clone(&peak);
        let handler_shutdown = Arc::clone(&shutdown_check);
        let (completed_tx, completed) = mpsc::channel();
        let handler: ConnectionHandler = Arc::new(move |mut stream| {
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_millis(20)))
                .unwrap();
            let count = handler_active.fetch_add(1, Ordering::SeqCst) + 1;
            handler_peak.fetch_max(count, Ordering::SeqCst);
            let result = operation(&mut stream, &handler_shutdown);
            handler_active.fetch_sub(1, Ordering::SeqCst);
            // The receiver may have gone away during assertion unwinding.
            let _ = completed_tx.send(result.clone());
            result
        });
        let (done_tx, done) = mpsc::channel();
        let server = thread::spawn(move || {
            let result = run_lan_listener(
                listener,
                handler,
                shutdown_check,
                1,
                1,
                Duration::from_millis(2),
            );
            let _ = done_tx.send(result);
        });
        Self {
            address,
            shutdown,
            active,
            peak,
            completed,
            done,
            server: Some(server),
        }
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect(self.address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
    }

    fn completion(&self) -> Result<(), String> {
        let result = self
            .completed
            .recv_timeout(Duration::from_secs(3))
            .expect("the handler must release its worker within the test deadline");
        assert_eq!(self.active.load(Ordering::SeqCst), 0);
        result
    }

    fn healthy_request(&self) {
        let mut client = self.connect();
        client
            .write_all(b"GET /healthy HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with("\r\n\r\nok"));
        self.completion().unwrap();
    }

    fn finish(mut self) -> LanHostRunSummary {
        self.shutdown.store(true, Ordering::SeqCst);
        let summary = self
            .done
            .recv_timeout(Duration::from_secs(3))
            .expect("listener and worker must join after the failure scenarios")
            .unwrap();
        self.server.take().unwrap().join().unwrap();
        assert_eq!(self.active.load(Ordering::SeqCst), 0);
        assert_eq!(self.peak.load(Ordering::SeqCst), 1);
        summary
    }
}

impl Drop for ReliabilityHost {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(server) = self.server.take()
            && self.done.recv_timeout(Duration::from_secs(3)).is_ok()
        {
            let _ = server.join();
        }
    }
}

fn read_and_reply(stream: &mut TcpStream, shutdown: &ShutdownCheck) -> Result<(), String> {
    read_and_reply_before(
        stream,
        shutdown,
        Instant::now() + Duration::from_millis(250),
    )
}

fn read_and_reply_before(
    stream: &mut TcpStream,
    shutdown: &ShutdownCheck,
    deadline: Instant,
) -> Result<(), String> {
    let head = read_request_head(stream, shutdown, deadline)?;
    let length = request_content_length(&head)?;
    read_request_body(stream, &head.buffered_body, length, shutdown, deadline)?;
    write_response(
        stream,
        ResponseSpec::new(200, "text/plain", b"ok"),
        ResponseWritePolicy::bounded(Duration::from_secs(1)),
    )
}

#[test]
fn reliability_lan_disconnects_and_malformed_reads_return_to_the_idle_baseline() {
    let host = ReliabilityHost::start(read_and_reply);
    for _ in 0..8 {
        for request in [
            b"GET / HTTP/1.1\r\nX-Incomplete: ".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 32\r\n\r\n{".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: invalid\r\n\r\n".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 2\r\n\r\n".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n"
                .as_slice(),
        ] {
            let mut client = host.connect();
            client.write_all(request).unwrap();
            client.shutdown(Shutdown::Write).unwrap();
            assert!(host.completion().is_err());
            drop(client);
            host.healthy_request();
        }
    }
    let summary = host.finish();
    assert_eq!(summary.accepted_connections, 80);
    assert_eq!(summary.rejected_connections, 0);
}

#[test]
fn reliability_lan_incomplete_header_and_body_deadlines_release_the_worker() {
    let host = ReliabilityHost::start(read_and_reply);
    for request in [
        b"GET / HTTP/1.1\r\nX-Wait: ".as_slice(),
        b"POST / HTTP/1.1\r\nContent-Length: 32\r\n\r\n{".as_slice(),
    ] {
        let mut client = host.connect();
        client.write_all(request).unwrap();
        assert!(host.completion().unwrap_err().contains("timed out"));
        drop(client);
        host.healthy_request();
    }
    assert_eq!(host.finish().accepted_connections, 4);
}

#[test]
fn reliability_lan_overload_then_recovery_reuses_the_same_worker() {
    let (entered_tx, entered) = mpsc::channel();
    let host = ReliabilityHost::start(move |stream, shutdown| {
        entered_tx.send(()).unwrap();
        read_and_reply_before(stream, shutdown, Instant::now() + Duration::from_secs(3))
    });
    let mut blocked = host.connect();
    blocked.write_all(b"GET /blocked HTTP/1.1\r\n").unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    let mut queued = host.connect();
    queued
        .write_all(b"GET /queued HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut rejected = host.connect();
    let mut response = String::new();
    rejected.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503 Service Unavailable\r\n"));
    assert_eq!(host.peak.load(Ordering::SeqCst), 1);
    blocked.shutdown(Shutdown::Write).unwrap();
    // The next queued handler may already be running when the first result is
    // observed, so assert the idle baseline only after both have completed.
    assert!(
        host.completed
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .is_err()
    );
    let mut queued_response = String::new();
    queued.read_to_string(&mut queued_response).unwrap();
    assert!(queued_response.ends_with("\r\n\r\nok"));
    host.completion().unwrap();
    host.healthy_request();
    let summary = host.finish();
    assert_eq!(summary.accepted_connections, 4);
    assert_eq!(summary.rejected_connections, 1);
}

// Inject retryable errors around the real socket without relying on Windows
// loopback buffering to become saturated. Production owns every retry/deadline.
const RESPONSE_PREFIX: &str = "HTTP/1.1 200 OK\r\n";

struct StalledResponseWriter<'a> {
    stream: &'a mut TcpStream,
    shutdown: &'a ShutdownCheck,
    stall_writes: bool,
    written: usize,
    would_block: usize,
    timed_out: usize,
}

impl StalledResponseWriter<'_> {
    fn stall(&mut self) -> std::io::Result<()> {
        // A failed deadline assertion must still let fixture Drop join the
        // worker, even when the production retry loop itself has regressed.
        if (self.shutdown)() {
            return Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted));
        }
        let kind = if self.would_block == self.timed_out {
            self.would_block += 1;
            std::io::ErrorKind::WouldBlock
        } else {
            self.timed_out += 1;
            std::io::ErrorKind::TimedOut
        };
        thread::yield_now();
        Err(std::io::Error::from(kind))
    }
}

impl Write for StalledResponseWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.stall_writes && self.written >= RESPONSE_PREFIX.len() {
            return self.stall().map(|()| 0);
        }
        let limit = if self.stall_writes {
            bytes.len().min(RESPONSE_PREFIX.len() - self.written)
        } else {
            bytes.len()
        };
        let written = self.stream.write(&bytes[..limit])?;
        self.written += written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stall()
    }
}

#[test]
fn reliability_lan_write_disconnect_and_deadline_leave_the_worker_usable() {
    let (writing_tx, writing) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let continue_rx = Mutex::new(continue_rx);
    let host = ReliabilityHost::start(move |stream, shutdown| {
        let head = read_request_head(stream, shutdown, Instant::now() + Duration::from_secs(1))?;
        if head.path == "/healthy" {
            return write_response(
                stream,
                ResponseSpec::new(200, "text/plain", b"ok"),
                ResponseWritePolicy::bounded(Duration::from_secs(1)),
            );
        }
        writing_tx.send(()).unwrap();
        continue_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let response = ResponseSpec::new(200, "text/plain", b"deterministic LAN response");
        if head.path == "/disconnect" {
            // Observe the abortive close before writing, so packet delivery
            // timing cannot let a tiny response race ahead of the TCP reset.
            let deadline = Instant::now() + Duration::from_secs(1);
            loop {
                match stream.read(&mut [0u8; 1]) {
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        break;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) && Instant::now() < deadline => {}
                    result => panic!("abortive client close was not observed: {result:?}"),
                }
            }
            return write_response(
                stream,
                response,
                ResponseWritePolicy::bounded(Duration::from_millis(250)),
            );
        }
        let mut writer = StalledResponseWriter {
            stream,
            shutdown,
            stall_writes: head.path == "/write-deadline",
            written: 0,
            would_block: 0,
            timed_out: 0,
        };
        let policy = ResponseWritePolicy::bounded(Duration::from_millis(250));
        let result = write_response(&mut writer, response, policy);
        assert!(writer.written > 0, "fault must follow a real socket write");
        assert!(
            writer.would_block > 0 && writer.timed_out > 0,
            "both retryable errors must reach the production loop"
        );
        assert!(
            Instant::now() >= policy.deadline,
            "retryable errors must not terminate early"
        );
        if writer.stall_writes {
            assert_eq!(writer.written, RESPONSE_PREFIX.len());
        } else {
            assert!(
                writer.written > response.body.len(),
                "headers and body precede the stalled flush"
            );
        }
        result
    });
    for path in ["/disconnect", "/write-deadline", "/flush-deadline"] {
        let mut client = host.connect();
        write!(client, "GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        writing.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut client = Some(client);
        if path == "/disconnect" {
            socket2::SockRef::from(client.as_ref().unwrap())
                .set_linger(Some(Duration::ZERO))
                .unwrap();
            drop(client.take());
        }
        continue_tx.send(()).unwrap();
        let error = host.completion().unwrap_err();
        if path == "/disconnect" {
            assert!(
                !error.contains("response timed out"),
                "the disconnected peer must fail the write before its deadline: {error}"
            );
        } else {
            assert_eq!(error, "LAN response timed out");
            let mut response = String::new();
            client
                .as_mut()
                .unwrap()
                .read_to_string(&mut response)
                .unwrap();
            if path == "/write-deadline" {
                assert_eq!(response, RESPONSE_PREFIX);
            } else {
                assert!(response.ends_with("\r\n\r\ndeterministic LAN response"));
            }
        }
        drop(client);
        host.healthy_request();
    }
    assert_eq!(host.finish().accepted_connections, 6);
}
