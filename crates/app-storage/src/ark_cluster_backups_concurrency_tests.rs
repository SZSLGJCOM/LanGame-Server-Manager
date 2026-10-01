use super::*;
use crate::instance_archive::test_gate::{self, Point};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::time::Duration;

impl Fixture {
    async fn database() -> (Self, StoragePaths) {
        let mut fixture = Self::new();
        let mut paths = fixture_paths(&fixture.root);
        paths.modules_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules");
        paths.migrations_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
        fs::create_dir_all(&paths.app_data_root).unwrap();
        fs::create_dir_all(&paths.games_root).unwrap();
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query(
            "INSERT INTO modules(id,name,version) VALUES('arksurvivalevolved','ARK fixture','1')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (index, member) in fixture.plan.report.members.iter_mut().enumerate() {
            let config = Path::new(&member.config_file_path).parent().unwrap();
            let root = config.parent().unwrap();
            let runtime = root.join("runtime");
            let saves = runtime.join("ShooterGame/Saved").join(&member.summary.id);
            fs::create_dir_all(&saves).unwrap();
            fs::write(runtime.join(".langame-private-runtime"), b"managed\n").unwrap();
            member.saves_path = saves.to_string_lossy().into_owned();
            member.ports[0].port = 7777 + index as u16 * 10;
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&member.config_file_path).unwrap()).unwrap();
            value["ports"] = serde_json::to_value(&member.ports).unwrap();
            fs::write(
                &member.config_file_path,
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
            sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES(?1,?1,'arksurvivalevolved',?2,?3,?4,?5)")
                .bind(&member.summary.id).bind(root.join("data").to_string_lossy().as_ref()).bind(config.to_string_lossy().as_ref())
                .bind(root.join("logs").to_string_lossy().as_ref()).bind(&member.saves_path).execute(&pool).await.unwrap();
            let install = sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope,owner_instance_id) VALUES('arksurvivalevolved',?1,'installed','instance',?2)")
                .bind(runtime.to_string_lossy().as_ref()).bind(&member.summary.id).execute(&pool).await.unwrap().last_insert_rowid();
            sqlx::query("UPDATE instances SET install_id=?2 WHERE id=?1")
                .bind(&member.summary.id)
                .bind(install)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO instance_ports(instance_id,name,port,protocol) VALUES(?1,'game',?2,'udp')")
                .bind(&member.summary.id).bind(i64::from(member.ports[0].port)).execute(&pool).await.unwrap();
        }
        pool.close().await;
        fixture.plan.report = read_ark_cluster_report(&paths, "island").await.unwrap();
        assert!(
            !fixture.plan.report.start_blocked,
            "{:?}",
            fixture.plan.report.issues
        );
        fixture.plan.root =
            backup_root(&paths, fixture.plan.report.identity.as_ref().unwrap()).unwrap();
        fs::create_dir_all(&fixture.plan.root).unwrap();
        (fixture, paths)
    }
}

