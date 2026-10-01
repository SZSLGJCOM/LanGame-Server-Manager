use std::time::Duration;

use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};

use super::*;
use crate::instance_archive::test_gate::{self, Point};

async fn write_unrelated_install(paths: &StoragePaths) -> Result<(), sqlx::Error> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.database_path)
        .busy_timeout(Duration::from_millis(100));
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let result = async {
        sqlx::query("INSERT INTO modules(id,name,version) VALUES('parallel-download','Parallel download','1')")
            .execute(&mut connection).await?;
        sqlx::query("INSERT INTO game_installs(module_id,install_root,install_state,scope) VALUES('parallel-download',?1,'installed','library')")
            .bind(paths.games_root.join("parallel-download").to_string_lossy().as_ref())
            .execute(&mut connection).await?;
        Ok(())
    }.await;
    connection.close().await?;
    result
}

#[tokio::test]
async fn archive_file_work_allows_unrelated_database_writes() {
    let (fixture, _) = operations::with_program().await;
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap();
    let mut pause = test_gate::register(&fixture.paths.database_path, Point::Archiving);
    let paths = fixture.paths.clone();
    let worker =
        tokio::spawn(async move { crate::archive_instance(&paths, "archive-instance").await });
    pause.reached().await;
    let written = write_unrelated_install(&fixture.paths).await;
    pause.resume();
    let archive = worker.await.unwrap().unwrap();
    written.expect("an archive file worker must not hold SQLite's write transaction");
    restore_instance_archive(&fixture.paths, &archive.archive_id)
        .await
        .unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.instance_root).unwrap(),
        before
    );
}

#[tokio::test]
async fn permanent_deletion_keeps_its_existing_database_transaction_boundary() {
    let fixture = Fixture::new().await;
    let mut pause = test_gate::register(&fixture.paths.database_path, Point::Archiving);
    let paths = fixture.paths.clone();
    let worker =
        tokio::spawn(async move { crate::delete_instance(&paths, "archive-instance").await });
    pause.reached().await;
    let written = write_unrelated_install(&fixture.paths).await;
    pause.resume();
    worker.await.unwrap().unwrap();
    let error = written.expect_err("permanent deletion retains its original write transaction");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("5")
    );
    assert!(!fixture.instance_root.exists());
}

#[tokio::test]
async fn restoration_file_work_allows_unrelated_writes_and_reserves_instance_paths() {
    let (fixture, library) = external_programs::exclusive_fixture(true).await;
    let before = crate::test_file_snapshot::tree_snapshot(&library).unwrap();
    let archived = fixture.archive().await.unwrap();
    let mut pause = test_gate::register(&fixture.paths.database_path, Point::Restoring);
    let paths = fixture.paths.clone();
    let id = archived.archive_id.clone();
    let worker = tokio::spawn(async move { restore_instance_archive(&paths, &id).await });
    pause.reached().await;
    let written = write_unrelated_install(&fixture.paths).await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let peer = fixture.paths.instances_root.join("peer");
    let conflict = crate::instance_isolation::ensure_instance_paths_available(
        &fixture.paths,
        &mut tx,
        "peer",
        "parallel-download",
        &fixture.paths.games_root.join("other"),
        &peer.join("config"),
        &library.join("native"),
    )
    .await;
    let unrelated = crate::instance_isolation::ensure_instance_paths_available(
        &fixture.paths,
        &mut tx,
        "peer",
        "parallel-download",
        &fixture.paths.games_root.join("other"),
        &peer.join("config"),
        &peer.join("saves"),
    )
    .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    pause.resume();
    worker.await.unwrap().unwrap();
    written.expect("archive reconstruction must not hold SQLite's write transaction");
    assert!(
        matches!(conflict, Err(StorageError::InstancePathConflict { .. })),
        "{conflict:?}"
    );
    unrelated.unwrap();
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&library).unwrap(),
        before
    );
}

#[tokio::test]
async fn archive_resource_reads_and_unrelated_dependency_checks_do_not_take_inventory_lock() {
    let (fixture, library) = operations::with_program().await;
    let active = crate::read_instance_retirement_resources(&fixture.paths, "archive-instance")
        .await
        .unwrap();
    assert_eq!(active.instance_id.as_deref(), Some("archive-instance"));
    assert_eq!(active.module_id.as_deref(), Some("archive-fixture"));
    assert!(
        active.program_roots.contains(
            &crate::instance_isolation::paths::normalize_resource_path(&library).unwrap()
        )
    );
    let archived = fixture.archive().await.unwrap();
    let _inventory = inventory_lock(&fixture.paths).unwrap();
    let resources = crate::read_instance_archive_resources(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert!(
        resources.program_roots.contains(
            &crate::instance_isolation::paths::normalize_resource_path(&library).unwrap()
        )
    );
    assert!(
        resources
            .program_roots
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    let other = fixture.paths.games_root.join("unrelated-download");
    external::ensure_program_archive_dependencies(&fixture.paths, &other)
        .await
        .unwrap();
    assert!(
        external::ensure_program_archive_dependencies(&fixture.paths, &library)
            .await
            .is_err()
    );
    let pool = connect_pool(&fixture.paths).await.unwrap();
    store::set_state(&pool, &archived.archive_id, "restoring", None)
        .await
        .unwrap();
    pool.close().await;
    assert_eq!(
        crate::read_instance_retirement_resources(&fixture.paths, "archive-instance")
            .await
            .unwrap(),
        resources
    );
}

#[tokio::test]
async fn archive_cleanup_resources_preserve_existing_corrupt_snapshot_contract() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let expected =
        crate::read_instance_archive_cleanup_resources(&fixture.paths, &archived.archive_id)
            .await
            .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    sqlx::query("UPDATE instance_archives SET snapshot_json='corrupt' WHERE archive_id=?1")
        .bind(&archived.archive_id)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(
        crate::read_instance_archive_resources(&fixture.paths, &archived.archive_id)
            .await
            .is_err()
    );
    assert_eq!(
        crate::read_instance_archive_cleanup_resources(&fixture.paths, &archived.archive_id)
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        expected.program_roots,
        vec![
            crate::instance_isolation::paths::normalize_resource_path(&PathBuf::from(
                archived.archived_instance_root.unwrap()
            ))
            .unwrap()
        ]
    );
    // Metadata reading neither removes data nor grants permission to clear it.
    assert!(expected.program_roots[0].join("saves/world.dat").is_file());
}

#[tokio::test]
async fn archive_dependency_check_keeps_a_consistent_snapshot_during_unrelated_compensation() {
    let fixture = Fixture::new().await;
    let archived = fixture.archive().await.unwrap();
    let mut pause = test_gate::register(&fixture.paths.database_path, Point::Dependencies);
    let paths = fixture.paths.clone();
    let other = fixture.paths.games_root.join("unrelated-download");
    let worker =
        tokio::spawn(
            async move { external::ensure_program_archive_dependencies(&paths, &other).await },
        );
    pause.reached().await;
    let pool = connect_pool(&fixture.paths).await.unwrap();
    let removed = sqlx::query("DELETE FROM instance_archives WHERE archive_id=?1")
        .bind(&archived.archive_id)
        .execute(&pool)
        .await;
    pool.close().await;
    pause.resume();
    worker.await.unwrap().unwrap();
    assert_eq!(removed.unwrap().rows_affected(), 1);
    assert!(
        Path::new(archived.archived_instance_root.as_ref().unwrap())
            .join("saves/world.dat")
            .is_file()
    );
}
