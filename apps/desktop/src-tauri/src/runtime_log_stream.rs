use serde::Serialize;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub(crate) mod file_identity;
use file_identity::{LogFileIdentity, log_identity};
mod managed_segments;

pub const RUNTIME_LOG_STREAM_EVENT: &str = "runtime-log-stream";

#[derive(Debug, Clone, Default)]
pub struct RuntimeLogTailState {
    pub byte_offset: u64,
    pub pending_text: String,
    pending_bytes: Vec<u8>,
    discarding_overlong_line: bool,
    suppress_leading_line_feed: bool,
    file_identity: Option<LogFileIdentity>,
    managed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeLogBoundedDelta {
    pub lines: Vec<String>,
    pub stream_error: Option<String>,
    pub byte_offset: u64,
    pub bytes_read: usize,
    pub limit_exhausted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLogStreamPayload {
    pub instance_id: String,
    pub process_key: Option<String>,
    pub display_name: Option<String>,
    pub run_id: Option<i64>,
    pub log_path: String,
    pub lines: Vec<String>,
    pub byte_offset: u64,
    pub emitted_at_unix_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<app_core::LogTailSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_revision: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_error: Option<String>,
}

/// Snapshot the current end before requesting new console output. Managed
/// offsets include archived segments; an active filename's length does not.
pub fn runtime_log_tail_at_end(path: &Path) -> Result<RuntimeLogTailState, std::io::Error> {
    if let Some(segments) = app_storage::managed_console_log::open_log_segments(path)? {
        let last = segments
            .last()
            .ok_or_else(|| std::io::Error::other("managed console log has no retained segment"))?;
        let byte_offset = last
            .start_offset
            .checked_add(last.file.metadata()?.len())
            .ok_or_else(|| std::io::Error::other("console log cursor overflow"))?;
        return Ok(RuntimeLogTailState {
            byte_offset,
            managed: true,
            ..Default::default()
        });
    }
    let file = fs::File::open(path)?;
    Ok(RuntimeLogTailState {
        byte_offset: file.metadata()?.len(),
        file_identity: Some(log_identity(path, &file)?),
        ..Default::default()
    })
}

/// Flush the final unterminated line only after the producer has stopped and
/// all remaining bytes have been read. An empty poll while running is not EOF.
pub fn finish_runtime_log_tail(state: &mut RuntimeLogTailState) -> RuntimeLogBoundedDelta {
    let discarded = std::mem::take(&mut state.discarding_overlong_line);
    let pending = std::mem::take(&mut state.pending_text);
    state.pending_bytes.clear();
    state.suppress_leading_line_feed = false;
    let lines = if discarded || pending.is_empty() {
        Vec::new()
    } else {
        vec![pending]
    };
    RuntimeLogBoundedDelta {
        lines,
        stream_error: discarded.then(|| {
            String::from(
                "[LanGame] The final console line exceeded the streaming limit and was omitted.",
            )
        }),
        byte_offset: state.byte_offset,
        bytes_read: 0,
        limit_exhausted: discarded,
    }
}

pub fn read_runtime_log_delta_bounded(
    path: &Path,
    state: &mut RuntimeLogTailState,
    max_bytes: usize,
    max_pending_bytes: usize,
) -> Result<RuntimeLogBoundedDelta, std::io::Error> {
    let (bytes, unread_file_bytes, gap) =
        if let Some(segments) = app_storage::managed_console_log::open_log_segments(path)? {
            if !state.managed {
                *state = RuntimeLogTailState::default();
                state.managed = true;
            }
            let delta = managed_segments::read(segments, state.byte_offset, max_bytes)?;
            state.byte_offset = delta.offset;
            if delta.gap {
                state.pending_text.clear();
                state.pending_bytes.clear();
                state.discarding_overlong_line = false;
                state.suppress_leading_line_feed = false;
            }
            (delta.bytes, delta.more, delta.gap)
        } else {
            if state.managed {
                *state = RuntimeLogTailState::default();
            }
            let (bytes, more) = read_unmanaged_bytes(path, state, max_bytes)?;
            (bytes, more, false)
        };
    parse_log_bytes(
        state,
        bytes,
        unread_file_bytes,
        gap,
        max_bytes,
        max_pending_bytes,
    )
}

/// Command response captures use offsets within one verified file generation.
/// Their caller checks identity before and after this read, rejecting rotation
/// instead of following the viewer's cumulative managed-log cursor.
pub(crate) fn read_runtime_log_generation_delta_bounded(
    path: &Path,
    state: &mut RuntimeLogTailState,
    max_bytes: usize,
    max_pending_bytes: usize,
) -> Result<RuntimeLogBoundedDelta, std::io::Error> {
    let (bytes, more) = read_unmanaged_bytes(path, state, max_bytes)?;
    parse_log_bytes(state, bytes, more, false, max_bytes, max_pending_bytes)
}

fn read_unmanaged_bytes(
    path: &Path,
    state: &mut RuntimeLogTailState,
    max_bytes: usize,
) -> Result<(Vec<u8>, bool), std::io::Error> {
    let mut file = fs::File::open(path)?;
    // Length, identity and content must describe the same open file generation.
    // A replacement may be longer than the old file, so length alone is insufficient.
    let file_len = file.metadata()?.len();
    let identity = log_identity(path, &file)?;
    if file_len < state.byte_offset
        || state
            .file_identity
            .as_ref()
            .is_some_and(|previous| *previous != identity)
    {
        *state = RuntimeLogTailState::default();
    }
    state.file_identity = Some(identity);

    file.seek(SeekFrom::Start(state.byte_offset))?;
    let mut limited = file.take(max_bytes as u64);
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = limited.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }

    state.byte_offset = state.byte_offset.saturating_add(bytes.len() as u64);
    let unread_file_bytes = state.byte_offset < file_len;
    Ok((bytes, unread_file_bytes))
}

fn parse_log_bytes(
    state: &mut RuntimeLogTailState,
    bytes: Vec<u8>,
    unread_file_bytes: bool,
    gap: bool,
    max_bytes: usize,
    max_pending_bytes: usize,
) -> Result<RuntimeLogBoundedDelta, std::io::Error> {
    let bytes_read = bytes.len();
    let byte_budget_exhausted = max_bytes > 0 && bytes_read == max_bytes;
    let mut start = 0;
    if state.suppress_leading_line_feed && !bytes.is_empty() {
        state.suppress_leading_line_feed = false;
        if bytes.first() == Some(&b'\n') {
            start = 1;
        }
    }
    if state.discarding_overlong_line {
        if let Some(delimiter) = bytes[start..]
            .iter()
            .position(|byte| matches!(*byte, b'\r' | b'\n'))
        {
            let delimiter = start + delimiter;
            start = delimiter + 1;
            if bytes[delimiter] == b'\r' {
                if bytes.get(start) == Some(&b'\n') {
                    start += 1;
                } else if start == bytes.len() {
                    state.suppress_leading_line_feed = true;
                }
            }
            state.discarding_overlong_line = false;
        } else {
            return Ok(RuntimeLogBoundedDelta {
                lines: Vec::new(),
                stream_error: None,
                byte_offset: state.byte_offset,
                bytes_read,
                limit_exhausted: true,
            });
        }
    }

    let existing_pending = if state.pending_bytes.is_empty() {
        state.pending_text.as_bytes()
    } else {
        state.pending_bytes.as_slice()
    };
    let mut combined = Vec::with_capacity(existing_pending.len() + bytes.len() - start);
    combined.extend_from_slice(existing_pending);
    combined.extend_from_slice(&bytes[start..]);
    state.pending_text.clear();
    state.pending_bytes.clear();

    let mut lines = Vec::new();
    let mut line_start = 0;
    let mut index = 0;
    while index < combined.len() {
        match combined[index] {
            b'\r' => {
                lines.push(String::from_utf8_lossy(&combined[line_start..index]).into_owned());
                index += 1;
                if combined.get(index) == Some(&b'\n') {
                    index += 1;
                } else if index == combined.len() {
                    state.suppress_leading_line_feed = true;
                }
                line_start = index;
            }
            b'\n' => {
                lines.push(String::from_utf8_lossy(&combined[line_start..index]).into_owned());
                index += 1;
                line_start = index;
            }
            _ => index += 1,
        }
    }

    let pending = &combined[line_start..];
    let pending_limit_exhausted = pending.len() > max_pending_bytes;
    if pending_limit_exhausted {
        state.discarding_overlong_line = true;
    } else {
        state.pending_bytes.extend_from_slice(pending);
        state.pending_text = bounded_lossy_text(pending, max_pending_bytes);
    }

    Ok(RuntimeLogBoundedDelta {
        lines,
        stream_error: gap.then(|| {
            String::from("[LanGame] Earlier console output expired under the log retention policy.")
        }),
        byte_offset: state.byte_offset,
        bytes_read,
        limit_exhausted: unread_file_bytes
            || byte_budget_exhausted
            || pending_limit_exhausted
            || state.discarding_overlong_line,
    })
}

fn bounded_lossy_text(bytes: &[u8], max_bytes: usize) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text
}

