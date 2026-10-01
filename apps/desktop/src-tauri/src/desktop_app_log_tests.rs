use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    log: AppLog,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("langame-app-log-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self {
            root,
            log: AppLog {
                policy: LogPolicy {
                    segment_bytes: 1024,
                    total_bytes: 4096,
                    record_bytes: 512,
                },
                lock: Mutex::new(LogState::default()),
            },
        }
    }

    fn active(&self) -> PathBuf {
        self.root.join("desktop-app").join("active.jsonl")
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut result = Vec::new();
        for entry in fs::read_dir(self.active().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                result.push(path.join("entries.jsonl"));
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                result.push(path);
            }
        }
        result
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn rotates_complete_records_within_a_persistent_disk_budget() {
    let fixture = Fixture::new();
    for index in 0..120 {
        fixture
            .log
            .append(
                &fixture.active(),
                &json!({"index": index, "message": "x".repeat(100)}),
            )
            .unwrap();
    }
    let files = fixture.files();
    assert!(files.len() > 1, "threshold must create archives");
    let mut indexes = Vec::new();
    let mut total = 0;
    for path in files {
        let length = fs::metadata(&path).unwrap().len();
        assert!(length <= fixture.log.policy.segment_bytes);
        total += length;
        for line in fs::read_to_string(path).unwrap().lines() {
            let entry: Value = serde_json::from_str(line).unwrap();
            if let Some(index) = entry["index"].as_u64() {
                indexes.push(index);
            }
        }
    }
    assert!(total <= fixture.log.policy.total_bytes);
    indexes.sort_unstable();
    assert_eq!(indexes.last(), Some(&119));
    assert!(
        indexes[0] > 0,
        "only oldest generated records should expire"
    );
    assert!(indexes.windows(2).all(|pair| pair[1] == pair[0] + 1));
}

#[test]
fn refuses_an_existing_unowned_active_file_without_changing_it() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.active().parent().unwrap()).unwrap();
    fs::write(fixture.active(), b"existing user diagnostic data\n").unwrap();
    assert!(
        fixture
            .log
            .append(&fixture.active(), &json!({"message":"new"}))
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.active()).unwrap(),
        b"existing user diagnostic data\n"
    );
}

#[test]
fn interrupted_initial_publication_leaves_no_invalid_target_and_can_retry() {
    let fixture = Fixture::new();
    let path = fixture.root.join("owner");
    let result = publish_new_file_using(&path, |file| {
        file.write_all(b"incomplete")?;
        Err(io::Error::other("injected disk write failure"))
    });
    assert!(result.is_err());
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 0);
    publish_new_file(&path, b"complete").unwrap();
    assert_eq!(fs::read(path).unwrap(), b"complete");
}

#[test]
fn an_unknown_pending_file_is_preserved_without_creating_more_pending_files() {
    let fixture = Fixture::new();
    let target = fixture.root.join("owner");
    fs::write(
        fixture.root.join("owner.pending"),
        b"unidentified publication",
    )
    .unwrap();
    for _ in 0..8 {
        assert!(publish_new_file(&target, b"new").is_err());
    }
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 1);
    assert_eq!(
        fs::read(fixture.root.join("owner.pending")).unwrap(),
        b"unidentified publication"
    );
}

#[test]
fn oversized_record_stays_valid_json_and_declares_omitted_content() {
    let fixture = Fixture::new();
    fixture.log.append(&fixture.active(), &json!({"action":"fixture.large", "message":"large", "context":{"payload":"中".repeat(2048)}})).unwrap();
    let content = fs::read_to_string(fixture.active()).unwrap();
    let line = content.lines().last().unwrap();
    assert!(line.len() < fixture.log.policy.record_bytes);
    let entry: Value = serde_json::from_str(line).unwrap();
    assert_eq!(entry["log_record_truncated"], true);
    assert_eq!(entry["action"], "fixture.large");
}

