use super::*;
use crate::instance_archive_store as store;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

#[path = "storage_migration_retained_history_tests.rs"]
mod retained_history_tests;
#[path = "storage_migration_workspace_archives_tests.rs"]
mod workspace_archives_tests;

async fn archived_fixture(fixture: &Fixture) -> (sqlx::SqlitePool, String, PathBuf) {
    let module = fixture.paths.modules_root.join("fixture");
    fs::create_dir_all(&module).unwrap();
    fs::write(
        module.join("module.toml"),
        "id='fixture'\nname='Fixture'\nversion='1'\n",
    )
    .unwrap();
    fs::create_dir_all(fixture.paths.database_path.parent().unwrap()).unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&fixture.paths.database_path)
                .create_if_missing(true),
        )
        .await
        .unwrap();
    crate::MIGRATOR.run(&pool).await.unwrap();
    let original = fixture.paths.instances_root.join("one");
    for leaf in ["config", "data", "logs", "saves", "runtime"] {
        fs::create_dir_all(original.join(leaf)).unwrap();
    }
    fs::write(
        original.join("runtime/.langame-private-runtime"),
        b"managed\n",
    )
    .unwrap();
    fs::write(
        original.join("runtime/server.exe"),
        b"preserved server payload",
    )
    .unwrap();
    fs::write(original.join("saves/world.dat"), b"preserved world payload").unwrap();
    let config = write_json(
        &original.join("config/instance.json"),
        &json!({
            "instance_id":"one","module_id":"fixture","instance_name":"One",
            "autostart":false,"settings":{},"ports":[],
        }),
    );
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('fixture','Fixture','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('one','One','fixture',?1,?2,?3,?4)")
        .bind(original.join("data").to_string_lossy().into_owned())
        .bind(original.join("config").to_string_lossy().into_owned())
        .bind(original.join("logs").to_string_lossy().into_owned())
        .bind(original.join("saves").to_string_lossy().into_owned())
        .execute(&pool).await.unwrap();
    let install = sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('fixture',?1,'installed','instance','one')")
        .bind(original.join("runtime").to_string_lossy().into_owned())
        .execute(&pool).await.unwrap().last_insert_rowid();
    sqlx::query("UPDATE instances SET install_id=?1 WHERE id='one'")
        .bind(install)
        .execute(&pool)
        .await
        .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    let snapshot = store::capture(&mut connection, "one", digest(&config))
        .await
        .unwrap();
    drop(connection);
    let id = uuid::Uuid::new_v4().to_string();
    let archived = fixture.paths.archives_root.join(&id);
    let directory_identity = identity(&original, true).unwrap();
    fs::rename(&original, &archived).unwrap();
    sqlx::query("DELETE FROM instances WHERE id='one'")
        .execute(&pool)
        .await
        .unwrap();
    let serialized = serde_json::to_string(&snapshot).unwrap();
    sqlx::query("INSERT INTO instance_archives(archive_id,instance_id,instance_name,module_id,original_root,archive_leaf,directory_identity_json,snapshot_json,snapshot_sha256,state,purpose,instances_root_identity_json,archive_parent_identity_json) VALUES(?1,'one','One','fixture',?2,?1,?3,?4,?5,'archived','archive',?6,?7)")
        .bind(&id).bind(original.to_string_lossy().as_ref()).bind(directory_identity)
        .bind(&serialized).bind(digest(serialized.as_bytes()))
        .bind(identity(&fixture.paths.instances_root, true).unwrap())
        .bind(identity(&fixture.paths.archives_root, true).unwrap())
        .execute(&pool).await.unwrap();
    (pool, id, archived)
}

