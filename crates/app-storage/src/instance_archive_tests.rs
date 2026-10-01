use super::*;
use std::fs;
use std::path::PathBuf;

#[path = "instance_archive_capture_safety_tests.rs"]
mod capture_safety;
#[path = "instance_archive_operation_tests.rs"]
mod operations;
#[path = "instance_archive_performance_tests.rs"]
mod performance;

#[path = "instance_archive_explicit_delete_tests.rs"]
mod explicit_delete;

#[path = "instance_archive_concurrency_tests.rs"]
mod concurrency;
#[path = "instance_archive_details_tests.rs"]
mod details;
#[path = "instance_archive_external_tests.rs"]
mod external_programs;
#[path = "instance_archive_roots_tests.rs"]
mod roots;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    instance_root: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("ia-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: fs::canonicalize(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"),
            )
            .unwrap(),
            migrations_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        let instance_root = paths.instances_root.join("archive-instance");
        for directory in [
            &paths.modules_root,
            &paths.games_root,
            paths.database_path.parent().unwrap(),
        ] {
            fs::create_dir_all(directory).unwrap();
        }
        for child in ["config", "logs", "saves", "runtime", "data"] {
            fs::create_dir_all(instance_root.join(child)).unwrap();
        }
        fs::write(instance_root.join("config/instance.json"), br#"{"settings":{"motd":"retained world"},"ports":[{"name":"game","port":27981,"protocol":"udp"}]}"#).unwrap();
        fs::write(
            instance_root.join("runtime/.langame-private-runtime"),
            b"managed\n",
        )
        .unwrap();
        fs::write(instance_root.join("saves/world.dat"), b"world state").unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query("INSERT INTO modules (id,name,version) VALUES ('archive-fixture','Archive fixture','1')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path,env_json,args_json,autostart) VALUES ('archive-instance','Preserved world','archive-fixture',?1,?2,?3,?4,'{\"FIXTURE\":\"value\"}','[\"--keep\"]',1)")
            .bind(instance_root.join("data").to_string_lossy().as_ref())
            .bind(instance_root.join("config").to_string_lossy().as_ref())
            .bind(instance_root.join("logs").to_string_lossy().as_ref())
            .bind(instance_root.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs (id,module_id,install_root,scope,owner_instance_id) VALUES (17,'archive-fixture',?1,'instance','archive-instance')")
            .bind(instance_root.join("runtime").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        for statement in [
            "UPDATE instances SET install_id=17 WHERE id='archive-instance'",
            "INSERT INTO instance_ports (id,instance_id,name,port,protocol) VALUES (23,'archive-instance','game',27981,'udp')",
            "INSERT INTO instance_runs (id,instance_id,status,pid,exit_code) VALUES (29,'archive-instance','stopped',123,0)",
            "INSERT INTO instance_broadcast_policies VALUES ('archive-instance',1,'[]',456)",
            "INSERT INTO instance_broadcast_events (event_id,instance_id,module_id,source,message,status,created_at_unix_ms) VALUES ('event-original','archive-instance','archive-fixture','manual','retained message','sent',789)",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool.close().await;
        Self {
            root,
            paths,
            instance_root,
        }
    }

    async fn archive(&self) -> Result<app_core::InstanceArchiveResult, StorageError> {
        let _inventory = inventory_lock(&self.paths)?;
        let lock = acquire_instance_settings_mutation_lock(&self.paths, "archive-instance")?;
        archive_instance_locked(&self.paths, "archive-instance", &lock, "archive").await
    }

    async fn snapshot(&self) -> store::Snapshot {
        let pool = connect_pool(&self.paths).await.unwrap();
        let mut connection = pool.acquire().await.unwrap();
        let snapshot = store::capture(
            &mut connection,
            "archive-instance",
            files::config_hash(&self.instance_root).unwrap(),
        )
        .await
        .unwrap();
        drop(connection);
        pool.close().await;
        snapshot
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Only this test's UUID-owned directory is removed; link tests unlink first.
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn explicit_delete_removes_owned_files_without_creating_a_recoverable_archive() {
    let fixture = Fixture::new().await;
    fs::create_dir_all(fixture.instance_root.join("backups")).unwrap();
    fs::write(
        fixture.instance_root.join("backups/world.zip"),
        b"private backup",
    )
    .unwrap();
    crate::delete_instance(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert!(!fixture.instance_root.exists());
    let archives = list_instance_archives(&fixture.paths).await.unwrap();
    assert!(
        archives.archives.is_empty(),
        "explicit deletion must not become an archive"
    );
    let trash = fixture.paths.instances_root.join(".trash");
    assert_eq!(
        fs::read_dir(trash).unwrap().count(),
        0,
        "owned deleted files must be physically removed"
    );
}

#[tokio::test]
async fn archive_omits_only_matching_rebuildable_program_and_keeps_personal_files() {
    let mut fixture = Fixture::new().await;
    fixture.paths.modules_root = fixture.root.join("modules");
    fs::create_dir_all(fixture.paths.modules_root.join("archive-fixture")).unwrap();
    fs::write(fixture.paths.modules_root.join("archive-fixture/module.toml"),
        "id='archive-fixture'\nname='Archive fixture'\nversion='1'\n[storage]\nruntime_copy_exclusions=['Mods','settings.ini']\n").unwrap();
    let descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .remove(0);
    let library = fixture.paths.games_root.join("archive-fixture");
    fs::create_dir_all(library.join("Mods")).unwrap();
    for (relative, bytes) in [
        ("server.exe", b"official program".as_slice()),
        ("changed.dll", b"official library"),
        ("settings.ini", b"personal settings"),
        ("Mods/mod.dll", b"personal mod"),
    ] {
        fs::write(library.join(relative), bytes).unwrap();
    }
    crate::record_library_program_baseline(&library, &descriptor, true, None).unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO game_installs (module_id,install_root,install_state,current_version,scope) VALUES ('archive-fixture',?1,'installed','fixture-build','library')")
        .bind(library.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    let runtime = fixture.instance_root.join("runtime");
    fs::create_dir_all(runtime.join("Mods")).unwrap();
    fs::write(runtime.join("server.exe"), b"official program").unwrap();
    fs::write(runtime.join("changed.dll"), b"operator modified library").unwrap();
    fs::write(runtime.join("settings.ini"), b"personal settings").unwrap();
    fs::write(runtime.join("Mods/mod.dll"), b"personal mod").unwrap();
    fs::write(runtime.join("unknown.dll"), b"unknown program").unwrap();
    fs::create_dir_all(fixture.instance_root.join("backups")).unwrap();
    fs::write(
        fixture.instance_root.join("backups/world.zip"),
        b"all backups stay",
    )
    .unwrap();
    let before_library = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    let root = Path::new(archived.archived_instance_root.as_ref().unwrap());
    assert!(
        !root.join("runtime/server.exe").exists(),
        "only proven rebuildable program payload should be omitted"
    );
    assert_eq!(
        fs::read(root.join("runtime/changed.dll")).unwrap(),
        b"operator modified library"
    );
    assert_eq!(
        fs::read(root.join("runtime/settings.ini")).unwrap(),
        b"personal settings"
    );
    assert_eq!(
        fs::read(root.join("runtime/Mods/mod.dll")).unwrap(),
        b"personal mod"
    );
    assert_eq!(
        fs::read(root.join("runtime/unknown.dll")).unwrap(),
        b"unknown program"
    );
    assert_eq!(
        fs::read(root.join("backups/world.zip")).unwrap(),
        b"all backups stay"
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before_library
    );
}

#[tokio::test]
async fn archive_restore_preserves_all_six_tables_original_ids_and_files() {
    let fixture = Fixture::new().await;
    let mut before = fixture.snapshot().await;
    assert_eq!(before.tables["instances"][0]["autostart"], 1);
    before.tables.get_mut("instances").unwrap()[0].insert("autostart".into(), serde_json::json!(0));
    let config = fs::read(fixture.instance_root.join("config/instance.json")).unwrap();
    let deleted = fixture.archive().await.unwrap();
    assert!(!fixture.instance_root.exists());
    assert!(
        Path::new(deleted.archived_instance_root.as_ref().unwrap())
            .join("saves/world.dat")
            .is_file()
    );
    let archives = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives;
    assert_eq!(archives.len(), 1);
    assert!(archives[0].can_restore, "{:?}", archives[0].issues);
    let restored = restore_instance_archive(&fixture.paths, &archives[0].archive_id)
        .await
        .unwrap();
    assert_eq!(restored.instance_id, "archive-instance");
    assert_eq!(fixture.snapshot().await, before);
    assert_eq!(
        fs::read(fixture.instance_root.join("config/instance.json")).unwrap(),
        config
    );
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives
            .is_empty()
    );
}

#[tokio::test]
async fn archive_commit_failure_restores_database_and_files() {
    let fixture = Fixture::new().await;
    let before = fixture.snapshot().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for statement in [
        "CREATE TABLE fixture_delete_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE fixture_delete_child (parent_id INTEGER REFERENCES fixture_delete_parent(id) DEFERRABLE INITIALLY DEFERRED)",
        "CREATE TRIGGER fixture_reject_delete_commit AFTER DELETE ON instances BEGIN INSERT INTO fixture_delete_child VALUES (1); END",
    ] {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    pool.close().await;
    let error = fixture.archive().await.unwrap_err();
    assert!(matches!(error, StorageError::Sqlx(_)), "{error}");
    assert_eq!(fixture.snapshot().await, before);
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
    assert!(
        list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives
            .is_empty()
    );
}

#[tokio::test]
async fn archive_missing_metadata_requires_explicit_cleanup_and_cannot_restore() {
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/previous-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"retained old world").unwrap();
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert!(historical.join("world.dat").is_file());
    let archives = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives;
    assert_eq!(archives.len(), 1);
    assert_eq!(archives[0].state, InstanceArchiveState::MissingMetadata);
    assert!(!archives[0].can_restore);
    assert!(archives[0].can_purge, "{:?}", archives[0].issues);
    assert!(
        restore_instance_archive(&fixture.paths, &archives[0].archive_id)
            .await
            .is_err()
    );
    assert!(
        purge_instance_archive(&fixture.paths, "../previous-world")
            .await
            .is_err()
    );
    assert!(historical.join("world.dat").is_file());
    assert!(
        purge_instance_archive(&fixture.paths, &archives[0].archive_id)
            .await
            .unwrap()
            .purged
    );
    assert!(!historical.exists());
    assert!(fixture.instance_root.join("saves/world.dat").is_file());
}

#[tokio::test]
async fn archive_same_name_after_purge_gets_new_id_and_old_id_cannot_clear_it() {
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/previous-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"first archive").unwrap();
    let old_id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    assert!(
        purge_instance_archive(&fixture.paths, &old_id)
            .await
            .unwrap()
            .purged
    );
    fs::create_dir(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"later archive").unwrap();
    let archives = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives;
    assert_eq!(archives.len(), 1);
    assert_ne!(archives[0].archive_id, old_id);
    assert_eq!(archives[0].state, InstanceArchiveState::MissingMetadata);
    assert!(
        purge_instance_archive(&fixture.paths, &old_id)
            .await
            .unwrap()
            .purged
    );
    assert_eq!(
        fs::read(historical.join("world.dat")).unwrap(),
        b"later archive"
    );
}

#[tokio::test]
async fn archive_refuses_active_run_even_with_stopped_instance_status() {
    let fixture = Fixture::new().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_runs SET status='running' WHERE id=29")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(matches!(
        fixture.archive().await,
        Err(StorageError::ActiveInstanceDeletion { .. })
    ));
    assert!(fixture.instance_root.join("saves/world.dat").is_file());
    assert!(
        pending_instance_archive_ids(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn stage_archiving(fixture: &Fixture, move_files: bool) -> String {
    fs::create_dir_all(fixture.paths.instances_root.join(".trash")).unwrap();
    let snapshot = fixture.snapshot().await;
    let text = serde_json::to_string(&snapshot).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let identity = files::identity(&fixture.instance_root).unwrap().unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("INSERT INTO instance_archives (archive_id,instance_id,instance_name,module_id,original_root,archive_leaf,directory_identity_json,snapshot_json,snapshot_sha256,state) VALUES (?1,'archive-instance','Preserved world','archive-fixture',?2,?1,?3,?4,?5,'archiving')")
        .bind(&id).bind(fixture.instance_root.to_string_lossy().as_ref()).bind(&identity).bind(&text).bind(store::digest(text.as_bytes()))
        .execute(&pool).await.unwrap();
    pool.close().await;
    if move_files {
        let target = fixture.paths.instances_root.join(".trash").join(&id);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        files::move_directory(&fixture.instance_root, &target, &identity).unwrap();
    }
    id
}

#[tokio::test]
async fn archive_recovers_file_move_before_database_commit_and_restore_move_before_commit() {
    let fixture = Fixture::new().await;
    let id = stage_archiving(&fixture, true).await;
    assert_eq!(
        pending_instance_archive_ids(&fixture.paths).await.unwrap(),
        ["archive-instance"]
    );
    recover_instance_archives(&fixture.paths).await.unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let archive = store::load(&pool, &id).await.unwrap();
    assert_eq!(archive.state, "archived");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM instances")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    store::set_state(&pool, &id, "restoring", None)
        .await
        .unwrap();
    pool.close().await;
    let archived = fixture.paths.instances_root.join(".trash").join(&id);
    files::move_directory(
        &archived,
        &fixture.instance_root,
        archive.identity.as_deref().unwrap(),
    )
    .unwrap();
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert!(
        pending_instance_archive_ids(&fixture.paths)
            .await
            .unwrap()
            .is_empty(),
        "{:?}",
        list_instance_archives(&fixture.paths).await.unwrap()
    );
    assert_eq!(
        fixture.snapshot().await.tables["instances"][0]["autostart"],
        0
    );
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
}

#[tokio::test]
async fn archive_recovery_retains_pending_when_a_process_appears_after_admission() {
    let fixture = Fixture::new().await;
    let id = stage_archiving(&fixture, false).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_runs SET status='running' WHERE id=29")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert!(fixture.instance_root.join("saves/world.dat").is_file());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let pending = store::load(&pool, &id).await.unwrap();
    assert_eq!(pending.state, "archiving");
    assert!(pending.problem.is_some());
    sqlx::query("UPDATE instance_runs SET status='stopped' WHERE id=29")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    recover_instance_archives(&fixture.paths).await.unwrap();
    assert!(
        !fixture.instance_root.exists(),
        "{:?}",
        list_instance_archives(&fixture.paths).await.unwrap()
    );
    assert!(
        pending_instance_archive_ids(&fixture.paths)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn archive_restore_refuses_reserved_ports_and_retries_without_losing_history() {
    let fixture = Fixture::new().await;
    fixture.archive().await.unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let peer = fixture.paths.instances_root.join("peer");
    sqlx::query("INSERT INTO instances (id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES ('peer','Peer','archive-fixture',?1,?2,?3,?4)")
        .bind(peer.join("data").to_string_lossy().as_ref()).bind(peer.join("config").to_string_lossy().as_ref())
        .bind(peer.join("logs").to_string_lossy().as_ref()).bind(peer.join("saves").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO instance_ports (instance_id,name,port,protocol) VALUES ('peer','game',27981,'udp')").execute(&pool).await.unwrap();
    pool.close().await;
    assert!(
        !list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_restore
    );
    assert!(restore_instance_archive(&fixture.paths, &id).await.is_err());
    assert!(!fixture.instance_root.exists());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("DELETE FROM instances WHERE id='peer'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    restore_instance_archive(&fixture.paths, &id).await.unwrap();
    assert_eq!(
        fixture.snapshot().await.tables["instance_runs"][0]["id"],
        29
    );
}

#[tokio::test]
async fn archive_listing_does_not_execute_restore_writes_but_restore_keeps_constraints() {
    let fixture = Fixture::new().await;
    fixture.archive().await.unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("CREATE TRIGGER fixture_reject_restore BEFORE INSERT ON instances BEGIN SELECT RAISE(ABORT,'fixture rejects restore insert'); END")
        .execute(&pool).await.unwrap();
    pool.close().await;
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert!(archive.can_restore, "{:?}", archive.issues);
    assert!(matches!(
        restore_instance_archive(&fixture.paths, &id).await,
        Err(StorageError::Sqlx(_))
    ));
    assert!(
        Path::new(&archive.archived_instance_root)
            .join("saves/world.dat")
            .is_file()
    );
    assert!(!fixture.instance_root.exists());
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("DROP TRIGGER fixture_reject_restore")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    restore_instance_archive(&fixture.paths, &id).await.unwrap();
    assert_eq!(
        fs::read(fixture.instance_root.join("saves/world.dat")).unwrap(),
        b"world state"
    );
}

#[tokio::test]
async fn archive_corrupt_metadata_is_visible_but_never_inferred_for_restore() {
    let fixture = Fixture::new().await;
    fixture.archive().await.unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_json='{}' WHERE archive_id=?1")
        .bind(&id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert!(!archive.can_restore);
    assert!(archive.can_purge);
    assert!(!archive.issues.is_empty());
    assert!(restore_instance_archive(&fixture.paths, &id).await.is_err());
    assert!(
        Path::new(&archive.archived_instance_root)
            .join("saves/world.dat")
            .is_file()
    );
}

#[tokio::test]
async fn archive_shared_program_must_exist_with_original_identity_before_restoring() {
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
    fs::remove_file(
        fixture
            .instance_root
            .join("runtime/.langame-private-runtime"),
    )
    .unwrap();
    fs::remove_dir(fixture.instance_root.join("runtime")).unwrap();
    let program = fixture.paths.games_root.join("shared-program");
    fs::create_dir(&program).unwrap();
    fs::write(program.join("server.fixture"), b"shared program").unwrap();
    crate::record_library_program_baseline(&program, &descriptor, true, None).unwrap();
    crate::program_runtime::prepare_shared_program_reference(
        &program,
        &fixture.instance_root,
        "archive-fixture",
    )
    .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE game_installs SET scope='library',owner_instance_id=NULL,install_root=?1 WHERE id=17")
        .bind(program.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    sqlx::query("UPDATE instances SET runtime_mode='shared' WHERE id='archive-instance'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    fixture.archive().await.unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let disconnected = fixture.root.join("disconnected-program");
    fs::rename(&program, &disconnected).unwrap();
    assert!(
        !list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_restore
    );
    assert!(restore_instance_archive(&fixture.paths, &id).await.is_err());
    fs::rename(&disconnected, &program).unwrap();
    restore_instance_archive(&fixture.paths, &id).await.unwrap();
    assert_eq!(
        fixture.snapshot().await.tables["instances"][0]["runtime_mode"],
        "shared"
    );
    assert_eq!(
        fs::read(program.join("server.fixture")).unwrap(),
        b"shared program"
    );
}

#[tokio::test]
async fn archive_purge_refuses_a_registered_instance_path() {
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/referenced-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("world.dat"), b"registered save").unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET saves_path=?1 WHERE id='archive-instance'")
        .bind(historical.to_string_lossy().as_ref())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        !list_instance_archives(&fixture.paths)
            .await
            .unwrap()
            .archives[0]
            .can_purge
    );
    assert!(purge_instance_archive(&fixture.paths, &id).await.is_err());
    assert_eq!(
        fs::read(historical.join("world.dat")).unwrap(),
        b"registered save"
    );
}

#[tokio::test]
async fn archive_purge_refuses_configured_steamcmd_storage_inside_archive() {
    let mut fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/program-owner");
    fixture.paths.steamcmd_root = historical.join("steamcmd");
    fs::create_dir_all(&fixture.paths.steamcmd_root).unwrap();
    let sentinel = fixture.paths.steamcmd_root.join("steamcmd.fixture");
    fs::write(&sentinel, b"configured application program").unwrap();
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert!(!archive.can_purge);
    assert!(
        purge_instance_archive(&fixture.paths, &archive.archive_id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"configured application program"
    );
}

#[tokio::test]
async fn archive_purge_refuses_external_saves_still_owned_by_another_archive() {
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/referenced-world");
    let external_saves = historical.join("worlds");
    fs::create_dir_all(&external_saves).unwrap();
    fs::write(external_saves.join("world.dat"), b"archived external save").unwrap();
    let historical_id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let archived = fixture.archive().await.unwrap();
    // Model an archive created before archive roots became exclusive. New
    // admission correctly rejects this layout, but old recovery metadata must
    // still protect the referenced world from another archive's purge.
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let mut snapshot =
        store::snapshot(&store::load(&pool, &archived.archive_id).await.unwrap()).unwrap();
    snapshot.tables.get_mut("instances").unwrap()[0].insert(
        "saves_path".into(),
        serde_json::json!(external_saves.to_string_lossy()),
    );
    snapshot.effective_saves_path = Some(external_saves.to_string_lossy().into_owned());
    let snapshot_json = serde_json::to_string(&snapshot).unwrap();
    sqlx::query("UPDATE instance_archives SET preserved_external_saves_path=?1,snapshot_json=?2,snapshot_sha256=?3 WHERE archive_id=?4")
        .bind(external_saves.to_string_lossy().as_ref()).bind(&snapshot_json)
        .bind(store::digest(snapshot_json.as_bytes())).bind(&archived.archive_id)
        .execute(&pool).await.unwrap();
    pool.close().await;
    let archives = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives;
    assert_eq!(
        archives
            .iter()
            .find(|archive| archive.archive_id == archived.archive_id)
            .unwrap()
            .preserved_external_saves_path
            .as_deref(),
        Some(external_saves.to_string_lossy().as_ref()),
    );
    assert!(
        !archives
            .iter()
            .find(|archive| archive.archive_id == historical_id)
            .unwrap()
            .can_purge
    );
    assert!(
        purge_instance_archive(&fixture.paths, &historical_id)
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(external_saves.join("world.dat")).unwrap(),
        b"archived external save"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn archive_purge_keeps_failed_os_cleanup_pending_and_can_retry() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/locked-world");
    fs::create_dir_all(&historical).unwrap();
    let locked = historical.join("locked.dat");
    fs::write(&locked, b"open handle").unwrap();
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .open(&locked)
        .unwrap();
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    assert!(purge_instance_archive(&fixture.paths, &id).await.is_err());
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert_eq!(archive.state, InstanceArchiveState::Purging);
    assert!(!archive.issues.is_empty());
    assert_eq!(fs::read(&locked).unwrap(), b"open handle");
    drop(blocker);
    assert!(
        purge_instance_archive(&fixture.paths, &id)
            .await
            .unwrap()
            .purged
    );
    assert!(!historical.exists());
}

#[cfg(windows)]
#[tokio::test]
async fn archive_purge_preflights_every_link_before_deleting_and_remains_retryable() {
    let fixture = Fixture::new().await;
    let historical = fixture.paths.instances_root.join(".trash/previous-world");
    fs::create_dir_all(&historical).unwrap();
    fs::write(historical.join("owned.dat"), b"owned").unwrap();
    let external = fixture.root.join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("external.dat"), b"external").unwrap();
    let link = historical.join("external-link");
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(external.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let id = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives[0]
        .archive_id
        .clone();
    let result = purge_instance_archive(&fixture.paths, &id).await;
    assert!(result.is_err());
    assert_eq!(fs::read(historical.join("owned.dat")).unwrap(), b"owned");
    assert_eq!(
        fs::read(external.join("external.dat")).unwrap(),
        b"external"
    );
    fs::remove_dir(&link).unwrap();
    let archive = list_instance_archives(&fixture.paths)
        .await
        .unwrap()
        .archives
        .remove(0);
    assert_eq!(archive.state, InstanceArchiveState::MissingMetadata);
    assert!(
        purge_instance_archive(&fixture.paths, &id)
            .await
            .unwrap()
            .purged
    );
    assert!(!historical.exists());
    assert_eq!(
        fs::read(external.join("external.dat")).unwrap(),
        b"external"
    );
}
