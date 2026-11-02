use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-startup-session-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
    fn begin(&self, attempt: &str, at: u128) -> (StartupSessionReceipt, ManagedConsoleLog) {
        begin(&self.0, "instance", attempt, at).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn independent_startup_receipt_survives_failure_cancel_and_reopen_without_a_run() {
    let fixture = Fixture::new();
    let (first, mut writer) = fixture.begin("attempt-failed", 100);
    writeln!(writer, "self-check failed").unwrap();
    let failed = finish(
        &fixture.0,
        &first.attempt_id,
        StartupSessionStatus::Failed,
        vec![],
        Some("original cause".into()),
    )
    .unwrap();
    drop(writer);
    assert_eq!(failed.status, StartupSessionStatus::Failed);
    assert!(failed.finished_at_unix_ms.is_some());
    let restored = read_latest(&fixture.0, "instance").unwrap().unwrap();
    assert_eq!(restored.error.as_deref(), Some("original cause"));
    assert!(restored.run_ids.is_empty());
    assert_eq!(
        read_log_page(Path::new(&restored.source_path), None, 400)
            .unwrap()
            .snapshot
            .lines,
        ["self-check failed"]
    );
    let (second, writer) = fixture.begin("attempt-cancelled", 100);
    assert_eq!(second.started_at_unix_ms, 101);
    assert_ne!(first.source_path, second.source_path);
    finish(
        &fixture.0,
        &second.attempt_id,
        StartupSessionStatus::Cancelled,
        vec![],
        Some("cancelled by user".into()),
    )
    .unwrap();
    assert!(
        finish(
            &fixture.0,
            &first.attempt_id,
            StartupSessionStatus::Succeeded,
            vec![9],
            None
        )
        .is_err()
    );
    assert_eq!(
        read_latest(&fixture.0, "instance").unwrap().unwrap().status,
        StartupSessionStatus::Cancelled
    );
    assert!(read_latest(&fixture.0, "another-instance").is_err());
    drop(writer);
}

#[test]
fn an_unowned_starting_receipt_is_recovered_once_with_its_original_identity() {
    let fixture = Fixture::new();
    let (receipt, writer) = fixture.begin("interrupted", 150);
    let recovered = recover_interrupted(&fixture.0, "instance", "interrupted").unwrap();
    assert_eq!(recovered.status, StartupSessionStatus::Interrupted);
    assert_eq!(recovered.attempt_id, receipt.attempt_id);
    assert!(
        recovered
            .error
            .as_deref()
            .unwrap()
            .contains("runtime session ended")
    );
    recover_interrupted(&fixture.0, "instance", "interrupted").unwrap();
    let page = read_log_page(Path::new(&receipt.source_path), None, 400).unwrap();
    assert_eq!(page.snapshot.lines.len(), 1);
    assert!(page.snapshot.lines[0].contains("[LanGameCMD/startup] Startup interrupted"));
    drop(writer);
}

#[test]
fn startup_pages_preserve_positions_repeated_rows_and_older_than_four_hundred_lines() {
    let fixture = Fixture::new();
    let (receipt, mut writer) = fixture.begin("attempt", 200);
    let expected = (0..903)
        .map(|index| {
            if index % 3 == 0 {
                "real repeat".into()
            } else {
                format!("row {index}")
            }
        })
        .collect::<Vec<_>>();
    for line in &expected {
        writeln!(writer, "{line}").unwrap();
    }
    let path = Path::new(&receipt.source_path);
    let tail = read_log_page(path, None, 400).unwrap();
    assert_eq!(tail.snapshot.total_lines, 903);
    assert_eq!(tail.snapshot.lines, expected[503..]);
    let older = read_log_page(path, tail.before_cursor.as_deref(), 400).unwrap();
    assert_eq!(older.snapshot.total_lines, 503);
    assert_eq!(older.snapshot.lines, expected[103..503]);
    let oldest = read_log_page(path, older.before_cursor.as_deref(), 400).unwrap();
    assert_eq!(oldest.snapshot.total_lines, 103);
    assert_eq!(oldest.snapshot.lines, expected[..103]);
    assert!(oldest.before_cursor.is_none());
    assert_eq!(oldest.retained_start_cursor, "0");
    for invalid in ["-1", "abc", "1", "99999999"] {
        assert!(read_log_page(path, Some(invalid), 400).is_err());
    }
    drop(writer);
}

#[test]
fn diagnostic_retries_repair_unacknowledged_appends_without_repeating_them() {
    let fixture = Fixture::new();
    let (receipt, writer) = fixture.begin("attempt", 300);
    record_diagnostic(
        &fixture.0,
        "instance",
        "attempt",
        "read-1",
        "read failed\noriginal detail",
    )
    .unwrap();
    // Simulate a durable append whose final receipt ACK was lost.
    let canonical = fs::canonicalize(&fixture.0).unwrap();
    let mut stored = read_stored(&canonical).unwrap().unwrap();
    stored.diagnostics[0].written = false;
    save(&canonical, &stored).unwrap();
    record_diagnostic(
        &fixture.0,
        "instance",
        "attempt",
        "read-1",
        "read failed\noriginal detail",
    )
    .unwrap();
    record_diagnostic(
        &fixture.0,
        "instance",
        "attempt",
        "read-1",
        "read failed\noriginal detail",
    )
    .unwrap();
    let lines = read_log_page(Path::new(&receipt.source_path), None, 400)
        .unwrap()
        .snapshot
        .lines;
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("[LanGameCMD/frontend]"));
    assert!(lines[0].contains("original detail"));
    assert!(
        record_diagnostic(
            &fixture.0,
            "instance",
            "attempt",
            "read-1",
            "different error"
        )
        .is_err()
    );
    assert!(record_diagnostic(&fixture.0, "instance", "stale", "read-2", "error").is_err());
    assert!(
        record_diagnostic(
            &fixture.0,
            "instance",
            "attempt",
            "read-2",
            &"x".repeat(4097)
        )
        .is_err()
    );
    drop(writer);
}