#[cfg(test)]
#[path = "runtime_log_stream/completion_tests.rs"]
mod completion_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn bounded_reader_restarts_after_same_size_or_larger_log_replacement() {
        for replacement in ["new\n", "new log is longer\n"] {
            let root = std::env::temp_dir().join(format!(
                "langame-runtime-log-replacement-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).unwrap();
            let path = root.join("server.log");
            fs::write(&path, "old\n").unwrap();
            let mut state = RuntimeLogTailState::default();
            read_runtime_log_delta_bounded(&path, &mut state, 1024, 1024).unwrap();

            // Keep the old file alive so file-index reuse cannot mask replacement.
            fs::rename(&path, root.join("previous.log")).unwrap();
            fs::write(&path, replacement).unwrap();
            let delta = read_runtime_log_delta_bounded(&path, &mut state, 1024, 1024)
                .expect("read the replacement from its beginning");
            let _ = fs::remove_dir_all(root);

            assert_eq!(delta.lines, [replacement.trim_end()]);
            assert_eq!(delta.byte_offset, replacement.len() as u64);
        }
    }

    #[test]
    fn bounded_reader_does_not_join_partial_lines_from_different_files() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-log-pending-replacement-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("server.log");
        fs::write(&path, "old partial").unwrap();
        let mut state = RuntimeLogTailState::default();
        read_runtime_log_delta_bounded(&path, &mut state, 1024, 1024).unwrap();

        fs::rename(&path, root.join("previous.log")).unwrap();
        fs::write(&path, "new complete line\n").unwrap();
        let delta = read_runtime_log_delta_bounded(&path, &mut state, 1024, 1024).unwrap();
        let _ = fs::remove_dir_all(root);

        assert_eq!(delta.lines, ["new complete line"]);
        assert!(state.pending_text.is_empty());
    }

