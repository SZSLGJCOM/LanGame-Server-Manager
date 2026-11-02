use super::*;

#[test]
fn document_readback_and_event_cursors_describe_the_same_rows() {
    let mut streams = RuntimeLogStreams::default();
    let lease = streams.reserve("world", "server.log").unwrap();
    streams.begin_console_document("world", "server.log", lease, Some("main"), None, Some(1));
    let first = streams
        .publish_console_document(
            "world",
            "server.log",
            lease,
            &["same".into(), "same".into()],
            vec!["load".into()],
            true,
        )
        .unwrap();
    let before = streams.console_documents("world", Some("SERVER.LOG"), 400, None);
    assert_eq!(before[0].stream_id, first.stream_id);
    assert_eq!(before[0].revision, first.revision);
    assert_eq!(before[0].lines, ["same", "same"]);
    assert_eq!(before[0].partial_lines, ["load"]);
    let next = streams
        .publish_console_document(
            "world",
            "server.log",
            lease,
            &["loading".into()],
            vec![],
            false,
        )
        .unwrap();
    assert_eq!(next.previous_revision, before[0].revision);
    assert_eq!(
        streams.console_documents("world", None, 400, None)[0].lines,
        ["same", "same", "loading"]
    );
    assert_eq!(before[0].lines, ["same", "same"]);
}

#[test]
fn eof_recovery_retains_final_rows_without_retaining_a_live_lease() {
    let mut streams = RuntimeLogStreams::default();
    let lease = streams.reserve("world", "server.log").unwrap();
    streams.begin_console_document("world", "server.log", lease, Some("main"), None, Some(1));
    streams
        .publish_console_document(
            "world",
            "server.log",
            lease,
            &["final".into()],
            vec![],
            true,
        )
        .unwrap();
    streams.finish_path("world", "server.log");
    assert!(streams.release("world", "server.log", lease));
    assert!(streams.is_empty());
    let final_document = streams
        .console_documents("world", Some("server.log"), 400, None)
        .remove(0);
    assert_eq!(final_document.lines, ["final"]);
    let replacement = streams.reserve("world", "server.log").unwrap();
    streams.begin_console_document(
        "world",
        "server.log",
        replacement,
        Some("main"),
        None,
        Some(2),
    );
    let current = streams
        .console_documents("world", None, 400, None)
        .remove(0);
    assert_ne!(current.stream_id, final_document.stream_id);
    assert!(current.started_at_unix_ms > final_document.started_at_unix_ms);
    assert!(current.lines.is_empty());
    assert!(
        streams
            .publish_console_document("world", "server.log", lease, &["old".into()], vec![], false)
            .is_none()
    );
}

