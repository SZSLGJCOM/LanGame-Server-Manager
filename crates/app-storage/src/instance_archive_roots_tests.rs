use super::*;
use crate::guard_archive_root_settings_update;

#[tokio::test]
async fn archive_root_saved_settings_restart_and_full_archive_lifecycle() {
    let mut fixture = Fixture::new().await;
    let mut settings = fixture.paths.settings();
    settings.archives_root = fixture
        .root
        .join("custom/archive-area")
        .to_string_lossy()
        .into_owned();
    let guard = guard_archive_root_settings_update(&fixture.paths, &settings)
        .await
        .unwrap();
    crate::settings::save_app_settings_with_paths(settings.clone(), &fixture.paths).unwrap();
    drop(guard);
    let restarted = crate::bootstrap_storage_with_paths(fixture.paths.clone()).unwrap();
    assert_eq!(restarted.settings.archives_root, settings.archives_root);
    fixture.paths = restarted.paths;

    let archived = fixture.archive().await.unwrap();
    let archived_root = PathBuf::from(archived.archived_instance_root.unwrap());
    assert_eq!(
        archived_root.parent(),
        Some(fixture.paths.archives_root.as_path())
    );
    assert!(!fixture.paths.instances_root.join(".trash").exists());
    assert_eq!(
        fs::read(archived_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
    let list = list_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(list.archives.len(), 1);
    assert_eq!(list.archives[0].archive_id, archived.archive_id);
    assert!(
        list.archives[0].can_restore,
        "{:?}",
        list.archives[0].issues
    );
    let report = crate::scan_storage_usage(
        &fixture.paths,
        "configured-archive-root".into(),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .await
    .unwrap();
    let archive_usage = report
        .entries
        .iter()
        .find(|entry| entry.category == "archives")
        .unwrap();
    assert_eq!(
        archive_usage.path,
        fixture.paths.archives_root.to_string_lossy()
    );
    assert!(archive_usage.logical_bytes >= 11);
    restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
    let archived = fixture.archive().await.unwrap();
    assert!(
        purge_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .unwrap()
            .purged
    );
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives
            .is_empty()
    );
    assert_eq!(
        fs::read_dir(&fixture.paths.archives_root).unwrap().count(),
        0
    );
}

#[tokio::test]
async fn archive_root_change_rejects_retained_files_and_keeps_old_archives_restorable() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let mut settings = fixture.paths.settings();
    settings.archives_root = fixture
        .root
        .join("new-archives")
        .to_string_lossy()
        .into_owned();
    let error = guard_archive_root_settings_update(&fixture.paths, &settings)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("not empty"));
    assert!(!Path::new(&settings.archives_root).exists());
    settings = fixture.paths.settings();
    settings.servers_root = fixture
        .root
        .join("new-instances")
        .to_string_lossy()
        .into_owned();
    assert!(
        guard_archive_root_settings_update(&fixture.paths, &settings)
            .await
            .is_err()
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
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
}

#[tokio::test]
async fn archive_root_change_rejects_untracked_old_files_and_nonempty_destination() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(&fixture.paths.archives_root).unwrap();
    let historical = fixture.paths.archives_root.join("untracked-world.dat");
    fs::write(&historical, b"historical world").unwrap();
    let mut settings = fixture.paths.settings();
    settings.archives_root = fixture
        .root
        .join("new-archives")
        .to_string_lossy()
        .into_owned();
    assert!(
        guard_archive_root_settings_update(&fixture.paths, &settings)
            .await
            .is_err()
    );
    assert_eq!(fs::read(&historical).unwrap(), b"historical world");
    fs::remove_file(historical).unwrap();
    fs::create_dir_all(&settings.archives_root).unwrap();
    let foreign = Path::new(&settings.archives_root).join("other-data");
    fs::write(&foreign, b"unrelated").unwrap();
    let error = guard_archive_root_settings_update(&fixture.paths, &settings)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("must be empty"));
    assert_eq!(fs::read(foreign).unwrap(), b"unrelated");
}

#[tokio::test]
async fn archive_root_change_rejects_pending_deletion_journal_even_when_directory_is_empty() {
    let fixture = Fixture::new().await;
    let id = stage_archiving(&fixture, false).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET purpose='delete',problem='file is locked' WHERE archive_id=?1")
        .bind(&id).execute(&pool).await.unwrap();
    pool.close().await;
    assert_eq!(
        fs::read_dir(&fixture.paths.archives_root).unwrap().count(),
        0
    );
    let mut settings = fixture.paths.settings();
    settings.archives_root = fixture
        .root
        .join("new-archives")
        .to_string_lossy()
        .into_owned();
    let error = guard_archive_root_settings_update(&fixture.paths, &settings)
        .await
        .err()
        .unwrap();
    assert!(error.to_string().contains("finish failed deletions"));
    let list = list_instance_archives(&fixture.paths).await.unwrap();
    assert_eq!(list.pending_deletions.len(), 1);
    assert_eq!(list.pending_deletions[0].operation_id, id);
    assert!(list.pending_deletions[0].can_retry);
    assert_eq!(list.pending_deletions[0].issues, ["file is locked"]);
}

