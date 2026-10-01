use super::*;
use app_core::InstanceArchiveResult;

#[path = "tests_instance_reconciliation.rs"]
mod reconciliation;

struct DeletionFixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
    install_root: PathBuf,
}

impl DeletionFixture {
    async fn new(descriptor: &ModuleDescriptor) -> Self {
        let root = unique_test_root();
        let mut paths = test_paths(&root);
        paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(&paths).await.unwrap();
        sync_modules(&paths, std::slice::from_ref(descriptor))
            .await
            .unwrap();
        let install_root = paths
            .games_root
            .join(&descriptor.install.as_ref().unwrap().shared_game_dir);
        fs::create_dir_all(&install_root).unwrap();
        fs::write(install_root.join("package.fixture"), b"immutable package").unwrap();
        sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: descriptor.summary.id.clone(),
                install_root: install_root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        Self {
            root,
            paths,
            descriptor: descriptor.clone(),
            install_root,
        }
    }

    async fn create(&self, name: &str) -> InstanceDetails {
        let created = create_instance(
            &self.paths,
            &self.descriptor,
            CreateInstanceInput {
                name: name.to_owned(),
                module_id: self.descriptor.summary.id.clone(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("{} / {name}: {error}", self.descriptor.summary.id));
        read_instance_details(&self.paths, &created.summary.id)
            .await
            .unwrap()
    }

    async fn set_status(&self, instance: &InstanceDetails, status: &str) {
        let pool = connect_pool(&self.paths).await.unwrap();
        sqlx::query("UPDATE instances SET status = ?2 WHERE id = ?1")
            .bind(&instance.summary.id)
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
}

impl Drop for DeletionFixture {
    fn drop(&mut self) {
        cleanup_root(&self.root);
    }
}

fn repo_descriptors() -> Vec<ModuleDescriptor> {
    app_modules::discover_modules(repo_root().join("modules")).unwrap()
}

fn instance_root(instance: &InstanceDetails) -> PathBuf {
    Path::new(&instance.config_file_path)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn write_world(instance: &InstanceDetails, content: &[u8]) -> PathBuf {
    let path = Path::new(&instance.saves_path).join("world.fixture");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    path
}

fn assert_preserved_file(result: &InstanceArchiveResult, previous: &Path, expected: &[u8]) {
    let previous_root = Path::new(&result.previous_instance_root);
    let actual = match previous.strip_prefix(previous_root) {
        Ok(relative) => Path::new(result.archived_instance_root.as_ref().unwrap()).join(relative),
        Err(_) => previous.to_path_buf(),
    };
    assert_eq!(fs::read(actual).unwrap(), expected);
}

#[tokio::test]
async fn instance_deletion_missing_runtime_removes_remaining_owned_files() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("Deleted runtime").await;
    let root = instance_root(&instance);
    fs::remove_dir_all(root.join("runtime")).unwrap();
    fs::write(root.join("remaining.sentinel"), b"owned remainder").unwrap();
    assert!(
        archive_instance(&fixture.paths, &instance.summary.id)
            .await
            .is_err()
    );
    let plan = inspect_instance_removal(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert_eq!(Path::new(&plan.data_path), root);
    read_instance_retirement_resources(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    delete_instance(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert!(!root.exists());
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    assert_eq!(
        fs::read(fixture.install_root.join("package.fixture")).unwrap(),
        b"immutable package"
    );
}

#[tokio::test]
async fn instance_deletion_missing_root_removes_only_registration() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("Deleted root").await;
    let root = instance_root(&instance);
    fs::remove_dir_all(&root).unwrap();
    delete_instance(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
    assert!(!root.exists());
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    assert_eq!(
        fs::read(fixture.install_root.join("package.fixture")).unwrap(),
        b"immutable package"
    );
}

#[tokio::test]
async fn instance_deletion_damaged_runtime_uses_registered_owned_tree() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    for empty in [false, true] {
        let fixture = DeletionFixture::new(&descriptor).await;
        let instance = fixture.create("Damaged runtime").await;
        let root = instance_root(&instance);
        let runtime = root.join("runtime");
        if empty {
            fs::remove_dir_all(&runtime).unwrap();
            fs::create_dir(&runtime).unwrap();
        } else {
            fs::remove_file(runtime.join(".langame-private-runtime")).unwrap();
            fs::write(runtime.join("remaining-data"), b"owned data").unwrap();
        }
        assert!(
            archive_instance(&fixture.paths, &instance.summary.id)
                .await
                .is_err()
        );
        assert!(
            list_missing_instance_candidates(&fixture.paths)
                .await
                .unwrap()
                .is_empty()
        );
        inspect_instance_removal(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        delete_instance(&fixture.paths, &instance.summary.id)
            .await
            .unwrap();
        assert!(!root.exists());
        assert_eq!(
            fs::read(fixture.install_root.join("package.fixture")).unwrap(),
            b"immutable package"
        );
    }
}

#[tokio::test]
async fn instance_archive_and_delete_all_repo_modules_preserve_worlds_until_explicit_deletion() {
    let descriptors = repo_descriptors();
    assert_eq!(
        descriptors.len(),
        32,
        "update the fixture matrix for catalog changes"
    );
    for descriptor in descriptors {
        let fixture = DeletionFixture::new(&descriptor).await;
        // Exercise actual catalog templates, schema defaults and storage APIs.
        // Package/world sentinels are synthetic files; no game process is run.
        let empty = fixture.create("Empty shared instance").await;
        let empty_config = fs::read(&empty.config_file_path).unwrap();
        let empty_result = archive_instance(&fixture.paths, &empty.summary.id)
            .await
            .unwrap();
        assert_preserved_file(
            &empty_result,
            Path::new(&empty.config_file_path),
            &empty_config,
        );
        assert!(!instance_root(&empty).exists());
        restore_instance_archive(&fixture.paths, &empty_result.archive_id)
            .await
            .unwrap();
        let removed = delete_instance(&fixture.paths, &empty.summary.id)
            .await
            .unwrap();
        assert!(!Path::new(&removed.deleted_instance_root).exists());
        assert!(
            list_instance_archives(&fixture.paths)
                .await
                .unwrap()
                .archives
                .is_empty()
        );
        drop(fixture);

        // Begin the peer ownership scenario with a fresh synthetic acquisition.
        let fixture = DeletionFixture::new(&descriptor).await;
        let shared = fixture.create("Shared world").await;
        let private = fixture.create("Private world").await;
        let empty_private = fixture.create("Empty private instance").await;
        let empty_private_result = archive_instance(&fixture.paths, &empty_private.summary.id)
            .await
            .unwrap();
        assert!(empty_private_result.archived_instance_root.is_some());
        assert!(!instance_root(&empty_private).exists());
        assert!(Path::new(&shared.config_file_path).is_file());
        assert!(Path::new(&private.config_file_path).is_file());
        let shared_root = instance_root(&shared);
        let private_root = instance_root(&private);
        let expected_mode =
            if descriptor.storage.program_sharing == app_modules::ModuleProgramSharing::Shared {
                InstanceProgramMode::Shared
            } else {
                InstanceProgramMode::Independent
            };
        assert_eq!(
            instance_program_mode(&shared_root).unwrap(),
            expected_mode,
            "{} first instance",
            descriptor.summary.id
        );
        assert_eq!(
            instance_program_mode(&private_root).unwrap(),
            expected_mode,
            "{} second instance",
            descriptor.summary.id
        );
        let shared_world = write_world(&shared, b"shared-world");
        let private_world = write_world(&private, b"private-world");
        assert_ne!(
            shared_world, private_world,
            "{} save ownership",
            descriptor.summary.id
        );
        let shared_backup = create_instance_backup(&fixture.paths, &shared.summary.id)
            .await
            .unwrap();
        let private_backup = create_instance_backup(&fixture.paths, &private.summary.id)
            .await
            .unwrap();
        let private_config = fs::read(&private.config_file_path).unwrap();
        let private_ports = serde_json::to_value(&private.ports).unwrap();

        let run =
            record_started_test_instance(&fixture.paths, &shared.summary.id, 12345, "fixture.log")
                .await
                .unwrap();
        // A stale status cannot bypass the active process count.
        fixture.set_status(&shared, "stopped").await;
        let blocked = archive_instance(&fixture.paths, &shared.summary.id)
            .await
            .unwrap_err();
        assert!(
            matches!(blocked, StorageError::ActiveInstanceDeletion { .. }),
            "{}: {blocked}",
            descriptor.summary.id
        );
        assert!(shared_root.is_dir());
        assert!(
            read_active_instance_run(&fixture.paths, &shared.summary.id)
                .await
                .unwrap()
                .is_some()
        );
        mark_instance_process_stopped(
            &fixture.paths,
            &shared.summary.id,
            run.run_id,
            Some(0),
            false,
        )
        .await
        .unwrap();

        let shared_result = archive_instance(&fixture.paths, &shared.summary.id)
            .await
            .unwrap();
        assert_eq!(
            shared_result.effective_saves_path, shared.saves_path,
            "{} effective path",
            descriptor.summary.id
        );
        assert_preserved_file(&shared_result, &shared_world, b"shared-world");
        assert_preserved_file(
            &shared_result,
            &shared_root
                .join("backups")
                .join(&shared_backup.backup_id)
                .join("saves/world.fixture"),
            b"shared-world",
        );
        assert_eq!(
            shared_result.saves_archived_with_instance_root,
            Path::new(&shared.saves_path).starts_with(&shared_root)
        );
        let retained = read_instance_details(&fixture.paths, &private.summary.id)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(&retained.ports).unwrap(),
            private_ports
        );
        assert_eq!(fs::read(&private.config_file_path).unwrap(), private_config);
        assert_eq!(fs::read(&private_world).unwrap(), b"private-world");
        assert_eq!(
            list_instance_backups(&fixture.paths, &private.summary.id)
                .await
                .unwrap()
                .len(),
            1
        );

        let private_result = archive_instance(&fixture.paths, &private.summary.id)
            .await
            .unwrap();
        assert!(
            private_result.saves_archived_with_instance_root,
            "{} private saves",
            descriptor.summary.id
        );
        assert_eq!(private_result.preserved_external_saves_path, None);
        assert_preserved_file(&private_result, &private_world, b"private-world");
        assert_preserved_file(
            &private_result,
            &private_root
                .join("backups")
                .join(&private_backup.backup_id)
                .join("saves/world.fixture"),
            b"private-world",
        );
        assert_eq!(
            fs::read(fixture.install_root.join("package.fixture")).unwrap(),
            b"immutable package"
        );
        if expected_mode == InstanceProgramMode::Independent {
            assert_preserved_file(
                &private_result,
                &private_root.join("runtime/package.fixture"),
                b"immutable package",
            );
        }
        assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
        for archived in [&empty_private_result, &shared_result, &private_result] {
            restore_instance_archive(&fixture.paths, &archived.archive_id)
                .await
                .unwrap();
            let removed = delete_instance(&fixture.paths, &archived.instance_id)
                .await
                .unwrap();
            assert!(!Path::new(&removed.deleted_instance_root).exists());
            assert!(!Path::new(archived.archived_instance_root.as_ref().unwrap()).exists());
            assert_eq!(
                fs::read(fixture.install_root.join("package.fixture")).unwrap(),
                b"immutable package"
            );
        }
        let archived = list_instance_archives(&fixture.paths).await.unwrap();
        assert!(archived.archives.is_empty());
        assert!(archived.pending_deletions.is_empty());
        assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
        eprintln!(
            "archive and delete matrix passed: {}",
            descriptor.summary.id
        );
    }
}

#[tokio::test]
async fn instance_deletion_all_repo_modules_removes_owned_data_and_preserves_library_and_peer() {
    let descriptors = repo_descriptors();
    assert_eq!(descriptors.len(), 32);
    for descriptor in descriptors {
        let fixture = DeletionFixture::new(&descriptor).await;
        let removed = fixture.create("Delete owned server").await;
        let kept = fixture.create("Keep peer server").await;
        let world = write_world(&removed, b"removed world");
        let backup = create_instance_backup(&fixture.paths, &removed.summary.id)
            .await
            .unwrap();
        let peer_world = write_world(&kept, b"preserved peer world");
        let peer_config = fs::read(&kept.config_file_path).unwrap();
        let library_before = read_library_program_install(&fixture.paths, &descriptor.summary.id)
            .await
            .unwrap()
            .unwrap();
        let removed_root = instance_root(&removed);
        assert!(world.starts_with(&removed_root));
        assert!(Path::new(&backup.backup_path).starts_with(&removed_root));

        let result = delete_instance(&fixture.paths, &removed.summary.id)
            .await
            .unwrap();

        assert_eq!(Path::new(&result.deleted_instance_root), removed_root);
        assert!(
            !removed_root.exists(),
            "{} owned directory",
            descriptor.summary.id
        );
        assert!(!world.exists());
        assert!(!Path::new(&backup.backup_path).exists());
        assert_eq!(fs::read(&peer_world).unwrap(), b"preserved peer world");
        assert_eq!(fs::read(&kept.config_file_path).unwrap(), peer_config);
        assert_eq!(
            fs::read(fixture.install_root.join("package.fixture")).unwrap(),
            b"immutable package"
        );
        let library_after = read_library_program_install(&fixture.paths, &descriptor.summary.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(library_after.id, library_before.id);
        assert_eq!(library_after.install_root, library_before.install_root);
        assert_eq!(library_after.install_state, InstallState::Installed);
        let archives = list_instance_archives(&fixture.paths).await.unwrap();
        assert!(
            archives.archives.is_empty(),
            "delete must not create a recoverable archive"
        );
        assert!(
            archives.pending_deletions.is_empty(),
            "completed delete must release owned space"
        );
        let remaining = list_instances(&fixture.paths).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, kept.summary.id);
    }
}

#[tokio::test]
async fn instance_deletion_rejects_all_active_states_without_side_effects() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("State guard").await;
    let config = fs::read(&instance.config_file_path).unwrap();
    let world = write_world(&instance, b"untouched-world");
    for status in ["starting", "running", "stopping"] {
        fixture.set_status(&instance, status).await;
        assert!(
            matches!(
                delete_instance(&fixture.paths, &instance.summary.id).await,
                Err(StorageError::ActiveInstanceDeletion { .. })
            ),
            "{status}"
        );
        assert_eq!(fs::read(&instance.config_file_path).unwrap(), config);
        assert_eq!(fs::read(&world).unwrap(), b"untouched-world");
        assert!(!fixture.paths.instances_root.join(".trash").exists());
    }
    fixture.set_status(&instance, "error").await;
    delete_instance(&fixture.paths, &instance.summary.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn instance_archive_then_delete_uses_private_save_path_after_install_override() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let original = fixture.create("Relocated installation").await;
    let old_world = write_world(&original, b"previous-world");
    let current_root = fixture.root.join("relocated-package");
    fs::create_dir_all(&current_root).unwrap();
    sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id,
            install_root: current_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: None,
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    let current = read_instance_details(&fixture.paths, &original.summary.id)
        .await
        .unwrap();
    assert_eq!(current.saves_path, original.saves_path);
    assert_eq!(fs::read(&old_world).unwrap(), b"previous-world");
    let result = archive_instance(&fixture.paths, &original.summary.id)
        .await
        .unwrap();
    assert_eq!(result.effective_saves_path, current.saves_path);
    assert!(result.saves_archived_with_instance_root);
    assert_eq!(result.preserved_external_saves_path, None);
    assert_preserved_file(&result, &old_world, b"previous-world");
    restore_instance_archive(&fixture.paths, &result.archive_id)
        .await
        .unwrap();
    assert_eq!(fs::read(&old_world).unwrap(), b"previous-world");
    let removed = delete_instance(&fixture.paths, &original.summary.id)
        .await
        .unwrap();
    assert!(!Path::new(&removed.deleted_instance_root).exists());
    assert!(!old_world.exists());
    assert!(current_root.is_dir());
}

#[cfg(windows)]
#[tokio::test]
async fn instance_deletion_rejects_archive_junction_and_retains_database_and_files() {
    let descriptor = repo_descriptors()
        .into_iter()
        .find(|item| item.summary.id == "astroneer")
        .unwrap();
    let fixture = DeletionFixture::new(&descriptor).await;
    let instance = fixture.create("Archive boundary").await;
    let external = fixture.root.join("external");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("sentinel"), b"external").unwrap();
    let archive = fixture.paths.instances_root.join(".trash");
    let output = Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(&archive)
        .arg(&external)
        .output()
        .unwrap();
    assert!(output.status.success(), "junction fixture: {output:?}");
    let result = delete_instance(&fixture.paths, &instance.summary.id).await;
    fs::remove_dir(&archive).unwrap();
    assert!(matches!(
        result,
        Err(StorageError::UnsafeManagedPath { .. })
    ));
    assert!(
        read_instance_details(&fixture.paths, &instance.summary.id)
            .await
            .is_ok()
    );
    assert!(Path::new(&instance.config_file_path).is_file());
    assert_eq!(fs::read(external.join("sentinel")).unwrap(), b"external");
    assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
}