#[test]
fn failed_source_recovery_replaces_partial_rows_and_keeps_watching_the_retained_file() {
    use std::io::Write;
    let root =
        std::env::temp_dir().join(format!("langame-console-readback-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("failed.log");
    std::fs::write(&path, "first\nloading complete\nfinal\n").unwrap();
    let path = path.to_string_lossy().into_owned();
    let mut streams = RuntimeLogStreams::default();
    let failed = streams.reserve("ark", &path).unwrap();
    streams.begin_console_document("ark", &path, failed, Some("island"), None, Some(1));
    let update = streams
        .publish_console_document(
            "ark",
            &path,
            failed,
            &["first".into()],
            vec!["load".into()],
            false,
        )
        .unwrap();
    let initial = streams
        .console_documents("ark", Some(&path), 400, None)
        .remove(0);
    streams.invalidate_console_document("ark", &path, failed);
    streams.release("ark", &path, failed);

    let healthy = streams.reserve("ark", "healthy.log").unwrap();
    streams.begin_console_document("ark", "healthy.log", healthy, Some("desert"), None, Some(2));
    streams.publish_console_document(
        "ark",
        "healthy.log",
        healthy,
        &["healthy".into()],
        vec![],
        false,
    );
    let sources = streams.console_documents("ark", None, 400, None);
    assert_eq!(sources.len(), 2);
    let recovered = sources
        .iter()
        .find(|source| source.log_path == path)
        .unwrap();
    assert_eq!(recovered.lines, ["first", "loading complete", "final"]);
    assert!(recovered.partial_lines.is_empty());
    assert!(recovered.read_error.is_none());
    assert_eq!(recovered.stream_id, update.stream_id);
    assert_eq!(recovered.started_at_unix_ms, initial.started_at_unix_ms);
    assert!(recovered.revision > update.revision);
    let first_revision = recovered.revision;
    assert_eq!(
        streams.console_documents("ark", Some(&path), 400, None)[0].revision,
        first_revision
    );

    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"after observer exit\n")
        .unwrap();
    let later = streams
        .console_documents("ark", Some(&path), 400, None)
        .remove(0);
    assert_eq!(later.lines.last().unwrap(), "after observer exit");
    assert!(later.revision > first_revision);
    assert!(later.requires_readback);
    assert!(
        serde_json::to_value(&later)
            .unwrap()
            .get("requires_readback")
            .is_none()
    );

    std::fs::remove_file(&path).unwrap();
    let failure = streams
        .console_documents("ark", Some(&path), 400, None)
        .remove(0);
    assert!(failure.read_error.is_some());
    assert!(failure.lines.is_empty());
    assert!(failure.revision > later.revision);
    std::fs::write(&path, "restored final\n").unwrap();
    let restored = streams
        .console_documents("ark", Some(&path), 400, None)
        .remove(0);
    assert_eq!(restored.lines, ["restored final"]);
    assert!(restored.read_error.is_none());
    assert!(restored.revision > failure.revision);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn pending_start_filters_an_old_failed_source_before_any_disk_read() {
    let mut streams = RuntimeLogStreams::default();
    let old = streams
        .reserve("ark", "nonexistent-old-source.log")
        .unwrap();
    streams.begin_console_document(
        "ark",
        "nonexistent-old-source.log",
        old,
        None,
        None,
        Some(1),
    );
    streams.publish_console_document(
        "ark",
        "nonexistent-old-source.log",
        old,
        &["old".into()],
        vec!["partial".into()],
        false,
    );
    streams.invalidate_console_document("ark", "nonexistent-old-source.log", old);
    streams.release("ark", "nonexistent-old-source.log", old);
    let current = streams.reserve("ark", "current.log").unwrap();
    streams.begin_console_document("ark", "current.log", current, None, None, Some(2));
    streams.publish_console_document(
        "ark",
        "current.log",
        current,
        &["current".into()],
        vec![],
        false,
    );
    let since = streams
        .active
        .get(&stream_key("ark", "current.log"))
        .unwrap()
        .document
        .as_ref()
        .unwrap()
        .started_at_unix_ms;
    let selected = streams.console_documents("ark", None, 400, Some(since));
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].lines, ["current"]);
    let old = &streams.completed_documents[0].1;
    assert_eq!(
        old.revision, 1,
        "Filtering must precede the failed file's recovery read"
    );
    assert_eq!(old.partial_lines, ["partial"]);
    assert!(old.read_error.is_none());
}

#[test]
fn a_stale_failure_cannot_invalidate_a_replacement_console_document() {
    let mut streams = RuntimeLogStreams::default();
    let old = streams.reserve("ark", "console.log").unwrap();
    streams.finish_path("ark", "console.log");
    let current = streams.reserve("ark", "console.log").unwrap();
    streams.begin_console_document("ark", "console.log", current, None, None, Some(2));
    streams.publish_console_document(
        "ark",
        "console.log",
        current,
        &["current".into()],
        vec![],
        false,
    );
    streams.invalidate_console_document("ark", "console.log", old);
    let current = streams.console_documents("ark", None, 400, None).remove(0);
    assert_eq!(current.lines, ["current"]);
    assert!(!current.requires_readback);
}
