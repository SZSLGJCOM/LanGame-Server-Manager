//! Idempotent, transactional publication of the fixed migration row projections.
use super::*;
use crate::storage_migration_metadata::SqlPatch;
use serde_json::Value;
use sqlx::{QueryBuilder, Sqlite};

fn columns(patch: &SqlPatch) -> Option<&'static [&'static str]> {
    match (patch.table.as_str(), patch.key_column.as_str()) {
        ("instances", "id") => Some(&[
            "module_id",
            "status",
            "install_id",
            "data_path",
            "config_path",
            "logs_path",
            "saves_path",
            "env_json",
            "args_json",
            "runtime_mode",
        ]),
        ("game_installs", "id") => {
            Some(&["module_id", "install_root", "scope", "owner_instance_id"])
        }
        ("instance_runs", "id") => {
            Some(&["instance_id", "status", "log_path", "process_image_path"])
        }
        ("player_history_cursors", "run_id") => {
            Some(&["byte_offset", "generation", "completed", "source_identity"])
        }
        ("instance_archives", "archive_id") => Some(&[
            "instance_id",
            "module_id",
            "original_root",
            "archive_leaf",
            "directory_identity_json",
            "snapshot_json",
            "snapshot_sha256",
            "preserved_external_saves_path",
            "state",
            "purpose",
            "instances_root_identity_json",
            "archive_parent_identity_json",
            "restore_staging_identity_json",
        ]),
        _ => None,
    }
}

fn bind(
    builder: &mut QueryBuilder<Sqlite>,
    value: &Value,
    path: &Path,
) -> Result<(), StorageError> {
    match value {
        Value::Null => {
            builder.push_bind(None::<String>);
        }
        Value::String(value) => {
            builder.push_bind(value.clone());
        }
        Value::Number(value) if value.as_i64().is_some() => {
            builder.push_bind(value.as_i64());
        }
        _ => return Err(invalid(path, "Unsupported migration SQL value.")),
    }
    Ok(())
}

pub(super) async fn apply(paths: &StoragePaths, patches: &[SqlPatch]) -> Result<(), StorageError> {
    if patches.is_empty() {
        return Ok(());
    }
    let pool = crate::storage_db::connect_pool(paths).await?;
    let result = apply_transaction(&pool, paths, patches).await;
    pool.close().await;
    result
}

async fn apply_transaction(
    pool: &sqlx::SqlitePool,
    paths: &StoragePaths,
    patches: &[SqlPatch],
) -> Result<(), StorageError> {
    let mut tx = pool.begin().await?;
    for patch in patches {
        let allowed = columns(patch).ok_or_else(|| {
            invalid(
                &paths.database_path,
                "Unsupported migration SQL projection.",
            )
        })?;
        if patch.before.len() != allowed.len()
            || patch.after.len() != allowed.len()
            || allowed.iter().any(|column| {
                !patch.before.contains_key(*column) || !patch.after.contains_key(*column)
            })
        {
            return Err(invalid(
                &paths.database_path,
                "Migration SQL columns do not match the fixed projection.",
            ));
        }
        // Only fixed, checked identifiers enter SQL. Every data value is bound.
        let mut read = QueryBuilder::<Sqlite>::new("SELECT json_object(");
        for (index, column) in allowed.iter().enumerate() {
            if index > 0 {
                read.push(",");
            }
            read.push("'").push(*column).push("',").push(*column);
        }
        read.push(") FROM ")
            .push(&patch.table)
            .push(" WHERE ")
            .push(&patch.key_column)
            .push("=");
        bind(&mut read, &patch.key_value, &paths.database_path)?;
        let value: Option<String> = read.build_query_scalar().fetch_optional(&mut *tx).await?;
        let current: std::collections::BTreeMap<String, Value> =
            serde_json::from_str(&value.ok_or_else(|| {
                invalid(
                    &paths.database_path,
                    "A migration record disappeared before publication.",
                )
            })?)?;
        if current == patch.after {
            continue;
        }
        if current != patch.before {
            return Err(invalid(
                &paths.database_path,
                format!(
                    "A {} record changed after migration preparation; no database changes were committed.",
                    patch.table
                ),
            ));
        }
        let mut update = QueryBuilder::<Sqlite>::new("UPDATE ");
        update.push(&patch.table).push(" SET ");
        for (index, column) in allowed.iter().enumerate() {
            if index > 0 {
                update.push(",");
            }
            update.push(*column).push("=");
            bind(&mut update, &patch.after[*column], &paths.database_path)?;
        }
        update.push(" WHERE ").push(&patch.key_column).push("=");
        bind(&mut update, &patch.key_value, &paths.database_path)?;
        if update.build().execute(&mut *tx).await?.rows_affected() != 1 {
            return Err(invalid(
                &paths.database_path,
                "Migration record publication did not update exactly one row.",
            ));
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "storage_migration_sql_tests.rs"]
mod tests;
