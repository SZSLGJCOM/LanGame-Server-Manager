use super::*;
use crate::runtime_log_stream::{RuntimeLogTailState, parse_log_bytes};
use std::fs;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-log-segments-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn segments(&self, contents: &[(u64, &[u8])]) -> Vec<ManagedLogSegment> {
        contents
            .iter()
            .enumerate()
            .map(|(index, (offset, bytes))| {
                let path = self.0.join(index.to_string());
                fs::write(&path, bytes).unwrap();
                ManagedLogSegment {
                    file: fs::File::open(&path).unwrap(),
                    path,
                    start_offset: *offset,
                }
            })
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rotation_preserves_unread_tail_split_unicode_and_total_read_budget() {
    let fixture = Fixture::new();
    let source = "first\n玩家😀\r\nlast\n".as_bytes();
    let split = 8; // Inside the first multibyte character.
    let contents = [(0, &source[..split]), (split as u64, &source[split..])];
    let mut state = RuntimeLogTailState::default();
    let mut lines = Vec::new();
    while state.byte_offset < source.len() as u64 {
        let delta = read(fixture.segments(&contents), state.byte_offset, 3).unwrap();
        assert!(delta.bytes.len() <= 3);
        assert!(!delta.gap);
        state.byte_offset = delta.offset;
        lines.extend(
            parse_log_bytes(&mut state, delta.bytes, delta.more, false, 3, 128)
                .unwrap()
                .lines,
        );
    }
    assert_eq!(lines, ["first", "玩家😀", "last"]);
    assert!(state.pending_text.is_empty());
}

#[test]
fn a_missing_segment_separates_contiguous_ranges_and_is_reported_once() {
    let fixture = Fixture::new();
    let contents = [(0, b"old\npartial".as_slice()), (100, b"new\n".as_slice())];
    let first = read(fixture.segments(&contents), 0, 1024).unwrap();
    assert_eq!(first.bytes, b"old\npartial");
    assert!(first.more);
    assert!(!first.gap);
    let second = read(fixture.segments(&contents), first.offset, 1024).unwrap();
    assert_eq!(second.bytes, b"new\n");
    assert!(second.gap);
    assert_eq!(second.offset, 104);
    let third = read(fixture.segments(&contents), second.offset, 1024).unwrap();
    assert!(third.bytes.is_empty());
    assert!(!third.gap);
}

#[test]
fn overlapping_segments_are_rejected_instead_of_repeating_output() {
    let fixture = Fixture::new();
    assert!(
        read(
            fixture.segments(&[(0, b"first\n"), (2, b"second\n")]),
            0,
            128
        )
        .is_err()
    );
}

#[test]
fn the_stream_reads_retained_tails_after_managed_rotation() {
    use crate::runtime_log_stream::read_runtime_log_delta_bounded;
    use app_storage::managed_console_log::ManagedConsoleLog;
    let fixture = Fixture::new();
    let path = fixture.0.join("managed-console").join("run-1-main.log");
    let writer = ManagedConsoleLog::open(&path).unwrap();
    let fill = vec![b'x'; 8 * 1024 * 1024 - 5];
    writer.write_all(&fill).unwrap();
    writer.write_all("\n玩家😀\n".as_bytes()).unwrap();
    let mut state = RuntimeLogTailState {
        byte_offset: fill.len() as u64,
        managed: true,
        ..Default::default()
    };
    let delta = read_runtime_log_delta_bounded(&path, &mut state, 1024, 128).unwrap();
    assert_eq!(delta.lines, ["", "玩家😀"]);
    assert_eq!(delta.bytes_read, "\n玩家😀\n".len());
    drop(writer);
}

#[test]
fn expired_output_does_not_join_a_pending_line_to_a_later_segment() {
    use crate::runtime_log_stream::read_runtime_log_delta_bounded;
    use app_storage::managed_console_log::ManagedConsoleLog;
    let fixture = Fixture::new();
    let path = fixture.0.join("managed-console").join("run-1-main.log");
    let writer = ManagedConsoleLog::open(&path).unwrap();
    writer.write_all(b"old partial").unwrap();
    let mut state = RuntimeLogTailState::default();
    read_runtime_log_delta_bounded(&path, &mut state, 128, 128).unwrap();
    let block = vec![b'\n'; 8 * 1024 * 1024];
    for _ in 0..5 {
        writer.write_all(&block).unwrap();
    }
    let delta = read_runtime_log_delta_bounded(&path, &mut state, 4, 128).unwrap();
    assert!(delta.stream_error.as_ref().unwrap().contains("expired"));
    assert!(delta.lines.iter().all(|line| !line.contains("old partial")));
    assert_eq!(delta.bytes_read, 4);
    let next = read_runtime_log_delta_bounded(&path, &mut state, 4, 128).unwrap();
    assert!(next.lines.iter().all(|line| !line.contains("expired")));
    assert!(next.stream_error.is_none());
    drop(writer);
}
