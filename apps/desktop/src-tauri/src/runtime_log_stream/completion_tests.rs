use super::*;
use app_storage::managed_console_log::ManagedConsoleLog;
use std::io::Write;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lg-log-completion-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn an_idle_poll_preserves_partial_unicode_until_confirmed_eof() {
    let fixture = Fixture::new();
    let path = fixture.0.join("server.log");
    let mut writer = fs::File::create(&path).unwrap();
    writer.write_all("first\n玩家".as_bytes()).unwrap();
    let mut state = RuntimeLogTailState::default();
    let mut lines = Vec::new();
    loop {
        let delta = read_runtime_log_delta_bounded(&path, &mut state, 2, 128).unwrap();
        lines.extend(delta.lines);
        if delta.bytes_read == 0 {
            break;
        }
    }
    assert_eq!(lines, ["first"]);
    assert_eq!(state.pending_text, "玩家");
    assert!(
        read_runtime_log_delta_bounded(&path, &mut state, 2, 128)
            .unwrap()
            .lines
            .is_empty()
    );
    drop(writer);
    let final_delta = finish_runtime_log_tail(&mut state);
    assert_eq!(final_delta.lines, ["玩家"]);
    assert_eq!(final_delta.byte_offset, "first\n玩家".len() as u64);
    assert_eq!(final_delta.bytes_read, 0);
    assert!(!final_delta.limit_exhausted);
    assert!(finish_runtime_log_tail(&mut state).lines.is_empty());
}

#[test]
fn completed_newlines_do_not_gain_an_extra_line_at_eof() {
    let fixture = Fixture::new();
    for ending in ["\n", "\r", "\r\n"] {
        let path = fixture.0.join("server.log");
        fs::write(&path, format!("complete{ending}")).unwrap();
        let mut state = RuntimeLogTailState::default();
        let delta = read_runtime_log_delta_bounded(&path, &mut state, 128, 128).unwrap();
        assert_eq!(delta.lines, ["complete"]);
        assert!(finish_runtime_log_tail(&mut state).lines.is_empty());
    }
}

#[test]
fn an_overlong_final_line_is_reported_without_exceeding_the_pending_budget() {
    let fixture = Fixture::new();
    let path = fixture.0.join("server.log");
    fs::write(&path, b"123456789").unwrap();
    let mut state = RuntimeLogTailState::default();
    let delta = read_runtime_log_delta_bounded(&path, &mut state, 128, 4).unwrap();
    assert!(delta.lines.is_empty());
    assert!(delta.limit_exhausted);
    let final_delta = finish_runtime_log_tail(&mut state);
    assert!(final_delta.lines.is_empty());
    assert!(
        final_delta
            .stream_error
            .as_ref()
            .unwrap()
            .contains("streaming limit")
    );
    assert!(final_delta.limit_exhausted);
    assert!(state.pending_text.is_empty());
    assert!(state.pending_bytes.is_empty());
    let repeated = finish_runtime_log_tail(&mut state);
    assert!(repeated.lines.is_empty());
    assert!(repeated.stream_error.is_none());
}

#[test]
fn a_managed_end_cursor_survives_rotation_after_the_snapshot() {
    let fixture = Fixture::new();
    let path = fixture.0.join("managed-console").join("run-1-main.log");
    let writer = ManagedConsoleLog::open(&path).unwrap();
    writer.write_all(&vec![b'\n'; 8 * 1024 * 1024]).unwrap();
    let mut state = runtime_log_tail_at_end(&path).unwrap();
    writer.write_all("new 玩家\nfinal".as_bytes()).unwrap();
    let delta = read_runtime_log_delta_bounded(&path, &mut state, 128, 128).unwrap();
    assert_eq!(delta.lines, ["new 玩家"]);
    assert_eq!(delta.bytes_read, "new 玩家\nfinal".len());
    assert_eq!(finish_runtime_log_tail(&mut state).lines, ["final"]);
}

#[test]
fn an_unmanaged_end_cursor_restarts_if_the_file_is_replaced() {
    let fixture = Fixture::new();
    let path = fixture.0.join("server.log");
    fs::write(&path, b"historical\n").unwrap();
    let mut state = runtime_log_tail_at_end(&path).unwrap();
    fs::rename(&path, fixture.0.join("previous.log")).unwrap();
    fs::write(&path, b"replacement output\n").unwrap();
    let delta = read_runtime_log_delta_bounded(&path, &mut state, 128, 128).unwrap();
    assert_eq!(delta.lines, ["replacement output"]);
}