#[test]
fn restarted_writer_keeps_the_budget_and_never_changes_existing_logs() {
    let fixture = Fixture::new();
    let previous = fixture.root.join("desktop-app.log");
    let original = b"historical diagnostic content\n";
    fs::write(&previous, original).unwrap();
    for index in 0..80 {
        fixture
            .log
            .append(
                &fixture.active(),
                &json!({"index":index,"message":"x".repeat(100)}),
            )
            .unwrap();
    }
    fs::write(
        fixture
            .active()
            .parent()
            .unwrap()
            .join("operator-notes.jsonl"),
        b"do not touch",
    )
    .unwrap();
    let restarted = AppLog {
        policy: fixture.log.policy,
        lock: Mutex::new(LogState::default()),
    };
    for index in 80..160 {
        restarted
            .append(
                &fixture.active(),
                &json!({"index":index,"message":"x".repeat(100)}),
            )
            .unwrap();
    }
    assert_eq!(fs::read(previous).unwrap(), original);
    assert_eq!(
        fs::read(
            fixture
                .active()
                .parent()
                .unwrap()
                .join("operator-notes.jsonl")
        )
        .unwrap(),
        b"do not touch"
    );
    let generated_bytes: u64 = fixture
        .files()
        .into_iter()
        .filter(|path| path.file_name().unwrap() != "operator-notes.jsonl")
        .map(|path| fs::metadata(path).unwrap().len())
        .sum();
    assert!(generated_bytes <= restarted.policy.total_bytes);
    let latest = read_recent_lines(&fixture.active(), None, 1, 64 * 1024).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&latest[0]).unwrap()["index"],
        159
    );
}

#[test]
fn concurrent_writers_preserve_every_complete_record_when_retention_is_not_needed() {
    let mut fixture = Fixture::new();
    fixture.log.policy.total_bytes = 64 * 1024;
    std::thread::scope(|scope| {
        for worker in 0..6 {
            let fixture = &fixture;
            scope.spawn(move || {
                for index in 0..20 {
                    fixture
                        .log
                        .append(
                            &fixture.active(),
                            &json!({"worker":worker,"index":index,"message":"concurrent"}),
                        )
                        .unwrap();
                }
            });
        }
    });
    let mut records = std::collections::BTreeSet::new();
    for path in fixture.files() {
        for line in fs::read_to_string(path).unwrap().lines() {
            let entry: Value = serde_json::from_str(line).unwrap();
            if let (Some(worker), Some(index)) = (entry["worker"].as_u64(), entry["index"].as_u64())
            {
                assert!(records.insert((worker, index)), "duplicate record");
            }
        }
    }
    assert_eq!(records.len(), 120);
}

#[test]
fn latest_window_suppression_result_is_read_from_the_previous_segment() {
    let fixture = Fixture::new();
    fixture.log.append(&fixture.active(), &json!({
        "ts_unix_ms":123,"level":"info","action":"instance.runtime_window.manual_suppression",
        "message":"window hidden","context":{"instance_id":"fixture-instance","suppressed_window_count":1}
    })).unwrap();
    for _ in 0..7 {
        fixture
            .log
            .append(&fixture.active(), &json!({"message":"x".repeat(120)}))
            .unwrap();
    }
    assert!(fixture.files().len() > 1);
    let result = read_recent_lines(&fixture.active(), None, 256, 64 * 1024)
        .unwrap()
        .into_iter()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .find(|entry| entry["action"] == "instance.runtime_window.manual_suppression")
        .unwrap();
    assert_eq!(result["ts_unix_ms"], 123);
    assert_eq!(result["context"]["instance_id"], "fixture-instance");
    assert_eq!(result["context"]["suppressed_window_count"], 1);
}

#[test]
fn historical_fallback_and_rotated_segments_share_the_same_read_budget() {
    let fixture = Fixture::new();
    let previous = fixture.root.join("desktop-app.log");
    fs::write(&previous, b"previous\n").unwrap();
    assert_eq!(
        read_recent_lines(&fixture.active(), Some(&previous), 3, 20).unwrap(),
        vec!["previous"]
    );
    for index in 0..10 {
        fixture
            .log
            .append(
                &fixture.active(),
                &json!({"index":index,"message":"x".repeat(120)}),
            )
            .unwrap();
    }
    let latest = read_recent_lines(&fixture.active(), Some(&previous), 3, 64 * 1024).unwrap();
    assert_eq!(latest.len(), 3);
    assert_eq!(
        serde_json::from_str::<Value>(latest.last().unwrap()).unwrap()["index"],
        9
    );
    let active_bytes = fs::metadata(fixture.active()).unwrap().len();
    let limited = read_recent_lines(&fixture.active(), Some(&previous), 256, active_bytes).unwrap();
    assert!(!limited.iter().any(|line| line == "previous"));
    let all = read_recent_lines(&fixture.active(), Some(&previous), 256, 64 * 1024).unwrap();
    assert_eq!(all.first().unwrap(), "previous");
}

#[test]
fn byte_window_starting_inside_utf8_keeps_complete_newer_records() {
    let fixture = Fixture::new();
    let previous = fixture.root.join("desktop-app.log");
    fs::write(&previous, "中中中\n{\"message\":\"完成\"}\n").unwrap();
    let bytes = fs::metadata(&previous).unwrap().len() - 1;
    assert_eq!(
        read_recent_lines(&previous, None, 256, bytes).unwrap(),
        vec!["{\"message\":\"完成\"}"]
    );
}

