use super::*;

fn archive_fixture() -> (Fixture, PathBuf, PathBuf) {
    let mut fixture = Fixture::new();
    let install = fixture.descriptor.install.as_mut().unwrap();
    install.download_url_windows = Some("https://example.invalid/server.zip".into());
    install.verification_path = Some("server.bin".into());
    install.download_integrity_windows = Some(app_core::DownloadIntegritySpec {
        sha256: "1".repeat(64),
        size: 1,
    });
    fixture
        .descriptor
        .storage
        .retained_paths
        .push("Configs".into());
    let source = fixture.source();
    fixture.record(&source);
    // This is the actual old schema: no archive identity was recorded in 0.1.4.
    for name in [
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = source.join(name);
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value.as_object_mut().unwrap().remove("archive_sha256");
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    let stage = fixture.root.join("stage");
    put(&stage, "server.bin", b"new official executable");
    put(&stage, "assets/required.bin", b"new official assets");
    put(&stage, "Configs/native.json", b"new shipped default");
    fixture.record(&stage);
    (fixture, source, stage)
}

#[test]
fn archive_update_from_old_manifest_preserves_data_and_program_identity() {
    let (fixture, source, stage) = archive_fixture();
    for (path, bytes) in [
        ("Saved/world.sav", b"world".as_slice()),
        ("Configs/native.json", b"operator configuration".as_slice()),
        ("unknown-mods/custom.dll", b"operator mod".as_slice()),
        (".langame-program-identity.json", br#"{"version":1,"module_id":"fixture","id":"program-1"}"#.as_slice()),
        (".langame-program-usage.json", br#"{"version":1,"module_id":"fixture","program_id":"program-1","instance_id":"instance-1"}"#.as_slice()),
    ] {
        put(&source, path, bytes);
    }
    // An actual instance data path remains protected even if its bytes were
    // previously listed as an original package file.
    let actual_save = source.join("assets/required.bin");
    fs::remove_file(stage.join("assets/required.bin")).unwrap();
    fixture.record(&stage);
    let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
    assert!(archive_program_has_baseline(&source, &fixture.descriptor).unwrap());
    assert!(!archive_program_is_current(&source, &fixture.descriptor, None).unwrap());
    merge_archive_program_update(&source, &stage, &fixture.descriptor, &[actual_save], None)
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
        before
    );
    for path in [
        "Saved/world.sav",
        "Configs/native.json",
        "unknown-mods/custom.dll",
        "assets/required.bin",
        ".langame-program-identity.json",
        ".langame-program-usage.json",
    ] {
        assert_eq!(
            fs::read(stage.join(path)).unwrap(),
            fs::read(source.join(path)).unwrap(),
            "{path}"
        );
    }
    assert_eq!(
        fs::read(stage.join("server.bin")).unwrap(),
        b"new official executable"
    );
    assert!(archive_program_is_current(&stage, &fixture.descriptor, None).unwrap());
    fs::write(stage.join("server.bin"), b"modified after installation").unwrap();
    assert!(!archive_program_is_current(&stage, &fixture.descriptor, None).unwrap());
}

#[test]
fn archive_update_accepts_a_new_executable_directory() {
    let (mut fixture, source, stage) = archive_fixture();
    fixture
        .descriptor
        .install
        .as_mut()
        .unwrap()
        .verification_path = Some("next/server.bin".into());
    fs::create_dir_all(stage.join("next")).unwrap();
    fs::rename(stage.join("server.bin"), stage.join("next/server.bin")).unwrap();
    fixture.record(&stage);
    assert!(archive_program_has_baseline(&source, &fixture.descriptor).unwrap());
    merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None).unwrap();
    assert!(!stage.join("server.bin").exists());
    assert_eq!(
        fs::read(stage.join("next/server.bin")).unwrap(),
        b"new official executable"
    );
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
}

#[test]
fn archive_update_rejects_required_file_conflicts_without_touching_originals() {
    for path in ["server.bin", "assets/required.bin"] {
        let (fixture, source, stage) = archive_fixture();
        put(&source, path, b"operator customization");
        let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
        let staged = crate::test_file_snapshot::tree_snapshot(&stage).unwrap();
        let error = merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None)
            .unwrap_err();
        assert!(
            error.to_string().contains("conflicts with a required file"),
            "{error}"
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
            before
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&stage).unwrap(),
            staged
        );
    }
}

#[test]
fn archive_update_rejects_missing_inventory_and_mismatched_ownership() {
    for corrupt_owner in [false, true] {
        let (fixture, source, stage) = archive_fixture();
        if corrupt_owner {
            put(
                &source,
                ".langame-program-identity.json",
                br#"{"version":1,"module_id":"another-game","id":"owner"}"#,
            );
        } else {
            fs::remove_file(source.join(".langame-clean-package.json")).unwrap();
        }
        let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
        assert!(
            merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None).is_err()
        );
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn cancelled_archive_merge_leaves_original_data_intact() {
    let (fixture, source, stage) = archive_fixture();
    put(&source, "Saved/world.sav", &vec![9; 128 * 1024]);
    let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut gate = pause_at(&cancellation, PausePoint::Copy);
    let worker_source = source.clone();
    let descriptor = fixture.descriptor.clone();
    let worker_cancel = cancellation.clone();
    let worker = tokio::task::spawn_blocking(move || {
        merge_archive_program_update(
            &worker_source,
            &stage,
            &descriptor,
            &[],
            Some(&worker_cancel),
        )
    });
    gate.reached().await;
    cancellation.store(true, std::sync::atomic::Ordering::Release);
    drop(gate);
    assert!(matches!(
        worker.await.unwrap(),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
        before
    );
}

#[tokio::test]
async fn concurrent_retained_children_changes_reject_the_staged_update() {
    for add in [true, false] {
        let (fixture, source, stage) = archive_fixture();
        put(&source, "unknown-mods/large.dat", &vec![8; 128 * 1024]);
        put(&source, "unknown-mods/other.dat", b"existing customization");
        let cancellation = Arc::new(AtomicBool::new(false));
        let mut gate = pause_at(&cancellation, PausePoint::Copy);
        let worker_source = source.clone();
        let descriptor = fixture.descriptor.clone();
        let worker = tokio::task::spawn_blocking(move || {
            merge_archive_program_update(
                &worker_source,
                &stage,
                &descriptor,
                &[],
                Some(&cancellation),
            )
        });
        gate.reached().await;
        if add {
            put(&source, "unknown-mods/added.dat", b"concurrent user data");
        } else {
            fs::remove_file(source.join("unknown-mods/other.dat")).unwrap();
        }
        let changed = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
        drop(gate);
        assert!(worker.await.unwrap().is_err());
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
            changed
        );
    }
}
