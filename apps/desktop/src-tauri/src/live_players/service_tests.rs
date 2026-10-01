use std::fs::{self, OpenOptions};
use std::io::Write;
use std::time::{Duration, Instant};

use app_core::{ModulePlayerListSource, RuntimeLivePlayerIssueCode, RuntimeLivePlayerStatus};

use super::service::{
    DstStructuredLogCollectionBudget, RuntimeActionDispatchDeadlineOutcome,
    await_runtime_action_dispatch_until, collect_dst_structured_log,
    collect_dst_structured_log_with_budget, unsupported_snapshot,
};

fn temp_log(test_name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "langame-live-players-{test_name}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&root).expect("create isolated test directory");
    let path = root.join("server.log");
    fs::write(&path, "old server log\n").expect("seed isolated player log");
    (root, path)
}

#[test]
fn commands_live_players_unsupported_snapshot_never_fakes_an_empty_list() {
    let snapshot = unsupported_snapshot("instance-a", String::from("capability-a"));

    assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Unsupported);
    assert!(!snapshot.complete);
    assert!(snapshot.current_players.is_none());
    assert!(snapshot.entries.is_empty());
    assert!(snapshot.observed_at_unix_ms.is_none());
}

#[tokio::test]
async fn runtime_action_deadline_preserves_submitted_pending_outcome() {
    let deadline = Instant::now() + Duration::from_millis(20);

    let outcome =
        await_runtime_action_dispatch_until(deadline, |submission, same_deadline| async move {
            assert_eq!(same_deadline, deadline);
            submission.mark_submitted();
            std::future::pending::<Result<(), String>>().await
        })
        .await
        .expect("an accepted submission is not a retry-safe dispatch failure");

    assert_eq!(
        outcome,
        RuntimeActionDispatchDeadlineOutcome::SubmittedPending
    );
}

#[tokio::test]
async fn pending_dispatch_phase_leaves_budget_for_the_correlated_response() {
    let (root, path) = temp_log("pending-dispatch-response");
    let request_id = "a123456789abcdef0123456789abcdef";
    let append_path = path.clone();

    let result = collect_dst_structured_log_with_budget(
        "instance-a",
        ModulePlayerListSource::StructuredLog,
        request_id,
        &path,
        &[String::from("kick_userid")],
        10_000,
        DstStructuredLogCollectionBudget::for_test(
            Duration::from_millis(500),
            Duration::from_millis(50),
        ),
        move |submission, dispatch_deadline| async move {
            submission.mark_submitted();
            tokio::spawn(async move {
                tokio::time::sleep_until(
                    tokio::time::Instant::from_std(dispatch_deadline) + Duration::from_millis(40),
                )
                .await;
                let mut file = OpenOptions::new()
                    .append(true)
                    .open(append_path)
                    .expect("open delayed response log");
                writeln!(file, "[LGM-DST-PLAYERS-BEGIN]\t{request_id}")
                    .expect("append delayed response start");
                writeln!(
                    file,
                    "[LGM-DST-PLAYER]\t{request_id}\t1\tKU_delayed\tDelayed\twilson\t0"
                )
                .expect("append delayed response row");
                writeln!(file, "[LGM-DST-PLAYERS-END]\t{request_id}\t1")
                    .expect("append delayed response end");
                file.flush().expect("flush delayed response");
            });
            std::future::pending::<Result<(), String>>().await
        },
    )
    .await
    .expect("a submitted command may complete through its remaining response budget");

    assert_eq!(
        result.public_snapshot.status,
        RuntimeLivePlayerStatus::Ready
    );
    assert_eq!(result.public_snapshot.current_players, Some(1));
    assert_eq!(result.public_snapshot.entries[0].display_name, "Delayed");
    fs::remove_dir_all(root).expect("remove isolated test directory");
}

