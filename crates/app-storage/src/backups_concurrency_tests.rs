use super::*;
use crate::instance_archive::test_gate::{self, Point};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    saves: PathBuf,
    backup: InstanceBackupResult,
}

impl Fixture {
    async fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("backup-concurrency-{}", uuid::Uuid::new_v4()));
        let paths = StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db.sqlite"),
            logs_root: root.join("logs"),
            modules_root: root.join("modules"),
            migrations_root: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances/.trash"),
        };
        let instance = paths.instances_root.join("backup-instance");
        for path in [
            &paths.app_data_root,
            &paths.modules_root,
            &paths.games_root,
            &instance.join("config"),
            &instance.join("runtime"),
            &instance.join("saves"),
        ] {
            fs::create_dir_all(path).unwrap();
        }
        let module = paths.modules_root.join("backup-fixture");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.toml"),
            r#"
id = "backup-fixture"
name = "Backup fixture"
version = "1.0.0"
[install]
shared_game_dir = "backup-fixture"
[process]
executable = "server.bin"
[storage]
saves_path_template = "{{paths.instance_root}}/saves"
"#,
        )
        .unwrap();
        fs::write(
            module.join("schema.json"),
            br#"{"type":"object","properties":{}}"#,
        )
        .unwrap();
        fs::write(
            instance.join("runtime/.langame-private-runtime"),
            b"managed\n",
        )
        .unwrap();
        fs::write(
            instance.join("config/instance.json"),
            br#"{"settings":{},"ports":[]}"#,
        )
        .unwrap();
        let saves = instance.join("saves");
        fs::write(saves.join("world"), b"earlier world").unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query(
            "INSERT INTO modules(id,name,version) VALUES('backup-fixture','Backup fixture','1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('backup-instance','Backup instance','backup-fixture',?1,?2,?3,?4)")
            .bind(instance.join("data").to_string_lossy().as_ref()).bind(instance.join("config").to_string_lossy().as_ref())
            .bind(instance.join("logs").to_string_lossy().as_ref()).bind(saves.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO game_installs(module_id,install_root,scope,owner_instance_id) VALUES('backup-fixture',?1,'instance','backup-instance')")
            .bind(instance.join("runtime").to_string_lossy().as_ref()).execute(&pool).await.unwrap();
        sqlx::query("UPDATE instances SET install_id=(SELECT id FROM game_installs WHERE owner_instance_id='backup-instance') WHERE id='backup-instance'").execute(&pool).await.unwrap();
        pool.close().await;
        let backup = create_instance_backup(&paths, "backup-instance")
            .await
            .unwrap();
        fs::write(saves.join("world"), b"current world").unwrap();
        Self {
            root,
            paths,
            saves,
            backup,
        }
    }

    fn restore(
        &self,
    ) -> tokio::task::JoinHandle<Result<InstanceBackupRestoreResult, StorageError>> {
        let paths = self.paths.clone();
        let id = self.backup.backup_id.clone();
        tokio::spawn(async move { restore_instance_backup(&paths, "backup-instance", &id).await })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn backup_creation_rejects_every_active_transition_without_changing_saved_data() {
    let fixture = Box::pin(Fixture::new()).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    for status in ["starting", "running", "stopping"] {
        sqlx::query("UPDATE instances SET status=?1 WHERE id='backup-instance'")
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
        assert!(
            Box::pin(create_instance_backup(&fixture.paths, "backup-instance"))
                .await
                .is_err(),
            "manual backup must reject {status} until the game is stopped"
        );
        assert!(
            Box::pin(create_instance_auto_stop_backup(
                &fixture.paths,
                "backup-instance"
            ))
            .await
            .is_err(),
            "auto-stop backup must also verify that {status} has actually ended"
        );
        assert!(
            Box::pin(create_instance_pre_restore_backup(
                &fixture.paths,
                "backup-instance",
                &fixture.backup.backup_id
            ))
            .await
            .is_err(),
            "a safeguard must not copy a live {status} world"
        );
    }
    pool.close().await;
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"current world"
    );
    let backups = list_instance_backups(&fixture.paths, "backup-instance")
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].backup_id, fixture.backup.backup_id);
    assert_eq!(
        fs::read(Path::new(&fixture.backup.backup_path).join("saves/world")).unwrap(),
        b"earlier world"
    );
}

