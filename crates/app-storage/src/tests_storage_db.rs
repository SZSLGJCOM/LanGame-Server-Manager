use super::{cleanup_root, test_paths, unique_test_root};
use crate::storage_db::{
    connect_pool, install_storage_before_init_open_hook_for_test,
    storage_full_readiness_passes_for_test,
};
use crate::{MIGRATOR, StorageError, initialize_database};
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn storage_status_serializes_the_structured_app_log_path() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let status = paths.probe_status();
    let serialized = serde_json::to_value(&status).unwrap();

    assert_eq!(status.app_log_path, paths.app_log_path().to_string_lossy());
    assert_eq!(
        serialized["app_log_path"],
        paths.app_log_path().to_string_lossy().as_ref()
    );
}

#[tokio::test]
async fn instances_schema_contains_only_local_runtime_fields() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    let columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('instances') ORDER BY cid")
            .fetch_all(&pool)
            .await
            .unwrap();
    pool.close().await;

    assert_eq!(
        columns.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "id",
            "name",
            "module_id",
            "bind_ip",
            "install_id",
            "status",
            "data_path",
            "config_path",
            "logs_path",
            "saves_path",
            "env_json",
            "args_json",
            "autostart",
            "crash_restart_limit",
            "auto_backup_on_stop",
            "backup_retention_count",
            "created_at",
            "updated_at",
            "runtime_mode",
        ]
    );
}

#[tokio::test]
async fn ready_cache_reinitializes_a_deleted_database_at_the_same_path() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    initialize_database(&paths).await.unwrap();
    remove_sqlite_database_files(&paths.database_path);

    let status = initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    let applied_migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;

    assert_eq!(status.schema_version, latest_migration_version());
    assert_eq!(applied_migrations, canonical_migration_count());
    cleanup_root(&root);
}

#[tokio::test]
async fn ready_cache_validates_a_replacement_database_at_the_same_path() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let replacement_path = root.join("replacement.db");
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    initialize_database(&paths).await.unwrap();

    let latest_version = latest_migration_version();
    let replacement_pool = open_current_test_database(&replacement_path).await;
    sqlx::query(
        "INSERT INTO modules (id, name, version, supported_platforms) \
         VALUES ('replacement-sentinel', 'Replacement Sentinel', '1', 'windows')",
    )
    .execute(&replacement_pool)
    .await
    .unwrap();
    replacement_pool.close().await;

    remove_sqlite_database_files(&paths.database_path);
    fs::rename(&replacement_path, &paths.database_path).unwrap();

    let status = initialize_database(&paths).await.unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    let sentinel_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM modules WHERE id = 'replacement-sentinel'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let applied_migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;

    assert_eq!(status.schema_version, latest_version);
    assert_eq!(sentinel_count, 1);
    assert_eq!(applied_migrations, canonical_migration_count());
    cleanup_root(&root);
}