#[test]
fn reader_diagnostic_survives_an_unreadable_owned_log_without_writing_into_it() {
    let fixture = Fixture::new();
    let (receipt, writer) = fixture.begin("attempt", 400);
    drop(writer);
    let path = Path::new(&receipt.source_path);
    let renamed = path.with_extension("saved");
    fs::rename(path, &renamed).unwrap();
    record_reader_error(path, "sharing violation: original error").unwrap();
    assert!(
        !path.exists(),
        "Reader errors must not recursively create their failed source"
    );
    let restored = read_latest(&fixture.0, "instance").unwrap().unwrap();
    assert!(
        restored
            .reader_error
            .unwrap()
            .contains("[LanGameCMD/reader] sharing violation: original error")
    );
    fs::rename(renamed, path).unwrap();
}

#[test]
fn a_tampered_receipt_cannot_redirect_startup_reads_or_diagnostics() {
    let fixture = Fixture::new();
    let (_, writer) = fixture.begin("attempt", 500);
    let canonical = fs::canonicalize(&fixture.0).unwrap();
    let mut stored = read_stored(&canonical).unwrap().unwrap();
    stored.receipt.source_path = fixture.0.join("unowned.log").to_string_lossy().into_owned();
    save(&canonical, &stored).unwrap();
    assert!(read_latest(&fixture.0, "instance").is_err());
    assert!(record_diagnostic(&fixture.0, "instance", "attempt", "id", "error").is_err());
    assert!(!fixture.0.join("unowned.log").exists());
    drop(writer);
}

#[test]
fn native_checkpoint_is_persisted_with_the_successful_run_ids() {
    let fixture = Fixture::new();
    let (receipt, writer) = fixture.begin("attempt", 600);
    let native = fixture.0.join("native.log");
    fs::write(&native, "startup\n").unwrap();
    let mut file = fs::File::open(&native).unwrap();
    let checkpoint =
        NativeStartupSource::capture("main".into(), &native, 123, &mut file, 8).unwrap();
    record_native_source(Path::new(&receipt.source_path), &checkpoint).unwrap();
    let saved = finish(
        &fixture.0,
        "attempt",
        StartupSessionStatus::Succeeded,
        vec![7, 8],
        None,
    )
    .unwrap();
    assert_eq!(saved.run_ids, [7, 8]);
    let reopened = read_latest(&fixture.0, "instance").unwrap().unwrap();
    assert_eq!(reopened.native_sources.len(), 1);
    assert_eq!(
        reopened.native_sources[0]
            .validated_offset(&native, &mut file, 123)
            .unwrap(),
        Some(8)
    );
    assert!(
        reopened.native_sources[0]
            .validated_offset(&native, &mut file, 124)
            .unwrap()
            .is_none()
    );
    drop(file);
    drop(writer);
}