#[tokio::test]
async fn backup_creation_rejects_active_run_with_a_stale_stopped_summary() {
    let fixture = Box::pin(Fixture::new()).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query(
        "INSERT INTO instance_runs(instance_id,status,pid) VALUES('backup-instance','running',123)",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
    assert!(
        Box::pin(create_instance_backup(&fixture.paths, "backup-instance"))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"current world"
    );
    assert_eq!(
        list_instance_backups(&fixture.paths, "backup-instance")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn backup_creation_refuses_missing_saves_without_pruning_the_last_backup() {
    let fixture = Box::pin(Fixture::new()).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET backup_retention_count=1 WHERE id='backup-instance'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    fs::remove_dir_all(&fixture.saves).unwrap();
    assert!(
        Box::pin(create_instance_backup(&fixture.paths, "backup-instance"))
            .await
            .is_err()
    );
    assert!(
        !fixture.saves.exists(),
        "a missing save root must never be replaced by an empty one"
    );
    let backups = list_instance_backups(&fixture.paths, "backup-instance")
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(backups[0].backup_id, fixture.backup.backup_id);
    assert_eq!(
        fs::read(Path::new(&fixture.backup.backup_path).join("saves/world")).unwrap(),
        b"earlier world"
    );
}

async fn write_unrelated(paths: &StoragePaths, id: &str) -> Result<(), sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .busy_timeout(Duration::from_millis(100));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = async {
        sqlx::query("INSERT INTO modules(id,name,version) VALUES(?1,'Parallel download','1')").bind(id).execute(&mut connection).await?;
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope) VALUES(?1,?2,'installed','library')")
            .bind(id).bind(paths.games_root.join(id).to_string_lossy().as_ref()).execute(&mut connection).await?;
        Ok(())
    }.await;
    connection.close().await?;
    result
}

#[tokio::test]
async fn backup_restore_copy_and_cleanup_allow_unrelated_writes_but_publication_is_exclusive() {
    let fixture = Fixture::new().await;
    let mut preparing = test_gate::register(&fixture.saves, Point::BackupPreparing);
    let mut publishing = test_gate::register(&fixture.saves, Point::BackupPublishing);
    let mut cleanup = test_gate::register(&fixture.saves, Point::BackupCleanup);
    let worker = fixture.restore();
    preparing.reached().await;
    let copied = write_unrelated(&fixture.paths, "copy-download").await;
    assert!(matches!(
        acquire_instance_settings_mutation_lock(&fixture.paths, "backup-instance"),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    preparing.resume();
    publishing.reached().await;
    let fenced = write_unrelated(&fixture.paths, "publish-download").await;
    publishing.resume();
    cleanup.reached().await;
    let cleaned = write_unrelated(&fixture.paths, "cleanup-download").await;
    cleanup.resume();
    let restored = worker.await.unwrap().unwrap();
    copied.unwrap();
    cleaned.unwrap();
    assert_eq!(
        fenced
            .unwrap_err()
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("5")
    );
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"earlier world"
    );
    assert_eq!(
        fs::read(Path::new(&restored.safeguard_backup_path).join("saves/world")).unwrap(),
        b"current world"
    );
}

#[tokio::test]
async fn backup_restore_rechecks_ownership_before_replacing_live_saves() {
    let fixture = Fixture::new().await;
    let mut preparing = test_gate::register(&fixture.saves, Point::BackupPreparing);
    let worker = fixture.restore();
    preparing.reached().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instances SET status='running' WHERE id='backup-instance'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    preparing.resume();
    assert!(
        worker
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("runtime state changed")
    );
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"current world"
    );
    assert!(acquire_instance_settings_mutation_lock(&fixture.paths, "backup-instance").is_ok());
    assert!(
        !fs::read_dir(fixture.saves.parent().unwrap())
            .unwrap()
            .any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".restore-publish"))
    );
}

#[tokio::test]
async fn cancelled_backup_waiter_keeps_publication_coordinator_and_instance_lease() {
    let fixture = Fixture::new().await;
    let mut preparing = test_gate::register(&fixture.saves, Point::BackupPreparing);
    let mut cleanup = test_gate::register(&fixture.saves, Point::BackupCleanup);
    let worker = fixture.restore();
    preparing.reached().await;
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(matches!(
        acquire_instance_settings_mutation_lock(&fixture.paths, "backup-instance"),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    preparing.resume();
    cleanup.reached().await;
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"earlier world"
    );
    cleanup.resume();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if acquire_instance_settings_mutation_lock(&fixture.paths, "backup-instance").is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn interrupt_fixture_restore(fixture: &Fixture) {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "backups::tests::backup_restore_interruption_child",
            "--nocapture",
        ])
        .env("LANGAME_BACKUP_RESTORE_CRASH_ROOT", &fixture.root)
        .env("LANGAME_BACKUP_RESTORE_CRASH_TARGET", &fixture.saves)
        .env(
            "LANGAME_BACKUP_RESTORE_CRASH_SOURCE",
            Path::new(&fixture.backup.backup_path).join("saves"),
        )
        .env("LANGAME_BACKUP_RESTORE_CRASH_STAGE", "displaced")
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(73),
        "{}",
        String::from_utf8_lossy(&child.stdout)
    );
}

