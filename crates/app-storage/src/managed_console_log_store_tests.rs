use super::*;
use serde_json::json;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("langame-console-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn invalid_journal_is_rejected_before_it_can_delete_an_unknown_file() {
    let fixture = Fixture::new();
    let store = Store::open(&fixture.0).unwrap();
    let unknown = fixture.0.join("operator-data.txt");
    fs::write(&unknown, b"must survive").unwrap();
    let file_identity = identity(&File::open(&unknown).unwrap()).unwrap();
    let mut ledger = serde_json::to_value(&store.ledger).unwrap();
    ledger["next_sequence"] = json!(2);
    ledger["deletions"] =
        json!([{ "file":"operator-data.txt", "identity":file_identity,"start":0,"sequence":1 }]);
    fs::write(
        fixture.0.join(MANIFEST),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    assert!(Store::open(&fixture.0).is_err());
    assert_eq!(fs::read(unknown).unwrap(), b"must survive");
}

#[test]
fn an_unpublished_initial_manifest_never_causes_additional_pending_files() {
    let fixture = Fixture::new();
    let pending = fixture.0.join("ownership.pending");
    fs::write(&pending, b"unidentified interrupted publication").unwrap();
    for _ in 0..8 {
        assert!(Store::open(&fixture.0).is_err());
    }
    assert_eq!(
        fs::read(&pending).unwrap(),
        b"unidentified interrupted publication"
    );
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}

#[test]
fn a_leftover_manifest_update_is_preserved_without_growing_retry_files() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.0).unwrap();
    let manifest = fixture.0.join(MANIFEST);
    let before = fs::read(&manifest).unwrap();
    let pending = fixture.0.join("ownership.update");
    fs::write(&pending, b"unidentified interrupted update").unwrap();
    let mut next = store.ledger.clone();
    next.next_sequence += 1;

    for _ in 0..8 {
        assert_eq!(
            store.save(next.clone()).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&manifest).unwrap(), before);
        assert_eq!(store.ledger.next_sequence, 1);
    }

    assert_eq!(
        fs::read(&pending).unwrap(),
        b"unidentified interrupted update"
    );
    let mut names = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            std::ffi::OsString::from(MANIFEST),
            std::ffi::OsString::from("ownership.update")
        ]
    );
}

#[test]
fn successful_manifest_updates_leave_only_the_complete_current_ledger() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.0).unwrap();
    for sequence in 2..=8 {
        let mut next = store.ledger.clone();
        next.next_sequence = sequence;
        store.save(next).unwrap();
        assert_eq!(
            Store::open(&fixture.0).unwrap().ledger.next_sequence,
            sequence
        );
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
    }
}

#[test]
fn a_failed_registration_reclaims_only_the_new_unpublished_temporary() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.0).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.0.join(MANIFEST));
    assert!(store.create_run("run-1-main.log").is_err());
    let entries = fs::read_dir(&fixture.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(entries, vec![std::ffi::OsString::from(MANIFEST)]);
    store.create_run("run-1-main.log").unwrap();
    assert!(fixture.0.join("run-1-main.log").is_file());
}

#[test]
fn completed_moves_replay_after_the_final_journal_clear_fails() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.0).unwrap();
    let run = "run-1-main.log";
    store.create_run(run).unwrap();
    store
        .append_part(&store.ledger.runs[run][0])
        .unwrap()
        .write_all(b"old output")
        .unwrap();
    let current = store.ledger.runs[run][0].clone();
    let (temporary, new_identity) = store.new_temporary().unwrap();
    let archive_dir = "segment-00000000000000000002";
    fs::create_dir(fixture.0.join(archive_dir)).unwrap();
    let archive = format!("{archive_dir}/entries.log");
    let mut next = store.ledger.clone();
    next.next_sequence = 3;
    next.runs.get_mut(run).unwrap()[0].file = archive.clone();
    next.runs.get_mut(run).unwrap().push(Part {
        file: run.into(),
        identity: new_identity.clone(),
        start: 10,
        sequence: 2,
    });
    next.moves = vec![
        FileMove {
            from: run.into(),
            to: archive.clone(),
            identity: current.identity,
        },
        FileMove {
            from: temporary.clone(),
            to: run.into(),
            identity: new_identity,
        },
    ];
    store.save(next).unwrap();
    fs::rename(fixture.0.join(run), fixture.0.join(&archive)).unwrap();
    fs::rename(fixture.0.join(&temporary), fixture.0.join(run)).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.0.join(MANIFEST));
    assert!(store.recover().is_err());
    drop(store);
    let recovered = Store::open(&fixture.0).unwrap();
    assert!(recovered.ledger.moves.is_empty());
    assert_eq!(fs::read(fixture.0.join(archive)).unwrap(), b"old output");
    assert!(fs::read(fixture.0.join(run)).unwrap().is_empty());
}
