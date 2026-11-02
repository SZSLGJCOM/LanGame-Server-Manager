use super::*;

fn document() -> ConsoleSourceDocument {
    ConsoleSourceDocument::new("run.log", Some("main"), None, Some(1), 100)
}

#[test]
fn partial_updates_and_commit_have_distinct_revisions_without_duplicate_rows() {
    let mut document = document();
    let first = document.publish(&[], vec!["Load".into()], true, 101);
    assert_eq!((first.previous_revision, first.revision), (0, 1));
    assert_eq!(document.lines, Vec::<String>::new());
    let next = document.publish(&[], vec!["Loading".into()], false, 0);
    assert_eq!((next.previous_revision, next.revision), (1, 2));
    document.publish(&["Loading".into(), "Loading".into()], vec![], false, 0);
    assert_eq!(document.lines, ["Loading", "Loading"]);
    assert_eq!(document.total_lines, 2);
    assert!(document.partial_lines.is_empty());
}

#[test]
fn rotation_changes_generation_and_does_not_reuse_old_history_or_partial() {
    let mut document = document();
    let first = document.publish(&["old".into()], vec!["pending".into()], true, 101);
    let next = document.publish(&["new".into()], vec![], true, 102);
    assert_ne!(next.stream_id, first.stream_id);
    assert!(next.started_at_unix_ms > first.started_at_unix_ms);
    assert_eq!((next.previous_revision, next.revision), (0, 1));
    assert_eq!(document.lines, ["new"]);
    assert_eq!(document.total_lines, 1);
    assert!(document.partial_lines.is_empty());
}

#[test]
fn bounded_readback_keeps_real_repetitions_and_reports_retention() {
    let mut document = document();
    document.publish(
        &vec!["repeat".into(); 1000],
        vec!["玩家😀".into()],
        true,
        101,
    );
    assert_eq!(document.lines.len(), MAX_LINES);
    assert!(document.lines.iter().all(|line| line == "repeat"));
    assert_eq!(document.total_lines, 1000);
    assert_eq!(document.partial_lines, ["玩家😀"]);
    assert!(document.truncated);
    document.publish(&vec!["x".repeat(64 * 1024); 20], vec![], false, 0);
    assert!(document.lines.iter().map(String::len).sum::<usize>() <= MAX_BYTES);
}
