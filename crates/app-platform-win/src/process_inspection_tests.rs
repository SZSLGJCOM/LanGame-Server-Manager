use super::*;

fn identity(created: u64) -> ProcessIdentity {
    ProcessIdentity {
        creation_time: created,
        image_path: String::from(r"c:\servers\server.exe"),
    }
}

fn target() -> WindowInspectionTarget {
    WindowInspectionTarget {
        pid: 100,
        process_key: String::from("main"),
        display_name: String::from("Server"),
        process_identity: identity(100),
    }
}

fn entry(pid: u32, parent: u32) -> ProcessSnapshotEntry {
    ProcessSnapshotEntry {
        process_id: pid,
        parent_process_id: parent,
        process_name: String::from("server.exe"),
        thread_count: 0,
    }
}

fn observe(created: u64) -> ProcessObservation {
    ProcessObservation {
        identity: identity(created),
        handle: std::ptr::null_mut(),
    }
}

#[test]
fn verified_tree_includes_descendants_but_not_unrelated_processes() {
    let snapshot = [
        entry(100, 0),
        entry(101, 100),
        entry(102, 101),
        entry(103, 0),
    ];
    let records = build_target_process_records(&[target()], &snapshot, 200, |pid| {
        Some(observe(u64::from(pid)))
    });
    assert_eq!(records.len(), 3);
    assert_eq!(records[&100].relation, "tracked_process");
    assert_eq!(records[&101].relation, "descendant_process");
    assert_eq!(records[&102].process_key, "main");
}

#[test]
fn missing_or_reused_root_cannot_claim_a_process_tree() {
    let target = target();
    let snapshot = [entry(101, 100)];
    assert!(
        build_target_process_records(std::slice::from_ref(&target), &snapshot, 200, |_| {
            panic!("missing root must not be opened")
        })
        .is_empty()
    );

    let snapshot = [entry(100, 0), entry(101, 100)];
    for actual in [
        identity(99),
        ProcessIdentity {
            image_path: String::from(r"c:\other.exe"),
            ..identity(100)
        },
    ] {
        let records =
            build_target_process_records(std::slice::from_ref(&target), &snapshot, 200, |pid| {
                assert_eq!(pid, 100, "unverified root must not admit descendants");
                Some(ProcessObservation {
                    identity: actual.clone(),
                    handle: std::ptr::null_mut(),
                })
            });
        assert!(records.is_empty());
    }
}

#[test]
fn descendant_must_belong_to_the_observed_parent_lifetime() {
    let snapshot = [entry(100, 0), entry(101, 100), entry(102, 101)];
    for child_created_at in [99, 201] {
        let records = build_target_process_records(&[target()], &snapshot, 200, |pid| match pid {
            100 => Some(observe(100)),
            101 => Some(observe(child_created_at)),
            _ => panic!("unverified descendant must not admit its children"),
        });
        assert_eq!(records.len(), 1);
        assert!(records.contains_key(&100));
    }
}

#[test]
fn inaccessible_process_is_never_assumed_to_be_owned() {
    let snapshot = [entry(100, 0), entry(101, 100)];
    assert!(build_target_process_records(&[target()], &snapshot, 200, |_| None).is_empty());
}

#[test]
fn identity_comparison_accepts_equivalent_windows_path_not_another_start() {
    let actual = identity(100);
    let expected = ProcessIdentity {
        image_path: String::from(r"\\?\C:\Servers\SERVER.EXE"),
        ..actual.clone()
    };
    assert!(identity_matches(&expected, &actual));
    assert!(!identity_matches(&identity(99), &actual));
}

#[test]
fn native_inspection_retains_the_verified_process_object() {
    let pid = std::process::id();
    let process = ProcessObservation::open(pid).expect("current process observation");
    let target = WindowInspectionTarget {
        pid,
        process_identity: process.identity.clone(),
        process_key: String::from("fixture"),
        display_name: String::from("Fixture"),
    };
    let records =
        collect_verified_target_process_records(&[target]).expect("verified process tree");
    assert!(records[&pid].is_running());
    assert!(!records[&pid].observation.handle.is_null());
    assert_eq!(records[&pid].observation.identity, process.identity);
}
