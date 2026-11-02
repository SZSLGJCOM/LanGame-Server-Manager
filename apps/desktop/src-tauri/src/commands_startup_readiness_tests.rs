use super::*;
use crate::state::RuntimeStartReservationAttempt;
use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-startup-ready-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn target(&self, key: &str, module: &str) -> Target {
        Target {
            key: key.into(),
            process: StartupProcessTarget {
                pid: std::process::id(),
                identity: app_runtime::inspect_process_identity(std::process::id())
                    .unwrap()
                    .unwrap(),
                host: "127.0.0.1".into(),
            },
            evidence: Evidence::Log {
                cursor: Box::new(
                    StartupLogCursor::capture(self.0.join(format!("{key}.log")), false).unwrap(),
                ),
                state: StartupLogReadiness::new(module).unwrap(),
                // Tests use fresh console-like fixture lines without native timestamps.
                managed: true,
                fallback: None,
                required_probe: None,
            },
            ready: false,
            transcript: NativeStartupTranscript::default(),
        }
    }
    fn append(&self, key: &str, text: &str) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.0.join(format!("{key}.log")))
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        assert!(
            self.0
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("langame-startup-ready-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn reserve(state: &DesktopState) -> RuntimeStartReservationLease {
    match state
        .try_reserve_runtime_start("generic-readiness-test", "manual")
        .unwrap()
    {
        RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
        other => panic!("unexpected reservation: {other:?}"),
    }
}

#[test]
fn native_startup_transcript_keeps_repeated_rows_and_complete_byte_boundary() {
    let fixture = Fixture::new();
    fixture.append("main", "previous startup\n");
    let mut target = fixture.target("main", "terraria");
    if let Evidence::Log { managed, .. } = &mut target.evidence {
        *managed = false;
    }
    let (receipt, writer) =
        app_storage::startup_sessions::begin(&fixture.0, "test", "native-transcript", 10).unwrap();
    let path = Path::new(&receipt.source_path);
    let mut targets = [target];
    fixture.append("main", "Loading\nrepeat\nrepeat\n\n尾");
    assert!(
        !poll_targets_with_transcript(&mut targets, |_| Ok(true), || false, Some(path))
            .unwrap()
            .ready
    );
    let first = app_storage::read_log_path_snapshot(receipt.source_path.clone(), 400);
    assert_eq!(
        first.lines,
        [
            "[game:main] Loading",
            "[game:main] repeat",
            "[game:main] repeat",
            "[game:main] "
        ]
    );
    poll_targets_with_transcript(&mut targets, |_| Ok(true), || false, Some(path)).unwrap();
    assert_eq!(
        app_storage::read_log_path_snapshot(receipt.source_path.clone(), 400).lines,
        first.lines
    );
    fixture.append("main", "行\nServer started\n");
    assert!(
        poll_targets_with_transcript(&mut targets, |_| Ok(true), || false, Some(path))
            .unwrap()
            .ready
    );
    let final_lines = app_storage::read_log_path_snapshot(receipt.source_path.clone(), 400).lines;
    assert_eq!(
        &final_lines[4..],
        ["[game:main] 尾行", "[game:main] Server started"]
    );
    let saved = app_storage::startup_sessions::read_latest(&fixture.0, "test")
        .unwrap()
        .unwrap();
    assert_eq!(saved.native_sources.len(), 1);
    let native = fixture.0.join("main.log");
    let mut file = fs::File::open(&native).unwrap();
    assert_eq!(
        saved.native_sources[0]
            .validated_offset(
                &native,
                &mut file,
                targets[0].process.identity.creation_time
            )
            .unwrap(),
        Some(file.metadata().unwrap().len())
    );
    drop(file);
    drop(writer);
}

#[test]
fn oversized_native_record_never_advances_the_handoff_over_unretained_rows() {
    let fixture = Fixture::new();
    let mut target = fixture.target("main", "terraria");
    if let Evidence::Log { managed, .. } = &mut target.evidence {
        *managed = false;
    }
    let (receipt, writer) =
        app_storage::startup_sessions::begin(&fixture.0, "test", "limit", 11).unwrap();
    let path = Path::new(&receipt.source_path);
    let mut targets = [target];
    fixture.append("main", "retained\n");
    poll_targets_with_transcript(&mut targets, |_| Ok(true), || false, Some(path)).unwrap();
    let before = app_storage::startup_sessions::read_latest(&fixture.0, "test")
        .unwrap()
        .unwrap()
        .native_sources[0]
        .through_byte_offset;
    fixture.append(
        "main",
        &format!("{}\nServer started\n", "x".repeat(32 * 1024)),
    );
    assert!(
        poll_targets_with_transcript(&mut targets, |_| Ok(true), || false, Some(path))
            .unwrap()
            .ready
    );
    let after = app_storage::startup_sessions::read_latest(&fixture.0, "test")
        .unwrap()
        .unwrap();
    assert_eq!(after.native_sources[0].through_byte_offset, before);
    let lines = app_storage::read_log_path_snapshot(receipt.source_path.clone(), 400).lines;
    assert_eq!(lines[0], "[game:main] retained");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("remaining output is retained"));
    assert!(
        fs::read_to_string(fixture.0.join("main.log"))
            .unwrap()
            .ends_with("Server started\n")
    );
    drop(writer);
}

