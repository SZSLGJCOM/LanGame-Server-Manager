use super::*;
use std::collections::HashSet;
use std::fs;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
    source: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-program-inventory-{}",
            uuid::Uuid::new_v4()
        ));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/store.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .into_iter()
            .find(|descriptor| descriptor.summary.id == "palworld")
            .unwrap();
        let source = paths.games_root.join("palworld");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        fs::create_dir_all(&paths.archives_root).unwrap();
        crate::initialize_database(&paths).await.unwrap();
        crate::sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let fixture = Self {
            root,
            paths,
            descriptor,
            source,
        };
        fixture.register(&fixture.source).await;
        fixture
    }

    async fn register(&self, root: &Path) {
        crate::sync_game_installs(
            &self.paths,
            &[crate::GameInstallSyncRecord {
                module_id: self.descriptor.summary.id.clone(),
                install_root: root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("fixture-build".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
    }

    fn record_used_package(&self) {
        crate::record_library_program_baseline(&self.source, &self.descriptor, true, None).unwrap();
        let instance = self.paths.instances_root.join("previous-instance");
        fs::create_dir_all(&instance).unwrap();
        crate::program_runtime::prepare_exclusive_program_reference(
            &self.source,
            &instance,
            &self.descriptor.summary.id,
        )
        .unwrap();
        crate::program_runtime::record_exclusive_program_use(&self.source, &instance).unwrap();
    }

    async fn inspect(&self) -> Result<ModuleProgramInventory, StorageError> {
        inspect_module_programs(
            &self.paths,
            &self.descriptor,
            Some(InstanceProgramMode::Independent),
            InstanceProgramSource::Verified,
            Arc::new(AtomicBool::new(false)),
            false,
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Every fixture owns this UUID directory; no user directories are inspected.
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn program_inventory_exposes_pending_removal_without_counting_an_installed_program() {
    let fixture = Fixture::new().await;
    let owner = crate::read_program_install_owner(&fixture.paths, &fixture.source)
        .await
        .unwrap()
        .unwrap();
    let mut removal = crate::program_removals_db::ProgramRemovalRecord {
        operation_id: uuid::Uuid::new_v4().to_string(),
        module_id: fixture.descriptor.summary.id.clone(),
        install_id: owner.id,
        source_root: fixture.source.clone(),
        phase: "prepared".into(),
        journal_json: "{}".into(),
    };
    crate::program_removals_db::begin(&fixture.paths, &removal)
        .await
        .unwrap();
    crate::program_removals_db::commit(&fixture.paths, &removal)
        .await
        .unwrap();
    let pending = fixture.inspect().await.unwrap();
    assert_eq!(pending.installations.len(), 1);
    assert!(pending.installations[0].pending_removal);
    assert_eq!(
        pending.installations[0].install_state,
        InstallState::NotInstalled
    );
    removal.phase = "committed".into();
    crate::program_removals_db::finish(&fixture.paths, &removal)
        .await
        .unwrap();
    let finished = fixture.inspect().await.unwrap();
    assert!(!finished.installations[0].pending_removal);
    assert_eq!(
        finished.installations[0].install_state,
        InstallState::NotInstalled
    );
}

#[tokio::test]
async fn program_inventory_estimate_excludes_instance_data_and_current_descriptor_exclusions() {
    let mut fixture = Fixture::new().await;
    fs::write(fixture.source.join("server.bin"), [1; 11]).unwrap();
    fs::write(fixture.source.join("later-private.bin"), [2; 17]).unwrap();
    fs::create_dir_all(fixture.source.join("Pal/Saved/Config/WindowsServer")).unwrap();
    fs::write(
        fixture
            .source
            .join("Pal/Saved/Config/WindowsServer/server.ini"),
        [3; 19],
    )
    .unwrap();
    fixture.record_used_package();
    fixture
        .descriptor
        .storage
        .runtime_copy_exclusions
        .push("later-private.bin".into());
    for (relative, length) in [
        ("world.sav", 23),
        ("Mods/custom.dll", 29),
        ("custom.ini", 31),
    ] {
        let path = fixture.source.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![4; length]).unwrap();
    }
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let inventory = fixture.inspect().await.unwrap();
    assert_eq!(inventory.creation.action, "independent_install");
    assert_eq!(inventory.creation.additional_bytes, Some(11));
    let actual_bytes = before
        .values()
        .flatten()
        .map(|bytes| bytes.len() as u64)
        .sum::<u64>();
    assert_eq!(inventory.installations[0].size_bytes, Some(actual_bytes));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        before
    );
}

#[tokio::test]
async fn program_inventory_estimate_is_unknown_without_a_usable_clean_allowlist() {
    for damage in [
        "missing-manifest",
        "foreign-module",
        "missing-file",
        "file-became-directory",
    ] {
        let fixture = Fixture::new().await;
        fs::write(fixture.source.join("server.bin"), [1; 11]).unwrap();
        fixture.record_used_package();
        let manifest = fixture.source.join(".langame-clean-package.json");
        match damage {
            "missing-manifest" => fs::remove_file(&manifest).unwrap(),
            "foreign-module" => {
                let mut content: serde_json::Value =
                    serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
                content["module_id"] = serde_json::json!("other-module");
                fs::write(&manifest, serde_json::to_vec(&content).unwrap()).unwrap();
            }
            _ => {
                fs::remove_file(fixture.source.join("server.bin")).unwrap();
                if damage == "file-became-directory" {
                    fs::create_dir(fixture.source.join("server.bin")).unwrap();
                }
            }
        }
        let inventory = fixture.inspect().await.unwrap();
        assert_eq!(inventory.creation.additional_bytes, None, "{damage}");
    }
}

#[tokio::test]
async fn program_inventory_estimate_preserves_invalid_manifest_and_io_errors() {
    for io_failure in [false, true] {
        let fixture = Fixture::new().await;
        fs::write(fixture.source.join("server.bin"), [1; 11]).unwrap();
        fixture.record_used_package();
        let manifest = fixture.source.join(".langame-clean-package.json");
        if io_failure {
            fs::remove_file(&manifest).unwrap();
            fs::create_dir(&manifest).unwrap();
            let error = fixture.inspect().await.unwrap_err();
            let StorageError::ReadPath { path, .. } = error else {
                panic!("an unreadable clean manifest must preserve its IO error: {error:?}");
            };
            assert_eq!(
                fs::canonicalize(path).unwrap(),
                fs::canonicalize(&manifest).unwrap()
            );
        } else {
            fs::write(&manifest, b"invalid JSON").unwrap();
            let error = fixture.inspect().await.unwrap_err();
            let StorageError::PrivateRuntimeRefresh { path, message } = error else {
                panic!("invalid clean manifests must preserve their package error: {error:?}");
            };
            assert_eq!(
                fs::canonicalize(path).unwrap(),
                fs::canonicalize(&manifest).unwrap()
            );
            assert!(
                message.starts_with("invalid clean package manifest:"),
                "{message}"
            );
        }
    }
}

#[tokio::test]
async fn program_inventory_storage_entries_distinguish_multiple_library_roots() {
    let fixture = Fixture::new().await;
    fs::write(fixture.source.join("server.bin"), [1; 13]).unwrap();
    let repair = fixture.paths.games_root.join("palworld-repair");
    fs::create_dir_all(&repair).unwrap();
    fs::write(repair.join("server.bin"), [2; 17]).unwrap();
    fixture.register(&repair).await;
    let report = crate::scan_storage_usage(
        &fixture.paths,
        "repair-libraries".into(),
        Arc::new(AtomicBool::new(false)),
    )
    .await
    .unwrap();
    assert_eq!(report.status, "complete", "{:?}", report.entries);
    let libraries = report
        .entries
        .iter()
        .filter(|entry| entry.module_id.as_deref() == Some("palworld"))
        .collect::<Vec<_>>();
    assert_eq!(libraries.len(), 2);
    assert_eq!(
        libraries
            .iter()
            .map(|entry| &entry.id)
            .collect::<HashSet<_>>()
            .len(),
        2
    );
    assert_eq!(
        libraries
            .iter()
            .map(|entry| entry.logical_bytes)
            .sum::<u64>(),
        30
    );
    assert_eq!(
        report
            .entries
            .iter()
            .find(|entry| entry.id == "library")
            .unwrap()
            .logical_bytes,
        0
    );
}

#[tokio::test]
async fn program_inventory_estimate_ignores_old_manifest_at_repair_fallback_destination() {
    let fixture = Fixture::new().await;
    fs::write(fixture.source.join("server.bin"), [1; 11]).unwrap();
    fixture.record_used_package();
    let pending = fixture.paths.games_root.join("palworld-pending");
    fs::create_dir_all(&pending).unwrap();
    fixture.register(&pending).await;
    crate::sync_game_installs(
        &fixture.paths,
        &[crate::GameInstallSyncRecord {
            module_id: fixture.descriptor.summary.id.clone(),
            install_root: pending.to_string_lossy().into_owned(),
            install_state: InstallState::Incomplete,
            current_version: None,
            mark_verified: false,
        }],
    )
    .await
    .unwrap();
    let pool = crate::storage_db::connect_pool(&fixture.paths)
        .await
        .unwrap();
    sqlx::query("UPDATE game_installs SET last_verified_at='2000-01-01 00:00:00', updated_at='2000-01-01 00:00:00' WHERE id=(SELECT MIN(id) FROM game_installs)")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let selected = crate::read_library_program_install(&fixture.paths, "palworld")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(selected.install_state, InstallState::Incomplete);
    assert_eq!(
        fs::canonicalize(selected.install_root).unwrap(),
        fs::canonicalize(&pending).unwrap()
    );
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let inventory = inspect_module_programs(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        InstanceProgramSource::Verified,
        Arc::new(AtomicBool::new(false)),
        true,
    )
    .await
    .unwrap();
    assert!(inventory.creation.can_create);
    assert_eq!(
        fs::canonicalize(&inventory.creation.program_path).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert_eq!(inventory.creation.additional_bytes, None);
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        before
    );
}