#[tokio::test]
async fn interrupted_backup_restore_blocks_configuration_and_live_access_until_recovered() {
    let fixture = Box::pin(Fixture::new()).await;
    let input = || UpdateInstanceInput {
        id: "backup-instance".to_owned(),
        bind_ip: "0.0.0.0".to_owned(),
        auto_backup_on_stop: false,
        backup_retention_count: 3,
        settings_json: "{}".to_owned(),
        ports: Vec::new(),
    };
    // Prove that the same valid configuration succeeds before interruption.
    Box::pin(crate::update_instance(&fixture.paths, input()))
        .await
        .unwrap();
    Box::pin(crate::materialize_instance_configuration(
        &fixture.paths,
        "backup-instance",
    ))
    .await
    .unwrap();
    let config = fixture.saves.parent().unwrap().join("config/instance.json");
    let before = fs::read(&config).unwrap();
    interrupt_fixture_restore(&fixture);
    assert!(!fixture.saves.exists());
    let error = Box::pin(crate::update_instance(&fixture.paths, input()))
        .await
        .expect_err("configuration must not change a pending restore's save location");
    assert!(
        error.to_string().contains("interrupted backup restore"),
        "{error}"
    );
    let error = Box::pin(crate::materialize_instance_configuration(
        &fixture.paths,
        "backup-instance",
    ))
    .await
    .expect_err("startup materialization must not recreate a displaced world");
    assert!(
        error.to_string().contains("interrupted backup restore"),
        "{error}"
    );
    assert_eq!(fs::read(&config).unwrap(), before);
    assert!(!fixture.saves.exists());
    assert!(
        Box::pin(ensure_instance_backup_ready(
            &fixture.paths,
            "backup-instance"
        ))
        .await
        .is_err()
    );
    assert!(
        Box::pin(list_instance_save_files(
            &fixture.paths,
            "backup-instance",
            None
        ))
        .await
        .is_err()
    );
    assert!(
        Box::pin(create_instance_backup(&fixture.paths, "backup-instance"))
            .await
            .is_err()
    );
    assert!(
        Box::pin(list_instance_save_files(
            &fixture.paths,
            "backup-instance",
            Some(&fixture.backup.backup_id)
        ))
        .await
        .is_ok()
    );
    Box::pin(recover_instance_backup_restore(
        &fixture.paths,
        "backup-instance",
    ))
    .await
    .unwrap();
    Box::pin(ensure_instance_backup_ready(
        &fixture.paths,
        "backup-instance",
    ))
    .await
    .unwrap();
    assert_eq!(
        fs::read(fixture.saves.join("world")).unwrap(),
        b"current world"
    );
    assert_eq!(
        fs::read(Path::new(&fixture.backup.backup_path).join("saves/world")).unwrap(),
        b"earlier world"
    );
}

#[tokio::test]
async fn interrupted_backup_restore_retains_changed_recovery_copies_and_blocks_startup() {
    let fixture = Box::pin(Fixture::new()).await;
    interrupt_fixture_restore(&fixture);
    let rollback = fs::read_dir(fixture.saves.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".restore-rollback-")
        })
        .unwrap();
    fs::write(rollback.join("world"), b"changed recovery world").unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(fixture.saves.parent().unwrap()).unwrap();
    let error = Box::pin(recover_instance_backup_restore(
        &fixture.paths,
        "backup-instance",
    ))
    .await
    .expect_err("unverified recovery contents must not be published or discarded");
    assert!(error.to_string().contains("changed"), "{error}");
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(fixture.saves.parent().unwrap()).unwrap(),
        before
    );
    assert!(
        Box::pin(ensure_instance_backup_ready(
            &fixture.paths,
            "backup-instance"
        ))
        .await
        .is_err()
    );
    assert!(!fixture.saves.exists());
}
