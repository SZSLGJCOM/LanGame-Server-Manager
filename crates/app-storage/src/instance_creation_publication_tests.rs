use super::*;
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

struct Fixture {
    root: PathBuf,
    stage: PathBuf,
    target: PathBuf,
    started: Instant,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-runtime-publication-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let stage = root.join("runtime.staging");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("server.dat"), b"instance-owned payload").unwrap();
        Self {
            target: root.join("runtime"),
            root,
            stage,
            started: Instant::now(),
        }
    }

    fn hold_child(&self) -> fs::File {
        fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(self.stage.join("server.dat"))
            .unwrap()
    }

    fn verify_pending(&self) {
        assert!(!self.target.exists());
        assert_eq!(
            fs::read(self.stage.join("server.dat")).unwrap(),
            b"instance-owned payload"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!(
                "Test fixture cleanup failed for {}: {error}",
                self.root.display()
            );
        }
    }
}

#[test]
fn publication_waits_for_real_windows_child_handle_then_publishes_complete_directory() {
    let fixture = Fixture::new();
    let mut held = Some(fixture.hold_child());
    // Establish the actual OS failure that the previous one-shot rename exposed.
    let error = fs::rename(&fixture.stage, &fixture.target).unwrap_err();
    assert!(is_windows_publication_contention(&error), "{error}");
    fixture.verify_pending();
    let mut waits = 0;
    publish_directory_with_wait(
        &fixture.stage,
        &fixture.target,
        None,
        Duration::from_secs(2),
        || fixture.started.elapsed(),
        |_| {
            fixture.verify_pending();
            waits += 1;
            drop(held.take());
            Ok(())
        },
    )
    .unwrap();
    assert!(
        waits >= 1,
        "The real conflicting handle must prevent the first publication"
    );
    assert!(!fixture.stage.exists());
    assert_eq!(
        fs::read(fixture.target.join("server.dat")).unwrap(),
        b"instance-owned payload"
    );
}

#[test]
fn persistent_file_lock_keeps_original_error_and_never_publishes_partial_data() {
    let fixture = Fixture::new();
    let _held = fixture.hold_child();
    let error = publish_directory_with_wait(
        &fixture.stage,
        &fixture.target,
        None,
        Duration::ZERO,
        || fixture.started.elapsed(),
        |_| {
            panic!("An exhausted budget must not wait");
        },
    )
    .unwrap_err();
    match error {
        StorageError::PublishInstanceRuntime {
            source, attempts, ..
        } => {
            assert!(is_windows_publication_contention(&source));
            assert_eq!(attempts, 1);
        }
        other => panic!("Unexpected publication error: {other}"),
    }
    fixture.verify_pending();
}

#[test]
fn cancellation_during_contention_leaves_the_pending_owner_in_control() {
    let fixture = Fixture::new();
    let _held = fixture.hold_child();
    let cancellation = AtomicBool::new(false);
    let error = publish_directory_with_wait(
        &fixture.stage,
        &fixture.target,
        Some(&cancellation),
        Duration::from_secs(2),
        || fixture.started.elapsed(),
        |_| {
            cancellation.store(true, Ordering::Release);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(matches!(error, StorageError::InstanceCreationCancelled));
    fixture.verify_pending();
}

#[test]
fn destination_appearing_during_wait_is_not_overwritten() {
    let fixture = Fixture::new();
    let mut held = Some(fixture.hold_child());
    let error = publish_directory_with_wait(
        &fixture.stage,
        &fixture.target,
        None,
        Duration::from_secs(2),
        || fixture.started.elapsed(),
        |_| {
            fs::write(&fixture.target, b"another owner").unwrap();
            drop(held.take());
            Ok(())
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, StorageError::PublishInstanceRuntime { source, .. } if source.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&fixture.target).unwrap(), b"another owner");
    assert_eq!(
        fs::read(fixture.stage.join("server.dat")).unwrap(),
        b"instance-owned payload"
    );
}

#[test]
fn missing_source_is_an_immediate_error() {
    let fixture = Fixture::new();
    let error = publish_directory_with_wait(
        &fixture.root.join("missing"),
        &fixture.target,
        None,
        Duration::from_secs(2),
        || fixture.started.elapsed(),
        |_| {
            panic!("A missing source is not transient sharing contention");
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, StorageError::PublishInstanceRuntime { source, attempts: 0, .. } if source.kind() == io::ErrorKind::NotFound)
    );
    fixture.verify_pending();
}

#[test]
fn release_at_deadline_does_not_allow_another_publication_attempt() {
    let fixture = Fixture::new();
    let mut held = Some(fixture.hold_child());
    let elapsed = std::cell::Cell::new(Duration::ZERO);
    let budget = Duration::from_secs(5);
    let error = publish_directory_with_wait(
        &fixture.stage,
        &fixture.target,
        None,
        budget,
        || elapsed.get(),
        |_| {
            drop(held.take());
            elapsed.set(budget);
            Ok(())
        },
    )
    .unwrap_err();
    assert!(
        matches!(error, StorageError::PublishInstanceRuntime { source, attempts: 1, .. } if is_windows_publication_contention(&source))
    );
    assert!(held.is_none(), "The wait boundary must have been reached");
    fixture.verify_pending();
}

#[test]
fn native_publication_itself_rejects_an_existing_empty_directory() {
    let fixture = Fixture::new();
    fs::create_dir(&fixture.target).unwrap();
    assert!(rename_new_runtime(&fixture.stage, &fixture.target).is_err());
    assert!(fixture.stage.is_dir());
    assert!(fixture.target.is_dir());
    assert_eq!(fs::read_dir(&fixture.target).unwrap().count(), 0);
    assert_eq!(
        fs::read(fixture.stage.join("server.dat")).unwrap(),
        b"instance-owned payload"
    );
}

#[test]
fn native_publication_preserves_long_mixed_separator_paths() {
    let fixture = Fixture::new();
    let mut parent = fixture.root.clone();
    for _ in 0..4 {
        parent = parent.join("long-runtime-component-".repeat(3));
    }
    let source = PathBuf::from(format!("{}/runtime.staging", parent.display()));
    let target = PathBuf::from(format!("{}/runtime", parent.display()));
    assert!(source.as_os_str().len() > 260);
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("server.dat"), b"long-path payload").unwrap();
    publish_creation_directory(&source, &target, None).unwrap();
    assert!(!source.exists());
    assert_eq!(
        fs::read(target.join("server.dat")).unwrap(),
        b"long-path payload"
    );
}
