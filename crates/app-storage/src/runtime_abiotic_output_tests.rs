use super::*;

#[test]
fn native_log_snapshots_redact_modio_values_without_changing_files_or_line_counts() {
    let root = std::env::temp_dir().join(format!(
        "langame-abiotic-log-redaction-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("AbioticFactor.log");
    let original = concat!(
        "LogInit: -ModioToken=synthetic-native-token-one -Port=7777\n",
        "Session creation completed\n",
        "Saved world successfully\n",
        "LogInit: -mOdIoToKeN=\"synthetic-native-token-two\"\n"
    );
    fs::write(&path, original).unwrap();
    let complete = read_log_path_snapshot(path.to_string_lossy().into_owned(), 20);
    assert_eq!(complete.total_lines, 4);
    assert!(!complete.truncated);
    assert_eq!(
        complete.lines,
        [
            "LogInit: -ModioToken=[REDACTED] -Port=7777",
            "Session creation completed",
            "Saved world successfully",
            "LogInit: -mOdIoToKeN=\"[REDACTED]\"",
        ]
    );
    let tail = read_log_path_snapshot(path.to_string_lossy().into_owned(), 2);
    assert_eq!(tail.total_lines, 4);
    assert!(tail.truncated);
    assert_eq!(tail.lines, complete.lines[2..]);
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}
