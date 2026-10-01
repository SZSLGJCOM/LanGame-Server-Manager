use super::*;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-console-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("managed-console").join("run-123-main.log")
    }
    fn writer(&self) -> ManagedConsoleLog {
        let writer = ManagedConsoleLog::open(self.path()).unwrap();
        if let SinkKind::Managed { directory, .. } = &writer.inner.kind {
            directory.state.lock().unwrap().limits = LogLimits {
                segment_bytes: 64,
                instance_bytes: 256,
                run_segments: 4,
                runs: 128,
            };
        }
        writer
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn does_not_adopt_an_existing_unregistered_console_file() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.path().parent().unwrap()).unwrap();
    fs::write(fixture.path(), b"old server log\n").unwrap();
    assert!(ManagedConsoleLog::open(fixture.path()).is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), b"old server log\n");
}

#[test]
fn current_console_file_rotates_without_truncating_the_previous_file_object() {
    let fixture = Fixture::new();
    let writer = ManagedConsoleLog::open(fixture.path()).unwrap();
    writer.write_all(&vec![b'a'; 8 * 1024 * 1024]).unwrap();
    let previous = File::open(fixture.path()).unwrap();
    writer.write_all(b"new generation\n").unwrap();
    assert_eq!(previous.metadata().unwrap().len(), 8 * 1024 * 1024);
    assert_eq!(fs::read(fixture.path()).unwrap(), b"new generation\n");
}

#[test]
fn generated_segments_expire_in_order_and_the_surviving_offsets_remain_contiguous() {
    let fixture = Fixture::new();
    let writer = fixture.writer();
    for value in 0..10_u8 {
        writer.write_all(&[value; 64]).unwrap();
    }
    let segments = open_log_segments(&fixture.path()).unwrap().unwrap();
    assert_eq!(segments.len(), 4);
    assert_eq!(segments[0].start_offset, 6 * 64);
    for (index, segment) in segments.iter().enumerate() {
        assert_eq!(segment.start_offset, (6 + index as u64) * 64);
        assert_eq!(fs::read(&segment.path).unwrap(), vec![6 + index as u8; 64]);
    }
    drop(segments);
    drop(writer);
    let restarted = fixture.writer();
    restarted.write_all(&[10; 64]).unwrap();
    let segments = open_log_segments(&fixture.path()).unwrap().unwrap();
    assert_eq!(segments.len(), 4);
    assert_eq!(segments[0].start_offset, 7 * 64);
}

#[test]
fn separate_opens_share_output_order_and_never_interleave_records() {
    let fixture = Fixture::new();
    let first = ManagedConsoleLog::open(fixture.path()).unwrap();
    let second = ManagedConsoleLog::open(fixture.path()).unwrap();
    std::thread::scope(|scope| {
        for writer in [&first, &second] {
            scope.spawn(move || {
                for _ in 0..100 {
                    writer.write_all(b"complete-record\n").unwrap();
                }
            });
        }
    });
    assert_eq!(
        fs::read_to_string(fixture.path()).unwrap().lines().count(),
        200
    );
}

#[test]
fn instance_budget_does_not_delete_any_open_run_current_segment() {
    let fixture = Fixture::new();
    let first = fixture.writer();
    first.write_all(&[1; 64]).unwrap();
    let mut writers = vec![first];
    for index in 2..=4 {
        let path = fixture
            .path()
            .with_file_name(format!("run-{index}-main.log"));
        let writer = ManagedConsoleLog::open(path).unwrap();
        writer.write_all(&[index; 64]).unwrap();
        writers.push(writer);
    }
    let newest_path = fixture.path().with_file_name("run-5-main.log");
    let newest = ManagedConsoleLog::open(&newest_path).unwrap();
    assert!(
        newest
            .write_all(b"x")
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    assert_eq!(fs::metadata(&newest_path).unwrap().len(), 0);
    assert!(newest.failure_summary().unwrap().contains("budget"));
    assert_eq!(
        crate::read_log_path_snapshot(newest_path.to_string_lossy().into_owned(), 10).read_error,
        newest.failure_summary()
    );
    drop(writers.remove(0));
    assert!(
        newest
            .write_all(b"manager append must not hide the failure")
            .is_err()
    );
    assert_eq!(
        crate::read_log_path_snapshot(newest_path.to_string_lossy().into_owned(), 10).read_error,
        newest.failure_summary()
    );
    let next_path = fixture.path().with_file_name("run-6-main.log");
    ManagedConsoleLog::open(&next_path)
        .unwrap()
        .write_all(b"x")
        .unwrap();
    assert!(!fixture.path().exists());
    assert_eq!(fs::read(next_path).unwrap(), b"x");
}

#[test]
fn repeated_empty_runs_bound_the_catalog_and_dead_writer_registry() {
    let fixture = Fixture::new();
    let keeper = fixture.writer();
    if let SinkKind::Managed { directory, .. } = &keeper.inner.kind {
        directory.state.lock().unwrap().limits.runs = 4;
    }
    for index in 0..24 {
        drop(
            ManagedConsoleLog::open(
                fixture
                    .path()
                    .with_file_name(format!("run-{index}-fixture.log")),
            )
            .unwrap(),
        );
    }
    if let SinkKind::Managed { directory, .. } = &keeper.inner.kind {
        let state = directory.state.lock().unwrap();
        assert!(state.store.ledger.runs.len() <= 4);
        assert!(state.active.len() <= 2);
    }
}

#[test]
fn ownership_ledger_and_unknown_files_are_not_rewritten_by_normal_chunks() {
    let fixture = Fixture::new();
    let writer = ManagedConsoleLog::open(fixture.path()).unwrap();
    let manifest = fixture.path().parent().unwrap().join("ownership.json");
    let before = fs::read(&manifest).unwrap();
    let unknown = fixture.path().parent().unwrap().join("operator.log");
    fs::write(&unknown, b"old data\n").unwrap();
    for _ in 0..100 {
        writer.write_all(b"output\n").unwrap();
    }
    assert_eq!(fs::read(manifest).unwrap(), before);
    assert_eq!(fs::read(unknown).unwrap(), b"old data\n");
}

#[test]
fn storage_tail_reads_lines_across_console_generation_boundaries() {
    let fixture = Fixture::new();
    let writer = fixture.writer();
    let expected = format!("{}中\nnext line\n", "x".repeat(62));
    writer.write_all(expected.as_bytes()).unwrap();
    let snapshot = crate::read_log_path_snapshot(fixture.path().to_string_lossy().into_owned(), 10);
    assert_eq!(
        snapshot.lines,
        expected.lines().map(String::from).collect::<Vec<_>>()
    );
    assert_eq!(snapshot.read_error, None);
}

#[test]
fn replacing_a_registered_file_is_reported_and_the_replacement_stays_untouched() {
    let fixture = Fixture::new();
    let writer = fixture.writer();
    writer.write_all(b"owned\n").unwrap();
    let previous = fixture.path().with_file_name("preserved.log");
    fs::rename(fixture.path(), previous).unwrap();
    fs::write(fixture.path(), b"external replacement\n").unwrap();
    assert!(writer.write_all(b"do not append").is_err());
    assert_eq!(fs::read(fixture.path()).unwrap(), b"external replacement\n");
    assert!(writer.failure_summary().unwrap().contains("replaced"));
}
