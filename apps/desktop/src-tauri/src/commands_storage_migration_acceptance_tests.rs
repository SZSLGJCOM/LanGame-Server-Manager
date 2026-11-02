//! Explicit cross-volume acceptance using disposable roots and production commands.
//! The mock Tauri shell supplies isolated DesktopState; all storage I/O is real.
use super::tests::{preserved_backup, tree_snapshot};
use super::*;
use app_core::InstanceStatus;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn create(variable: &str) -> Self {
        let parent = PathBuf::from(std::env::var_os(variable).expect(variable));
        assert!(parent.is_absolute(), "acceptance parent must be absolute");
        let parent = fs::canonicalize(parent).unwrap();
        for ancestor in parent.ancestors() {
            use std::os::windows::fs::MetadataExt;
            assert_eq!(
                fs::symlink_metadata(ancestor).unwrap().file_attributes() & 0x400,
                0,
                "acceptance parent must not contain reparse points"
            );
        }
        let root = parent.join(format!(
            "lgsm-migration-acceptance-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join(".owned-fixture"), b"lgsm-migration-acceptance|1").unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // Retain a failed fixture for diagnosis, never remove a caller-owned root.
        if std::thread::panicking() {
            eprintln!("Acceptance fixture retained: {}", self.root.display());
            return;
        }
        let name = self.root.file_name().unwrap().to_string_lossy();
        uuid::Uuid::parse_str(name.strip_prefix("lgsm-migration-acceptance-").unwrap()).unwrap();
        assert_eq!(fs::canonicalize(&self.root).unwrap(), self.root);
        assert_eq!(
            fs::read(self.root.join(".owned-fixture")).unwrap(),
            b"lgsm-migration-acceptance|1"
        );
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn app(storage: &app_storage::StorageBootstrap) -> tauri::App<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .manage(DesktopState::from_storage(storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap()
}

async fn settled(app: &tauri::App<tauri::test::MockRuntime>) -> StorageMigrationStatus {
    tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            let status = read_app_storage_migration(app.state::<DesktopState>())
                .await
                .unwrap()
                .unwrap();
            if !status.is_active() {
                return status;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cross-volume acceptance must settle")
}

async fn create_archive(paths: &StoragePaths) -> app_core::InstanceArchiveResult {
    let root = paths.instances_root.join("archived-one");
    for leaf in ["config", "data", "logs", "saves", "runtime"] {
        fs::create_dir_all(root.join(leaf)).unwrap();
    }
    fs::write(root.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
    fs::write(
        root.join("runtime/server.exe"),
        b"preserved program payload",
    )
    .unwrap();
    fs::write(root.join("saves/world.dat"), b"preserved archive world").unwrap();
    fs::write(
        root.join("config/instance.json"),
        serde_json::to_vec(&serde_json::json!({
            "instance_id": "archived-one", "module_id": "fixture", "instance_name": "Archived one",
            "autostart": false, "settings": {}, "ports": []
        }))
        .unwrap(),
    )
    .unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&paths.database_path))
        .await
        .unwrap();
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('fixture','Fixture','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('archived-one','Archived one','fixture',?1,?2,?3,?4)")
        .bind(root.join("data").to_string_lossy().as_ref())
        .bind(root.join("config").to_string_lossy().as_ref())
        .bind(root.join("logs").to_string_lossy().as_ref())
        .bind(root.join("saves").to_string_lossy().as_ref())
        .execute(&pool).await.unwrap();
    let install = sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('fixture',?1,'installed','instance','archived-one')")
        .bind(root.join("runtime").to_string_lossy().as_ref())
        .execute(&pool).await.unwrap().last_insert_rowid();
    sqlx::query("UPDATE instances SET install_id=?1 WHERE id='archived-one'")
        .bind(install)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    app_storage::archive_instance(paths, "archived-one")
        .await
        .unwrap()
}

async fn create_stopped_instance(
    paths: &StoragePaths,
) -> (
    app_core::InstanceProvisioning,
    app_core::InstanceBackupResult,
    Vec<u8>,
) {
    let descriptor = app_modules::discover_modules(&paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|module| module.summary.id == "fixture")
        .unwrap();
    app_storage::sync_modules(paths, std::slice::from_ref(&descriptor))
        .await
        .unwrap();
    let package = paths.games_root.join("fixture");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("server.exe"), b"stopped instance program").unwrap();
    let instance = app_storage::create_instance(
        paths,
        &descriptor,
        app_core::CreateInstanceInput {
            name: "Retained stopped instance".into(),
            module_id: "fixture".into(),
        },
    )
    .await
    .unwrap();
    let root = paths.instances_root.join(&instance.summary.id);
    let details = app_storage::read_instance_details(paths, &instance.summary.id)
        .await
        .unwrap();
    assert!(matches!(details.summary.status, InstanceStatus::Stopped));
    assert!(details.active_run.is_none());
    let world = Path::new(&details.saves_path).join("world.dat");
    fs::write(&world, b"backed-up instance world").unwrap();
    fs::write(
        root.join("logs/server.log"),
        b"retained stopped instance log\n",
    )
    .unwrap();
    let backup = app_storage::create_instance_backup(paths, &instance.summary.id)
        .await
        .unwrap();
    fs::write(&world, b"current instance world").unwrap();
    let config = fs::read(&instance.config_file_path).unwrap();
    (instance, backup, config)
}

async fn assert_registered_paths(paths: &StoragePaths, id: &str, root: &Path) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&paths.database_path)
                .read_only(true),
        )
        .await
        .unwrap();
    let stored: (String, String, String, String, String) = sqlx::query_as(
        "SELECT i.config_path,i.data_path,i.logs_path,i.saves_path,g.install_root FROM instances i JOIN game_installs g ON g.id=i.install_id WHERE i.id=?1"
    ).bind(id).fetch_one(&pool).await.unwrap();
    for (path, leaf) in [
        (&stored.0, "config"),
        (&stored.1, "data"),
        (&stored.2, "logs"),
        (&stored.3, "saves"),
        (&stored.4, "runtime"),
    ] {
        assert_eq!(Path::new(path), root.join(leaf));
    }
    pool.close().await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicit disposable source/destination parents on different Windows volumes"]
async fn native_commands_migrate_between_explicit_test_volumes() {
    migrate_between_test_volumes(false).await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires explicit disposable source/destination parents on different Windows volumes"]
async fn native_commands_migrate_external_archives_with_the_workspace() {
    migrate_between_test_volumes(true).await;
}

async fn migrate_between_test_volumes(external_archives: bool) {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let source = Scratch::create("LANGAME_MIGRATION_SOURCE_TEST_ROOT");
    let destination = Scratch::create("LANGAME_MIGRATION_DESTINATION_TEST_ROOT");
    assert_ne!(
        source.root.components().next(),
        destination.root.components().next(),
        "cross-volume acceptance must use different drive roots"
    );
    let defaults = StoragePaths::default();
    let state_root = source.root.join("state");
    let paths = StoragePaths {
        app_data_root: state_root.clone(),
        settings_path: state_root.join("settings.json"),
        database_path: state_root.join("db/lgs.db"),
        logs_root: state_root.join("logs"),
        modules_root: source.root.join("modules"),
        migrations_root: defaults.migrations_root,
        instances_root: source.root.join("instances"),
        archives_root: source.root.join(if external_archives {
            "external-archives"
        } else {
            "instances/.trash"
        }),
        games_root: source.root.join("games"),
        steamcmd_root: source.root.join("steamcmd"),
    };
    source.write(
        "modules/fixture/module.toml",
        b"id='fixture'\nname='Fixture'\nversion='1'\n",
    );
    let mut storage = app_storage::bootstrap_storage_with_paths(paths.clone()).unwrap();
    storage.storage_status = app_storage::initialize_database(&storage.paths)
        .await
        .unwrap();
    app_storage::save_app_settings_with_paths(storage.settings.clone(), &paths).unwrap();
    let original_settings = fs::read(&paths.settings_path).unwrap();
    let archived = create_archive(&paths).await;
    let (retained, instance_backup, managed_config) = create_stopped_instance(&paths).await;
    let app = app(&storage);
    let payload: Vec<u8> = (0..1024 * 1024 + 37)
        .map(|index| (index % 251) as u8)
        .collect();
    let payloads: [(&str, &[u8]); 4] = [
        ("instances/alpha/世界 存档/world.bin", &payload),
        (
            "instances/identical.bin",
            b"identical payload is also preserved",
        ),
        ("games/program/data.bin", b"program library content"),
        ("steamcmd/fixture-data.bin", b"tool directory content"),
    ];
    for (relative, bytes) in &payloads {
        source.write(relative, bytes);
    }
    let outer_roots = ["instances", "games", "steamcmd"];
    for root in outer_roots {
        source.write(&format!("{root}/conflict.bin"), b"source version");
        destination.write(&format!("{root}/conflict.bin"), b"destination version");
        source.write(&format!("{root}/identical.bin"), payloads[1].1);
        destination.write(&format!("{root}/identical.bin"), payloads[1].1);
        destination.write(&format!("{root}/already-here.bin"), b"keep target data");
        fs::create_dir_all(source.root.join(root).join("source-empty")).unwrap();
        fs::create_dir_all(destination.root.join(root).join("destination-empty")).unwrap();
    }
    assert!(paths.instances_root.join(".langame/locks").is_dir());
    destination.write(
        "instances/.trash/previous-archive.bin",
        b"preserve old target archives",
    );
    let archive_tree = tree_snapshot(&paths.archives_root);
    let source_trees: Vec<_> = outer_roots
        .iter()
        .map(|root| tree_snapshot(&source.root.join(root)))
        .collect();
    let target_trees: Vec<_> = outer_roots
        .iter()
        .map(|root| tree_snapshot(&destination.root.join(root)))
        .collect();
    let old_roots = [
        &paths.instances_root,
        &paths.archives_root,
        &paths.games_root,
        &paths.steamcmd_root,
    ];
    let input = AppPathSettingsInput {
        servers_root: destination
            .root
            .join("instances")
            .to_string_lossy()
            .into_owned(),
        archives_root: destination
            .root
            .join("instances/.trash")
            .to_string_lossy()
            .into_owned(),
        games_root: destination
            .root
            .join("games")
            .to_string_lossy()
            .into_owned(),
        steamcmd_root: destination
            .root
            .join("steamcmd")
            .to_string_lossy()
            .into_owned(),
    };
    let checking = preview_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        input.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&checking.input).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    let ready = settled(&app).await;
    assert_eq!(ready.phase, Phase::Ready, "{:?}", ready.error);
    assert_eq!(ready.conflict_count, 0);
    assert_eq!(
        serde_json::to_value(&ready.input).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    assert_eq!(tree_snapshot(&paths.archives_root), archive_tree);
    assert!(
        ready
            .roots
            .iter()
            .any(|root| Path::new(&root.source) == paths.archives_root
                && root.target == input.archives_root)
    );
    for (index, root) in outer_roots.iter().enumerate() {
        assert_eq!(tree_snapshot(&source.root.join(root)), source_trees[index]);
        assert_eq!(
            tree_snapshot(&destination.root.join(root)),
            target_trees[index]
        );
    }
    assert_eq!(fs::read(&paths.settings_path).unwrap(), original_settings);
    assert!(!destination.root.join(payloads[0].0).exists());
    execute_app_storage_migration(
        app.handle().clone(),
        app.state::<DesktopState>(),
        ready.operation_id,
    )
    .await
    .unwrap();
    let completed = settled(&app).await;
    assert_eq!(completed.phase, Phase::Completed, "{:?}", completed.error);
    assert_eq!(
        serde_json::to_value(&completed.input).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    assert!(
        completed.cleanup_warnings.is_empty(),
        "{:?}",
        completed.cleanup_warnings
    );
    for (relative, bytes) in &payloads {
        assert_eq!(fs::read(destination.root.join(relative)).unwrap(), *bytes);
        assert!(!source.root.join(relative).exists());
    }
    assert_eq!(completed.preserved_destinations.len(), outer_roots.len());
    for (index, root) in outer_roots.iter().enumerate() {
        let target = destination.root.join(root);
        let backup = preserved_backup(&completed, &target, &destination.root);
        assert_eq!(tree_snapshot(&backup), target_trees[index]);
        let mut expected = source_trees[index].clone();
        if *root == "instances" {
            if external_archives {
                expected.extend(
                    archive_tree.iter().map(|(relative, bytes)| {
                        (Path::new(".trash").join(relative), bytes.clone())
                    }),
                );
            }
            let relative = Path::new(&instance_backup.backup_path)
                .strip_prefix(&paths.instances_root)
                .unwrap()
                .join("backup.json");
            let mut manifest: serde_json::Value =
                serde_json::from_slice(expected[&relative].as_ref().unwrap()).unwrap();
            manifest["backup_path"] = serde_json::json!(target.join(relative.parent().unwrap()));
            manifest["saves_path"] =
                serde_json::json!(target.join(&retained.summary.id).join("saves"));
            let bytes = fs::read(target.join(&relative)).unwrap();
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
                manifest
            );
            expected.insert(relative, Some(bytes));
        }
        assert_eq!(tree_snapshot(&target), expected);
        assert_eq!(
            fs::read(target.join("conflict.bin")).unwrap(),
            b"source version"
        );
        assert!(!target.join("already-here.bin").exists());
        assert!(!target.join("destination-empty").exists());
    }
    for root in old_roots {
        assert!(
            !root.exists(),
            "source root must be removed: {}",
            root.display()
        );
    }
    let reopened = app_storage::bootstrap_storage_with_paths(paths.clone()).unwrap();
    assert_eq!(
        reopened.paths.instances_root,
        Path::new(&input.servers_root)
    );
    assert_eq!(
        reopened.paths.archives_root,
        Path::new(&input.archives_root)
    );
    assert_eq!(reopened.paths.games_root, Path::new(&input.games_root));
    assert_eq!(
        reopened.paths.steamcmd_root,
        Path::new(&input.steamcmd_root)
    );
    let retained_root = reopened.paths.instances_root.join(&retained.summary.id);
    let details = app_storage::read_instance_details(&reopened.paths, &retained.summary.id)
        .await
        .unwrap();
    assert_eq!(details.summary.id, retained.summary.id);
    assert!(matches!(details.summary.status, InstanceStatus::Stopped));
    assert_eq!(details.summary.active_process_count, 0);
    assert!(details.active_run.is_none());
    assert_eq!(
        Path::new(&details.config_file_path),
        retained_root.join("config/instance.json")
    );
    assert_eq!(Path::new(&details.saves_path), retained_root.join("saves"));
    assert_eq!(fs::read(&details.config_file_path).unwrap(), managed_config);
    assert_registered_paths(&reopened.paths, &retained.summary.id, &retained_root).await;
    let backups = app_storage::list_instance_backups(&reopened.paths, &retained.summary.id)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].backup_id, instance_backup.backup_id);
    assert_eq!(backups[0].instance_id, retained.summary.id);
    assert_eq!(
        Path::new(&backups[0].backup_path),
        retained_root
            .join("backups")
            .join(&instance_backup.backup_id)
    );
    assert_eq!(backups[0].saves_path, details.saves_path);
    assert_eq!(
        fs::read(Path::new(&backups[0].backup_path).join("saves/world.dat")).unwrap(),
        b"backed-up instance world"
    );
    assert_eq!(
        fs::read(retained_root.join("saves/world.dat")).unwrap(),
        b"current instance world"
    );
    assert_eq!(
        fs::read(retained_root.join("logs/server.log")).unwrap(),
        b"retained stopped instance log\n"
    );
    assert_eq!(
        fs::read(retained_root.join("runtime/server.exe")).unwrap(),
        b"stopped instance program"
    );
    assert!(!Path::new(archived.archived_instance_root.as_ref().unwrap()).exists());
    let listed = app_storage::list_instance_archives(&reopened.paths)
        .await
        .unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert!(
        listed.archives[0].can_restore,
        "{:?}",
        listed.archives[0].issues
    );
    let restored = app_storage::restore_instance_archive(&reopened.paths, &archived.archive_id)
        .await
        .unwrap();
    let restored_root = Path::new(&restored.restored_instance_root);
    assert_eq!(
        restored_root,
        reopened.paths.instances_root.join("archived-one")
    );
    assert_eq!(
        fs::read(restored_root.join("saves/world.dat")).unwrap(),
        b"preserved archive world"
    );
    assert_eq!(
        fs::read(restored_root.join("runtime/server.exe")).unwrap(),
        b"preserved program payload"
    );
    assert_registered_paths(&reopened.paths, "archived-one", restored_root).await;
    let reopened_app = self::app(&reopened);
    let persisted = read_app_storage_migration(reopened_app.state::<DesktopState>())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.phase, Phase::Completed);
    assert_eq!(
        serde_json::to_value(&persisted.input).unwrap(),
        serde_json::to_value(&input).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&persisted.preserved_destinations).unwrap(),
        serde_json::to_value(&completed.preserved_destinations).unwrap()
    );
    for root in old_roots {
        assert!(
            !root.exists(),
            "reopen/restore must not recreate the source root"
        );
    }
    println!(
        "{}",
        serde_json::json!({
            "acceptance": "cross-volume-native-storage-migration", "result": "passed",
            "source": source.root, "destination": destination.root,
            "migrated_files": completed.total_files, "reused_files": completed.reused_files,
            "whole_target_preserved": true, "target_contains_only_source": true,
            "old_source_roots_removed": true, "reopened_settings_verified": true,
            "migrated_archive_restored": true,
            "external_archives_followed_workspace": external_archives,
            "stopped_registered_instance_preserved": true, "instance_backup_readable": true,
            "managed_configuration_and_logs_preserved": true,
            "operation_id": completed.operation_id
        })
    );
}
