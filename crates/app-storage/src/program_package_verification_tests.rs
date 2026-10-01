use super::*;
use crate::instance_creation_io::test_gate::count_hash_reads;

#[test]
fn first_use_reuses_installer_content_verification() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "assets/large.bin", &vec![19; 512 * 1024 + 1]);
    fixture.record(&source);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    let package = require_initial_package_tree(&source, "fixture", Some(&cancellation)).unwrap();
    assert_eq!(package.files.len(), 3);
    assert_eq!(
        reads.chunks(),
        0,
        "fresh installer results must avoid a second payload read"
    );
}

fn edit_initial(source: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = source.join(".langame-initial-package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    edit(&mut manifest);
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
}

#[test]
fn optional_verification_cache_cannot_exceed_the_manifest_budget() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let manifest = read_manifest(&source).unwrap().unwrap();
    assert!(!manifest.verified_files.is_empty());
    let mut plain = serde_json::to_value(&manifest).unwrap();
    plain.as_object_mut().unwrap().remove("verified_files");
    let budget = serde_json::to_vec(&plain).unwrap().len() as u64;
    let encoded = package::encode_manifest(&source, &manifest, budget).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encoded).unwrap(),
        plain
    );
    assert!(package::encode_manifest(&source, &manifest, budget - 1).is_err());
}

#[test]
fn first_use_without_reliable_cached_evidence_verifies_then_records_it() {
    for malformed in [false, true] {
        let fixture = Fixture::new();
        let source = fixture.source();
        fixture.record(&source);
        edit_initial(&source, |manifest| {
            if malformed {
                manifest["verified_files"] = serde_json::json!({"server.bin": "invalid cache"});
            } else {
                manifest.as_object_mut().unwrap().remove("verified_files");
            }
        });
        let cancellation = Arc::new(AtomicBool::new(false));
        let reads = count_hash_reads(&cancellation);
        require_initial_package_tree(&source, "fixture", Some(&cancellation)).unwrap();
        assert_eq!(
            reads.chunks(),
            2,
            "missing cache must verify the official contents"
        );
        require_initial_package_tree(&source, "fixture", Some(&cancellation)).unwrap();
        assert_eq!(
            reads.chunks(),
            2,
            "the verified evidence must survive reopening the manifest"
        );
    }
}

#[test]
fn first_use_rehashes_only_replaced_files_and_keeps_expected_digest_binding() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let replacement = source.join("replacement");
    fs::copy(source.join("server.bin"), &replacement).unwrap();
    fs::remove_file(source.join("server.bin")).unwrap();
    fs::rename(replacement, source.join("server.bin")).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    require_initial_package_tree(&source, "fixture", Some(&cancellation)).unwrap();
    assert_eq!(reads.chunks(), 1, "unchanged assets must not be rehashed");
    edit_initial(&source, |manifest| {
        manifest["files"]["server.bin"] = serde_json::Value::String("0".repeat(64));
    });
    assert!(matches!(
        require_initial_package_tree(&source, "fixture", Some(&cancellation)),
        Err(StorageError::CleanLibraryProgramRequired { .. })
    ));
    assert_eq!(
        reads.chunks(),
        2,
        "stamp alone cannot authorize another expected digest"
    );
}

#[test]
fn first_use_does_not_transfer_cached_file_identity_with_a_copied_manifest() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let target = fixture.target();
    fs::create_dir_all(target.join("assets/empty")).unwrap();
    for key in [
        "server.bin",
        "assets/required.bin",
        ".langame-initial-package.json",
        ".langame-clean-package.json",
    ] {
        fs::copy(source.join(key), target.join(key)).unwrap();
    }
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    require_initial_package_tree(&target, "fixture", Some(&cancellation)).unwrap();
    assert_eq!(reads.chunks(), 2);
}

#[test]
fn first_use_missing_or_type_changed_payload_cannot_pass_cached_verification() {
    for directory in [false, true] {
        let fixture = Fixture::new();
        let source = fixture.source();
        fixture.record(&source);
        fs::remove_file(source.join("server.bin")).unwrap();
        if directory {
            fs::create_dir(source.join("server.bin")).unwrap();
        }
        assert!(matches!(
            require_initial_package_tree(&source, "fixture", None),
            Err(StorageError::CleanLibraryProgramRequired { .. })
        ));
    }
}

