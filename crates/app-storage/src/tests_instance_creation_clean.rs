use super::*;
use crate::instance_creation_io::test_gate::count_hash_reads;
use std::sync::atomic::AtomicBool;

#[path = "program_exclusive_creation_tests.rs"]
mod exclusive;
#[path = "program_library_retention_creation_tests.rs"]
mod retained_library;

struct CleanCreationFixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
    source: PathBuf,
}

impl CleanCreationFixture {
    async fn new() -> Self {
        let root = unique_test_root();
        let paths = test_paths(&root);
        let descriptor = test_descriptor(&root);
        prepare_environment(&root, &descriptor);
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let source = paths.games_root.join(&descriptor.summary.id);
        fs::write(source.join("payload.bin"), vec![7; 512 * 1024 + 1]).unwrap();
        Self {
            root,
            paths,
            descriptor,
            source,
        }
    }

    fn record(&self) {
        record_library_program_baseline(&self.source, &self.descriptor, true, None).unwrap();
    }

    async fn create(
        &self,
        cancellation: Option<Arc<AtomicBool>>,
    ) -> Result<CreateInstanceResult, StorageError> {
        create_instance_with_options(
            &self.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: "Clean creation".into(),
                module_id: self.descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                require_clean_program: true,
                cancellation,
                ..Default::default()
            },
        )
        .await
    }

    async fn assert_no_instance(&self) {
        assert!(list_instances(&self.paths).await.unwrap().is_empty());
        assert!(managed_instance_directories(&self.paths).is_empty());
    }
}

impl Drop for CleanCreationFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

#[tokio::test]
async fn independent_local_creation_preserves_the_registered_library_for_reuse_after_deletion() {
    assert_independent_creation_preserves_library(false).await;
}

#[tokio::test]
async fn independent_verified_creation_preserves_the_registered_library_for_reuse_after_deletion() {
    assert_independent_creation_preserves_library(true).await;
}

#[tokio::test]
async fn independent_creation_rejects_managed_runtime_sources_without_changing_them() {
    for marker in [
        ".langame-private-runtime",
        ".langame-runtime-projection.json",
        ".langame-package-baseline.json",
    ] {
        let fixture = CleanCreationFixture::new().await;
        fs::write(
            fixture.source.join(marker),
            b"existing managed runtime identity",
        )
        .unwrap();
        let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
        let error = create_instance_with_options(
            &fixture.paths,
            &fixture.descriptor,
            CreateInstanceInput {
                name: "Wrong source".into(),
                module_id: fixture.descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                use_local_program: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            error.to_string().contains("already managed by an instance"),
            "{error}"
        );
        fixture.assert_no_instance().await;
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
            before
        );
    }
}

#[tokio::test]
async fn independent_copy_materialization_failure_preserves_the_registered_library() {
    let fixture = CleanCreationFixture::new().await;
    sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.source.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("downloaded-version".into()),
            mark_verified: false,
        }],
    )
    .await
    .unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    fs::write(
        fixture.descriptor.root.join("templates/cluster.ini.hbs"),
        [0xff, 0xfe],
    )
    .unwrap();
    let error = create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "Broken materialization".into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            use_local_program: true,
            ..Default::default()
        },
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&error, StorageError::ReadConfig { path, .. } if path.ends_with("templates/cluster.ini.hbs")),
        "{error}"
    );
    fixture.assert_no_instance().await;
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        before
    );
    let library = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(library.scope, ProgramInstallScope::Library);
    assert_eq!(library.install_state, InstallState::Installed);
    assert_eq!(
        library.current_version.as_deref(),
        Some("downloaded-version")
    );
    assert_eq!(library.owner_instance_id, None);
}

