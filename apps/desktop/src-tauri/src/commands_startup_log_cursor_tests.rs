use super::*;
use app_storage::managed_console_log::ManagedConsoleLog;
use std::cell::Cell;
use std::fs::{self, OpenOptions};
use std::io::Write;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-startup-cursor-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> PathBuf {
        self.0.join("native.log")
    }

    fn append(&self, bytes: &[u8]) {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }

    fn parts(&self, contents: &[(u64, &[u8])]) -> Vec<Segment> {
        contents
            .iter()
            .map(|(start, bytes)| {
                let path = self.0.join(format!("part-{start}.log"));
                fs::write(&path, bytes).unwrap();
                let file = File::open(&path).unwrap();
                Segment {
                    identity: log_identity(&path, &file).unwrap(),
                    file,
                    start: *start,
                    length: bytes.len() as u64,
                }
            })
            .collect()
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
                .starts_with("langame-startup-cursor-")
        );
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn prelaunch_records_are_never_returned_from_unchanged_or_appended_logs() {
    let fixture = Fixture::new();
    fixture.append(b"old ready\n");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    assert_eq!(cursor.poll(|| false).unwrap(), LogProgress::default());
    fixture.append(b"new loading\nnew ready\n");
    let progress = cursor.poll(|| false).unwrap();
    assert_eq!(progress.lines, ["new loading", "new ready"]);
    assert!(!progress.reset && !progress.more);
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
}

#[test]
fn a_prelaunch_partial_line_cannot_be_completed_into_current_evidence() {
    let fixture = Fixture::new();
    fixture.append(b"old partial ");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"ready");
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
    fixture.append(b"\ncurrent ready\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["current ready"]);
}

#[test]
fn a_baseline_ending_in_cr_does_not_turn_its_following_lf_into_an_empty_record() {
    let fixture = Fixture::new();
    fixture.append(b"old\r");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"\nnew\r\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["new"]);
}

#[test]
fn an_absent_log_is_pending_and_a_new_file_is_read_from_its_beginning() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    assert_eq!(cursor.poll(|| false).unwrap(), LogProgress::default());
    fixture.append(b"first\nlast");
    let progress = cursor.poll(|| false).unwrap();
    assert!(progress.reset);
    assert_eq!(progress.lines, ["first"]);
    assert_eq!(cursor.poll(|| false).unwrap(), LogProgress::default());
    fixture.append(b"\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["last"]);
}

#[test]
fn complete_unicode_records_and_mixed_newlines_survive_every_chunk_boundary() {
    let record = "加载 玩家😀\r\nready\rnext\n".as_bytes();
    for split in 0..=record.len() {
        let mut line = LineState::default();
        let mut lines = line.consume(&record[..split], &|| false).unwrap();
        lines.extend(line.consume(&record[split..], &|| false).unwrap());
        assert_eq!(lines, ["加载 玩家😀", "ready", "next"]);
        assert!(line.pending.is_empty());
    }
}

#[test]
fn current_partial_records_are_not_emitted_or_lost_on_empty_polls() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"new partial");
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
    fixture.append(b" record\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["new partial record"]);
}

#[test]
fn bounded_scans_preserve_markers_before_the_tail_and_resume_without_duplicates() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    let mut bytes = b"ready before huge tail\n".to_vec();
    bytes.extend(b"diagnostics\n".repeat(READ_BYTES / 4));
    fixture.append(&bytes);
    let first = cursor.poll(|| false).unwrap();
    assert_eq!(
        first.lines.first().map(String::as_str),
        Some("ready before huge tail")
    );
    assert_eq!(cursor.offset, READ_BYTES as u64);
    assert!(first.more);
    let mut delivered = first.lines;
    while cursor.offset < bytes.len() as u64 {
        let before = cursor.offset;
        let progress = cursor.poll(|| false).unwrap();
        assert!(cursor.offset - before <= READ_BYTES as u64);
        assert!(!progress.reset);
        delivered.extend(progress.lines);
    }
    assert_eq!(delivered.len(), 1 + READ_BYTES / 4);
    assert_eq!(
        delivered
            .iter()
            .filter(|line| line.as_str() == "ready before huge tail")
            .count(),
        1
    );
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
}

