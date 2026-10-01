use super::*;
use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
use crate::program_runtime::ProgramFileSelection;
use std::io::{Seek, SeekFrom, Write};

fn copy_owner(fixture: &AdoptionFixture) -> PendingInstanceDirectory {
    PendingInstanceDirectory {
        instances_root: fixture.instances.clone(),
        instance_root: fixture.instance.clone(),
        armed: true,
        adoption: None,
    }
}

#[test]
fn official_copy_records_only_its_filtered_program_and_preserves_empty_directories() {
    let fixture = AdoptionFixture::new();
    fs::create_dir(fixture.source.join("optional")).unwrap();
    fs::write(
        fixture.source.join("optional/content.bin"),
        b"optional program",
    )
    .unwrap();
    fs::create_dir(fixture.source.join("empty")).unwrap();
    fixture.record_clean_package();
    let mut exclusions = fixture.exclusions();
    exclusions.push(fixture.source.join("optional"));
    let runtime = prepare_private_runtime_root(
        &fixture.source,
        &fixture.instance,
        &exclusions,
        None,
        false,
        ProgramFileSelection::Verified("fixture"),
        None,
    )
    .unwrap();
    assert!(!runtime.join("optional").exists());
    assert!(runtime.join("empty").is_dir());
    let inventory =
        crate::program_seed::require_clean_package_tree(&runtime, "fixture", None).unwrap();
    assert_eq!(
        inventory.files.keys().collect::<Vec<_>>(),
        vec!["server.bin"]
    );
    assert_eq!(
        inventory.directories.iter().collect::<Vec<_>>(),
        vec!["empty"]
    );
    assert!(fixture.source.join("optional/content.bin").is_file());
}

#[cfg(windows)]
#[test]
fn official_copy_preserves_current_path_spelling_after_case_only_rename() {
    let fixture = AdoptionFixture::new();
    fs::create_dir(fixture.source.join("Bin")).unwrap();
    fs::write(fixture.source.join("Bin/official.dll"), b"official program").unwrap();
    fixture.record_clean_package();
    fs::rename(fixture.source.join("Bin"), fixture.source.join("BIN")).unwrap();
    let runtime = prepare_private_runtime_root(
        &fixture.source,
        &fixture.instance,
        &fixture.exclusions(),
        None,
        false,
        ProgramFileSelection::Verified("fixture"),
        None,
    )
    .unwrap();
    let inventory =
        crate::program_seed::require_clean_package_tree(&runtime, "fixture", None).unwrap();
    assert!(inventory.files.contains_key("BIN/official.dll"));
    assert!(inventory.directories.contains("BIN"));
}

#[test]
fn official_copy_rejects_missing_or_corrupt_payload_before_publication() {
    for missing in [false, true] {
        let fixture = AdoptionFixture::new();
        fixture.record_clean_package();
        if missing {
            fs::remove_file(fixture.source.join("server.bin")).unwrap();
        } else {
            fs::write(
                fixture.source.join("server.bin"),
                b"changed official payload",
            )
            .unwrap();
        }
        let source_before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
        let owner = copy_owner(&fixture);
        let error = prepare_private_runtime_root(
            &fixture.source,
            &fixture.instance,
            &fixture.exclusions(),
            None,
            false,
            ProgramFileSelection::Verified("fixture"),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
            "{error}"
        );
        assert!(!fixture.instance.join("runtime").exists());
        owner.rollback().unwrap();
        assert!(!fixture.instance.exists());
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
            source_before
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_copy_keeps_its_frozen_inventory_when_source_metadata_changes() {
    let fixture = AdoptionFixture::new();
    fs::write(fixture.source.join("server.bin"), vec![7_u8; 512 * 1024]).unwrap();
    fixture.record_clean_package();
    let expected: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.source.join(".langame-clean-package.json")).unwrap(),
    )
    .unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Copy);
    let source = fixture.source.clone();
    let instance = fixture.instance.clone();
    let exclusions = fixture.exclusions();
    let token = Arc::clone(&cancellation);
    let worker = tokio::task::spawn_blocking(move || {
        prepare_private_runtime_root(
            &source,
            &instance,
            &exclusions,
            None,
            false,
            ProgramFileSelection::Verified("fixture"),
            Some(&token),
        )
    });
    gate.reached().await;
    fs::write(
        fixture.source.join(".langame-clean-package.json"),
        b"concurrent manifest replacement",
    )
    .unwrap();
    fs::write(
        fixture.source.join("loader.dll"),
        b"concurrent unlisted payload",
    )
    .unwrap();
    let source_after_change = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    drop(gate);
    let runtime = worker.await.unwrap().unwrap();
    let copied: serde_json::Value =
        serde_json::from_slice(&fs::read(runtime.join(".langame-clean-package.json")).unwrap())
            .unwrap();
    for field in ["version", "module_id", "source", "files", "directories"] {
        assert_eq!(
            copied[field], expected[field],
            "frozen inventory field {field}"
        );
    }
    assert!(!runtime.join(".langame-initial-package.json").exists());
    assert!(!runtime.join("loader.dll").exists());
    assert_eq!(
        fs::read(runtime.join("server.bin")).unwrap(),
        vec![7_u8; 512 * 1024]
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        source_after_change
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_copy_rejects_payload_changed_during_its_only_read() {
    let fixture = AdoptionFixture::new();
    fs::write(fixture.source.join("server.bin"), vec![7_u8; 512 * 1024]).unwrap();
    fixture.record_clean_package();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Copy);
    let source = fixture.source.clone();
    let instance = fixture.instance.clone();
    let exclusions = fixture.exclusions();
    let token = Arc::clone(&cancellation);
    let owner = copy_owner(&fixture);
    let worker = tokio::task::spawn_blocking(move || {
        let result = prepare_private_runtime_root(
            &source,
            &instance,
            &exclusions,
            None,
            false,
            ProgramFileSelection::Verified("fixture"),
            Some(&token),
        );
        assert!(!instance.join("runtime").exists());
        owner.rollback().unwrap();
        result
    });
    gate.reached().await;
    let mut payload = fs::OpenOptions::new()
        .write(true)
        .open(fixture.source.join("server.bin"))
        .unwrap();
    payload.seek(SeekFrom::Start(256 * 1024)).unwrap();
    payload.write_all(b"concurrently changed payload").unwrap();
    drop(payload);
    let source_after_change = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    drop(gate);
    let error = worker.await.unwrap().unwrap_err();
    assert!(
        matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
        "{error}"
    );
    assert!(!fixture.instance.exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        source_after_change
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn official_copy_cancellation_removes_the_unpublished_copy_only() {
    let fixture = AdoptionFixture::new();
    fs::write(fixture.source.join("server.bin"), vec![7_u8; 512 * 1024]).unwrap();
    fixture.record_clean_package();
    let source_before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Copy);
    let source = fixture.source.clone();
    let instance = fixture.instance.clone();
    let exclusions = fixture.exclusions();
    let token = Arc::clone(&cancellation);
    let owner = copy_owner(&fixture);
    let worker = tokio::task::spawn_blocking(move || {
        let result = prepare_private_runtime_root(
            &source,
            &instance,
            &exclusions,
            None,
            false,
            ProgramFileSelection::Verified("fixture"),
            Some(&token),
        );
        owner.rollback().unwrap();
        result
    });
    gate.reached().await;
    assert!(fixture.instance.join("runtime.staging").is_dir());
    cancellation.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert!(!fixture.instance.exists());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        source_before
    );
}