async fn assert_independent_creation_preserves_library(verified: bool) {
    use crate::test_file_snapshot::tree_snapshot;

    let fixture = CleanCreationFixture::new().await;
    if verified {
        fixture.record();
    }
    sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: fixture.source.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("downloaded-version".into()),
            mark_verified: verified,
        }],
    )
    .await
    .unwrap();
    let library = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    let source_before = tree_snapshot(&fixture.source).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let hash_reads = count_hash_reads(&cancellation);
    let mut ids = Vec::new();
    for name in ["First independent", "Second independent"] {
        let created = create_instance_with_options(
            &fixture.paths,
            &fixture.descriptor,
            CreateInstanceInput {
                name: name.into(),
                module_id: fixture.descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                use_local_program: !verified,
                require_clean_program: verified,
                cancellation: Some(cancellation.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            fixture.source.is_dir(),
            "creating an instance must retain the library"
        );
        assert_eq!(tree_snapshot(&fixture.source).unwrap(), source_before);
        assert_ne!(created.effective_install_root, fixture.source);
        assert_eq!(
            fs::read(created.effective_install_root.join("payload.bin")).unwrap(),
            vec![7; 512 * 1024 + 1]
        );
        let owned = read_instance_program_install(&fixture.paths, &created.provisioning.summary.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(owned.install.scope, ProgramInstallScope::Instance);
        assert_ne!(owned.install.id, library.id);
        assert_eq!(owned.install.current_version, library.current_version);
        assert_eq!(
            owned.install.owner_instance_id.as_deref(),
            Some(created.provisioning.summary.id.as_str())
        );
        fs::write(
            created.effective_install_root.join("payload.bin"),
            name.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            tree_snapshot(&fixture.source).unwrap(),
            source_before,
            "private writes must not affect library files through links"
        );
        let current = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(current.id, library.id);
        assert_eq!(current.scope, ProgramInstallScope::Library);
        assert_eq!(current.install_state, InstallState::Installed);
        assert_eq!(current.current_version, library.current_version);
        assert_eq!(current.owner_instance_id, None);
        ids.push(created.provisioning.summary.id);
    }
    if verified {
        assert!(
            hash_reads.chunks() > 0,
            "official source verification must remain active"
        );
    } else {
        assert_eq!(
            hash_reads.chunks(),
            0,
            "local copies must not hash unused refresh baselines"
        );
    }
    for id in ids {
        delete_instance(&fixture.paths, &id).await.unwrap();
        assert_eq!(tree_snapshot(&fixture.source).unwrap(), source_before);
    }
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    let current = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.id, library.id);
    assert_eq!(current.install_state, InstallState::Installed);
    assert_eq!(current.current_version, library.current_version);
    assert_eq!(current.scope, ProgramInstallScope::Library);
    assert_eq!(current.owner_instance_id, None);
    assert_eq!(
        PathBuf::from(
            resolve_module_install_root(&fixture.paths, &fixture.descriptor.summary.id)
                .await
                .unwrap()
                .unwrap()
        ),
        fixture.source
    );
    assert_eq!(tree_snapshot(&fixture.source).unwrap(), source_before);
}

#[tokio::test]
async fn clean_copy_verifies_the_official_payload_and_leaves_unknown_data_in_the_library() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let manifest = fs::read(fixture.source.join(".langame-clean-package.json")).unwrap();
    assert!(!manifest.is_empty() && manifest.len() <= 256 * 1024);
    fs::create_dir_all(fixture.source.join("unknown-mod")).unwrap();
    fs::write(
        fixture.source.join("unknown-mod/user.dat"),
        b"keep this private data",
    )
    .unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    let created = fixture.create(Some(cancellation)).await.unwrap();
    assert_eq!(
        reads.chunks(),
        3,
        "the 512 KiB + 1 byte payload is verified in 3 copy-hash blocks without a second read"
    );
    assert!(fixture.source.is_dir());
    let source_manifest: Value = serde_json::from_slice(&manifest).unwrap();
    let copied_manifest: Value = serde_json::from_slice(
        &fs::read(
            created
                .effective_install_root
                .join(".langame-clean-package.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for field in ["version", "module_id", "source", "files", "directories"] {
        assert_eq!(copied_manifest[field], source_manifest[field], "{field}");
    }
    assert!(
        copied_manifest
            .get("verified_files")
            .is_none_or(|cache| cache.as_object().is_some_and(|cache| cache.is_empty()))
    );
    assert!(
        !created
            .effective_install_root
            .join(".langame-initial-package.json")
            .exists()
    );
    assert_eq!(
        fs::read(fixture.source.join(".langame-clean-package.json")).unwrap(),
        manifest
    );
    assert_eq!(
        fs::read(created.effective_install_root.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
    assert!(!created.effective_install_root.join("unknown-mod").exists());
    assert_eq!(
        fs::read(fixture.source.join("unknown-mod/user.dat")).unwrap(),
        b"keep this private data"
    );
}

#[tokio::test]
async fn clean_requirement_rejects_untrusted_changed_missing_and_foreign_packages_without_ownership()
 {
    for damage in ["untrusted", "changed", "missing", "foreign"] {
        let fixture = CleanCreationFixture::new().await;
        if damage != "untrusted" {
            fixture.record();
        }
        match damage {
            "changed" => {
                fs::write(fixture.source.join("payload.bin"), vec![8; 512 * 1024 + 1]).unwrap()
            }
            "missing" => fs::remove_file(fixture.source.join("payload.bin")).unwrap(),
            "foreign" => {
                let path = fixture.source.join(".langame-clean-package.json");
                let mut manifest: Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                manifest["module_id"] = Value::String("another-game".into());
                fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
            _ => {}
        }
        fs::write(
            fixture.source.join("operator-data.bin"),
            b"must remain in place",
        )
        .unwrap();
        let error = fixture.create(None).await.unwrap_err();
        assert!(
            matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
            "{damage}: {error}"
        );
        fixture.assert_no_instance().await;
        assert_eq!(
            fs::read(fixture.source.join("operator-data.bin")).unwrap(),
            b"must remain in place"
        );
        assert!(!fixture.source.join(".langame-private-runtime").exists());
        assert!(
            !fixture
                .source
                .join(".langame-package-baseline.json")
                .exists()
        );
    }
}

#[tokio::test]
async fn corrupt_clean_metadata_is_not_a_fresh_acquisition_signal() {
    let fixture = CleanCreationFixture::new().await;
    fs::write(
        fixture.source.join(".langame-clean-package.json"),
        b"{broken",
    )
    .unwrap();
    let error = fixture.create(None).await.unwrap_err();
    assert!(
        !matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
        "{error}"
    );
    assert!(
        error.to_string().contains("invalid clean package manifest"),
        "{error}"
    );
    fixture.assert_no_instance().await;
    assert!(fixture.source.join("payload.bin").is_file());
}

#[tokio::test]
async fn official_path_type_file_replaced_by_directory_requests_clean_acquisition() {
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let replaced = fixture.source.join("payload.bin");
    let preserved = fixture.source.join("payload.original");
    fs::rename(&replaced, &preserved).unwrap();
    fs::create_dir(&replaced).unwrap();
    fs::write(
        replaced.join("operator-save.dat"),
        b"preserve the replacement directory",
    )
    .unwrap();

    let error = fixture.create(None).await.unwrap_err();
    assert!(
        matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
        "{error}"
    );
    fixture.assert_no_instance().await;
    assert_eq!(fs::read(preserved).unwrap(), vec![7; 512 * 1024 + 1]);
    assert_eq!(
        fs::read(replaced.join("operator-save.dat")).unwrap(),
        b"preserve the replacement directory"
    );
    assert!(fixture.source.join(".langame-clean-package.json").is_file());
}

#[tokio::test]
async fn official_path_type_directory_replaced_by_file_requests_clean_acquisition() {
    let fixture = CleanCreationFixture::new().await;
    let replaced = fixture.source.join("assets");
    let preserved = fixture.source.join("preserved-assets");
    fs::create_dir(&replaced).unwrap();
    fs::write(replaced.join("required.bin"), b"official nested program").unwrap();
    fixture.record();
    fs::rename(&replaced, &preserved).unwrap();
    fs::write(&replaced, b"preserve the replacement file").unwrap();

    let error = fixture.create(None).await.unwrap_err();
    assert!(
        matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
        "{error}"
    );
    fixture.assert_no_instance().await;
    assert_eq!(
        fs::read(&replaced).unwrap(),
        b"preserve the replacement file"
    );
    assert_eq!(
        fs::read(preserved.join("required.bin")).unwrap(),
        b"official nested program"
    );
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
    assert!(fixture.source.join(".langame-clean-package.json").is_file());
}

#[cfg(windows)]
#[tokio::test]
async fn unreadable_official_file_is_not_a_fresh_acquisition_signal() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = CleanCreationFixture::new().await;
    fixture.record();
    let exclusive_file = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(fixture.source.join("payload.bin"))
        .unwrap();
    let error = fixture.create(None).await.unwrap_err();
    assert!(matches!(error, StorageError::ReadPath { .. }), "{error}");
    fixture.assert_no_instance().await;
    drop(exclusive_file);
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[tokio::test]
async fn clean_requirement_applies_to_shared_and_private_copy_modes() {
    for mode in [
        InstanceProgramMode::Shared,
        InstanceProgramMode::Independent,
    ] {
        let fixture = CleanCreationFixture::new().await;
        let mut descriptor = fixture.descriptor.clone();
        descriptor.storage.program_sharing = app_modules::ModuleProgramSharing::Shared;
        sync_game_installs(
            &fixture.paths,
            &[GameInstallSyncRecord {
                module_id: descriptor.summary.id.clone(),
                install_root: fixture.source.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: None,
                mark_verified: false,
            }],
        )
        .await
        .unwrap();
        let error = create_instance_with_options(
            &fixture.paths,
            &descriptor,
            CreateInstanceInput {
                name: "Strict mode".into(),
                module_id: descriptor.summary.id.clone(),
            },
            InstanceCreationOptions {
                program_mode: Some(mode),
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, StorageError::CleanLibraryProgramRequired { .. }),
            "{mode:?}: {error}"
        );
        fixture.assert_no_instance().await;
        assert!(fixture.source.join("payload.bin").is_file());
    }
}