#[test]
fn a_complete_record_split_at_the_byte_budget_is_only_returned_after_completion() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(&b"x\n".repeat(READ_BYTES / 2 - 2));
    fixture.append(b"ready after split\n");
    let first = cursor.poll(|| false).unwrap();
    assert!(first.more);
    assert!(!first.lines.iter().any(|line| line.contains("ready")));
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready after split"]);
}

#[test]
fn oversized_lines_are_discarded_whole_and_cannot_expose_a_ready_suffix() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(&vec![b'x'; READ_BYTES + LINE_BYTES]);
    fixture.append(b"ready suffix\ncurrent ready\n");
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
    assert!(cursor.line.pending.len() <= LINE_BYTES);
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["current ready"]);
    let mut line = LineState::default();
    let mut exact = vec![b'x'; LINE_BYTES];
    exact.push(b'\n');
    assert_eq!(
        line.consume(&exact, &|| false).unwrap()[0].len(),
        LINE_BYTES
    );
}

#[test]
fn a_replacement_with_identical_anchors_is_detected_by_file_identity() {
    let fixture = Fixture::new();
    fixture.append(&[b'a'; 256]);
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fs::rename(fixture.path(), fixture.0.join("retired.log")).unwrap();
    let mut replacement = [b'a'; 256];
    replacement[96..103].copy_from_slice(b"\nready\n");
    fs::write(fixture.path(), replacement).unwrap();
    let progress = cursor.poll(|| false).unwrap();
    assert!(progress.reset);
    assert!(progress.lines.iter().any(|line| line == "ready"));
}

#[test]
fn same_file_truncate_then_regrow_beyond_the_baseline_is_detected_by_the_anchor() {
    let fixture = Fixture::new();
    fixture.append(&[b'a'; 128]);
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    let identity = cursor.checkpoint.as_ref().unwrap().identity.clone();
    let mut replacement = vec![b'a'; 64];
    replacement.extend_from_slice(b"\nready\n");
    replacement.extend_from_slice(&[b'b'; 128]);
    fs::write(fixture.path(), replacement).unwrap();
    let progress = cursor.poll(|| false).unwrap();
    assert!(progress.reset);
    assert_eq!(cursor.checkpoint.as_ref().unwrap().identity, identity);
    assert!(progress.lines.iter().any(|line| line == "ready"));
}

#[test]
fn observed_truncation_resets_old_partial_state_and_keeps_the_new_cursor() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"old partial");
    cursor.poll(|| false).unwrap();
    fs::write(fixture.path(), b"").unwrap();
    assert!(cursor.poll(|| false).unwrap().reset);
    fixture.append(b"ready\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready"]);
    fixture.append(b"later\n");
    let progress = cursor.poll(|| false).unwrap();
    assert!(!progress.reset);
    assert_eq!(progress.lines, ["later"]);
}

#[test]
fn a_missing_file_revokes_readiness_without_replaying_its_prelaunch_records() {
    let fixture = Fixture::new();
    fixture.append(b"prelaunch ready\n");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"current ready\ncurrent");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["current ready"]);
    let committed = cursor.offset;
    let retired = fixture.0.join("retired.log");
    fs::rename(fixture.path(), &retired).unwrap();
    assert!(cursor.poll(|| true).unwrap_err().contains("cancelled"));
    assert_eq!(cursor.offset, committed);
    let missing = cursor.poll(|| false).unwrap();
    assert!(missing.reset && missing.lines.is_empty() && !missing.more);
    fs::rename(retired, fixture.path()).unwrap();
    assert_eq!(cursor.poll(|| false).unwrap(), LogProgress::default());
    fixture.append(b" ready\n");
    let progress = cursor.poll(|| false).unwrap();
    assert!(!progress.reset);
    assert_eq!(progress.lines, ["current ready"]);

    fs::rename(fixture.path(), fixture.0.join("replaced.log")).unwrap();
    assert!(cursor.poll(|| false).unwrap().reset);
    fixture.append(b"new generation ready\n");
    let progress = cursor.poll(|| false).unwrap();
    assert!(progress.reset);
    assert_eq!(progress.lines, ["new generation ready"]);
}

