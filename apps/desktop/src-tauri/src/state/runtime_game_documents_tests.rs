use super::*;
use app_core::LogTailSnapshot;

fn document(lines: &[&str], revision: u64, run_id: i64) -> NativeLogDocument {
    NativeLogDocument {
        snapshot: LogTailSnapshot {
            source_path: Some("native.log".into()),
            lines: lines.iter().map(|line| (*line).into()).collect(),
            total_lines: lines.len(),
            truncated: false,
            read_error: None,
        },
        revision,
        run_id,
    }
}

fn reserve(streams: &mut RuntimeLogStreams, instance: &str) -> RuntimeLogStreamLease {
    streams.reserve(instance, "console.log").unwrap();
    let lease = streams
        .reserve_linked(instance, "native.log", "console.log")
        .unwrap();
    streams.begin_native_document(instance, "native.log", lease, document(&["seed"], 10, 1));
    lease
}

#[test]
fn readback_and_native_patch_share_the_published_base_despite_interleaved_file_revisions() {
    let mut streams = RuntimeLogStreams::default();
    let lease = reserve(&mut streams, "ark");
    // A separate HTTP file read must not reseed a running stream's base.
    streams.begin_native_document(
        "ark",
        "native.log",
        lease,
        document(&["unpublished"], 99, 1),
    );
    let (seed, completed) = streams.native_document("ark", "NATIVE.LOG", 1).unwrap();
    assert!(!completed);
    assert_eq!(seed.revision, 10);
    assert_eq!(seed.snapshot.lines, ["seed"]);
    let patch = streams
        .publish_native_document(
            "ark",
            "native.log",
            lease,
            document(&["seed", "same", "same"], 100, 1),
            false,
        )
        .unwrap();
    assert_eq!(patch.previous_revision, seed.revision);
    assert_eq!(patch.lines, ["same", "same"]);
    let (readback, _) = streams.native_document("ark", "native.log", 1).unwrap();
    assert_eq!(readback.revision, patch.revision);
    assert_eq!(readback.snapshot.lines, ["seed", "same", "same"]);
    assert!(
        streams
            .publish_native_document(
                "ark",
                "native.log",
                lease,
                document(&["seed", "same", "same"], 101, 1),
                false
            )
            .is_none()
    );
    assert_eq!(
        streams
            .native_document("ark", "native.log", 1)
            .unwrap()
            .0
            .revision,
        100
    );
}

#[test]
fn stale_revisions_runs_and_leases_cannot_replace_the_current_native_document() {
    let mut streams = RuntimeLogStreams::default();
    let old = reserve(&mut streams, "ark");
    for stale in [
        document(&["late"], 9, 1),
        document(&["duplicate"], 10, 1),
        document(&["other run"], 20, 2),
    ] {
        assert!(
            streams
                .publish_native_document("ark", "native.log", old, stale, true)
                .is_none()
        );
    }
    streams.finish_path("ark", "console.log");
    streams.reserve("ark", "next-console.log").unwrap();
    let current = streams
        .reserve_linked("ark", "native.log", "next-console.log")
        .unwrap();
    streams.begin_native_document("ark", "native.log", current, document(&["new run"], 30, 2));
    assert!(
        streams
            .publish_native_document(
                "ark",
                "native.log",
                old,
                document(&["old run"], 40, 1),
                true
            )
            .is_none()
    );
    assert!(streams.native_document("ark", "native.log", 1).is_none());
    assert_eq!(
        streams
            .native_document("ark", "native.log", 2)
            .unwrap()
            .0
            .snapshot
            .lines,
        ["new run"]
    );
}

#[test]
fn final_patch_readback_survives_release_and_distinguishes_completed_error_recovery() {
    let mut streams = RuntimeLogStreams::default();
    let lease = reserve(&mut streams, "ark");
    streams.finish_path("ark", "console.log");
    let final_patch = streams
        .publish_native_document(
            "ark",
            "native.log",
            lease,
            document(&["seed", "saved"], 40, 1),
            false,
        )
        .unwrap();
    assert!(!streams.native_document("ark", "native.log", 1).unwrap().1);
    assert!(streams.release("ark", "native.log", lease));
    let (final_readback, completed) = streams.native_document("ark", "native.log", 1).unwrap();
    assert!(completed);
    assert_eq!(final_readback.revision, final_patch.revision);
    assert_eq!(final_readback.snapshot.lines, ["seed", "saved"]);

    streams.release_path("ark", "console.log");
    streams.release_path("ark", "native.log");
    assert!(streams.native_document("ark", "native.log", 1).is_none());
    let lease = reserve(&mut streams, "ark");
    let mut failure = document(&[], 80, 1);
    failure.snapshot.read_error = Some("read failed".into());
    streams
        .publish_native_document("ark", "native.log", lease, failure, true)
        .unwrap();
    assert!(
        !streams.native_document("ark", "native.log", 1).unwrap().1,
        "A live failure must recover through its publisher"
    );
    streams.release("ark", "native.log", lease);
    let (failure, completed) = streams.native_document("ark", "native.log", 1).unwrap();
    assert!(
        completed,
        "Only an ended publisher may yield its error to a successful disk read"
    );
    assert_eq!(failure.snapshot.read_error.as_deref(), Some("read failed"));
}

#[test]
fn completed_native_documents_are_bounded_and_cleared_for_a_new_source_lease() {
    let mut streams = RuntimeLogStreams::default();
    for index in 0..129 {
        let instance = format!("ark-{index}");
        let lease = reserve(&mut streams, &instance);
        streams.finish_path(&instance, "console.log");
        streams.release(&instance, "native.log", lease);
    }
    assert!(streams.native_document("ark-0", "native.log", 1).is_none());
    assert!(
        streams
            .native_document("ark-128", "native.log", 1)
            .unwrap()
            .1
    );
    streams.reserve("ark-128", "new-console.log").unwrap();
    streams
        .reserve_linked("ark-128", "native.log", "new-console.log")
        .unwrap();
    assert!(
        streams
            .native_document("ark-128", "native.log", 1)
            .is_none()
    );
}
