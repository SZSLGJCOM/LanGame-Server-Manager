use super::*;

fn recovery_directory(root: &Path) -> PathBuf {
    let directories = fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".langame-archive-recovery-")
        })
        .collect::<Vec<_>>();
    assert_eq!(directories.len(), 1);
    directories.into_iter().next().unwrap()
}

#[test]
fn archive_repair_restores_known_program_files_with_traceable_recovery_copies() {
    let (fixture, source, stage) = archive_fixture();
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(source.join(CLEAN_PACKAGE)).unwrap()).unwrap();
    for path in ["server.bin", "assets/required.bin"] {
        put(&source, path, b"damaged program bytes");
    }
    put(&source, "Configs/native.json", b"operator configuration");
    put(&source, "Saved/world.sav", b"valuable save");
    put(&source, "mods/Example.dll", b"operator mod");
    let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
    merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None).unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
        before
    );
    assert_eq!(
        fs::read(stage.join("server.bin")).unwrap(),
        b"new official executable"
    );
    assert_eq!(
        fs::read(stage.join("assets/required.bin")).unwrap(),
        b"new official assets"
    );
    let recovery = recovery_directory(&stage);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(recovery.join("recovery.json")).unwrap()).unwrap();
    assert_eq!(receipt["version"], 1);
    assert_eq!(receipt["module_id"], fixture.descriptor.summary.id);
    assert_eq!(receipt["archive_sha256"], "1".repeat(64));
    assert_eq!(receipt["files"].as_object().unwrap().len(), 2);
    for path in ["server.bin", "assets/required.bin"] {
        assert_eq!(
            fs::read(recovery.join("files").join(path)).unwrap(),
            b"damaged program bytes"
        );
        assert_eq!(
            receipt["files"][path]["original_sha256"],
            original["files"][path]
        );
        assert_eq!(
            receipt["files"][path]["preserved_sha256"],
            crate::private_runtime_refresh::hash_file(&source.join(path), None).unwrap()
        );
        assert_eq!(
            receipt["files"][path]["installed_sha256"],
            crate::private_runtime_refresh::hash_file(&stage.join(path), None).unwrap()
        );
    }
    for path in ["Configs/native.json", "Saved/world.sav", "mods/Example.dll"] {
        assert_eq!(
            fs::read(stage.join(path)).unwrap(),
            fs::read(source.join(path)).unwrap()
        );
    }
    assert!(archive_program_is_current(&stage, &fixture.descriptor, None).unwrap());

    // Recovery bytes survive the next ordinary update and remain protected by
    // the existing program-removal inventory rather than becoming program files.
    let next = fixture.root.join("next-stage");
    put(&next, "server.bin", b"next official executable");
    put(&next, "assets/required.bin", b"next official assets");
    fixture.record(&next);
    merge_archive_program_update(&stage, &next, &fixture.descriptor, &[], None).unwrap();
    let history_name = recovery.file_name().unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&next.join(history_name)).unwrap(),
        crate::test_file_snapshot::tree_snapshot(&recovery).unwrap()
    );
    let retained =
        crate::program_library_cleanup::library_cleanup_retained_paths(&next, &fixture.descriptor)
            .unwrap()
            .unwrap();
    // Cleanup returns canonical paths (including Windows' verbatim prefix).
    // Require the complete recovery directory itself, not a coincidentally
    // named file or a retained ancestor that would prevent program cleanup.
    let next_recovery = fs::canonicalize(next.join(history_name)).unwrap();
    assert!(
        retained.contains(&next_recovery),
        "recovery subtree must remain protected: {next_recovery:?}; retained: {retained:?}"
    );
    for relative in ["server.bin", "assets/required.bin"] {
        let program = fs::canonicalize(next.join(relative)).unwrap();
        assert!(
            !retained.iter().any(|path| program.starts_with(path)),
            "original program remains removable while recovery bytes survive: {program:?}"
        );
    }
    assert!(archive_program_is_current(&next, &fixture.descriptor, None).unwrap());
}

#[test]
fn archive_repair_never_treats_unknown_conflicting_files_as_original_program() {
    let (fixture, source, stage) = archive_fixture();
    let inventory = source.join(CLEAN_PACKAGE);
    let mut original: serde_json::Value =
        serde_json::from_slice(&fs::read(&inventory).unwrap()).unwrap();
    original["files"]
        .as_object_mut()
        .unwrap()
        .remove("server.bin");
    fs::write(inventory, serde_json::to_vec(&original).unwrap()).unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
    let staged = crate::test_file_snapshot::tree_snapshot(&stage).unwrap();
    let error =
        merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None).unwrap_err();
    assert!(error.to_string().contains("conflicts with a required file"));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
        before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&stage).unwrap(),
        staged
    );
}

#[test]
fn archive_repair_respects_current_data_declarations_even_if_old_inventory_listed_them() {
    for declaration in ["exclusion", "retained", "saves"] {
        let (mut fixture, source, stage) = archive_fixture();
        match declaration {
            "exclusion" => fixture
                .descriptor
                .storage
                .runtime_copy_exclusions
                .push("assets".into()),
            "retained" => fixture
                .descriptor
                .storage
                .retained_paths
                .push("assets".into()),
            "saves" => {
                fixture.descriptor.storage.saves_path_template =
                    Some("{{paths.install_root}}/assets/{{instance.id}}".into());
            }
            _ => unreachable!(),
        }
        put(&source, "assets/required.bin", b"protected user data");
        let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
        let staged = crate::test_file_snapshot::tree_snapshot(&stage).unwrap();
        let error = merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None)
            .unwrap_err();
        assert!(error.to_string().contains("conflicts with a required file"));
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
fn archive_repair_keeps_directory_conflicts_strict() {
    let (fixture, source, stage) = archive_fixture();
    fs::remove_file(source.join("server.bin")).unwrap();
    put(&source, "server.bin/user.dat", b"directory must survive");
    let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
    let staged = crate::test_file_snapshot::tree_snapshot(&stage).unwrap();
    assert!(merge_archive_program_update(&source, &stage, &fixture.descriptor, &[], None).is_err());
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
        before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&stage).unwrap(),
        staged
    );
}

#[tokio::test]
async fn archive_repair_cancellation_or_concurrent_edits_cannot_publish_stale_recovery() {
    for cancel in [true, false] {
        let (fixture, source, stage) = archive_fixture();
        put(&source, "server.bin", &vec![7; 128 * 1024]);
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
        if cancel {
            cancellation.store(true, Ordering::Release);
        } else {
            put(&source, "server.bin", b"concurrent operator bytes");
        }
        let before = crate::test_file_snapshot::tree_snapshot(&source).unwrap();
        drop(gate);
        let result = worker.await.unwrap();
        if cancel {
            assert!(matches!(
                result,
                Err(StorageError::InstanceCreationCancelled)
            ));
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&source).unwrap(),
            before
        );
    }
}
