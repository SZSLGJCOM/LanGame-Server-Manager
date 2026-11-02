use super::*;

fn status(id: &str, phase: Phase) -> StorageMigrationStatus {
    StorageMigrationStatus {
        operation_id: id.into(),
        phase,
        input: AppPathSettingsInput {
            servers_root: "D:/instances".into(),
            archives_root: "D:/instances/.trash".into(),
            games_root: "D:/games".into(),
            steamcmd_root: "D:/steamcmd".into(),
        },
        roots: Vec::new(),
        total_files: 0,
        total_bytes: 0,
        completed_files: 0,
        completed_bytes: 0,
        reused_files: 0,
        preserved_destinations: Vec::new(),
        conflicts: Vec::new(),
        conflict_count: 0,
        can_cancel: true,
        recovery_required: false,
        error: None,
        cleanup_warnings: Vec::new(),
    }
}

fn begin(tracker: &MigrationTracker, id: &str) -> Run {
    match tracker.begin_preview(status(id, Phase::Checking)).unwrap() {
        Claim::Started(run, _) => run,
        Claim::Observe(_) => panic!("new fixture operation must start"),
    }
}

fn ready(tracker: &MigrationTracker, id: &str) -> Run {
    let run = begin(tracker, id);
    tracker
        .finish(&run, status(id, Phase::Ready), false)
        .unwrap();
    run
}

#[test]
fn cancellation_remains_active_until_worker_completion() {
    let tracker = MigrationTracker::default();
    let run = begin(&tracker, "first");
    let observed = tracker.request_cancel("first").unwrap();
    assert_eq!(observed.phase, Phase::Checking);
    assert!(!observed.can_cancel);
    assert!(run.cancellation.load(Ordering::Acquire));
    let mut different = status("second", Phase::Checking);
    different.input.servers_root = "E:/other".into();
    assert!(tracker.begin_preview(different).is_err());
    tracker
        .update(&run, status("first", Phase::Checking))
        .unwrap();
    assert!(!tracker.snapshot().unwrap().unwrap().can_cancel);
    tracker
        .finish(&run, status("first", Phase::Cancelled), false)
        .unwrap();
    let finished = tracker.snapshot().unwrap().unwrap();
    assert_eq!(finished.phase, Phase::Cancelled);
    assert!(!finished.recovery_required);
    assert!(matches!(
        tracker.begin_execute("first").unwrap(),
        Claim::Observe(_)
    ));
    assert!(matches!(
        tracker
            .begin_preview(status("second", Phase::Checking))
            .unwrap(),
        Claim::Started(_, _)
    ));
}

#[test]
fn concurrent_duplicate_execution_admits_exactly_one_worker() {
    let tracker = Arc::new(MigrationTracker::default());
    ready(&tracker, "one");
    let gate = Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let tracker = tracker.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                match tracker.begin_execute("one").unwrap() {
                    Claim::Started(_, status) => {
                        assert_eq!(status.phase, Phase::Verifying);
                        true
                    }
                    Claim::Observe(status) => {
                        assert_eq!(status.phase, Phase::Verifying);
                        false
                    }
                }
            })
        })
        .collect();
    let started = workers
        .into_iter()
        .map(|worker| usize::from(worker.join().unwrap()))
        .sum::<usize>();
    assert_eq!(started, 1);
    assert!(tracker.begin_execute("another").is_err());
}

#[test]
fn delayed_preview_callbacks_cannot_rewind_the_same_operations_execution() {
    let tracker = MigrationTracker::default();
    let preview = ready(&tracker, "one");
    let execution = match tracker.begin_execute("one").unwrap() {
        Claim::Started(run, _) => run,
        Claim::Observe(_) => panic!("ready fixture must start execution"),
    };
    tracker
        .update(&execution, status("one", Phase::Copying))
        .unwrap();
    tracker
        .update(&preview, status("one", Phase::Checking))
        .unwrap();
    tracker
        .finish(&preview, status("one", Phase::Failed), false)
        .unwrap();
    assert_eq!(tracker.snapshot().unwrap().unwrap().phase, Phase::Copying);
    tracker
        .finish(&execution, status("one", Phase::Completed), false)
        .unwrap();
    tracker
        .update(&execution, status("one", Phase::Verifying))
        .unwrap();
    assert_eq!(tracker.snapshot().unwrap().unwrap().phase, Phase::Completed);
    assert!(matches!(
        tracker.begin_execute("one").unwrap(),
        Claim::Observe(_)
    ));
}