#[tokio::test]
async fn archive_root_change_rejects_overlaps_relative_paths_and_files() {
    let fixture = Fixture::new().await;
    let file = fixture.root.join("not-a-directory");
    fs::write(&file, b"keep").unwrap();
    for target in [
        fixture.paths.instances_root.clone(),
        fixture.paths.instances_root.join("nested"),
        fixture.paths.games_root.clone(),
        fixture.paths.games_root.join("nested"),
        fixture.paths.steamcmd_root.clone(),
        fixture.paths.logs_root.clone(),
        fixture.paths.app_data_root.clone(),
        fixture.root.clone(),
        PathBuf::from("relative-archives"),
        fixture.root.join("unused/../archives"),
        file.clone(),
    ] {
        let mut settings = fixture.paths.settings();
        settings.archives_root = target.to_string_lossy().into_owned();
        assert!(
            guard_archive_root_settings_update(&fixture.paths, &settings)
                .await
                .is_err(),
            "{}",
            target.display()
        );
    }
    assert_eq!(fs::read(file).unwrap(), b"keep");
    guard_archive_root_settings_update(&fixture.paths, &fixture.paths.settings())
        .await
        .unwrap();
}

#[tokio::test]
async fn archive_root_update_holds_inventory_lock_until_settings_are_saved() {
    let fixture = Fixture::new().await;
    let guard = guard_archive_root_settings_update(&fixture.paths, &fixture.paths.settings())
        .await
        .unwrap();
    assert!(inventory_lock(&fixture.paths).is_err());
    drop(guard);
    assert!(inventory_lock(&fixture.paths).is_ok());
}

#[tokio::test]
async fn permanent_delete_uses_configured_archive_root_without_creating_a_recoverable_archive() {
    let mut fixture = Fixture::new().await;
    fixture.paths.archives_root = fixture.root.join("delete-staging");
    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!fixture.instance_root.exists());
    assert!(!fixture.paths.instances_root.join(".trash").exists());
    assert_eq!(
        fs::read_dir(&fixture.paths.archives_root).unwrap().count(),
        0
    );
    let list = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(list.archives.is_empty());
    assert!(list.pending_deletions.is_empty());
}

#[cfg(windows)]
#[tokio::test]
async fn archive_root_update_rejects_junction_without_touching_its_target() {
    let mut fixture = Fixture::new().await;
    let target = fixture.root.join("external");
    let link = fixture.root.join("archive-link");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("world.dat"), b"external world").unwrap();
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let mut settings = fixture.paths.settings();
    settings.archives_root = link.to_string_lossy().into_owned();
    let result = guard_archive_root_settings_update(&fixture.paths, &settings).await;
    settings.archives_root = fixture
        .root
        .join("new-archives")
        .to_string_lossy()
        .into_owned();
    fixture.paths.migrations_root = link.join("../migrations");
    let bundled_result = guard_archive_root_settings_update(&fixture.paths, &settings).await;
    fs::remove_dir(&link).unwrap();
    assert!(result.is_err());
    assert!(
        bundled_result.is_err(),
        "trusted parent components must not hide a junction"
    );
    assert_eq!(
        fs::read(target.join("world.dat")).unwrap(),
        b"external world"
    );
}