#[test]
fn an_interrupted_record_is_preserved_in_an_archive_before_writing_a_new_one() {
    let fixture = Fixture::new();
    fixture
        .log
        .append(&fixture.active(), &json!({"message":"first"}))
        .unwrap();
    OpenOptions::new()
        .append(true)
        .open(fixture.active())
        .unwrap()
        .write_all(b"{\"message\":\"interrupted")
        .unwrap();
    fixture
        .log
        .append(&fixture.active(), &json!({"message":"after recovery"}))
        .unwrap();
    let active = fs::read_to_string(fixture.active()).unwrap();
    assert!(
        active
            .lines()
            .all(|line| serde_json::from_str::<Value>(line).is_ok())
    );
    assert!(
        fixture
            .files()
            .iter()
            .any(|path| fs::read_to_string(path).unwrap().ends_with("interrupted"))
    );
}

#[test]
fn poisoned_write_lock_rejects_new_records_without_growing_the_file() {
    let fixture = Fixture::new();
    fixture
        .log
        .append(&fixture.active(), &json!({"message":"before"}))
        .unwrap();
    let before = fs::read(fixture.active()).unwrap();
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = fixture.log.lock.lock().unwrap();
        panic!("fixture lock poisoning");
    }));
    assert!(
        fixture
            .log
            .append(&fixture.active(), &json!({"message":"after"}))
            .is_err()
    );
    assert_eq!(fs::read(fixture.active()).unwrap(), before);
    assert!(
        fixture
            .log
            .failure_summary(&fixture.active())
            .unwrap()
            .contains("lock poisoned")
    );
}

#[cfg(windows)]
#[test]
fn a_locked_archive_stops_writes_and_reports_the_dropped_record() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    for index in 0..15 {
        fixture
            .log
            .append(
                &fixture.active(),
                &json!({"index":index,"message":"x".repeat(100)}),
            )
            .unwrap();
    }
    let archive = fixture
        .files()
        .into_iter()
        .find(|path| path != &fixture.active())
        .unwrap();
    let before = fs::read(fixture.active()).unwrap();
    let lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(archive)
        .unwrap();
    assert!(
        fixture
            .log
            .append(&fixture.active(), &json!({"message":"blocked"}))
            .is_err()
    );
    assert_eq!(fs::read(fixture.active()).unwrap(), before);
    assert_eq!(fixture.log.lock.lock().unwrap().dropped_records, 1);
    assert!(fixture.log.lock.lock().unwrap().failure.is_some());
    assert!(
        fixture
            .log
            .failure_summary(&fixture.active())
            .unwrap()
            .contains("1 record(s) not persisted")
    );
    drop(lock);
    fixture
        .log
        .append(&fixture.active(), &json!({"message":"recovered"}))
        .unwrap();
    assert!(fixture.log.lock.lock().unwrap().failure.is_none());
}

#[test]
fn archive_names_alone_never_authorize_retention_deletion() {
    let fixture = Fixture::new();
    fixture
        .log
        .append(&fixture.active(), &json!({"message":"initial"}))
        .unwrap();
    let unknown = fixture
        .active()
        .parent()
        .unwrap()
        .join("segment-00000000000000000100");
    fs::create_dir(&unknown).unwrap();
    fs::write(
        unknown.join("entries.jsonl"),
        b"operator-provided archive\n",
    )
    .unwrap();
    for index in 0..100 {
        fixture
            .log
            .append(
                &fixture.active(),
                &json!({"index":index,"message":"x".repeat(100)}),
            )
            .unwrap();
    }
    assert_eq!(
        fs::read(unknown.join("entries.jsonl")).unwrap(),
        b"operator-provided archive\n"
    );
}

#[cfg(windows)]
#[test]
fn refuses_a_managed_directory_junction_without_touching_its_target() {
    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("active.jsonl"), b"must survive\n").unwrap();
    let junction = fixture.active().parent().unwrap().to_path_buf();
    let output = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .output()
        .unwrap();
    assert!(output.status.success(), "create isolated fixture junction");
    let result = fixture
        .log
        .append(&fixture.active(), &json!({"message":"blocked"}));
    fs::remove_dir(&junction).unwrap();
    assert!(result.unwrap_err().to_string().contains("reparse"));
    assert_eq!(
        fs::read(outside.join("active.jsonl")).unwrap(),
        b"must survive\n"
    );
    assert!(!outside.join(OWNER_FILE).exists());
}
