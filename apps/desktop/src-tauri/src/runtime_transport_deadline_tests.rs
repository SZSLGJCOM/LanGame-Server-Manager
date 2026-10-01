use super::*;
use std::io::Cursor;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

// Synthetic loopback protocol peers; these tests do not connect to game servers.
struct FixturePeer {
    endpoint: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl FixturePeer {
    fn start(serve: impl FnOnce(TcpStream, Arc<AtomicBool>) + Send + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline && !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_millis(100)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_millis(100)))
                            .unwrap();
                        serve(stream, stopped);
                        return;
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("fixture accept failed: {error}"),
                }
            }
        });
        Self {
            endpoint,
            stop,
            worker: Some(worker),
        }
    }

    fn finish(mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

impl Drop for FixturePeer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn trickle(mut stream: TcpStream, stop: Arc<AtomicBool>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && !stop.load(Ordering::Acquire) {
        if stream.write_all(b"x").is_err() {
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn partial_reads_do_not_renew_the_total_deadline() {
    let peer = FixturePeer::start(trickle);
    let mut stream = DeadlineTcpStream::connect_with_budget(
        &peer.endpoint,
        Duration::from_secs(1),
        Duration::from_millis(80),
    )
    .unwrap();
    let started = Instant::now();
    let error = stream.read_exact(&mut [0; 5000]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(error.to_string().contains("total deadline"));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(
        stream.write(b"late").unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    drop(stream);
    peer.finish();
}

#[test]
fn telnet_trickling_response_fails_instead_of_returning_partial_success() {
    let peer = FixturePeer::start(trickle);
    let mut stream = DeadlineTcpStream::connect_with_budget(
        &peer.endpoint,
        Duration::from_secs(1),
        Duration::from_millis(80),
    )
    .unwrap();
    let error = super::super::telnet_read_available(&mut stream, 65536).unwrap_err();
    assert!(error.contains("total deadline"));
    drop(stream);
    peer.finish();
}

#[test]
fn telnet_idle_read_can_finish_before_the_total_deadline() {
    let peer = FixturePeer::start(|mut stream, stop| {
        stream.write_all(b"complete response").unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(2));
        }
    });
    let mut stream = DeadlineTcpStream::connect_with_budget(
        &peer.endpoint,
        Duration::from_millis(30),
        Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        super::super::telnet_read_available(&mut stream, 1024).unwrap(),
        "complete response"
    );
    assert!(stream.check_deadline().is_ok());
    drop(stream);
    peer.finish();
}

#[test]
fn telnet_total_timeout_without_input_is_not_a_successful_idle_boundary() {
    let peer = FixturePeer::start(|_stream, stop| {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !stop.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(2));
        }
    });
    let mut stream = DeadlineTcpStream::connect_with_budget(
        &peer.endpoint,
        Duration::from_secs(1),
        Duration::from_millis(80),
    )
    .unwrap();
    assert!(
        super::super::telnet_read_available(&mut stream, 1024)
            .unwrap_err()
            .contains("total deadline")
    );
    drop(stream);
    peer.finish();
}

#[test]
fn telnet_response_limit_rejects_truncation_but_accepts_exactly_complete_output() {
    for (body, expected) in [
        (b"four".as_slice(), Some("four")),
        (b"five!".as_slice(), None),
    ] {
        let body = body.to_vec();
        let peer = FixturePeer::start(move |mut stream, _| {
            stream.write_all(&body).unwrap();
        });
        let mut stream = DeadlineTcpStream::connect_with_budget(
            &peer.endpoint,
            Duration::from_millis(100),
            Duration::from_secs(1),
        )
        .unwrap();
        let response = super::super::telnet_read_available(&mut stream, 4);
        match expected {
            Some(expected) => assert_eq!(response.unwrap(), expected),
            None => assert!(response.unwrap_err().contains("collection limit")),
        }
        drop(stream);
        peer.finish();
    }
}

#[test]
fn websocket_control_frames_cannot_bypass_the_frame_limit() {
    let mut frames = Cursor::new([0x8a, 0].repeat(1025));
    let error = super::super::websocket_read_text_message(&mut frames).unwrap_err();
    assert!(error.contains("frame limit"));
    assert_eq!(frames.position(), 2048);
}

#[test]
fn websocket_empty_fragments_cannot_bypass_the_frame_limit() {
    let mut bytes = vec![0x01, 0];
    bytes.extend([0x00, 0].repeat(1024));
    let mut frames = Cursor::new(bytes);
    let error = super::super::websocket_read_text_message(&mut frames).unwrap_err();
    assert!(error.contains("frame limit"));
}

#[test]
fn transport_rejects_dns_endpoints_without_uncancellable_resolution() {
    let result = DeadlineTcpStream::connect_with_budget(
        "unresolved.invalid:27015",
        Duration::from_secs(1),
        Duration::from_millis(80),
    );
    assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::InvalidInput));
}