#[test]
fn early_listener_and_old_ready_output_never_complete_a_new_start() {
    let fixture = Fixture::new();
    fixture.append("main", "Server started\n");
    let mut targets = [fixture.target("main", "terraria")];
    fixture.append(
        "main",
        "Listening on port 7777\nServer ready\n[LanGame startup] Server started\n",
    );
    assert!(
        !poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
    fixture.append("main", "Server started\n");
    assert!(
        poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
}

#[test]
fn every_process_must_be_ready_and_earlier_ready_can_be_revoked() {
    let fixture = Fixture::new();
    let mut targets = [
        fixture.target("first", "unturned"),
        fixture.target("second", "unturned"),
    ];
    fixture.append("first", "Steam servers ready!\nLoading level: 100%\n");
    assert!(
        !poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
    fixture.append("first", "Loading level: 10%\n");
    fixture.append("second", "Steam servers ready!\nLoading level: 100%\n");
    assert!(
        !poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
    fixture.append("first", "Loading level: 100%\n");
    assert!(
        poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
}

#[cfg(windows)]
#[test]
fn an_owned_query_and_final_log_are_both_required() {
    let fixture = Fixture::new();
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let port = socket.local_addr().unwrap().port();
    let (finish, finished) = std::sync::mpsc::channel();
    let responder = std::thread::spawn(move || {
        // Complete standard A2S INFO, including the native server identity
        // strings, player fields and version; no synthetic ready log is sent.
        let mut reply = b"\xff\xff\xff\xffI\x11Fixture\0World\0game\0Game\0".to_vec();
        reply.extend_from_slice(&[123, 0, 0, 8, 0, b'd', b'w', 0, 0]);
        reply.extend_from_slice(b"1.0\0");
        for _ in 0..2 {
            let mut request = [0; 128];
            let (_, peer) = socket.recv_from(&mut request).unwrap();
            socket.send_to(&reply, peer).unwrap();
        }
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let mut target = fixture.target("main", "terraria");
    assert!(app_storage::poll_owned_a2s_info(&target.process, port).unwrap());
    if let Evidence::Log { required_probe, .. } = &mut target.evidence {
        *required_probe = Some(StartupProbe::a2s(port));
    }
    let mut targets = [target];
    assert!(
        !poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
    fixture.append("main", "Server started\n");
    assert!(
        poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
    finish.send(()).unwrap();
    responder.join().unwrap();
    assert!(
        !poll_targets(&mut targets, |_| Ok(true), || false)
            .unwrap()
            .ready
    );
}

#[test]
fn process_exit_or_replacement_after_evidence_invalidates_the_group() {
    let fixture = Fixture::new();
    let mut targets = [
        fixture.target("first", "terraria"),
        fixture.target("second", "terraria"),
    ];
    fixture.append("first", "Server started\n");
    fixture.append("second", "Server started\n");
    let checks = Cell::new(0);
    let error = poll_targets(
        &mut targets,
        |_| {
            checks.set(checks.get() + 1);
            Ok(checks.get() != 3)
        },
        || false,
    )
    .err()
    .unwrap();
    assert!(error.contains("first") && error.contains("identity"));
}

#[test]
fn cancellation_wins_even_if_the_last_probe_observed_ready() {
    let fixture = Fixture::new();
    let mut targets = [fixture.target("main", "barotrauma")];
    fixture.append("main", "Server started\n");
    let cancelled = Cell::new(false);
    let error = poll_targets(
        &mut targets,
        |_| {
            cancelled.set(true);
            Ok(true)
        },
        || cancelled.get(),
    )
    .err()
    .unwrap();
    assert!(error.contains("cancelled"));
}

#[tokio::test]
async fn waiting_uses_the_shared_stop_cancellation_reservation() {
    let fixture = Fixture::new();
    let state = DesktopState::default();
    let reservation = reserve(&state);
    state
        .request_runtime_start_cancellation("generic-readiness-test")
        .unwrap();
    let error = wait_for_targets(
        vec![fixture.target("main", "minecraft")],
        tokio::time::Instant::now() + Duration::from_secs(60),
        Duration::from_secs(60),
        &reservation,
        None,
    )
    .await
    .err()
    .unwrap();
    assert!(error.contains("cancelled"));
    assert!(!reservation.confirm_ready().unwrap());
}

#[tokio::test]
async fn deadline_cannot_be_converted_into_running_by_a_late_marker() {
    let fixture = Fixture::new();
    let state = DesktopState::default();
    let reservation = reserve(&state);
    let target = fixture.target("main", "terraria");
    fixture.append("main", "Server started\n");
    let error = wait_for_targets(
        vec![target],
        tokio::time::Instant::now(),
        Duration::from_secs(1),
        &reservation,
        None,
    )
    .await
    .err()
    .unwrap();
    assert!(error.contains("timed out") && error.contains("main"));
}

#[tokio::test]
async fn native_log_handoff_failure_keeps_startup_uncommitted_and_cancellable() {
    let fixture = Fixture::new();
    let desktop = DesktopState::default();
    let reservation = reserve(&desktop);
    let mut target = fixture.target("main", "valheim");
    let Evidence::Log { managed, .. } = &mut target.evidence else {
        panic!("fixture must provide log evidence");
    };
    *managed = false;
    fixture.append("main", "Opened Steam server\n");
    let console_path = fixture.0.join("managed-console/run-1-main.log");
    fs::create_dir(console_path.parent().unwrap()).unwrap();
    fs::write(&console_path, "managed console\n").unwrap();
    // A real write failure at the durable handoff boundary must propagate into
    // the caller's existing startup cleanup, before confirm_ready / Running.
    fs::create_dir(console_path.parent().unwrap().join(".valheim-session.json")).unwrap();
    let error = wait_for_targets(
        vec![target],
        tokio::time::Instant::now() + Duration::from_secs(10),
        Duration::from_secs(10),
        &reservation,
        Some(NativeLogBinding {
            console_path,
            session_id: "native-handoff".into(),
        }),
    )
    .await
    .unwrap_err();
    assert!(error.contains("Valheim log source"), "{error}");
    desktop
        .request_runtime_start_cancellation("generic-readiness-test")
        .unwrap();
    assert!(reservation.is_cancelled());
    assert!(!reservation.confirm_ready().unwrap());
}
