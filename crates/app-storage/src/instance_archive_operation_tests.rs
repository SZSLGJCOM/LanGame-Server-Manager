use super::*;

pub(super) async fn with_program() -> (Fixture, PathBuf) {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(
        fixture
            .paths
            .modules_root
            .join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n",
    )
    .unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .remove(0);
    let library = fixture.paths.games_root.join("archive-fixture");
    fs::create_dir_all(library.join("assets")).unwrap();
    fs::write(library.join("server.exe"), b"official program").unwrap();
    fs::write(library.join("assets/content.bin"), b"official content").unwrap();
    crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    fs::create_dir_all(fixture.instance_root.join("runtime/assets")).unwrap();
    fs::copy(
        library.join("server.exe"),
        fixture.instance_root.join("runtime/server.exe"),
    )
    .unwrap();
    fs::copy(
        library.join("assets/content.bin"),
        fixture.instance_root.join("runtime/assets/content.bin"),
    )
    .unwrap();
    fs::write(
        fixture.instance_root.join("runtime/unknown.dll"),
        b"operator bytes",
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,current_version,scope) VALUES('archive-fixture',?1,'installed','build-1','library')")
        .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    (fixture, library)
}

#[tokio::test]
async fn compact_archive_restores_exact_program_and_keeps_personal_bytes() {
    let (fixture, library) = with_program().await;
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    let summary = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert_eq!(summary.program_storage, "reconstructable");
    assert_eq!(summary.omitted_program_files, 2);
    assert!(summary.omitted_program_bytes > 0);
    assert!(summary.can_restore, "{:?}", summary.issues);
    assert!(
        crate::read_archived_program_sources(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap(),
        before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
}

#[tokio::test]
async fn compact_archive_identity_does_not_depend_on_optional_verification_cache() {
    let (fixture, library) = with_program().await;
    let fingerprint = program::manifest_fingerprint(&library, "archive-fixture").unwrap();
    let archived = fixture.archive().await.unwrap();
    let manifest_path = library.join(".langame-clean-package.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["verified_files"] = serde_json::json!({"unsupported": "advisory cache"});
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert_eq!(
        program::manifest_fingerprint(&library, "archive-fixture").unwrap(),
        fingerprint
    );
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_restore
    );
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.instance_root.join("runtime/server.exe")).unwrap(),
        b"official program"
    );
    assert_eq!(
        fs::read(fixture.instance_root.join("runtime/unknown.dll")).unwrap(),
        b"operator bytes"
    );
}

#[tokio::test]
async fn compact_archive_requires_matching_library_and_never_overwrites_existing_payload() {
    let (fixture, library) = with_program().await;
    let archived = fixture.archive().await.unwrap();
    let archive_root = PathBuf::from(archived.archived_instance_root.unwrap());
    let before = crate::test_file_snapshot::tree_snapshot(&archive_root).unwrap();
    let manifest = library.join(".langame-clean-package.json");
    let manifest_bytes = fs::read(&manifest).unwrap();
    fs::remove_file(&manifest).unwrap();
    assert!(
        !list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_restore
    );
    assert!(
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    fs::write(&manifest, &manifest_bytes).unwrap();
    fs::write(library.join("server.exe"), b"different build").unwrap();
    assert!(
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&archive_root).unwrap(),
        before
    );
    assert!(!fixture.instance_root.exists());
    fs::write(library.join("server.exe"), b"official program").unwrap();
    let target = archive_root.join("runtime/server.exe");
    fs::write(&target, b"new operator file").unwrap();
    assert!(
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"new operator file");
    fs::remove_file(target).unwrap();
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.instance_root.join("runtime/server.exe")).unwrap(),
        b"official program"
    );
}

