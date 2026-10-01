use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("lg-log-reliability-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn run(&self) -> Store {
        let mut store = Store::open(&self.0).unwrap();
        store.create_run("run-1-main.log").unwrap();
        store
            .append_part(&store.ledger.runs["run-1-main.log"][0])
            .unwrap()
            .write_all(b"before interruption\n")
            .unwrap();
        store
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn contents(store: &Store) -> Vec<u8> {
    let mut bytes = Vec::new();
    for part in &store.ledger.runs["run-1-main.log"] {
        store
            .open_part(part)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
    }
    bytes
}

#[test]
fn reliability_rotation_journal_recovers_each_rename_boundary() {
    for completed_moves in 0..=2 {
        let fixture = Fixture::new();
        let mut store = fixture.run();
        let run = "run-1-main.log";
        let previous = store.ledger.runs[run][0].clone();
        let length = store
            .open_part(&previous)
            .unwrap()
            .metadata()
            .unwrap()
            .len();
        let (temporary, new_identity) = store.new_temporary().unwrap();
        let archive = "segment-00000000000000000002/entries.log";
        fs::create_dir(store.path(archive).unwrap().parent().unwrap()).unwrap();
        let mut journal = store.ledger.clone();
        journal.next_sequence = 3;
        journal.runs.get_mut(run).unwrap()[0].file = archive.into();
        journal.runs.get_mut(run).unwrap().push(Part {
            file: run.into(),
            identity: new_identity.clone(),
            start: length,
            sequence: 2,
        });
        journal.moves = vec![
            FileMove {
                from: run.into(),
                to: archive.into(),
                identity: previous.identity,
            },
            FileMove {
                from: temporary,
                to: run.into(),
                identity: new_identity,
            },
        ];
        store.save(journal).unwrap();
        // Simulate process loss at each persisted write-ahead journal boundary.
        for movement in store.ledger.moves.iter().take(completed_moves) {
            move_owned(
                &store.path(&movement.from).unwrap(),
                &store.path(&movement.to).unwrap(),
                &movement.identity,
            )
            .unwrap();
        }
        drop(store);
        for _ in 0..3 {
            let recovered = Store::open(&fixture.0).unwrap();
            assert!(recovered.ledger.moves.is_empty());
            assert_eq!(contents(&recovered), b"before interruption\n");
            assert_eq!(recovered.ledger.runs[run].len(), 2);
        }
        let recovered = Store::open(&fixture.0).unwrap();
        recovered
            .append_part(recovered.ledger.runs[run].last().unwrap())
            .unwrap()
            .write_all(b"after recovery\n")
            .unwrap();
        assert_eq!(
            contents(&recovered),
            b"before interruption\nafter recovery\n"
        );
    }
}

#[test]
fn reliability_retention_replays_completed_unlink_and_preserves_unknown_data() {
    let fixture = Fixture::new();
    let mut store = fixture.run();
    let run = "run-1-main.log";
    store.rotate(run).unwrap();
    let unknown = fixture.0.join("operator-data.txt");
    fs::write(&unknown, b"retained operator data").unwrap();
    let mut journal = store.ledger.clone();
    let expired = journal.runs.get_mut(run).unwrap().remove(0);
    journal.deletions.push(expired.clone());
    store.save(journal).unwrap();
    remove_owned(&store.path(&expired.file).unwrap(), &expired.identity).unwrap();
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.0.join(MANIFEST));
    assert!(store.recover().is_err());
    drop(store);
    for _ in 0..3 {
        let recovered = Store::open(&fixture.0).unwrap();
        assert!(recovered.ledger.deletions.is_empty());
        assert_eq!(recovered.ledger.runs[run].len(), 1);
        assert_eq!(fs::read(&unknown).unwrap(), b"retained operator data");
    }
}

#[cfg(windows)]
#[test]
fn reliability_locked_rotation_can_resume_after_releasing_the_conflicting_handle() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    let fixture = Fixture::new();
    let mut store = fixture.run();
    let run = "run-1-main.log";
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(fixture.0.join(run))
        .unwrap();
    assert!(store.rotate(run).is_err());
    assert_eq!(
        fs::read(fixture.0.join(run)).unwrap(),
        b"before interruption\n"
    );
    drop(locked);
    drop(store);
    let recovered = Store::open(&fixture.0).unwrap();
    assert!(recovered.ledger.moves.is_empty());
    recovered
        .append_part(recovered.ledger.runs[run].last().unwrap())
        .unwrap()
        .write_all(b"after recovery\n")
        .unwrap();
    assert_eq!(
        contents(&recovered),
        b"before interruption\nafter recovery\n"
    );
}
