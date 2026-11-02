//! External archives join the selected workspace without merging archive owners.
use super::*;
use crate::storage_migration as migration;
use app_core::{AppSettings, StorageMigrationPhase};
use std::sync::{Arc, atomic::AtomicBool};

fn external_archive_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.paths.archives_root = fixture.root.join("external-archives");
    fs::create_dir_all(&fixture.paths.archives_root).unwrap();
    fixture
}

fn workspace_request(fixture: &Fixture) -> (AppSettings, PathBuf) {
    let target = fixture.root.join("moved-workspace");
    let mut requested = fixture.paths.settings();
    requested.servers_root = target.to_string_lossy().into_owned();
    requested.archives_root = target.join(".trash").to_string_lossy().into_owned();
    (requested, target)
}

async fn preview(
    fixture: &Fixture,
    requested: AppSettings,
) -> Result<migration::PreparedMigration, StorageError> {
    migration::preview(
        &fixture.paths,
        requested,
        uuid::Uuid::new_v4().to_string(),
        Vec::new(),
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
}

async fn execute(
    fixture: &Fixture,
    prepared: migration::PreparedMigration,
) -> app_core::StorageMigrationStatus {
    let completed = migration::execute(
        &fixture.paths,
        prepared,
        Arc::new(AtomicBool::new(false)),
        Arc::new(|_| {}),
    )
    .await
    .unwrap();
    assert_eq!(completed.phase, StorageMigrationPhase::Completed);
    assert!(
        completed.cleanup_warnings.is_empty(),
        "{:?}",
        completed.cleanup_warnings
    );
    assert!(!fixture.paths.instances_root.exists());
    assert!(!fixture.paths.archives_root.exists());
    completed
}

fn persisted_paths(fixture: &Fixture, target: &Path) -> StoragePaths {
    let persisted = crate::settings::load_persisted_app_settings(&fixture.paths)
        .unwrap()
        .unwrap();
    let next = fixture.paths.with_app_settings(&persisted);
    assert_eq!(next.instances_root, target);
    assert_eq!(next.archives_root, target.join(".trash"));
    assert_eq!(next.games_root, fixture.paths.games_root);
    assert_eq!(next.steamcmd_root, fixture.paths.steamcmd_root);
    next
}

#[tokio::test]
async fn workspace_migration_collects_external_archive_preserves_old_target_and_restores() {
    let fixture = external_archive_fixture();
    let (pool, id, archived) = archived_fixture(&fixture).await;
    let historical = fixture.paths.instances_root.join(".trash/previous-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"historical world").unwrap();
    fs::write(
        fixture.paths.instances_root.join("workspace.txt"),
        b"workspace contents",
    )
    .unwrap();
    let (requested, target) = workspace_request(&fixture);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join(".trash"), b"old destination archive-name file").unwrap();
    fs::write(target.join("previous.txt"), b"old destination data").unwrap();

    let prepared = preview(&fixture, requested).await.unwrap();
    assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
    assert_eq!(prepared.status().conflict_count, 0);
    assert!(archived.is_dir());
    assert_eq!(
        fs::read(target.join(".trash")).unwrap(),
        b"old destination archive-name file"
    );
    let completed = execute(&fixture, prepared).await;
    assert_eq!(completed.preserved_destinations.len(), 1);
    let backup = PathBuf::from(&completed.preserved_destinations[0].backup_path);
    assert_eq!(
        fs::read(backup.join(".trash")).unwrap(),
        b"old destination archive-name file"
    );
    assert_eq!(
        fs::read(backup.join("previous.txt")).unwrap(),
        b"old destination data"
    );
    assert!(!target.join("previous.txt").exists());
    let next = persisted_paths(&fixture, &target);
    assert_eq!(
        fs::read(target.join("workspace.txt")).unwrap(),
        b"workspace contents"
    );
    assert_eq!(
        fs::read(next.archives_root.join("previous-world/world.dat")).unwrap(),
        b"historical world"
    );
    assert_eq!(
        fs::read(next.archives_root.join(&id).join("saves/world.dat")).unwrap(),
        b"preserved world payload"
    );

    let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id=?1")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let migrated = store::map_archive(&row).unwrap();
    crate::instance_archive::validate_archived_files(&next, &migrated).unwrap();
    assert_eq!(
        Path::new(migrated.original_root.as_deref().unwrap()),
        target.join("one")
    );
    let restored = crate::restore_instance_archive(&next, &id).await.unwrap();
    assert_eq!(
        Path::new(&restored.restored_instance_root),
        target.join("one")
    );
    assert_eq!(
        fs::read(target.join("one/saves/world.dat")).unwrap(),
        b"preserved world payload"
    );
    assert_eq!(
        fs::read(target.join("one/runtime/server.exe")).unwrap(),
        b"preserved server payload"
    );
    let (config, install): (String, String) = sqlx::query_as(
        "SELECT i.config_path,g.install_root FROM instances i JOIN game_installs g ON g.id=i.install_id WHERE i.id='one'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(Path::new(&config), target.join("one/config"));
    assert_eq!(Path::new(&install), target.join("one/runtime"));
    let list = crate::list_instance_archives(&next).await.unwrap();
    assert_eq!(list.archives.len(), 1);
    assert_eq!(
        list.archives[0].state,
        crate::InstanceArchiveState::MissingMetadata
    );
    assert!(!list.archives[0].can_restore);
    assert_eq!(
        Path::new(&list.archives[0].archived_instance_root),
        next.archives_root.join("previous-world")
    );
    assert_eq!(
        fs::read(next.archives_root.join("previous-world/world.dat")).unwrap(),
        b"historical world"
    );
    pool.close().await;
}

#[tokio::test]
async fn workspace_migration_moves_empty_external_archive_with_default_history() {
    let fixture = external_archive_fixture();
    let historical = fixture.paths.instances_root.join(".trash/previous-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"only historical world").unwrap();
    let (requested, target) = workspace_request(&fixture);
    let prepared = preview(&fixture, requested).await.unwrap();
    assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
    assert!(!target.exists());
    let completed = execute(&fixture, prepared).await;
    assert!(completed.preserved_destinations.is_empty());
    let next = persisted_paths(&fixture, &target);
    assert_eq!(
        fs::read(next.archives_root.join("previous-world/world.dat")).unwrap(),
        b"only historical world"
    );
    assert_eq!(fs::read_dir(&next.archives_root).unwrap().count(), 1);
}

async fn assert_archive_leaf_collision_rejected(case_alias: bool) {
    let fixture = external_archive_fixture();
    let (pool, id, archived) = archived_fixture(&fixture).await;
    let leaf = if case_alias {
        id.to_uppercase()
    } else {
        id.clone()
    };
    let historical = fixture.paths.instances_root.join(".trash").join(leaf);
    fs::create_dir_all(&historical).unwrap();
    // No relative filename collides. Ordinary file-level conflict detection
    // would silently merge this unrelated payload into the registered archive.
    fs::write(historical.join("unrelated.dat"), b"another archive owner").unwrap();
    let snapshot: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let (requested, target) = workspace_request(&fixture);
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("untouched.txt"), b"old destination").unwrap();
    let error = match preview(&fixture, requested).await {
        Ok(prepared) => panic!(
            "Distinct archive owners must not merge: phase {:?}, file conflicts {}",
            prepared.status().phase,
            prepared.status().conflict_count,
        ),
        Err(error) => error,
    };
    assert!(error.to_string().contains("same archive name"), "{error}");
    assert_eq!(
        fs::read(historical.join("unrelated.dat")).unwrap(),
        b"another archive owner"
    );
    assert_eq!(
        fs::read(archived.join("saves/world.dat")).unwrap(),
        b"preserved world payload"
    );
    assert_eq!(
        fs::read(target.join("untouched.txt")).unwrap(),
        b"old destination"
    );
    assert!(!target.join(".trash").exists());
    assert!(!fixture.paths.settings_path.exists());
    let unchanged: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unchanged, snapshot);
    pool.close().await;
}

#[tokio::test]
async fn workspace_migration_rejects_same_archive_leaf_without_file_conflicts() {
    assert_archive_leaf_collision_rejected(false).await;
}

#[cfg(windows)]
#[tokio::test]
async fn workspace_migration_rejects_case_aliased_archive_leaf_without_file_conflicts() {
    assert_archive_leaf_collision_rejected(true).await;
}

#[tokio::test]
async fn steamcmd_migration_keeps_dormant_default_history_separate_from_external_archive() {
    let fixture = external_archive_fixture();
    let (pool, id, _) = archived_fixture(&fixture).await;
    let historical = fixture.paths.instances_root.join(".trash").join(&id);
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("unrelated.dat"), b"dormant history").unwrap();
    let mut requested = fixture.paths.settings();
    let target = fixture.root.join("moved-steamcmd");
    requested.steamcmd_root = target.to_string_lossy().into_owned();
    let prepared = preview(&fixture, requested).await.unwrap();
    assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
    assert_eq!(prepared.status().conflict_count, 0);
    assert_eq!(
        fs::read(historical.join("unrelated.dat")).unwrap(),
        b"dormant history"
    );
    assert!(!target.exists());
    assert!(!fixture.paths.settings_path.exists());
    pool.close().await;
}