#[test]
fn strict_validation_reads_payloads_once_for_overlapping_inventories() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    assert!(
        retain_verified_library_program_baseline(&source, &fixture.descriptor, Some(&cancellation))
            .unwrap()
    );
    assert_eq!(
        reads.chunks(),
        2,
        "clean and initial must share verified reads, not trust the persisted cache"
    );
    assert!(
        library_program_is_pristine(&source, &fixture.descriptor, Some(&cancellation)).unwrap()
    );
    assert_eq!(
        reads.chunks(),
        4,
        "explicit pristine checks must still read current bytes"
    );
}

#[test]
fn published_payload_reuses_checks_but_preserved_defaults_invalidate_first_use() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "user.cfg", b"shipped defaults");
    fixture.record(&source);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    assert!(
        retain_published_library_program_baseline(
            &source,
            &fixture.descriptor,
            Some(&cancellation)
        )
        .unwrap()
    );
    assert_eq!(reads.chunks(), 0);
    put(&source, "user.cfg", b"preserved personal settings");
    assert!(
        retain_published_library_program_baseline(
            &source,
            &fixture.descriptor,
            Some(&cancellation)
        )
        .unwrap()
    );
    assert_eq!(
        reads.chunks(),
        1,
        "only the changed default needs content verification"
    );
    assert!(!source.join(".langame-initial-package.json").exists());
    assert!(library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
}

#[tokio::test]
async fn first_use_cancellation_does_not_publish_updated_cache() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "assets/large.bin", &vec![29; 512 * 1024 + 1]);
    fixture.record(&source);
    edit_initial(&source, |manifest| {
        manifest.as_object_mut().unwrap().remove("verified_files");
    });
    let before = fs::read(source.join(".langame-initial-package.json")).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Hash);
    let worker_root = source.clone();
    let worker_cancel = cancellation.clone();
    let work = tokio::task::spawn_blocking(move || {
        require_initial_package_tree(&worker_root, "fixture", Some(&worker_cancel))
    });
    gate.reached().await;
    cancellation.store(true, Ordering::Release);
    drop(gate);
    assert!(matches!(
        work.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(
        fs::read(source.join(".langame-initial-package.json")).unwrap(),
        before
    );
}

#[cfg(windows)]
#[test]
fn first_use_cache_cannot_admit_a_file_with_an_open_writer() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let _writer = fs::OpenOptions::new()
        .write(true)
        .open(source.join("server.bin"))
        .unwrap();
    assert!(matches!(
        require_initial_package_tree(&source, "fixture", None),
        Err(StorageError::ReadPath { .. })
    ));
}

#[cfg(windows)]
#[test]
fn first_use_rejects_same_size_changed_bytes_even_with_restored_mtime() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let fixture = Fixture::new();
    let source = fixture.source();
    let path = source.join("server.bin");
    {
        let handle = fs::OpenOptions::new().write(true).open(&path).unwrap();
        let baseline_time = FILE_BASIC_INFO {
            ChangeTime: 132_000_000_000_000_000,
            ..Default::default()
        };
        assert_ne!(
            unsafe {
                SetFileInformationByHandle(
                    handle.as_raw_handle(),
                    FileBasicInfo,
                    (&baseline_time as *const FILE_BASIC_INFO).cast(),
                    std::mem::size_of_val(&baseline_time) as u32,
                )
            },
            0
        );
    }
    let before = fs::metadata(&path).unwrap();
    fixture.record(&source);
    fs::write(&path, vec![b'x'; before.len() as usize]).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(before.modified().unwrap()))
        .unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    assert!(matches!(
        require_initial_package_tree(&source, "fixture", Some(&cancellation)),
        Err(StorageError::CleanLibraryProgramRequired { .. })
    ));
    assert_eq!(
        reads.chunks(),
        1,
        "ctime change must rehash only the rewritten file"
    );
}

#[test]
fn first_use_verification_io_profile() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "assets/large.bin", &vec![41; 32 * 1024 * 1024]);
    fixture.record(&source);
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    let full_started = std::time::Instant::now();
    assert!(
        library_program_is_pristine(&source, &fixture.descriptor, Some(&cancellation)).unwrap()
    );
    let full_elapsed = full_started.elapsed();
    assert_eq!(reads.chunks(), 130);
    let quick_started = std::time::Instant::now();
    require_initial_package_tree(&source, "fixture", Some(&cancellation)).unwrap();
    let quick_elapsed = quick_started.elapsed();
    assert_eq!(reads.chunks(), 130, "first use must add no payload reads");
    eprintln!(
        "32 MiB synthetic package: full verification {full_elapsed:?}, first-use metadata verification {quick_elapsed:?}; additional hash chunks: 0"
    );
}