#[test]
fn replacement_during_read_cannot_publish_lines_from_the_old_handle() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"ready\n");
    let replacement = fixture.0.join("next.log");
    fs::write(&replacement, b"wait!\n").unwrap();
    let progress = cursor
        .poll_inner(&|| false, || {
            fs::rename(&replacement, fixture.path()).unwrap()
        })
        .unwrap();
    assert!(progress.reset && progress.lines.is_empty());
    assert_eq!(cursor.offset, 0);
    assert!(cursor.checkpoint.is_none());
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["wait!"]);
    fixture.append(b"ready\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready"]);
}

#[test]
fn cancellation_or_io_error_after_read_does_not_commit_the_staged_cursor() {
    for cancellation in [false, true] {
        let fixture = Fixture::new();
        let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
        fixture.append(b"ready\n");
        let cancelled = Cell::new(false);
        let retired = fixture.0.join("retired.log");
        assert!(
            cursor
                .poll_inner(&|| cancelled.get(), || {
                    if cancellation {
                        cancelled.set(true);
                    } else {
                        fs::rename(fixture.path(), &retired).unwrap();
                        fs::create_dir(fixture.path()).unwrap();
                    }
                })
                .is_err()
        );
        assert_eq!(cursor.offset, 0);
        assert!(cursor.checkpoint.is_none() && cursor.line.pending.is_empty());
        if !cancellation {
            fs::remove_dir(fixture.path()).unwrap();
            fs::rename(retired, fixture.path()).unwrap();
        }
        assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready"]);
    }
}

#[test]
fn cancellation_during_a_read_does_not_commit_or_drop_unreturned_records() {
    let fixture = Fixture::new();
    fixture.append(b"old\n");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(&b"current\n".repeat(READ_BYTES / 8));
    let baseline = cursor.offset;
    let polls = Cell::new(0);
    let error = cursor
        .poll(|| {
            polls.set(polls.get() + 1);
            polls.get() >= 8
        })
        .unwrap_err();
    assert!(error.contains("cancelled"));
    assert_eq!(cursor.offset, baseline);
    assert!(cursor.line.pending.is_empty());
    assert_eq!(cursor.poll(|| false).unwrap().lines.len(), READ_BYTES / 8);
}

#[test]
fn cancellation_before_io_and_real_io_errors_are_not_treated_as_empty_output() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    assert!(cursor.poll(|| true).unwrap_err().contains("cancelled"));
    fs::create_dir(fixture.path()).unwrap();
    assert!(cursor.poll(|| false).is_err());
    assert!(StartupLogCursor::capture(fixture.path(), false).is_err());
    assert_eq!(cursor.offset, 0);
}

