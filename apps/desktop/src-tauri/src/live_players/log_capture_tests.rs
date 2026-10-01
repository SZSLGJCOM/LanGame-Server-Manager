use super::*;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

struct FixtureLog {
    root: PathBuf,
    path: PathBuf,
}

impl FixtureLog {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-player-log-race-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("server.log");
        fs::write(&path, "historical line\n").unwrap();
        Self { root, path }
    }
}

impl Drop for FixtureLog {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn checked_capture_rejects_cursor_reset_during_the_read() {
    let log = FixtureLog::new();
    let baseline = log_baseline(&log.path).unwrap();
    let mut state = RuntimeLogTailState::default();
    state.byte_offset = baseline.byte_offset;
    let result = read_checked_delta_using(
        &log.path,
        &baseline,
        &mut state,
        1024,
        |path, state, max| {
            // Deterministic truncation at the metadata-check/read boundary.
            fs::write(path, "new\n").unwrap();
            read_runtime_log_generation_delta_bounded(path, state, max, MAX_LINE_BYTES)
        },
    );
    assert!(result.unwrap_err().contains("position changed"));
}

#[test]
fn checked_capture_rejects_replacement_after_read_before_publication() {
    let log = FixtureLog::new();
    let baseline = log_baseline(&log.path).unwrap();
    let mut state = RuntimeLogTailState::default();
    state.byte_offset = baseline.byte_offset;
    let result = read_checked_delta_using(
        &log.path,
        &baseline,
        &mut state,
        1024,
        |path, state, max| {
            let delta =
                read_runtime_log_generation_delta_bounded(path, state, max, MAX_LINE_BYTES)?;
            let retired = log.root.join("retired.log");
            fs::rename(path, &retired).unwrap();
            fs::write(path, "replacement file is longer than the old baseline\n").unwrap();
            // Make the timestamp collision deterministic, independently of the
            // machine's timestamp resolution or NTFS tunneling configuration.
            #[cfg(windows)]
            copy_creation_time(&retired, path);
            Ok(delta)
        },
    );
    assert!(result.unwrap_err().contains("changed while"));
}

#[cfg(windows)]
fn copy_creation_time(source: &Path, target: &Path) {
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::Storage::FileSystem::SetFileTime;

    let creation = fs::metadata(source).unwrap().creation_time();
    let timestamp = FILETIME {
        dwLowDateTime: creation as u32,
        dwHighDateTime: (creation >> 32) as u32,
    };
    let target_file = OpenOptions::new().write(true).open(target).unwrap();
    // SAFETY: target_file owns the handle and timestamp lives through the call;
    // null access/write time pointers preserve those unrelated attributes.
    let success = unsafe {
        SetFileTime(
            target_file.as_raw_handle(),
            &timestamp,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_ne!(success, 0, "{}", std::io::Error::last_os_error());
    assert_eq!(target_file.metadata().unwrap().creation_time(), creation);
}

#[test]
fn checked_capture_preserves_appends_and_pending_utf8_for_dst_and_console_consumers() {
    let log = FixtureLog::new();
    let baseline = log_baseline(&log.path).unwrap();
    let mut state = RuntimeLogTailState::default();
    state.byte_offset = baseline.byte_offset;
    let mut file = OpenOptions::new().append(true).open(&log.path).unwrap();
    let name = "玩家\n".as_bytes();
    file.write_all(&name[..2]).unwrap();
    file.flush().unwrap();
    let partial = read_checked_delta(&log.path, &baseline, &mut state, 1024).unwrap();
    assert!(partial.lines.is_empty());
    assert!(!partial.limit_exhausted);
    file.write_all(&name[2..]).unwrap();
    file.flush().unwrap();
    let complete = read_checked_delta(&log.path, &baseline, &mut state, 1024).unwrap();
    assert_eq!(complete.lines, ["玩家"]);
    assert!(!complete.limit_exhausted);
    drop(file);
}

#[test]
fn checked_capture_preserves_exact_budget_exhaustion() {
    let log = FixtureLog::new();
    let baseline = log_baseline(&log.path).unwrap();
    let mut state = RuntimeLogTailState::default();
    state.byte_offset = baseline.byte_offset;
    let mut file = OpenOptions::new().append(true).open(&log.path).unwrap();
    file.write_all(b"line\n").unwrap();
    file.flush().unwrap();
    let delta = read_checked_delta(&log.path, &baseline, &mut state, 5).unwrap();
    assert_eq!(delta.lines, ["line"]);
    assert!(delta.limit_exhausted);
    drop(file);
}

#[test]
fn checked_capture_reads_only_new_responses_from_a_managed_generation() {
    use app_storage::managed_console_log::ManagedConsoleLog;

    for rotate_before_baseline in [false, true] {
        let mut log = FixtureLog::new();
        log.path = log.root.join("managed-console").join("run-1-main.log");
        let writer = ManagedConsoleLog::open(&log.path).unwrap();
        if rotate_before_baseline {
            writer.write_all(&vec![b'x'; 8 * 1024 * 1024]).unwrap();
        }
        writer.write_all(b"historical output\n").unwrap();
        let baseline = log_baseline(&log.path).unwrap();
        let mut state = RuntimeLogTailState::default();
        state.byte_offset = baseline.byte_offset;

        let response = "玩家\n".as_bytes();
        writer.write_all(&response[..2]).unwrap();
        let partial = read_checked_delta(&log.path, &baseline, &mut state, 1024).unwrap();
        assert!(partial.lines.is_empty());
        assert_eq!(partial.bytes_read, 2);
        writer.write_all(&response[2..]).unwrap();
        let complete = read_checked_delta(&log.path, &baseline, &mut state, 1024).unwrap();
        assert_eq!(complete.lines, ["玩家"]);
        assert_eq!(
            state.byte_offset,
            baseline.byte_offset + response.len() as u64
        );
    }
}

#[test]
fn checked_capture_rejects_managed_rotation_before_publishing_a_response() {
    use app_storage::managed_console_log::ManagedConsoleLog;

    let mut log = FixtureLog::new();
    log.path = log.root.join("managed-console").join("run-1-main.log");
    let writer = ManagedConsoleLog::open(&log.path).unwrap();
    writer.write_all(b"historical output\n").unwrap();
    let baseline = log_baseline(&log.path).unwrap();
    let mut state = RuntimeLogTailState::default();
    state.byte_offset = baseline.byte_offset;
    writer
        .write_all(b"response from original generation\n")
        .unwrap();
    let result = read_checked_delta_using(
        &log.path,
        &baseline,
        &mut state,
        1024,
        |path, state, max| {
            let delta =
                read_runtime_log_generation_delta_bounded(path, state, max, MAX_LINE_BYTES)?;
            writer.write_all(&vec![b'x'; 8 * 1024 * 1024])?;
            writer.write_all(b"response from a different generation\n")?;
            Ok(delta)
        },
    );
    assert!(result.unwrap_err().contains("changed while"));
    assert!(
        read_checked_delta(&log.path, &baseline, &mut state, 1024)
            .unwrap_err()
            .contains("changed while")
    );
}
