use super::*;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app_core::{CreateInstanceInput, InstallState};
use app_modules::ModuleDescriptor;

use crate::instance_archive::inventory_lock;
use crate::instance_creation_io::test_gate::{PausePoint, pause_at};
use crate::storage_db::connect_pool;
use crate::test_file_snapshot::tree_snapshot;
use crate::{GameInstallSyncRecord, InstanceCreationOptions};

#[path = "archived_program_read_lock_tests.rs"]
mod read_locks;

#[path = "archived_program_seed_concurrency_tests.rs"]
mod seed_concurrency;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    descriptor: ModuleDescriptor,
    archive: PathBuf,
    archive_id: String,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("archived-program-{}", uuid::Uuid::new_v4()));
        let paths = fixture_paths(&root);
        crate::bootstrap_storage_with_paths(paths.clone()).unwrap();
        let module = paths.modules_root.join("fixture");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.toml"),
            r#"
id = "fixture"
name = "Archived program fixture"
version = "1.0.0"
[install]
shared_game_dir = "fixture"
[process]
executable = "server.bin"
[storage]
runtime_copy_exclusions = ["Saved", "mods", "user.cfg"]
saves_path_template = "{{paths.install_root}}/Saved/{{instance.id}}"
"#,
        )
        .unwrap();
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .unwrap()
            .remove(0);
        crate::initialize_database(&paths).await.unwrap();
        crate::sync_modules(&paths, std::slice::from_ref(&descriptor))
            .await
            .unwrap();
        let library = paths.games_root.join("fixture");
        fs::create_dir_all(library.join("assets")).unwrap();
        fs::write(library.join("server.bin"), b"official executable").unwrap();
        fs::write(library.join("assets/content.bin"), b"official content").unwrap();
        crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
        crate::sync_game_installs(
            &paths,
            &[GameInstallSyncRecord {
                module_id: "fixture".into(),
                install_root: library.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some("build-17".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();
        let created = crate::create_instance_with_options(
            &paths,
            &descriptor,
            CreateInstanceInput {
                name: "Preserved server".into(),
                module_id: "fixture".into(),
            },
            InstanceCreationOptions {
                require_clean_program: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(library.exists());
        // Simulate an explicitly uninstalled library; this fixture exercises an
        // intact archive as the only remaining program source.
        fs::remove_dir_all(&library).unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("DELETE FROM game_installs WHERE module_id='fixture' AND scope='library'")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        let details = crate::read_instance_details(&paths, &created.provisioning.summary.id)
            .await
            .unwrap();
        fs::create_dir_all(&details.saves_path).unwrap();
        fs::write(
            Path::new(&details.saves_path).join("world.sav"),
            b"original player world",
        )
        .unwrap();
        fs::create_dir_all(created.effective_install_root.join("mods")).unwrap();
        fs::write(
            created.effective_install_root.join("mods/custom.dll"),
            b"operator modification",
        )
        .unwrap();
        let deleted = crate::archive_instance(&paths, &created.provisioning.summary.id)
            .await
            .unwrap();
        let archive = PathBuf::from(deleted.archived_instance_root.unwrap());
        let archive_id = archive.file_name().unwrap().to_str().unwrap().to_owned();
        Self {
            root,
            paths,
            descriptor,
            archive,
            archive_id,
        }
    }

    async fn snapshot_json(&self) -> String {
        let pool = connect_pool(&self.paths).await.unwrap();
        let snapshot =
            sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
                .bind(&self.archive_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        pool.close().await;
        snapshot
    }

    async fn replace_snapshot(&self, snapshot: &str) {
        let pool = connect_pool(&self.paths).await.unwrap();
        sqlx::query(
            "UPDATE instance_archives SET snapshot_json=?2,snapshot_sha256=?3 WHERE archive_id=?1",
        )
        .bind(&self.archive_id)
        .bind(snapshot)
        .bind(store::digest(snapshot.as_bytes()))
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn fixture_paths(root: &Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/test.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("library"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

#[tokio::test]
async fn archived_only_program_seeds_without_acquisition_and_preserves_every_archive_byte() {
    let fixture = Fixture::new().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let metadata = fixture.snapshot_json().await;
    assert!(
        crate::read_module_instance_installs(&fixture.paths, "fixture")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        crate::read_library_program_install(&fixture.paths, "fixture")
            .await
            .unwrap()
            .is_none()
    );
    let sources = read_archived_program_sources(&fixture.paths).await.unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].module_id, "fixture");
    assert_eq!(sources[0].archive_id, fixture.archive_id);
    assert_eq!(sources[0].install_root, fixture.archive.join("runtime"));
    assert_eq!(sources[0].current_version.as_deref(), Some("build-17"));
    let seed = crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None)
        .await
        .unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("build-17"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"official executable"
    );
    assert_eq!(
        fs::read(seed.install_root.join("assets/content.bin")).unwrap(),
        b"official content"
    );
    assert!(!seed.install_root.join("mods").exists());
    assert!(!seed.install_root.join("Saved").exists());
    assert!(
        crate::library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap()
    );
    assert!(
        !crate::library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor)
            .unwrap()
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
    assert_eq!(fixture.snapshot_json().await, metadata);
}

#[tokio::test]
async fn changed_archived_payload_is_never_certified_and_its_original_bytes_are_retained() {
    let fixture = Fixture::new().await;
    fs::write(
        fixture.archive.join("runtime/server.bin"),
        b"custom patched executable",
    )
    .unwrap();
    let before = tree_snapshot(&fixture.archive).unwrap();
    let seed = crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None)
        .await
        .unwrap();
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    assert!(!seed.install_root.join("server.bin").exists());
    assert_eq!(
        fs::read(seed.install_root.join("assets/content.bin")).unwrap(),
        b"official content"
    );
    assert!(
        !crate::library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap()
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
}

#[tokio::test]
async fn archived_program_without_an_official_manifest_remains_unproven() {
    let fixture = Fixture::new().await;
    fs::remove_file(fixture.archive.join("runtime/.langame-clean-package.json")).unwrap();
    let before = tree_snapshot(&fixture.archive).unwrap();
    let seed = crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None)
        .await
        .unwrap();
    assert!(seed.requires_validation);
    assert!(seed.current_version.is_none());
    assert!(!seed.install_root.join("server.bin").exists());
    assert!(
        !seed
            .install_root
            .join(".langame-clean-package.json")
            .exists()
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
}

#[tokio::test]
async fn a_live_independent_program_keeps_priority_over_an_archived_build() {
    let fixture = Fixture::new().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let library = fixture.paths.games_root.join("fixture");
    fs::create_dir_all(&library).unwrap();
    fs::write(library.join("server.bin"), b"newer live program").unwrap();
    crate::record_library_program_baseline(&library, &fixture.descriptor, true, None).unwrap();
    crate::sync_game_installs(
        &fixture.paths,
        &[GameInstallSyncRecord {
            module_id: "fixture".into(),
            install_root: library.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("build-18".into()),
            mark_verified: true,
        }],
    )
    .await
    .unwrap();
    let created = crate::create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "Current server".into(),
            module_id: "fixture".into(),
        },
        InstanceCreationOptions {
            require_clean_program: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let live_before = tree_snapshot(&created.effective_install_root).unwrap();
    fs::remove_dir_all(&library).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("DELETE FROM game_installs WHERE module_id='fixture' AND scope='library'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    // A usable private source must not touch the unrelated archive catalog.
    let _inventory = inventory_lock(&fixture.paths).unwrap();
    let seed = crate::prepare_clean_library_seed(&fixture.paths, &fixture.descriptor, None)
        .await
        .unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(seed.current_version.as_deref(), Some("build-18"));
    assert_eq!(
        fs::read(seed.install_root.join("server.bin")).unwrap(),
        b"newer live program"
    );
    assert_eq!(
        tree_snapshot(&created.effective_install_root).unwrap(),
        live_before
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
}

#[tokio::test]
async fn archived_sources_reject_corrupt_metadata_scope_owner_module_and_runtime_paths() {
    let fixture = Fixture::new().await;
    let original = fixture.snapshot_json().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    fixture.replace_snapshot("{}").await;
    assert!(
        read_archived_program_sources(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    for (table, field, value) in [
        ("game_installs", "scope", serde_json::json!("library")),
        (
            "game_installs",
            "owner_instance_id",
            serde_json::json!("another-instance"),
        ),
        (
            "game_installs",
            "module_id",
            serde_json::json!("another-module"),
        ),
        (
            "game_installs",
            "install_root",
            serde_json::json!(fixture.root.join("unowned/runtime").to_string_lossy()),
        ),
        (
            "game_installs",
            "install_state",
            serde_json::json!("incomplete"),
        ),
        ("instances", "runtime_mode", serde_json::json!("shared")),
        (
            "instances",
            "config_path",
            serde_json::json!(fixture.root.join("unowned/config").to_string_lossy()),
        ),
    ] {
        let mut snapshot: serde_json::Value = serde_json::from_str(&original).unwrap();
        snapshot["tables"][table][0][field] = value;
        fixture
            .replace_snapshot(&serde_json::to_string(&snapshot).unwrap())
            .await;
        assert!(
            read_archived_program_sources(&fixture.paths)
                .await
                .unwrap()
                .is_empty(),
            "accepted {table}.{field}"
        );
    }
    fixture.replace_snapshot(&original).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_sha256='changed' WHERE archive_id=?1")
        .bind(&fixture.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        read_archived_program_sources(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    fixture.replace_snapshot(&original).await;
    assert_eq!(
        read_archived_program_sources(&fixture.paths)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
}

#[tokio::test]
async fn only_committed_archives_are_program_sources() {
    let fixture = Fixture::new().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for state in [
        "archiving",
        "restoring",
        "restored",
        "purging",
        "purged",
        "missing_metadata",
        "unrecognized",
    ] {
        sqlx::query("UPDATE instance_archives SET state=?2 WHERE archive_id=?1")
            .bind(&fixture.archive_id)
            .bind(state)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            read_archived_program_sources(&fixture.paths)
                .await
                .unwrap()
                .is_empty(),
            "accepted {state}"
        );
    }
    pool.close().await;
    assert!(fixture.archive.join("runtime/server.bin").is_file());
}

#[tokio::test]
async fn replaced_archive_directory_is_not_a_program_source() {
    let fixture = Fixture::new().await;
    let held = fixture.root.join("preserved-original");
    fs::rename(&fixture.archive, &held).unwrap();
    fs::create_dir_all(fixture.archive.join("config")).unwrap();
    fs::create_dir_all(fixture.archive.join("runtime")).unwrap();
    fs::copy(
        held.join("config/instance.json"),
        fixture.archive.join("config/instance.json"),
    )
    .unwrap();
    fs::write(
        fixture.archive.join("runtime/.langame-private-runtime"),
        b"managed\n",
    )
    .unwrap();
    fs::write(
        fixture.archive.join("runtime/server.bin"),
        b"replacement payload",
    )
    .unwrap();
    let before = tree_snapshot(&held).unwrap();
    assert!(
        read_archived_program_sources(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(tree_snapshot(&held).unwrap(), before);
    assert_eq!(
        fs::read(fixture.archive.join("runtime/server.bin")).unwrap(),
        b"replacement payload"
    );
}

#[tokio::test]
async fn archive_source_lease_survives_cancellation_of_a_seed_waiter() {
    let fixture = Fixture::new().await;
    let before = tree_snapshot(&fixture.archive).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let mut pause = pause_at(&cancellation, PausePoint::Copy);
    let paths = fixture.paths.clone();
    let descriptor = fixture.descriptor.clone();
    let token = Arc::clone(&cancellation);
    let worker = tokio::spawn(async move {
        crate::prepare_clean_library_seed(&paths, &descriptor, Some(token)).await
    });
    pause.reached().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(cancellation.load(Ordering::Acquire));
    drop(inventory_lock(&fixture.paths).unwrap());
    assert!(matches!(
        crate::instance_archive::program_source_mutation_lock(&fixture.paths, &fixture.archive_id),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    drop(pause);
    let lease = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            match crate::instance_archive::program_source_mutation_lock(
                &fixture.paths,
                &fixture.archive_id,
            ) {
                Ok(lease) => break lease,
                Err(StorageError::InstanceSettingsLocked { .. }) => tokio::task::yield_now().await,
                Err(error) => panic!("unexpected archive lock failure: {error}"),
            }
        }
    })
    .await
    .expect("cancelled seed did not release its archive lease");
    assert!(!fixture.paths.games_root.join("fixture").exists());
    assert!(
        fs::read_dir(&fixture.paths.games_root)
            .unwrap()
            .next()
            .is_none()
    );
    assert_eq!(tree_snapshot(&fixture.archive).unwrap(), before);
    drop(lease);
}

#[tokio::test]
async fn missing_archive_table_is_read_without_migrating_or_reconstructing_directories() {
    let root = std::env::temp_dir().join(format!(
        "archived-program-old-schema-{}",
        uuid::Uuid::new_v4()
    ));
    let paths = fixture_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql(include_str!("../../../migrations/0001_initial.sql"))
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/0002_program_ownership.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    connection.close().await.unwrap();
    let unknown = paths.instances_root.join(".trash/older-files");
    fs::create_dir_all(&unknown).unwrap();
    fs::write(unknown.join("world.sav"), b"unregistered old world").unwrap();
    assert!(
        read_archived_program_sources(&paths)
            .await
            .unwrap()
            .is_empty()
    );
    let mut connection = SqliteConnection::connect_with(&options.read_only(true))
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert_eq!(version, 2);
    let has_archives: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='instance_archives')",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert!(!has_archives);
    connection.close().await.unwrap();
    assert_eq!(
        fs::read(unknown.join("world.sav")).unwrap(),
        b"unregistered old world"
    );
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(&root).unwrap();
}