#[test]
fn managed_mode_keeps_the_legacy_plain_file_fallback() {
    let fixture = Fixture::new();
    fixture.append(b"old\n");
    let mut cursor = StartupLogCursor::capture(fixture.path(), true).unwrap();
    fixture.append(b"new\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["new"]);
}

#[test]
fn managed_rotation_preserves_unread_bytes_and_unicode_across_the_real_segment_boundary() {
    let fixture = Fixture::new();
    let path = fixture.0.join("managed-console/run-1-main.log");
    let writer = ManagedConsoleLog::open(&path).unwrap();
    let mut old = vec![b'x'; 8 * 1024 * 1024 - 5];
    *old.last_mut().unwrap() = b'\n';
    writer.write_all(&old).unwrap();
    let mut cursor = StartupLogCursor::capture(path.clone(), true).unwrap();
    writer.write_all("玩家😀\nready\n".as_bytes()).unwrap();
    assert_eq!(open_log_segments(&path).unwrap().unwrap().len(), 2);
    let progress = cursor.poll(|| false).unwrap();
    assert!(!progress.reset && !progress.more);
    assert_eq!(progress.lines, ["玩家😀", "ready"]);
    assert_eq!(cursor.offset, (old.len() + "玩家😀\nready\n".len()) as u64);
    assert!(cursor.poll(|| false).unwrap().lines.is_empty());
    drop(writer);
}

#[test]
fn managed_capture_before_the_writer_exists_ignores_no_new_output() {
    let fixture = Fixture::new();
    let path = fixture.0.join("managed-console/run-1-main.log");
    let mut cursor = StartupLogCursor::capture(path.clone(), true).unwrap();
    assert_eq!(cursor.poll(|| false).unwrap(), LogProgress::default());
    let writer = ManagedConsoleLog::open(path).unwrap();
    writer.write_all(b"ready\n").unwrap();
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready"]);
    drop(writer);
}

#[test]
fn a_retention_gap_drops_only_the_unknown_leading_fragment() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    let mut initial = fixture.parts(&[(0, b"partial")]);
    assert!(
        cursor
            .poll_segments(&mut initial, &|| false)
            .unwrap()
            .lines
            .is_empty()
    );
    let mut retained = fixture.parts(&[(100, b"suffix\nready\n")]);
    let progress = cursor.poll_segments(&mut retained, &|| false).unwrap();
    assert!(progress.reset);
    assert_eq!(progress.lines, ["ready"]);
    assert_eq!(
        cursor.poll_segments(&mut retained, &|| false).unwrap(),
        LogProgress::default()
    );
}

#[test]
fn an_internal_gap_is_delivered_after_the_previous_contiguous_range() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    let mut segments = fixture.parts(&[(0, b"first\npartial"), (100, b"suffix\nsecond\n")]);
    let first = cursor.poll_segments(&mut segments, &|| false).unwrap();
    assert_eq!(first.lines, ["first"]);
    assert!(first.more);
    let second = cursor.poll_segments(&mut segments, &|| false).unwrap();
    assert!(second.reset);
    assert_eq!(second.lines, ["second"]);
    assert!(!second.more);
}

#[test]
fn expiration_exactly_at_a_consumed_boundary_preserves_the_pending_line() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    let mut initial = fixture.parts(&[(0, b"rea")]);
    assert!(
        cursor
            .poll_segments(&mut initial, &|| false)
            .unwrap()
            .lines
            .is_empty()
    );
    let mut next = fixture.parts(&[(3, b"dy\n")]);
    let progress = cursor.poll_segments(&mut next, &|| false).unwrap();
    assert!(!progress.reset);
    assert_eq!(progress.lines, ["ready"]);
}

#[test]
fn native_health_handoff_rewinds_only_the_current_partial_record() {
    let fixture = Fixture::new();
    fixture.append(b"old ready\n");
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"current ready\ncurrent partial");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["current ready"]);
    let mut snapshot = cursor.native_snapshot().unwrap();
    assert_eq!(snapshot.offset, b"old ready\ncurrent ready\n".len() as u64);
    assert!(!snapshot.discard_fragment);
    snapshot
        .file
        .seek(SeekFrom::Start(snapshot.offset))
        .unwrap();
    let mut suffix = String::new();
    snapshot.file.read_to_string(&mut suffix).unwrap();
    assert_eq!(suffix, "current partial");
}

#[test]
fn native_health_handoff_rejects_a_changed_file_instead_of_publishing_old_ready() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"current ready\n");
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["current ready"]);
    fs::write(fixture.path(), b"old fake ready\n").unwrap();
    assert!(cursor.native_snapshot().is_err());
    fs::remove_file(fixture.path()).unwrap();
    assert!(cursor.native_snapshot().is_err());
}

#[test]
fn native_health_handoff_keeps_an_oversized_fragment_discarded() {
    let fixture = Fixture::new();
    let mut cursor = StartupLogCursor::capture(fixture.path(), false).unwrap();
    fixture.append(b"ready\n");
    fixture.append(&vec![b'x'; LINE_BYTES + 5]);
    assert_eq!(cursor.poll(|| false).unwrap().lines, ["ready"]);
    let snapshot = cursor.native_snapshot().unwrap();
    assert_eq!(snapshot.offset, (b"ready\n".len() + LINE_BYTES + 5) as u64);
    assert!(snapshot.discard_fragment);
}
