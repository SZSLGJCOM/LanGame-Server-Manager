use super::*;
use std::io::Write;
const HOST_READY: &str = "R5LogCoopProxy: [000566] ...opProxyServer::SetIsReadyForHostOwnerConnect Host server is ready for owner to connect. Semaphore null";
fn host_ready_line(line: &str) -> bool {
    matches!(
        app_storage::windrose_native_stage(line),
        Some(app_storage::WindroseNativeStage::HostReady)
    )
}

struct LogFixture {
    root: std::path::PathBuf,
    path: std::path::PathBuf,
    writer: Option<app_storage::managed_console_log::ManagedConsoleLog>,
}

impl LogFixture {
    fn new(managed: bool) -> Self {
        let root = std::env::temp_dir().join(format!("lg-windrose-ready-{}", uuid::Uuid::new_v4()));
        let path = if managed {
            root.join("managed-console/run-fixture.log")
        } else {
            root.join("fixture.log")
        };
        let writer =
            Some(app_storage::managed_console_log::ManagedConsoleLog::open(&path).unwrap());
        Self { root, path, writer }
    }
    fn append(&mut self, bytes: &[u8]) {
        self.writer.as_mut().unwrap().write_all(bytes).unwrap();
    }
}

impl Drop for LogFixture {
    fn drop(&mut self) {
        self.writer.take();
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn windrose_bootstrap_readiness_accepts_only_exact_native_stage() {
    assert!(host_ready_line(HOST_READY));
    assert!(host_ready_line(&format!(
        "[2026.09.30-12.38.26:116][  0]{HOST_READY}"
    )));
    for prefix in [
        "chat: ",
        "[LanGame] ",
        "[invalid][0]",
        "[2026.09.30-12.38.26:116][user]",
    ] {
        assert!(!host_ready_line(&format!("{prefix}{HOST_READY}")));
    }
    assert!(!host_ready_line(
        "LogInit: Display: Game Engine Initialized."
    ));
    assert!(!host_ready_line(
        "LogInit: Display: Engine is initialized. Leaving FEngineLoop::Init()"
    ));
}

#[test]
fn windrose_bootstrap_readiness_excludes_old_lines_and_waits_for_complete_new_line() {
    for managed in [false, true] {
        let mut fixture = LogFixture::new(managed);
        fixture.append(format!("{HOST_READY}\n").as_bytes());
        let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
        assert!(!reader.poll(&fixture.path).unwrap());
        fixture.append(b"unrecognized stage\n");
        assert!(!reader.poll(&fixture.path).unwrap());
        fixture.append(HOST_READY.as_bytes());
        assert!(!reader.poll(&fixture.path).unwrap());
        fixture.append(b"\n");
        assert!(reader.poll(&fixture.path).unwrap());
    }
}

#[test]
fn windrose_bootstrap_readiness_does_not_splice_a_pre_spawn_partial_line() {
    let mut fixture = LogFixture::new(false);
    fixture.append(b"old unrelated prefix ");
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    assert!(!reader.poll(&fixture.path).unwrap());
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    assert!(reader.poll(&fixture.path).unwrap());
}

#[test]
fn windrose_bootstrap_readiness_waits_until_bounded_scan_catches_up() {
    let mut fixture = LogFixture::new(true);
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    fixture.append(&b"noise\n".repeat(READ_BUDGET / 6 + 1));
    assert!(!reader.poll(&fixture.path).unwrap());
    assert!(reader.poll(&fixture.path).unwrap());
}

#[test]
fn windrose_bootstrap_readiness_uses_logical_offsets_across_rotation() {
    let mut fixture = LogFixture::new(true);
    // Cross the production 8 MiB segment boundary before capturing the start.
    for _ in 0..9 {
        fixture.append(&b"old\n".repeat(256 * 1024));
    }
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    assert!(!reader.poll(&fixture.path).unwrap());
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    assert!(reader.poll(&fixture.path).unwrap());
}

#[test]
fn windrose_bootstrap_readiness_rejects_replacement_instead_of_replaying_it() {
    let mut fixture = LogFixture::new(false);
    fixture.append(b"old\n");
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    std::fs::rename(&fixture.path, fixture.root.join("previous.log")).unwrap();
    std::fs::write(&fixture.path, format!("{HOST_READY}\n")).unwrap();
    assert!(reader.poll(&fixture.path).is_err());
}

#[test]
fn windrose_bootstrap_readiness_later_loading_or_exit_revokes_host_ready() {
    let mut fixture = LogFixture::new(true);
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    fixture.append(format!("{HOST_READY}\nLogLoad: LoadMap: /Game/Maps/NextWorld\n").as_bytes());
    assert!(!reader.poll(&fixture.path).unwrap());
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    assert!(reader.poll(&fixture.path).unwrap());
    fixture.append(b"LogCore: Engine exit requested (reason: ConsoleCtrl RequestExit)\n");
    assert!(!reader.poll(&fixture.path).unwrap());
}

#[test]
fn windrose_bootstrap_readiness_propagates_stream_error_after_retention_gap() {
    let mut fixture = LogFixture::new(true);
    let mut reader = WorldReadiness::before_spawn(&fixture.path).unwrap();
    fixture.append(format!("{HOST_READY}\n").as_bytes());
    assert!(reader.poll(&fixture.path).unwrap());
    let noise = b"ordinary stats \n".repeat(65_536);
    for _ in 0..33 {
        fixture.append(&noise);
    }
    let error = reader.poll(&fixture.path).unwrap_err().to_string();
    assert!(
        error.contains("bootstrap log stream is incomplete"),
        "{error}"
    );
    assert!(error.contains("expired"), "{error}");
}