#[tokio::test]
async fn archive_admission_rechecks_registered_saves_claims_before_writing_to_custom_root() {
    let mut fixture = Fixture::new().await;
    fixture.paths.archives_root = fixture.root.join("archive-parent/archive-area");
    fs::create_dir_all(&fixture.paths.archives_root).unwrap();
    let before = fixture.snapshot().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let peer = fixture.paths.instances_root.join("peer");
    sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES ('peer','Peer','archive-fixture',?1,?2,?3,?4)")
        .bind(peer.join("data").to_string_lossy().as_ref())
        .bind(peer.join("config").to_string_lossy().as_ref())
        .bind(peer.join("logs").to_string_lossy().as_ref())
        .bind(peer.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    for claim in [
        fixture.paths.archives_root.clone(),
        fixture.paths.archives_root.parent().unwrap().to_owned(),
        fixture.paths.archives_root.join("peer-world"),
    ] {
        sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='peer'")
            .bind(claim.to_string_lossy().as_ref())
            .execute(&pool)
            .await
            .unwrap();
        let error = fixture.archive().await.unwrap_err();
        assert!(error.to_string().contains("overlaps"), "{error}");
        assert_eq!(fixture.snapshot().await, before);
        assert_eq!(
            fs::read_dir(&fixture.paths.archives_root).unwrap().count(),
            0
        );
        assert_eq!(
            fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
            b"world state"
        );
    }
    let operations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instance_archives")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(operations, 0);
    pool.close().await;
}

#[tokio::test]
async fn archive_recovery_rechecks_container_claims_under_the_final_write_transaction() {
    let fixture = Fixture::new().await;
    let id = stage_archiving(&fixture, false).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let peer = fixture.paths.instances_root.join("peer");
    sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES ('peer','Peer','archive-fixture',?1,?2,?3,?4)")
        .bind(peer.join("data").to_string_lossy().as_ref())
        .bind(peer.join("config").to_string_lossy().as_ref())
        .bind(peer.join("logs").to_string_lossy().as_ref())
        .bind(fixture.paths.archives_root.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    let error = fixture.archive().await.unwrap_err();
    assert!(error.to_string().contains("overlaps"), "{error}");
    assert!(fixture.instance_root.exists());
    assert!(!fixture.paths.archives_root.join(id).exists());
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
}

#[tokio::test]
async fn archive_admission_checks_effective_saves_before_creating_an_external_backup() {
    let mut fixture = Fixture::new().await;
    fixture.paths.archives_root = fixture.root.join("archive-area");
    fixture.paths.modules_root = fixture.root.join("modules");
    let module = fixture.paths.modules_root.join("archive-fixture");
    fs::create_dir_all(&module).unwrap();
    fs::create_dir_all(&fixture.paths.archives_root).unwrap();
    fs::write(
        fixture.paths.archives_root.join("retained-world"),
        b"archive bytes",
    )
    .unwrap();
    fs::write(module.join("module.toml"), format!(
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n[storage]\nsaves_path_template={:?}\n",
        fixture.paths.archives_root.to_string_lossy(),
    )).unwrap();
    let error = fixture.archive().await.unwrap_err();
    assert!(error.to_string().contains("archive directory"), "{error}");
    assert!(!fixture.instance_root.join("backups").exists());
    assert_eq!(
        fs::read(fixture.paths.archives_root.join("retained-world")).unwrap(),
        b"archive bytes"
    );
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
}

#[tokio::test]
async fn archive_default_root_in_common_program_container_preserves_lifecycle_and_real_ownership() {
    for common_instance_parent in [false, true] {
        let mut fixture = Fixture::new().await;
        fixture.paths.games_root = if common_instance_parent {
            fixture.paths.instances_root.clone()
        } else {
            fixture.root.clone()
        };
        guard_archive_root_settings_update(&fixture.paths, &fixture.paths.settings())
            .await
            .unwrap();
        let archived = fixture.archive().await.unwrap();
        let list = list_instance_archives(&fixture.paths).await.unwrap();
        assert_eq!(list.archives.len(), 1);
        assert!(
            list.archives[0].can_restore,
            "{:?}",
            list.archives[0].issues
        );
        restore_instance_archive(&fixture.paths, &archived.archive_id)
            .await
            .unwrap();
        assert_eq!(
            fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
            b"world state"
        );

        // The common-root exception is only for a container. If an actual
        // installation claims that root, archival must still be rejected.
        let pool = connect_pool(&fixture.paths).await.unwrap();
        sqlx::query("INSERT INTO game_installs (id,module_id,install_root,scope) VALUES (18,'archive-fixture',?1,'library')")
            .bind(fixture.paths.games_root.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        let error = fixture.archive().await.unwrap_err();
        assert!(error.to_string().contains("overlaps"), "{error}");
        assert_eq!(
            fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
            b"world state"
        );
        sqlx::query("DELETE FROM game_installs WHERE id=18")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let archived = fixture.archive().await.unwrap();
        assert!(
            purge_instance_archive(&fixture.paths, &archived.archive_id)
                .await
                .unwrap()
                .purged
        );
        assert!(
            list_instance_archives(&fixture.paths)
                .await
                .unwrap()
                .archives
                .is_empty()
        );
        let mut custom = fixture.paths.settings();
        custom.archives_root = fixture
            .paths
            .games_root
            .join("custom-archives")
            .to_string_lossy()
            .into_owned();
        assert!(
            guard_archive_root_settings_update(&fixture.paths, &custom)
                .await
                .is_err()
        );
        assert!(!Path::new(&custom.archives_root).exists());
    }
}