#[tokio::test]
async fn ready_cache_reuses_stable_identity_without_repeating_full_readiness_checks() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    initialize_database(&paths).await.unwrap();
    let readiness_passes = storage_full_readiness_passes_for_test(&paths.database_path);
    assert_eq!(readiness_passes, 1);

    for revision in 0..8 {
        let pool = connect_pool(&paths).await.unwrap();
        sqlx::query(
            "INSERT INTO app_settings (key, value_json) VALUES ('cache-probe', ?1) \
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, \
             updated_at = CURRENT_TIMESTAMP",
        )
        .bind(format!("{{\"revision\":{revision}}}"))
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    assert_eq!(
        storage_full_readiness_passes_for_test(&paths.database_path),
        readiness_passes,
        "ordinary data writes must not invalidate the stable file identity"
    );
    cleanup_root(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_ready_cache_invalidation_runs_one_full_readiness_pass() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    let replacement_path = root.join("replacement.db");
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    initialize_database(&paths).await.unwrap();
    let readiness_passes = storage_full_readiness_passes_for_test(&paths.database_path);
    let replacement_pool = open_current_test_database(&replacement_path).await;
    replacement_pool.close().await;
    remove_sqlite_database_files(&paths.database_path);
    fs::rename(&replacement_path, &paths.database_path).unwrap();

    const CONNECTOR_COUNT: usize = 8;
    let barrier = Arc::new(tokio::sync::Barrier::new(CONNECTOR_COUNT + 1));
    let tasks = (0..CONNECTOR_COUNT)
        .map(|_| {
            let paths = paths.clone();
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                let pool = connect_pool(&paths).await?;
                let schema_version = sqlx::query_scalar("PRAGMA user_version")
                    .fetch_one(&pool)
                    .await?;
                pool.close().await;
                Ok::<i64, StorageError>(schema_version)
            })
        })
        .collect::<Vec<_>>();
    barrier.wait().await;

    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap(), latest_migration_version());
    }
    assert_eq!(
        storage_full_readiness_passes_for_test(&paths.database_path),
        readiness_passes + 1,
        "concurrent invalidation must share the single initialization lock pass"
    );
    cleanup_root(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initialization_retries_when_database_is_deleted_between_identity_and_open() {
    let root = unique_test_root();
    let paths = test_paths(&root);
    fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();

    let uncached_pool = open_test_database(&paths.database_path).await;
    MIGRATOR.run(&uncached_pool).await.unwrap();
    uncached_pool.close().await;

    let barrier = Arc::new(std::sync::Barrier::new(2));
    let deletion_barrier = Arc::clone(&barrier);
    let database_path = paths.database_path.clone();
    let deletion = thread::spawn(move || {
        deletion_barrier.wait();
        remove_sqlite_database_files(&database_path);
        deletion_barrier.wait();
    });
    install_storage_before_init_open_hook_for_test(&paths.database_path, move || {
        barrier.wait();
        barrier.wait();
    });

    let status = initialize_database(&paths).await.unwrap();
    deletion.join().unwrap();
    let pool = connect_pool(&paths).await.unwrap();
    let applied_migrations: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations WHERE success = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    pool.close().await;

    assert_eq!(status.schema_version, latest_migration_version());
    assert_eq!(applied_migrations, canonical_migration_count());
    assert_eq!(
        storage_full_readiness_passes_for_test(&paths.database_path),
        1,
        "the failed existing-only open must retry before running migrations"
    );
    cleanup_root(&root);
}

#[tokio::test]
async fn separate_database_initialization_does_not_import_another_databases_wal() {
    let root = unique_test_root();
    let legacy_root = root.join("legacy");
    let target_root = root.join("target");
    let legacy_paths = test_paths(&legacy_root);
    let target_paths = test_paths(&target_root);

    fs::create_dir_all(legacy_paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&legacy_paths).await.unwrap();
    let legacy_pool = connect_pool(&legacy_paths).await.unwrap();
    sqlx::query("PRAGMA wal_autocheckpoint = 0;")
        .execute(&legacy_pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE migration_sentinel (value TEXT NOT NULL);")
        .execute(&legacy_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO migration_sentinel (value) VALUES ('committed-in-wal');")
        .execute(&legacy_pool)
        .await
        .unwrap();

    let legacy_wal = PathBuf::from(format!(
        "{}-wal",
        legacy_paths.database_path.to_string_lossy()
    ));
    assert!(
        fs::metadata(&legacy_wal)
            .map(|metadata| metadata.len() > 0)
            .unwrap_or(false),
        "migration regression requires a non-empty legacy WAL"
    );

    fs::create_dir_all(target_paths.database_path.parent().unwrap()).unwrap();
    initialize_database(&target_paths).await.unwrap();
    let target_pool = connect_pool(&target_paths).await.unwrap();
    let imported_tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'migration_sentinel'",
    )
    .fetch_one(&target_pool)
    .await
    .unwrap();
    assert_eq!(imported_tables, 0);
    let sentinel: String = sqlx::query_scalar("SELECT value FROM migration_sentinel LIMIT 1")
        .fetch_one(&legacy_pool)
        .await
        .unwrap();
    assert_eq!(sentinel, "committed-in-wal");
    assert!(legacy_paths.database_path.exists());
    assert!(target_paths.database_path.exists());

    target_pool.close().await;
    legacy_pool.close().await;
    cleanup_root(&root);
}

#[tokio::test]
async fn initialization_keeps_independent_database_contents_separate() {
    let root = unique_test_root();
    let legacy_paths = test_paths(&root.join("legacy"));
    let target_paths_without_legacy = test_paths(&root.join("target"));

    for (paths, value) in [
        (&legacy_paths, "legacy"),
        (&target_paths_without_legacy, "current-user"),
    ] {
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        initialize_database(paths).await.unwrap();
        let pool = connect_pool(paths).await.unwrap();
        sqlx::query("CREATE TABLE migration_precedence (value TEXT NOT NULL);")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO migration_precedence (value) VALUES (?1);")
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    let target_paths = target_paths_without_legacy;
    initialize_database(&target_paths).await.unwrap();
    let target_pool = connect_pool(&target_paths).await.unwrap();
    let value: String = sqlx::query_scalar("SELECT value FROM migration_precedence LIMIT 1;")
        .fetch_one(&target_pool)
        .await
        .unwrap();

    assert_eq!(value, "current-user");

    target_pool.close().await;
    cleanup_root(&root);
}

async fn open_test_database(database_path: &Path) -> SqlitePool {
    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(30));
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .unwrap()
}

async fn open_current_test_database(database_path: &Path) -> SqlitePool {
    let pool = open_test_database(database_path).await;
    MIGRATOR.run(&pool).await.unwrap();
    pool
}

fn latest_migration_version() -> i64 {
    MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .map(|migration| migration.version)
        .max()
        .unwrap()
}

fn canonical_migration_count() -> i64 {
    MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .count() as i64
}

fn remove_sqlite_database_files(database_path: &Path) {
    for path in [
        database_path.to_path_buf(),
        sqlite_sidecar_path(database_path, "-wal"),
        sqlite_sidecar_path(database_path, "-shm"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }
}

fn sqlite_sidecar_path(database_path: &Path, suffix: &str) -> PathBuf {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
}