#[tokio::test]
async fn copied_archive_metadata_can_restore_through_the_real_archive_entrypoint() {
    let fixture = Fixture::new();
    let (pool, id, archived) = archived_fixture(&fixture).await;
    let original_snapshot: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    preflight_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    copy_tree(&movement.source, &movement.target);
    let plan = prepare_metadata(&fixture.paths, std::slice::from_ref(&movement))
        .await
        .unwrap();
    assert!(
        plan.files.is_empty(),
        "this fixture needs only database ownership/path patches"
    );
    assert_eq!(plan.rows.len(), 1);
    let patch = &plan.rows[0];
    assert_eq!(patch.table, "instance_archives");
    let stored_root = movement.target.join("one");
    assert_eq!(
        Path::new(patch.after["original_root"].as_str().unwrap()),
        stored_root,
        "persistent paths must retain the configured destination root spelling"
    );
    let migrated_snapshot: store::Snapshot =
        serde_json::from_str(patch.after["snapshot_json"].as_str().unwrap()).unwrap();
    for field in ["data_path", "config_path", "logs_path", "saves_path"] {
        let value = migrated_snapshot.tables["instances"][0][field]
            .as_str()
            .unwrap();
        assert_eq!(Path::new(value).parent(), Some(stored_root.as_path()));
    }
    let next_archive = remap_path(&archived, std::slice::from_ref(&movement)).unwrap();
    assert_eq!(
        patch.after["directory_identity_json"],
        identity(&next_archive, true).unwrap()
    );
    assert_ne!(
        patch.before["directory_identity_json"],
        patch.after["directory_identity_json"]
    );
    let unchanged: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(unchanged, original_snapshot, "preparation is read-only");

    // Apply this known fixture's typed row patch only inside its disposable DB.
    sqlx::query("UPDATE instance_archives SET original_root=?2,directory_identity_json=?3,snapshot_json=?4,snapshot_sha256=?5,instances_root_identity_json=?6,archive_parent_identity_json=?7 WHERE archive_id=?1 AND snapshot_json=?8")
        .bind(&id).bind(patch.after["original_root"].as_str().unwrap())
        .bind(patch.after["directory_identity_json"].as_str().unwrap())
        .bind(patch.after["snapshot_json"].as_str().unwrap())
        .bind(patch.after["snapshot_sha256"].as_str().unwrap())
        .bind(patch.after["instances_root_identity_json"].as_str().unwrap())
        .bind(patch.after["archive_parent_identity_json"].as_str().unwrap())
        .bind(&original_snapshot).execute(&pool).await.unwrap();
    let mut next = fixture.paths.clone();
    next.instances_root = movement.target.clone();
    next.archives_root = movement.target.join(".trash");
    let restored = crate::restore_instance_archive(&next, &id).await.unwrap();
    assert!(
        same_path(
            Path::new(&restored.restored_instance_root),
            &next.instances_root.join("one")
        )
        .unwrap()
    );
    assert_eq!(
        fs::read(next.instances_root.join("one/saves/world.dat")).unwrap(),
        b"preserved world payload"
    );
    assert_eq!(
        fs::read(next.instances_root.join("one/runtime/server.exe")).unwrap(),
        b"preserved server payload"
    );
    let config_path: String =
        sqlx::query_scalar("SELECT config_path FROM instances WHERE id='one'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        same_path(
            Path::new(&config_path),
            &next.instances_root.join("one/config")
        )
        .unwrap()
    );
    assert!(
        archived.exists(),
        "preparation/restoration never cleans the retained source migration copy"
    );
    pool.close().await;
}

