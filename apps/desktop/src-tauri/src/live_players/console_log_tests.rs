use super::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

// Synthetic command responses in isolated files, never captured from a live server.
struct FixtureLog {
    root: PathBuf,
    path: PathBuf,
}

impl FixtureLog {
    fn new(history: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-console-player-test-{}",
            uuid::Uuid::new_v4().simple(),
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("server.log");
        fs::write(&path, history).unwrap();
        Self { root, path }
    }

    fn context(&self) -> ConsoleCollection<'_> {
        ConsoleCollection {
            instance_id: "console-instance",
            request_id: "console-request",
            codec: ModulePlayerListCodec::NecessePlayers,
            log_path: &self.path,
            observed_at: 1234,
        }
    }
}

impl Drop for FixtureLog {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    file.flush().unwrap();
}

fn row(slot: usize, name: &str) -> String {
    format!("Slot {slot}: {slot} \"{name}\", latency: 42, level: 0x0d0,conn: LOCAL\n")
}

#[tokio::test]
async fn console_reads_only_rows_appended_after_the_request_baseline() {
    let log = FixtureLog::new(&format!("Players online: 1/8\n{}", row(1, "Historical")));
    let result = collect_console_players(log.context(), |submission, _| {
        submission.mark_submitted();
        async {
            append(
                &log.path,
                &format!("Players online: 1/8\n{}", row(2, "Current")),
            );
            Ok(())
        }
    })
    .await
    .unwrap();
    let snapshot = result.public_snapshot;
    assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Ready);
    assert_eq!(snapshot.source, Some(ModulePlayerListSource::ConsoleLog));
    assert_eq!(snapshot.current_players, Some(1));
    assert_eq!(snapshot.entries[0].display_name, "Current");
    assert!(snapshot.complete);
    assert!(result.private_action_bindings.is_empty());
    assert!(
        snapshot
            .entries
            .iter()
            .all(|entry| entry.available_action_ids.is_empty() && entry.identifiers.is_empty())
    );
}

#[tokio::test]
async fn console_waits_for_every_counted_row_and_its_line_terminator() {
    let log = FixtureLog::new("startup complete\n");
    let (submitted, observed) = tokio::sync::oneshot::channel();
    let mut collection = Box::pin(collect_console_players_with_budget(
        log.context(),
        Duration::from_secs(1),
        |submission, _| {
            submission.mark_submitted();
            async {
                append(
                    &log.path,
                    &format!(
                        "Players online: 2/8\n{}{}",
                        row(1, "Alice"),
                        row(2, "Bob").trim_end()
                    ),
                );
                submitted.send(()).unwrap();
                Ok(())
            }
        },
    ));
    tokio::select! {
        _ = observed => {}
        _ = &mut collection => panic!("a partial counted response was published"),
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut collection)
            .await
            .is_err()
    );
    append(&log.path, "\n");
    let snapshot = collection.await.unwrap().public_snapshot;
    assert!(snapshot.complete);
    assert_eq!(snapshot.current_players, Some(2));
    assert_eq!(
        snapshot
            .entries
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alice", "Bob"]
    );
}

#[tokio::test]
async fn console_history_and_incomplete_responses_never_become_empty_success() {
    for response in [
        String::new(),
        format!("Players online: 2/8\n{}", row(1, "Alice")),
        String::from("Players online: 0/8"),
    ] {
        let log = FixtureLog::new("Players online: 0/8\n");
        let failure = collect_console_players_with_budget(
            log.context(),
            Duration::from_millis(150),
            |submission, _| {
                submission.mark_submitted();
                async {
                    append(&log.path, &response);
                    Ok(())
                }
            },
        )
        .await
        .unwrap_err();
        assert_eq!(failure.status, RuntimeLivePlayerStatus::Failed);
        assert_eq!(
            failure.issue.as_ref().unwrap().code,
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        );
        assert!(!failure.complete);
        assert!(failure.current_players.is_none());
        assert!(failure.entries.is_empty());
    }
}

#[tokio::test]
async fn console_accepts_an_explicit_new_zero_count() {
    let log = FixtureLog::new("previous session\n");
    let result = collect_console_players(log.context(), |_, _| async {
        append(&log.path, "Players online: 0/8\n");
        Ok(())
    })
    .await
    .unwrap();
    assert!(result.public_snapshot.complete);
    assert_eq!(result.public_snapshot.current_players, Some(0));
    assert!(result.public_snapshot.entries.is_empty());
}

#[tokio::test]
async fn console_rejects_log_rotation_and_truncation_during_dispatch() {
    for rotate in [true, false] {
        let log = FixtureLog::new(&"old console output\n".repeat(20));
        let failure = collect_console_players(log.context(), |_, _| async {
            if rotate {
                fs::rename(&log.path, log.root.join("retired.log")).unwrap();
            }
            fs::write(&log.path, "Players online: 0/8\n").unwrap();
            Ok(())
        })
        .await
        .unwrap_err();
        assert_eq!(
            failure.issue.as_ref().unwrap().code,
            RuntimeLivePlayerIssueCode::LogUnavailable
        );
        assert!(!failure.complete);
        assert!(failure.entries.is_empty());
    }
}

#[tokio::test]
async fn console_fails_on_capture_limit_instead_of_publishing_a_prefix() {
    let log = FixtureLog::new("old console\n");
    let failure = collect_console_players(log.context(), |_, _| async {
        append(
            &log.path,
            &format!("Players online: 0/8\n{}", "x".repeat(64 * 1024)),
        );
        Ok(())
    })
    .await
    .unwrap_err();
    assert_eq!(
        failure.issue.as_ref().unwrap().code,
        RuntimeLivePlayerIssueCode::CaptureLimit
    );
    assert!(failure.truncated);
    assert!(!failure.complete);
    assert!(failure.entries.is_empty());
}

#[tokio::test]
async fn console_dispatch_failure_does_not_accept_incidental_log_output() {
    let log = FixtureLog::new("old console\n");
    let failure = collect_console_players(log.context(), |_, _| async {
        append(&log.path, "Players online: 0/8\n");
        Err(String::from("synthetic dispatch rejection"))
    })
    .await
    .unwrap_err();
    assert_eq!(
        failure.issue.as_ref().unwrap().code,
        RuntimeLivePlayerIssueCode::RuntimeActionUnavailable
    );
    assert!(failure.current_players.is_none());
}

#[tokio::test]
async fn console_unsubmitted_dispatch_timeout_is_reported_as_timeout() {
    let log = FixtureLog::new("old console\n");
    let failure = collect_console_players_with_budget(
        log.context(),
        Duration::from_millis(150),
        |_, _| async { std::future::pending::<Result<(), String>>().await },
    )
    .await
    .unwrap_err();
    assert_eq!(
        failure.issue.as_ref().unwrap().code,
        RuntimeLivePlayerIssueCode::CollectionTimeout
    );
    assert!(failure.current_players.is_none());
}