#[test]
fn progress_does_not_publish_completion_before_the_supervisor_finishes() {
    let tracker = MigrationTracker::default();
    let run = begin(&tracker, "one");
    tracker.update(&run, status("one", Phase::Ready)).unwrap();
    let observed = tracker.snapshot().unwrap().unwrap();
    assert_eq!(observed.phase, Phase::Checking);
    assert!(!observed.can_cancel);
    assert!(matches!(
        tracker.begin_execute("one").unwrap(),
        Claim::Observe(_)
    ));
    tracker
        .finish(&run, status("one", Phase::Ready), false)
        .unwrap();
    assert!(matches!(
        tracker.begin_execute("one").unwrap(),
        Claim::Started(_, _)
    ));
}

#[test]
fn publication_disables_cancellation_and_recovery_failure_blocks_new_work() {
    let tracker = MigrationTracker::default();
    let run = begin(&tracker, "one");
    tracker
        .update(&run, status("one", Phase::Applying))
        .unwrap();
    assert!(tracker.publication_started(&run).unwrap());
    let observed = tracker.request_cancel("one").unwrap();
    assert_eq!(observed.phase, Phase::Applying);
    assert!(!observed.can_cancel);
    assert!(!run.cancellation.load(Ordering::Acquire));
    tracker.update(&run, status("one", Phase::Copying)).unwrap();
    assert_eq!(tracker.snapshot().unwrap().unwrap().phase, Phase::Applying);
    tracker
        .finish(&run, status("one", Phase::Failed), true)
        .unwrap();
    assert!(tracker.snapshot().unwrap().unwrap().recovery_required);
    assert!(tracker.request_cancel("one").unwrap().recovery_required);
    assert!(
        tracker
            .begin_preview(status("two", Phase::Checking))
            .is_err()
    );
    assert!(matches!(
        tracker.begin_execute("one").unwrap(),
        Claim::Observe(_)
    ));
}

#[test]
fn disk_observation_cannot_replace_an_admitted_worker() {
    let tracker = MigrationTracker::default();
    let run = begin(&tracker, "current");
    let (observed, imported) = tracker
        .restore_if_empty(status("previous", Phase::Ready), false)
        .unwrap();
    assert!(!imported);
    assert_eq!(observed.operation_id, run.id);
    assert_eq!(observed.phase, Phase::Checking);
    assert!(tracker.request_cancel("previous").is_err());
}

#[test]
fn conflicts_and_idle_preview_cancellation_never_start_execution() {
    let tracker = MigrationTracker::default();
    let run = begin(&tracker, "one");
    let mut conflicted = status("one", Phase::Ready);
    conflicted.conflict_count = 1;
    tracker.finish(&run, conflicted, false).unwrap();
    assert!(tracker.begin_execute("one").is_err());
    assert_eq!(tracker.request_cancel("one").unwrap().phase, Phase::Ready);
    assert!(!run.cancellation.load(Ordering::Acquire));
}

#[test]
fn repeated_preview_observes_the_active_operation_without_a_second_worker() {
    let tracker = MigrationTracker::default();
    begin(&tracker, "one");
    match tracker
        .begin_preview(status("two", Phase::Checking))
        .unwrap()
    {
        Claim::Observe(status) => assert_eq!(status.operation_id, "one"),
        Claim::Started(_, _) => panic!("matching active preview must be observed"),
    }
}

#[test]
fn ordinary_terminal_results_clear_recovery_and_allow_the_next_preview() {
    for phase in [Phase::Completed, Phase::Cancelled, Phase::Failed] {
        let tracker = MigrationTracker::default();
        let run = begin(&tracker, "one");
        let mut finished = status("one", phase.clone());
        finished.recovery_required = true;
        tracker.finish(&run, finished, false).unwrap();
        let observed = tracker.snapshot().unwrap().unwrap();
        assert_eq!(observed.phase, phase);
        assert!(!observed.recovery_required);
        assert!(matches!(
            tracker
                .begin_preview(status("two", Phase::Checking))
                .unwrap(),
            Claim::Started(_, _)
        ));
    }
}

#[test]
fn restored_recovery_is_public_and_cannot_be_replaced_by_later_disk_observation() {
    let tracker = MigrationTracker::default();
    let (restored, imported) = tracker
        .restore_if_empty(status("one", Phase::Failed), true)
        .unwrap();
    assert!(imported);
    assert!(restored.recovery_required);
    assert!(tracker.snapshot().unwrap().unwrap().recovery_required);
    let (observed, imported) = tracker
        .restore_if_empty(status("other", Phase::Completed), false)
        .unwrap();
    assert!(!imported);
    assert_eq!(observed.operation_id, "one");
    assert!(observed.recovery_required);
    assert!(
        tracker
            .begin_preview(status("two", Phase::Checking))
            .is_err()
    );
}
