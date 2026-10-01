use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;
use std::path::PathBuf;

const MAX_MONITORED_INSTANCES: usize = 1_024;

/// Read only persisted storage paths in one bounded query. This does not load
/// module schemas, configuration documents, or recursively scan instance data.
pub async fn read_instance_storage_paths(
    paths: &StoragePaths,
) -> Result<Vec<PathBuf>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = project_instance_storage_paths(&pool).await;
    pool.close().await;
    result
}

async fn project_instance_storage_paths(pool: &SqlitePool) -> Result<Vec<PathBuf>, StorageError> {
    let rows = sqlx::query(
        "SELECT i.config_path, i.saves_path, i.data_path, i.logs_path, g.install_root \
         FROM instances i LEFT JOIN game_installs g ON g.id = i.install_id AND g.module_id = i.module_id \
         ORDER BY i.id LIMIT ?1",
    )
    .bind((MAX_MONITORED_INSTANCES + 1) as i64)
    .fetch_all(pool)
    .await?;
    if rows.len() > MAX_MONITORED_INSTANCES {
        return Err(StorageError::StorageTelemetryInstanceLimit {
            max: MAX_MONITORED_INSTANCES,
        });
    }
    let mut paths = BTreeSet::new();
    for row in rows {
        for key in [
            "config_path",
            "saves_path",
            "data_path",
            "logs_path",
            "install_root",
        ] {
            if let Some(path) = row
                .try_get::<Option<String>, _>(key)?
                .filter(|path| !path.trim().is_empty())
            {
                paths.insert(PathBuf::from(path));
            }
        }
        let config_path = PathBuf::from(row.try_get::<String, _>("config_path")?);
        if let Some(instance_root) = config_path.parent() {
            // Each may itself be a mounted volume or a junction.
            paths.insert(instance_root.join("backups"));
            paths.insert(instance_root.join("runtime"));
        }
    }
    Ok(paths.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE instances (id TEXT, module_id TEXT, install_id INTEGER, config_path TEXT, saves_path TEXT, data_path TEXT, logs_path TEXT)")
            .execute(&pool).await.unwrap();
        sqlx::query("CREATE TABLE game_installs (id INTEGER, module_id TEXT, install_root TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn projection_includes_split_save_install_and_backup_paths_without_duplicates() {
        let pool = pool().await;
        sqlx::query("INSERT INTO game_installs VALUES (1, 'game', 'F:/program')")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO instances VALUES ('one', 'game', 1, 'D:/instances/one/config', 'E:/saves/one', 'D:/instances/one/data', 'D:/instances/one/logs')")
            .execute(&pool).await.unwrap();
        let paths = project_instance_storage_paths(&pool).await.unwrap();
        for path in [
            "E:/saves/one",
            "F:/program",
            "D:/instances/one/backups",
            "D:/instances/one/runtime",
        ] {
            assert!(paths.contains(&PathBuf::from(path)), "missing {path}");
        }
        assert_eq!(paths.len(), 7);
        pool.close().await;
    }

    #[tokio::test]
    async fn projection_refuses_to_claim_coverage_when_instance_limit_is_exceeded() {
        let pool = pool().await;
        sqlx::query("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 1025) INSERT INTO instances SELECT CAST(x AS TEXT), 'game', NULL, 'D:/config', 'D:/saves', 'D:/data', 'D:/logs' FROM n")
            .execute(&pool).await.unwrap();
        assert!(matches!(
            project_instance_storage_paths(&pool).await,
            Err(StorageError::StorageTelemetryInstanceLimit { .. })
        ));
        pool.close().await;
    }
}