async fn unrelated_write(paths: &StoragePaths, id: &str) -> Result<(), sqlx::Error> {
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
async fn cluster_snapshot_and_restore_file_work_allow_unrelated_database_writes() {
    let (fixture, paths) = Fixture::database().await;
    let original = fixture.inventory();
    let identity = fixture.plan.report.identity.clone().unwrap();
    let mut pause = test_gate::register(&fixture.plan.root, Point::BackupPreparing);
    let worker_paths = paths.clone();
    let worker_identity = identity.clone();
    let worker = tokio::spawn(async move {
        create_ark_cluster_backup(&worker_paths, "island", &worker_identity, true).await
    });
    pause.reached().await;
    let written = unrelated_write(&paths, "cluster-snapshot-download").await;
    pause.resume();
    let backup = worker.await.unwrap().unwrap();
    written.unwrap();
    fixture.change();
    let mut pause = test_gate::register(&fixture.plan.root, Point::BackupPreparing);
    let mut cleanup = test_gate::register(&fixture.plan.root, Point::BackupCleanup);
    let worker_paths = paths.clone();
    let worker = tokio::spawn(async move {
        restore_ark_cluster_backup(&worker_paths, "island", &identity, &backup.backup_id, true)
            .await
    });
    pause.reached().await;
    let copied = unrelated_write(&paths, "cluster-restore-download").await;
    pause.resume();
    cleanup.reached().await;
    let cleaned = unrelated_write(&paths, "cluster-cleanup-download").await;
    cleanup.resume();
    worker.await.unwrap().unwrap();
    copied.unwrap();
    cleaned.unwrap();
    assert_eq!(fixture.inventory(), original);
}

#[tokio::test]
async fn cluster_restore_rejects_transfer_ownership_claimed_during_preparation() {
    let (fixture, paths) = Fixture::database().await;
    let identity = fixture.plan.report.identity.clone().unwrap();
    let backup = create_ark_cluster_backup(&paths, "island", &identity, true)
        .await
        .unwrap();
    fixture.change();
    let before = fixture.inventory();
    let mut pause = test_gate::register(&fixture.plan.root, Point::BackupPreparing);
    let worker_paths = paths.clone();
    let worker = tokio::spawn(async move {
        restore_ark_cluster_backup(&worker_paths, "island", &identity, &backup.backup_id, true)
            .await
    });
    pause.reached().await;
    let peer = paths.instances_root.join("peer");
    fs::create_dir_all(peer.join("config")).unwrap();
    fs::create_dir_all(peer.join("runtime")).unwrap();
    fs::write(peer.join("runtime/.langame-private-runtime"), b"managed\n").unwrap();
    fs::write(
        peer.join("config/instance.json"),
        br#"{"settings":{},"ports":[]}"#,
    )
    .unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    sqlx::query("INSERT INTO modules(id,name,version) VALUES('peer-fixture','Peer','1')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO instances(id,name,module_id,data_path,config_path,logs_path,saves_path) VALUES('peer','Peer','peer-fixture',?1,?2,?3,?4)")
        .bind(peer.join("data").to_string_lossy().as_ref()).bind(peer.join("config").to_string_lossy().as_ref())
        .bind(peer.join("logs").to_string_lossy().as_ref()).bind(fixture.plan.scopes.last().unwrap().target.to_string_lossy().as_ref()).execute(&pool).await.unwrap();
    pool.close().await;
    pause.resume();
    assert!(worker.await.unwrap().is_err());
    assert_eq!(fixture.inventory(), before);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
}

#[tokio::test]
async fn cluster_compensation_hashing_releases_writer_and_preserves_every_scope() {
    let (fixture, paths) = Fixture::database().await;
    let identity = fixture.plan.report.identity.clone().unwrap();
    let backup = create_ark_cluster_backup(&paths, "island", &identity, true)
        .await
        .unwrap();
    fixture.change();
    let before = fixture.inventory();
    let mut pause = test_gate::register(&fixture.plan.root, Point::BackupRollback);
    let worker_paths = paths.clone();
    let worker = tokio::spawn(async move {
        operate(
            &worker_paths,
            "island",
            &identity,
            true,
            None,
            move |plan, publication| {
                restore::restore_with_publication(
                    &plan,
                    &backup.backup_id,
                    publication,
                    |phase, index| {
                        if phase == restore::Phase::Published && index == 2 {
                            Err(invalid(&plan.root, "injected group publication failure"))
                        } else {
                            Ok(())
                        }
                    },
                )
            },
        )
        .await
    });
    pause.reached().await;
    let written = unrelated_write(&paths, "cluster-rollback-download").await;
    pause.resume();
    assert!(
        worker
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("injected group publication failure")
    );
    written.unwrap();
    assert_eq!(fixture.inventory(), before);
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
}

#[tokio::test]
async fn cluster_interrupted_committed_recovery_checks_files_without_holding_writer() {
    let (fixture, paths) = Fixture::database().await;
    let identity = fixture.plan.report.identity.clone().unwrap();
    let backup = create_ark_cluster_backup(&paths, "island", &identity, true)
        .await
        .unwrap();
    fixture.change();
    let id = backup.backup_id.clone();
    let failed = operate(
        &paths,
        "island",
        &identity,
        true,
        None,
        move |plan, publication| {
            restore::restore_with_publication(&plan, &id, publication, |phase, _| {
                if phase == restore::Phase::Committed {
                    Err(invalid(&plan.root, "interrupted after commit"))
                } else {
                    Ok(())
                }
            })
        },
    )
    .await;
    assert!(failed.is_err());
    assert!(fixture.plan.root.join(recovery::JOURNAL_NAME).exists());
    let mut pause = test_gate::register(&fixture.plan.root, Point::BackupPreparing);
    let worker_paths = paths.clone();
    let worker = tokio::spawn(async move {
        recover_ark_cluster_restore(&worker_paths, "island", &identity, &backup.backup_id, true)
            .await
    });
    pause.reached().await;
    let written = unrelated_write(&paths, "cluster-recovery-download").await;
    pause.resume();
    assert_eq!(worker.await.unwrap().unwrap().outcome, "completed");
    written.unwrap();
    assert!(transaction::ensure_ready(&fixture.plan.root).is_ok());
}

#[tokio::test]
async fn cluster_durable_commit_is_not_rolled_back_after_publication_release_failure() {
    let (fixture, paths) = Fixture::database().await;
    let identity = fixture.plan.report.identity.clone().unwrap();
    let original = fixture.inventory();
    let backup = create_ark_cluster_backup(&paths, "island", &identity, true)
        .await
        .unwrap();
    fixture.change();
    let id = backup.backup_id.clone();
    let failed = operate(
        &paths,
        "island",
        &identity,
        true,
        None,
        move |plan, publication| {
            restore::restore_with_publication(&plan, &id, publication, |phase, index| {
                if phase == restore::Phase::Published && index + 1 == plan.scopes.len() {
                    crate::backups::publication::fail_next_release(plan.scopes[0].target.clone());
                }
                Ok(())
            })
        },
    )
    .await
    .unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("Restore committed but publication release failed")
    );
    assert_eq!(fixture.inventory(), original);
    let journal: recovery::Journal = transaction::read_json(
        &fixture.plan.root.join(recovery::JOURNAL_NAME),
        2 * 1024 * 1024,
    )
    .unwrap();
    assert!(journal.committed);
    let result = recover_ark_cluster_restore(&paths, "island", &identity, &backup.backup_id, true)
        .await
        .unwrap();
    assert_eq!(result.outcome, "completed");
    assert_eq!(fixture.inventory(), original);
}

#[test]
fn cluster_admission_stamp_failure_cleans_stages_before_a_journal_exists() {
    let fixture = Fixture::new();
    let backup = transaction::snapshot(&fixture.plan, "manual").unwrap();
    let failed =
        restore::restore_with_hook(&fixture.plan, &backup.summary.backup_id, |phase, index| {
            if phase == restore::Phase::Staged && index + 1 == fixture.plan.scopes.len() {
                let mut directory = fixture.plan.scopes[1].target.clone();
                for _ in 0..65 {
                    directory = directory.join("d");
                }
                fs::create_dir_all(directory).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("Restore admission failed before publication")
    );
    assert!(!fixture.plan.root.join(recovery::JOURNAL_NAME).exists());
    for scope in &fixture.plan.scopes {
        assert!(
            !fs::read_dir(scope.target.parent().unwrap())
                .unwrap()
                .any(|entry| {
                    entry
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".langame-ark-restore-")
                })
        );
    }
}