    #[test]
    fn bounded_reader_emits_only_new_complete_lines() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-log-delta-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let log_path = root.join("server.log");
        fs::write(&log_path, "first\nsecond\npartial").unwrap();

        let mut state = RuntimeLogTailState::default();
        let first =
            read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024).expect("first delta");

        assert_eq!(first.lines, vec!["first", "second"]);
        assert_eq!(state.pending_text, "partial");

        fs::write(&log_path, "first\nsecond\npartial third\nfourth\n").unwrap();
        let second = read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024)
            .expect("second delta");

        assert_eq!(second.lines, vec!["partial third", "fourth"]);

        let empty =
            read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024).expect("empty delta");
        assert!(empty.lines.is_empty());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_reader_recovers_after_file_truncation() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-log-truncate-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let log_path = root.join("server.log");
        fs::write(&log_path, "old line\n").unwrap();

        let mut state = RuntimeLogTailState::default();
        let initial = read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024)
            .expect("initial delta");
        assert_eq!(initial.lines, vec!["old line"]);

        fs::write(&log_path, "new\n").unwrap();
        let delta = read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024)
            .expect("truncated delta");

        assert_eq!(delta.lines, vec!["new"]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_reader_limits_each_read_and_resumes_from_the_recorded_offset() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-log-bounded-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let log_path = root.join("server.log");
        fs::write(&log_path, "first\nsecond\nthird\n").unwrap();

        let mut state = RuntimeLogTailState::default();
        let first = read_runtime_log_delta_bounded(&log_path, &mut state, 6, 1024)
            .expect("first bounded delta");
        assert_eq!(first.lines, vec!["first"]);
        assert_eq!(first.bytes_read, 6);
        assert_eq!(first.byte_offset, 6);
        assert!(first.limit_exhausted);

        let second = read_runtime_log_delta_bounded(&log_path, &mut state, 7, 1024)
            .expect("second bounded delta");
        assert_eq!(second.lines, vec!["second"]);
        assert_eq!(second.bytes_read, 7);
        assert_eq!(second.byte_offset, 13);
        assert!(second.limit_exhausted);

        let third = read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024)
            .expect("final bounded delta");
        assert_eq!(third.lines, vec!["third"]);
        assert_eq!(third.bytes_read, 6);
        assert_eq!(third.byte_offset, 19);
        assert!(!third.limit_exhausted);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bounded_reader_normalizes_mixed_newlines_across_chunk_boundaries() {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-log-mixed-newlines-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let log_path = root.join("server.log");
        fs::write(&log_path, b"first\r\nsecond\rthird\nfourth\r\n").unwrap();

        let mut state = RuntimeLogTailState::default();
        let first = read_runtime_log_delta_bounded(&log_path, &mut state, 6, 1024)
            .expect("CR-terminated chunk");
        assert_eq!(first.lines, vec!["first"]);
        assert_eq!(first.byte_offset, 6);
        assert_eq!(first.bytes_read, 6);
        assert!(first.limit_exhausted);

        let second = read_runtime_log_delta_bounded(&log_path, &mut state, 8, 1024)
            .expect("cross-chunk CRLF and bare CR");
        assert_eq!(second.lines, vec!["second"]);
        assert_eq!(second.byte_offset, 14);
        assert_eq!(second.bytes_read, 8);
        assert!(second.limit_exhausted);

        let third = read_runtime_log_delta_bounded(&log_path, &mut state, 6, 1024)
            .expect("LF-terminated chunk");
        assert_eq!(third.lines, vec!["third"]);
        assert_eq!(third.byte_offset, 20);
        assert_eq!(third.bytes_read, 6);
        assert!(third.limit_exhausted);

        let fourth = read_runtime_log_delta_bounded(&log_path, &mut state, 1024, 1024)
            .expect("final CRLF-terminated chunk");
        assert_eq!(fourth.lines, vec!["fourth"]);
        assert_eq!(fourth.byte_offset, 28);
        assert_eq!(fourth.bytes_read, 8);
        assert!(!fourth.limit_exhausted);
        assert!(state.pending_text.is_empty());

        let _ = fs::remove_dir_all(root);
    }
}