#[tokio::test]
async fn compact_archive_resumes_only_persisted_staging_after_partial_copy() {
    let (fixture, _) = with_program().await;
    let archived = fixture.archive().await.unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    store::set_state(&pool, &archived.archive_id, "restoring", None)
        .await
        .unwrap();
    let archive = store::load(&pool, &archived.archive_id).await.unwrap();
    let plan = store::snapshot(&archive).unwrap().program.unwrap();
    let lock = acquire_instance_settings_mutation_lock(&fixture.paths, "archive-instance").unwrap();
    program::prepare_staging(&pool, &archive, &root, &plan, &lock)
        .await
        .unwrap();
    fs::write(plan.staging(&root).join("000000"), b"partial stream").unwrap();
    drop(lock);
    pool.close().await;
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(!plan.staging(&fixture.instance_root).exists());
    assert_eq!(
        fs::read(fixture.instance_root.join("runtime/assets/content.bin")).unwrap(),
        b"official content"
    );
}

#[tokio::test]
async fn external_archive_includes_snapshot_without_pruning_or_overwriting_external_saves() {
    let fixture = Fixture::new().await;
    let external = fixture.root.join("external-world");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("world.dat"), b"archive moment").unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(external.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    let mut previous = Vec::new();
    for _ in 0..3 {
        previous.push(
            crate::create_instance_backup(&fixture.paths, "archive-instance")
                .await
                .unwrap()
                .backup_id,
        );
    }
    sqlx::query("UPDATE instances SET backup_retention_count=1 WHERE id='archive-instance'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.archive().await.unwrap();
    let backup = archived.external_saves_backup_id.as_ref().unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    assert_eq!(
        fs::read(root.join("backups").join(backup).join("saves/world.dat")).unwrap(),
        b"archive moment"
    );
    for old in &previous {
        assert!(root.join("backups").join(old).exists());
    }
    fs::write(external.join("world.dat"), b"new external activity").unwrap();
    let restored = restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(restored.external_saves_restore_required);
    assert_eq!(restored.external_saves_backup_id.as_ref(), Some(backup));
    assert_eq!(
        fs::read(external.join("world.dat")).unwrap(),
        b"new external activity"
    );
    assert_eq!(
        crate::list_instance_backups(&fixture.paths, "archive-instance")
            .await
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn missing_external_saves_are_not_created_or_reported_as_archived() {
    let fixture = Fixture::new().await;
    let external = fixture.root.join("missing-world");
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(external.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(fixture.archive().await.is_err());
    assert!(!external.exists());
    assert!(fixture.instance_root.exists());
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives
            .is_empty()
    );
}

#[tokio::test]
async fn true_delete_preserves_library_peer_and_every_external_save() {
    let (fixture, library) = with_program().await;
    let external = fixture.root.join("external-world");
    let peer = fixture.paths.instances_root.join("peer");
    fs::create_dir(&external).unwrap();
    fs::create_dir(&peer).unwrap();
    fs::write(external.join("world.dat"), b"external shared world").unwrap();
    fs::write(peer.join("world.dat"), b"peer world").unwrap();
    let library_before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(external.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let result = crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert_eq!(
        result.preserved_external_saves_path.as_deref(),
        Some(external.to_string_lossy().as_ref())
    );
    assert!(!fixture.instance_root.exists());
    assert_eq!(
        fs::read(external.join("world.dat")).unwrap(),
        b"external shared world"
    );
    assert_eq!(fs::read(peer.join("world.dat")).unwrap(), b"peer world");
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        library_before
    );
    let list = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(list.archives.is_empty());
    assert!(list.pending_deletions.is_empty());
}

#[tokio::test]
async fn restore_normalizes_true_autostart_and_rejects_wrong_directory_identity() {
    let fixture = Fixture::new().await;
    let path = fixture.instance_root.join("config/instance.json");
    let mut before: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    before["autostart"] = true.into();
    fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
    let archived = fixture.archive().await.unwrap();
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    before["autostart"] = false.into();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
        before
    );
    assert_eq!(
        fixture.snapshot().await.tables["instances"][0]["autostart"],
        0
    );
    let bytes = fs::read(&path).unwrap();
    let wrong = files::identity(&fixture.paths.instances_root)
        .unwrap()
        .unwrap();
    assert!(
        config::restore(
            &fixture.instance_root,
            &wrong,
            &store::digest(&bytes),
            &store::digest(&bytes)
        )
        .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[tokio::test]
async fn purge_missing_leaf_requires_prior_purging_and_unchanged_parent_identity() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let root = PathBuf::from(archived.archived_instance_root.unwrap());
    let retained = fixture.root.join("retained-archive");
    fs::rename(&root, &retained).unwrap();
    assert!(
        purge_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    assert!(
        store::load(&pool, &archived.archive_id)
            .await
            .unwrap()
            .snapshot
            .is_some()
    );
    store::set_state(&pool, &archived.archive_id, "purging", None)
        .await
        .unwrap();
    let trash = fixture.paths.instances_root.join(".trash");
    let original_trash = fixture.root.join("retained-trash");
    fs::rename(&trash, &original_trash).unwrap();
    fs::create_dir(&trash).unwrap();
    assert!(
        purge_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert!(
        store::load(&pool, &archived.archive_id)
            .await
            .unwrap()
            .snapshot
            .is_some()
    );
    fs::remove_dir(&trash).unwrap();
    fs::rename(&original_trash, &trash).unwrap();
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_purge
    );
    assert!(
        purge_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .unwrap()
            .purged
    );
    assert!(
        store::load(&pool, &archived.archive_id)
            .await
            .unwrap()
            .snapshot
            .is_none()
    );
    assert_eq!(
        fs::read(retained.join("saves/world.dat")).unwrap(),
        b"world state"
    );
    pool.close().await;
}

#[tokio::test]
async fn migration_four_preserves_applied_three_archive_rows_and_readonly_mapping() {
    use sqlx::Connection;
    let mut connection = sqlx::SqliteConnection::connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../../migrations/0003_instance_archives.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::query("INSERT INTO instance_archives(archive_id,archive_leaf,snapshot_json,snapshot_sha256,state) VALUES ('old','old','original snapshot','original checksum','archived')").execute(&mut connection).await.unwrap();
    let row = sqlx::query("SELECT * FROM instance_archives")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    let before = store::map_archive(&row).unwrap();
    assert_eq!(before.purpose, "archive");
    assert!(before.parent_identity.is_none());
    sqlx::raw_sql(include_str!(
        "../../../migrations/0004_archive_operations.sql"
    ))
    .execute(&mut connection)
    .await
    .unwrap();
    connection.clear_cached_statements().await.unwrap();
    let row = sqlx::query("SELECT * FROM instance_archives")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    let after = store::map_archive(&row).unwrap();
    assert_eq!(after.snapshot, before.snapshot);
    assert_eq!(after.snapshot_hash, before.snapshot_hash);
    assert_eq!(after.purpose, "archive");
    assert!(after.parent_identity.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA user_version")
            .fetch_one(&mut connection)
            .await
            .unwrap(),
        4
    );
    connection.close().await.unwrap();
}

#[cfg(windows)]
#[test]
fn directory_guard_pins_the_recorded_root_until_released() {
    let root = std::env::temp_dir().join(format!("archive-pin-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let target = root.with_extension("moved");
    let identity = files::identity(&root).unwrap().unwrap();
    let guard = files::guard_identity(&root, &identity).unwrap();
    assert!(fs::rename(&root, &target).is_err());
    drop(guard);
    fs::rename(&root, &target).unwrap();
    fs::remove_dir(&target).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn true_delete_failure_is_visible_only_as_pending_and_retries_original_operation() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let fixture = Fixture::new().await;
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(fixture.instance_root.join("saves/world.dat"))
        .unwrap();
    assert!(
        crate::delete_instance(&fixture.paths, "archive-instance")
            .await
            .is_err()
    );
    let list = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(list.archives.is_empty());
    assert_eq!(list.pending_deletions.len(), 1);
    assert_eq!(list.pending_deletions[0].instance_id, "archive-instance");
    let operation = list.pending_deletions[0].operation_id.clone();
    assert!(
        restore_instance_archive(&fixture.paths, &operation)
            .await
            .is_err()
    );
    assert!(
        purge_instance_archive(&fixture.paths, &operation)
            .await
            .is_err()
    );
    drop(blocker);
    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!fixture.instance_root.exists());
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .pending_deletions
            .is_empty()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let completed = store::load(&pool, &operation).await.unwrap();
    assert_eq!(completed.state, "purged");
    assert!(completed.snapshot.is_none());
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM instance_archives WHERE instance_id='archive-instance'"
        )
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    pool.close().await;
}
