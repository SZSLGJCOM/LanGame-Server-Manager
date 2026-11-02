use super::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

#[derive(Deserialize, Serialize)]
struct JournalBytes {
    before: Vec<u8>,
    after: Vec<u8>,
}

struct Fixture {
    root: PathBuf,
    target: PathBuf,
    journal: PathBuf,
}

impl Fixture {
    fn new(before: &[u8], after: &[u8]) -> Self {
        let root =
            std::env::temp_dir().join(format!("lgsm-metadata-publish-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let fixture = Self {
            target: root.join("metadata.json"),
            journal: root.join("applying.json"),
            root,
        };
        fs::write(&fixture.target, before).unwrap();
        let bytes = serde_json::to_vec(&JournalBytes {
            before: before.to_vec(),
            after: after.to_vec(),
        })
        .unwrap();
        let mut journal = File::options()
            .create_new(true)
            .write(true)
            .open(&fixture.journal)
            .unwrap();
        journal.write_all(&bytes).unwrap();
        journal.sync_all().unwrap();
        fixture
    }

    fn versions(&self) -> JournalBytes {
        serde_json::from_slice(&fs::read(&self.journal).unwrap()).unwrap()
    }

    fn publish(&self) -> Result<(), StorageError> {
        let journal = self.versions();
        publish(&self.target, &journal.before, &journal.after)
    }

    fn assert_journal(&self, before: &[u8], after: &[u8]) {
        let journal = self.versions();
        assert_eq!(journal.before, before);
        assert_eq!(journal.after, after);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("lgsm-metadata-publish-")
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[cfg(windows)]
#[test]
fn publishes_new_bytes_and_replays_without_replacing_the_finished_file() {
    let fixture = Fixture::new(b"before", b"after");
    fixture.publish().unwrap();
    let first = native::open_verified_file(&fixture.target, false)
        .unwrap()
        .identity()
        .unwrap();
    fixture.publish().unwrap();
    let second = native::open_verified_file(&fixture.target, false)
        .unwrap()
        .identity()
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"after");
    fixture.assert_journal(b"before", b"after");
}

#[test]
fn already_published_and_unchanged_bytes_do_not_require_replacement() {
    let fixture = Fixture::new(b"same", b"same");
    let first = native::open_verified_file(&fixture.target, false)
        .unwrap()
        .identity()
        .unwrap();
    fixture.publish().unwrap();
    assert_eq!(
        native::open_verified_file(&fixture.target, false)
            .unwrap()
            .identity()
            .unwrap(),
        first
    );
    let journal = fixture.versions();
    fs::write(&fixture.target, &journal.after).unwrap();
    fixture.publish().unwrap();
    fixture.assert_journal(b"same", b"same");
}

#[test]
fn missing_destination_recovers_after_bytes_without_changing_journal() {
    let fixture = Fixture::new(b"original data", b"new data");
    fs::remove_file(&fixture.target).unwrap();
    fixture.publish().unwrap();
    assert_eq!(fs::read(&fixture.target).unwrap(), b"new data");
    fixture.assert_journal(b"original data", b"new data");
}

#[test]
fn rejects_different_existing_content_including_matching_prefixes() {
    for competing in [
        &b"external"[..],
        &b"before-extra"[..],
        &b"after-extra"[..],
        &b""[..],
    ] {
        let fixture = Fixture::new(b"before", b"after");
        fs::write(&fixture.target, competing).unwrap();
        assert!(fixture.publish().is_err());
        assert_eq!(fs::read(&fixture.target).unwrap(), competing);
        fixture.assert_journal(b"before", b"after");
    }
}

#[test]
fn refuses_a_directory_destination_without_modifying_its_contents() {
    let fixture = Fixture::new(b"before", b"after");
    fs::remove_file(&fixture.target).unwrap();
    fs::create_dir(&fixture.target).unwrap();
    fs::write(fixture.target.join("sentinel"), b"external").unwrap();
    assert!(fixture.publish().is_err());
    assert_eq!(
        fs::read(fixture.target.join("sentinel")).unwrap(),
        b"external"
    );
    fixture.assert_journal(b"before", b"after");
}

#[test]
fn exclusive_create_preserves_a_competing_file_and_the_journal() {
    let fixture = Fixture::new(b"before", b"after");
    fs::remove_file(&fixture.target).unwrap();
    let journal = fixture.versions();
    let result = publish_with(
        &fixture.target,
        &journal.before,
        &journal.after,
        |point, path| {
            if point == PublishPoint::BeforeCreate {
                fs::write(path, b"external winner")?;
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&fixture.target).unwrap(), b"external winner");
    fixture.assert_journal(b"before", b"after");
    assert!(
        fixture.publish().is_err(),
        "replay must not erase the winning file"
    );
    assert_eq!(fs::read(&fixture.target).unwrap(), b"external winner");
    assert_eq!(fs::read_dir(&fixture.root).unwrap().count(), 2);
}

#[test]
fn exclusive_create_accepts_an_identical_completed_version_from_a_racing_writer() {
    let fixture = Fixture::new(b"before", b"after");
    fs::remove_file(&fixture.target).unwrap();
    let journal = fixture.versions();
    publish_with(
        &fixture.target,
        &journal.before,
        &journal.after,
        |point, path| {
            if point == PublishPoint::BeforeCreate {
                fs::write(path, b"after")?;
            }
            Ok(())
        },
    )
    .unwrap();
    fixture.assert_journal(b"before", b"after");
    assert_eq!(fs::read(&fixture.target).unwrap(), b"after");
}

#[cfg(windows)]
#[test]
fn interruption_after_removal_recovers_using_only_persisted_journal_bytes() {
    let fixture = Fixture::new(b"original recovery bytes", b"published recovery bytes");
    let journal = fixture.versions();
    let result = publish_with(
        &fixture.target,
        &journal.before,
        &journal.after,
        |point, _| {
            if point == PublishPoint::Removed {
                return Err(io::Error::other("simulated crash after removal"));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(!fixture.target.exists());
    drop(journal);
    fixture.assert_journal(b"original recovery bytes", b"published recovery bytes");
    fixture.publish().unwrap();
    assert_eq!(
        fs::read(&fixture.target).unwrap(),
        b"published recovery bytes"
    );
    fixture.assert_journal(b"original recovery bytes", b"published recovery bytes");
}

#[cfg(windows)]
#[test]
fn publication_failure_after_removal_keeps_recovery_bytes_and_can_be_replayed() {
    let fixture = Fixture::new(b"before", b"after");
    crate::atomic_file::fail_next_atomic_write_for_test(&fixture.target);
    assert!(fixture.publish().is_err());
    assert!(!fixture.target.exists());
    fixture.assert_journal(b"before", b"after");
    fixture.publish().unwrap();
    assert_eq!(fs::read(&fixture.target).unwrap(), b"after");
    fixture.assert_journal(b"before", b"after");
}

#[cfg(windows)]
#[test]
fn verified_handle_denies_external_write_and_rename_before_removal() {
    let fixture = Fixture::new(b"before", b"after");
    let journal = fixture.versions();
    let mut verified = false;
    publish_with(
        &fixture.target,
        &journal.before,
        &journal.after,
        |point, path| {
            if point == PublishPoint::Verified {
                verified = true;
                assert!(File::options().write(true).open(path).is_err());
                let stolen = fixture.root.join("stolen");
                let renamed = fs::rename(path, &stolen);
                if renamed.is_ok() {
                    fs::rename(&stolen, path)?;
                }
                assert!(renamed.is_err());
                let elsewhere = fixture.root.with_extension("renamed");
                let renamed = fs::rename(&fixture.root, &elsewhere);
                if renamed.is_ok() {
                    fs::rename(&elsewhere, &fixture.root)?;
                }
                assert!(renamed.is_err());
            }
            Ok(())
        },
    )
    .unwrap();
    assert!(verified);
    assert_eq!(fs::read(&fixture.target).unwrap(), b"after");
    fixture.assert_journal(b"before", b"after");
}

#[cfg(windows)]
#[test]
fn competing_creation_after_verified_removal_preserves_external_and_original_bytes() {
    let fixture = Fixture::new(b"original", b"replacement");
    let journal = fixture.versions();
    let result = publish_with(
        &fixture.target,
        &journal.before,
        &journal.after,
        |point, path| {
            if point == PublishPoint::BeforeCreate {
                fs::write(path, b"external winner")?;
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&fixture.target).unwrap(), b"external winner");
    fixture.assert_journal(b"original", b"replacement");
}

#[cfg(not(windows))]
#[test]
fn replacing_existing_original_is_refused_without_handle_bound_unlink() {
    let fixture = Fixture::new(b"original", b"replacement");
    assert!(fixture.publish().is_err());
    assert_eq!(fs::read(&fixture.target).unwrap(), b"original");
    fixture.assert_journal(b"original", b"replacement");
}

#[test]
fn exact_comparison_streams_across_chunks_and_short_reads() {
    struct ShortReader(std::io::Cursor<Vec<u8>>);
    impl Read for ShortReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let length = bytes.len().min(11);
            self.0.read(&mut bytes[..length])
        }
    }
    let before = vec![3; BUFFER_BYTES + 7];
    let mut after = before.clone();
    after[BUFFER_BYTES] = 4;
    assert_eq!(
        compare_bytes(
            &mut ShortReader(std::io::Cursor::new(before.clone())),
            &before,
            &after
        )
        .unwrap(),
        (true, false)
    );
    assert_eq!(
        compare_bytes(
            &mut ShortReader(std::io::Cursor::new(after.clone())),
            &before,
            &after
        )
        .unwrap(),
        (false, true)
    );
}
