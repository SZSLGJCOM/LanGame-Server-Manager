use sqlx::{Row, SqlitePool};

use crate::{MIGRATOR, StorageError};

pub(crate) async fn verify_existing_migration_history(
    pool: &SqlitePool,
) -> Result<(), StorageError> {
    let has_history: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await?;
    if !has_history {
        return Err(incompatible_history(
            "an existing database has no SQLx migration history; automatic migration was refused",
        ));
    }

    let rows = sqlx::query(
        "SELECT version, description, success, checksum FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(pool)
    .await?;
    let canonical = MIGRATOR
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .collect::<Vec<_>>();
    if rows.is_empty() || rows.len() > canonical.len() {
        return Err(incompatible_history(
            "migration history is empty or contains an unsupported version",
        ));
    }
    let mut applied_version = 0;
    for (row, migration) in rows.iter().zip(canonical) {
        let version: i64 = row.try_get("version")?;
        let description: String = row.try_get("description")?;
        let success: bool = row.try_get("success")?;
        let checksum: Vec<u8> = row.try_get("checksum")?;
        if !success
            || version != migration.version
            || description != migration.description.as_ref()
            || checksum != migration.checksum.as_ref()
        {
            return Err(incompatible_history(format!(
                "migration {version} does not match the supported successful history"
            )));
        }
        applied_version = version;
    }

    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(pool)
        .await?;
    if user_version != applied_version {
        return Err(incompatible_history(format!(
            "schema version {user_version} does not match migration history {applied_version}"
        )));
    }
    Ok(())
}

fn incompatible_history(reason: impl Into<String>) -> StorageError {
    StorageError::IncompatibleMigrationHistory {
        reason: format!(
            "{}. This build requires its current database baseline; the existing database was not converted.",
            reason.into()
        ),
    }
}

#[cfg(test)]
#[path = "migration_history_tests.rs"]
mod tests;
