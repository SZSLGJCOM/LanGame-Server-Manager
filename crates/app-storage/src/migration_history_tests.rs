use super::*;
use crate::{StoragePaths, initialize_database};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test]
async fn canonical_history_starts_without_rewriting_migration_rows() {
    let fixture = TestDatabase::new("canonical-startup");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_sentinel(&pool).await;
    let before = history_snapshot(&pool).await;
    pool.close().await;
    initialize_database(&fixture.paths).await.unwrap();
    let pool = fixture.open().await;
    assert_eq!(history_snapshot(&pool).await, before);
    assert_sentinel_preserved(&pool).await;
    pool.close().await;
}

#[tokio::test]
async fn retired_history_is_refused_without_conversion() {
    for mutation in [
        "UPDATE _sqlx_migrations SET version = 11 WHERE version = 1; PRAGMA user_version = 11",
        "UPDATE _sqlx_migrations SET version = 6 WHERE version = 1; PRAGMA user_version = 6",
        "UPDATE _sqlx_migrations SET version = 7 WHERE version = 1; PRAGMA user_version = 7",
        "UPDATE _sqlx_migrations SET version = 8 WHERE version = 1; PRAGMA user_version = 8",
        "UPDATE _sqlx_migrations SET version = 9 WHERE version = 1; PRAGMA user_version = 9",
        "UPDATE _sqlx_migrations SET version = 10 WHERE version = 1; PRAGMA user_version = 10",
    ] {
        assert_refused_without_changes(mutation).await;
    }
}

#[tokio::test]
async fn unknown_checksum_is_refused_without_modifying_data_or_history() {
    assert_refused_without_changes("UPDATE _sqlx_migrations SET checksum = zeroblob(48)").await;
}

#[tokio::test]
async fn empty_history_is_refused_without_modifying_data_or_history() {
    assert_refused_without_changes("DELETE FROM _sqlx_migrations").await;
}

#[tokio::test]
async fn failed_history_is_refused_without_modifying_data_or_history() {
    assert_refused_without_changes("UPDATE _sqlx_migrations SET success = 0").await;
}

#[tokio::test]
async fn mismatched_description_is_refused_without_modifying_data_or_history() {
    assert_refused_without_changes("UPDATE _sqlx_migrations SET description = 'unrecognized'")
        .await;
}

#[tokio::test]
async fn schema_version_must_match_successful_history() {
    assert_refused_without_changes("PRAGMA user_version = 7").await;
}

#[tokio::test]
async fn history_verification_never_rewrites_metadata() {
    let fixture = TestDatabase::new("read-only-history");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_sentinel(&pool).await;
    for trigger in [
        "CREATE TRIGGER refuse_insert BEFORE INSERT ON _sqlx_migrations BEGIN SELECT RAISE(ABORT, 'history is read only'); END",
        "CREATE TRIGGER refuse_update BEFORE UPDATE ON _sqlx_migrations BEGIN SELECT RAISE(ABORT, 'history is read only'); END",
        "CREATE TRIGGER refuse_delete BEFORE DELETE ON _sqlx_migrations BEGIN SELECT RAISE(ABORT, 'history is read only'); END",
    ] {
        sqlx::raw_sql(trigger).execute(&pool).await.unwrap();
    }
    let before = history_snapshot(&pool).await;
    pool.close().await;
    initialize_database(&fixture.paths).await.unwrap();
    let pool = fixture.open().await;
    assert_eq!(history_snapshot(&pool).await, before);
    assert_sentinel_preserved(&pool).await;
    pool.close().await;
}

async fn assert_refused_without_changes(mutation: &'static str) {
    let fixture = TestDatabase::new("history-refusal");
    let pool = fixture.open().await;
    MIGRATOR.run(&pool).await.unwrap();
    insert_sentinel(&pool).await;
    sqlx::raw_sql(mutation).execute(&pool).await.unwrap();
    let history_before = history_snapshot(&pool).await;
    let version_before = pragma_user_version(&pool).await;
    pool.close().await;
    let bytes_before = fs::read(&fixture.paths.database_path).unwrap();

    let error = initialize_database(&fixture.paths).await.unwrap_err();
    assert!(
        matches!(error, StorageError::IncompatibleMigrationHistory { .. }),
        "{error}"
    );
    assert_eq!(
        fs::read(&fixture.paths.database_path).unwrap(),
        bytes_before
    );
    let pool = fixture.open().await;
    assert_eq!(history_snapshot(&pool).await, history_before);
    assert_eq!(pragma_user_version(&pool).await, version_before);
    assert_sentinel_preserved(&pool).await;
    pool.close().await;
}

#[path = "migration_baseline_tests.rs"]
mod baseline;

struct TestDatabase {
    root: PathBuf,
    paths: StoragePaths,
}

impl TestDatabase {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "langame-migration-history-{label}-{}-{stamp}-{counter}",
            std::process::id()
        ));
        let paths = StoragePaths {
            app_data_root: root.join("app-data"),
            settings_path: root.join("app-data/settings.json"),
            database_path: root.join("app-data/db/lgs.db"),
            logs_root: root.join("app-data/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("steamcmd"),
            games_root: root.join("games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        Self { root, paths }
    }

    async fn open(&self) -> SqlitePool {
        let options = SqliteConnectOptions::new()
            .filename(&self.paths.database_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap()
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        if self.root.starts_with(std::env::temp_dir()) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

async fn insert_sentinel(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO modules (id, name, version, supported_platforms) \
         VALUES ('history-test-module', 'History Test', '1', 'windows')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO instances \
         (id, name, module_id, data_path, config_path, logs_path, saves_path) \
         VALUES ('history-test-instance', 'History Test', 'history-test-module', \
         'data', 'config', 'logs', 'saves')",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn assert_sentinel_preserved(pool: &SqlitePool) {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instances \
         WHERE id = 'history-test-instance' AND module_id = 'history-test-module'",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

async fn history_snapshot(pool: &SqlitePool) -> Vec<(i64, String, i64, String, String, i64)> {
    sqlx::query(
        "SELECT version, description, success, hex(checksum) AS checksum, \
         CAST(installed_on AS TEXT) AS installed_on, execution_time \
         FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await
    .unwrap()
    .into_iter()
    .map(|row| {
        (
            row.get("version"),
            row.get("description"),
            row.get("success"),
            row.get("checksum"),
            row.get("installed_on"),
            row.get("execution_time"),
        )
    })
    .collect()
}

async fn pragma_user_version(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await
        .unwrap()
}