#[tokio::test]
async fn submitted_command_without_response_is_not_classified_as_dispatch_failure() {
    let (root, path) = temp_log("accepted-without-response");
    let request_id = "b123456789abcdef0123456789abcdef";

    let failure = collect_dst_structured_log_with_budget(
        "instance-a",
        ModulePlayerListSource::StructuredLog,
        request_id,
        &path,
        &[],
        10_000,
        DstStructuredLogCollectionBudget::for_test(
            Duration::from_millis(160),
            Duration::from_millis(20),
        ),
        move |submission, _| async move {
            submission.mark_submitted();
            std::future::pending::<Result<(), String>>().await
        },
    )
    .await
    .expect_err("the accepted action did not produce a response");

    let issue = failure.issue.expect("response wait issue");
    assert_eq!(issue.code, RuntimeLivePlayerIssueCode::CollectionTimeout);
    assert!(issue.summary.contains("action was accepted"));
    fs::remove_dir_all(root).expect("remove isolated test directory");
}

#[tokio::test]
async fn commands_live_players_dst_collection_reads_only_correlated_appended_rows() {
    let (root, path) = temp_log("correlated");
    let request_id = "0123456789abcdef0123456789abcdef";
    let append_path = path.clone();

    let result = collect_dst_structured_log(
        "instance-a",
        ModulePlayerListSource::StructuredLog,
        request_id,
        &path,
        &[String::from("kick_userid")],
        10_000,
        move |_, _| async move {
            let mut file = OpenOptions::new()
                .append(true)
                .open(append_path)
                .map_err(|error| error.to_string())?;
            writeln!(
                file,
                "[LGM-DST-PLAYERS-BEGIN]\tffffffffffffffffffffffffffffffff"
            )
            .map_err(|error| error.to_string())?;
            writeln!(
                file,
                "[LGM-DST-PLAYER]\tffffffffffffffffffffffffffffffff\t1\tKU_foreign\tForeign\twilson\t0"
            )
            .map_err(|error| error.to_string())?;
            writeln!(
                file,
                "[LGM-DST-PLAYERS-END]\tffffffffffffffffffffffffffffffff\t1"
            )
            .map_err(|error| error.to_string())?;
            writeln!(file, "[LGM-DST-PLAYERS-BEGIN]\t{request_id}")
                .map_err(|error| error.to_string())?;
            writeln!(
                file,
                "[LGM-DST-PLAYER]\t{request_id}\t1\tKU_local\t温蒂\twendy\t0"
            )
            .map_err(|error| error.to_string())?;
            writeln!(file, "[LGM-DST-PLAYERS-END]\t{request_id}\t1")
                .map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())
        },
    )
    .await
    .expect("complete correlated capture");

    assert_eq!(
        result.public_snapshot.status,
        RuntimeLivePlayerStatus::Ready
    );
    assert!(result.public_snapshot.complete);
    assert_eq!(result.public_snapshot.current_players, Some(1));
    assert_eq!(result.public_snapshot.entries.len(), 1);
    assert_eq!(result.public_snapshot.entries[0].display_name, "温蒂");
    assert_eq!(
        result.private_action_bindings.get(&(
            result.public_snapshot.entries[0].player_key.clone(),
            String::from("kick_userid")
        )),
        Some(&String::from("KU_local"))
    );

    fs::remove_dir_all(root).expect("remove isolated test directory");
}

#[tokio::test]
async fn commands_live_players_dst_collection_rejects_overlong_capture() {
    let (root, path) = temp_log("capture-limit");
    let request_id = "fedcba9876543210fedcba9876543210";
    let append_path = path.clone();

    let failure = collect_dst_structured_log(
        "instance-a",
        ModulePlayerListSource::StructuredLog,
        request_id,
        &path,
        &[String::from("kick_userid")],
        10_000,
        move |_, _| async move {
            let mut file = OpenOptions::new()
                .append(true)
                .open(append_path)
                .map_err(|error| error.to_string())?;
            writeln!(file, "[LGM-DST-PLAYERS-BEGIN]\t{request_id}")
                .map_err(|error| error.to_string())?;
            writeln!(file, "{}", "x".repeat(5_000)).map_err(|error| error.to_string())?;
            file.flush().map_err(|error| error.to_string())
        },
    )
    .await
    .expect_err("overlong response must not become authoritative");

    assert_eq!(failure.status, RuntimeLivePlayerStatus::Failed);
    assert!(!failure.complete);
    assert!(failure.truncated);
    assert_eq!(
        failure.issue.expect("capture issue").code,
        RuntimeLivePlayerIssueCode::CaptureLimit
    );

    fs::remove_dir_all(root).expect("remove isolated test directory");
}