#[tokio::test]
async fn coordinator_migrates_archive_commits_settings_cleans_source_and_restores() {
    use crate::storage_migration as migration;
    use app_core::StorageMigrationPhase;
    use std::sync::{Arc, atomic::AtomicBool};

    let fixture = Fixture::new();
    let (pool, id, archived) = archived_fixture(&fixture).await;
    let target = fixture.root.join("moved-instances");
    let mut requested = fixture.paths.settings();
    requested.servers_root = target.to_string_lossy().into_owned();
    requested.archives_root.clear();
    let cancellation = Arc::new(AtomicBool::new(false));
    let progress: migration::Progress = Arc::new(|_| {});
    let prepared = migration::preview(
        &fixture.paths,
        requested,
        uuid::Uuid::new_v4().to_string(),
        Vec::new(),
        cancellation.clone(),
        progress.clone(),
    )
    .await
    .unwrap();
    assert_eq!(prepared.status().phase, StorageMigrationPhase::Ready);
    assert_eq!(prepared.status().conflict_count, 0);
    assert!(archived.is_dir(), "preview retains the source archive");
    assert!(!target.exists(), "preview does not publish the destination");
    let status = migration::execute(&fixture.paths, prepared, cancellation, progress)
        .await
        .unwrap();
    assert_eq!(status.phase, StorageMigrationPhase::Completed);
    assert!(
        status.cleanup_warnings.is_empty(),
        "{:?}",
        status.cleanup_warnings
    );
    assert!(
        !archived.exists(),
        "successful migration cleans the old archive tree"
    );

    let persisted = crate::settings::load_persisted_app_settings(&fixture.paths)
        .unwrap()
        .expect("completed migration must publish settings");
    let next = fixture.paths.with_app_settings(&persisted);
    assert_eq!(next.instances_root, target);
    assert_eq!(next.archives_root, target.join(".trash"));
    let migrated_archive = next.archives_root.join(&id);
    for (relative, expected) in [
        ("saves/world.dat", b"preserved world payload".as_slice()),
        ("runtime/server.exe", b"preserved server payload".as_slice()),
    ] {
        assert_eq!(fs::read(migrated_archive.join(relative)).unwrap(), expected);
    }
    let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id=?1")
        .bind(&id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let migrated_record = store::map_archive(&row).unwrap();
    crate::instance_archive::validate_archived_files(&next, &migrated_record).unwrap();
    let instance_root = next.instances_root.join("one");
    assert_eq!(
        Path::new(migrated_record.original_root.as_deref().unwrap()),
        instance_root
    );
    let snapshot = store::snapshot(&migrated_record).unwrap();
    for field in ["data_path", "config_path", "logs_path", "saves_path"] {
        let stored = snapshot.tables["instances"][0][field].as_str().unwrap();
        assert_eq!(Path::new(stored).parent(), Some(instance_root.as_path()));
    }

    let restored = crate::restore_instance_archive(&next, &id).await.unwrap();
    assert_eq!(Path::new(&restored.restored_instance_root), instance_root);
    for (relative, expected) in [
        ("saves/world.dat", b"preserved world payload".as_slice()),
        ("runtime/server.exe", b"preserved server payload".as_slice()),
    ] {
        assert_eq!(fs::read(instance_root.join(relative)).unwrap(), expected);
    }
    let (config, install): (String, String) = sqlx::query_as(
        "SELECT i.config_path,g.install_root FROM instances i JOIN game_installs g ON g.id=i.install_id WHERE i.id='one'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(Path::new(&config), instance_root.join("config"));
    assert_eq!(Path::new(&install), instance_root.join("runtime"));
    assert!(
        !archived.exists(),
        "restoration never recreates the old storage root"
    );
    pool.close().await;
}

#[tokio::test]
async fn source_archive_replacement_is_rejected_even_when_its_bytes_match() {
    let fixture = Fixture::new();
    let (pool, _, archived) = archived_fixture(&fixture).await;
    let retained = fixture.root.join("retained-original-archive");
    fs::rename(&archived, &retained).unwrap();
    copy_tree(&retained, &archived);
    let movement = fixture.movement(&fixture.paths.instances_root, "moved-instances");
    let error = preflight_metadata(&fixture.paths, &[movement])
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("Archive directory identity changed")
    );
    assert!(retained.exists());
    pool.close().await;
}

#[tokio::test]
async fn revision_roots_include_archived_and_external_programs_and_deduplicate_live_rows() {
    let fixture = Fixture::new();
    let (pool, id, _) = archived_fixture(&fixture).await;
    sqlx::query("DELETE FROM game_installs")
        .execute(&pool)
        .await
        .unwrap();
    let archived_runtime = fixture.paths.instances_root.join("one/runtime");
    assert!(!archived_runtime.exists());
    assert_eq!(
        program_roots(&fixture.paths).await.unwrap(),
        vec![("fixture".into(), archived_runtime.clone())]
    );

    let external = fixture.paths.games_root.join("external");
    fs::create_dir_all(&external).unwrap();
    let stored: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let mut snapshot: Value = serde_json::from_str(&stored).unwrap();
    snapshot["version"] = Value::from(3);
    snapshot["external_program"] = json!({
        "root":external,"identity":identity(&external, true).unwrap(),
        "parent_identity":identity(&fixture.paths.games_root, true).unwrap(),
        "files":{},"directories":[],"fingerprint":digest(b"[{},[]]"),
        "payload_identity":null,"exclusive":false,"complete_inventory":true,
        "restore_token":uuid::Uuid::new_v4().to_string(),"restore_identity":null,
        "binding_sha256":digest(b"reference"),"binding_bytes":9,
    });
    let serialized = serde_json::to_string(&snapshot).unwrap();
    sqlx::query(
        "UPDATE instance_archives SET snapshot_json=?1,snapshot_sha256=?2 WHERE archive_id=?3",
    )
    .bind(&serialized)
    .bind(digest(serialized.as_bytes()))
    .bind(&id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        program_roots(&fixture.paths).await.unwrap().len(),
        2,
        "an external program has its own key even without a live install row"
    );
    for root in [&archived_runtime, &external] {
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope) VALUES('fixture',?1,'installed','library')")
            .bind(root.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    }
    let roots = program_roots(&fixture.paths).await.unwrap();
    assert_eq!(
        roots.len(),
        2,
        "live and archived references to one root share its revision directory"
    );
    assert!(roots.contains(&("fixture".into(), archived_runtime)));
    assert!(roots.contains(&("fixture".into(), external)));
    let unchanged: String =
        sqlx::query_scalar("SELECT snapshot_json FROM instance_archives WHERE archive_id=?1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        unchanged, serialized,
        "program discovery does not rewrite snapshots"
    );
    sqlx::query("UPDATE instance_archives SET snapshot_sha256='invalid' WHERE archive_id=?1")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        program_roots(&fixture.paths)
            .await
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    pool.close().await;
}
