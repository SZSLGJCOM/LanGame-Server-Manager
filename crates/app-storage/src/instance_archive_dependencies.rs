use std::path::Path;

use sqlx::Row;

use crate::instance_archive_store::{self as store, invalid};
use crate::instance_isolation::paths::normalize_path;
use crate::{StorageError, StoragePaths};

/// Must run under the program's lifecycle lease and before any program bytes
/// are changed. Archive admission/restoration takes the same module and source
/// leases, so unrelated inventory mutations may run concurrently. Pending
/// retirement journals reserve their source as well.
pub async fn ensure_program_archive_dependencies(
    paths: &StoragePaths,
    install_root: &Path,
) -> Result<(), StorageError> {
    let root = normalize_path(install_root)?;
    let reservation =
        inspect_program_archive_dependencies(paths, &root, DependencyCheck::ProgramMutation)
            .await?;
    match reservation {
        Some(reason) => Err(invalid(&root, reason)),
        None => Ok(()),
    }
}

/// A preview reads only the archive snapshots, never their payloads. Exclusive
/// reuse reserves even a self-contained archive's original program root, since
/// restoration may need to publish that archive's data there again. Recheck
/// under the program lifecycle lease before creating a new instance.
pub(crate) async fn program_has_archive_reservation(
    paths: &StoragePaths,
    install_root: &Path,
) -> Result<bool, StorageError> {
    let root = normalize_path(install_root)?;
    Ok(
        inspect_program_archive_dependencies(paths, &root, DependencyCheck::ExclusiveReuse)
            .await?
            .is_some(),
    )
}

enum DependencyCheck {
    ProgramMutation,
    ExclusiveReuse,
}

async fn inspect_program_archive_dependencies(
    paths: &StoragePaths,
    root: &Path,
    check: DependencyCheck,
) -> Result<Option<String>, StorageError> {
    let reserve_all = matches!(check, DependencyCheck::ExclusiveReuse);
    let pool = crate::storage_db::connect_pool(paths).await?;
    let result = async {
        // An unrelated archive can commit or compensate while this check runs.
        // One WAL read snapshot keeps its ID list and journal rows consistent
        // without excluding those writers or holding the catalog mutation lock.
        let mut tx = pool.begin().await?;
        let rows = sqlx::query("SELECT module_id,install_root FROM game_installs WHERE scope='library' LIMIT 4097").fetch_all(&mut *tx).await?;
        if rows.len() > store::MAX_ARCHIVES { return Err(invalid(root, "Program dependency inventory exceeds its limit.")); }
        let mut modules = Vec::new();
        for row in rows {
            if normalize_path(Path::new(&row.get::<String, _>("install_root")))?.as_path() == root { modules.push(row.get::<String, _>("module_id")); }
        }
        let ids: Vec<String> = sqlx::query_scalar("SELECT archive_id FROM instance_archives WHERE state NOT IN ('restored','purged') ORDER BY deleted_at_unix_ms DESC,archive_id LIMIT 4097")
            .fetch_all(&mut *tx).await?;
        if ids.len() > store::MAX_ARCHIVES { return Err(invalid(root, "Archive dependency inventory exceeds its limit.")); }
        #[cfg(test)]
        {
            let database = paths.database_path.clone();
            tokio::task::spawn_blocking(move || crate::instance_archive::test_gate::pause(&database, crate::instance_archive::test_gate::Point::Dependencies))
                .await.map_err(|error| StorageError::BlockingTaskFailed { operation: "checking archive dependency snapshot", message: error.to_string() })?;
        }
        for id in ids {
            store::validate_id(&id)?;
            let row = sqlx::query("SELECT * FROM instance_archives WHERE archive_id=?1").bind(&id).fetch_one(&mut *tx).await?;
            let archive = store::map_archive(&row)?;
            // Historical directories without snapshots are preserved; there is
            // no trustworthy binding to invent for them.
            if archive.snapshot.is_none() { continue; }
            let snapshot = store::snapshot(&archive)?;
            let install = snapshot.tables["game_installs"].first();
            let bound = install.is_some_and(|row| row.get("scope").and_then(serde_json::Value::as_str) == Some("library"))
                && install.map(|row| store::string(row, "install_root").and_then(|path| normalize_path(Path::new(path)))).transpose()?.as_deref() == Some(root);
            let reconstructs = snapshot.program.as_ref().is_some_and(|plan| modules.contains(&plan.module_id));
            let external = snapshot.external_program.as_ref().map(|plan| normalize_path(&plan.root)).transpose()?.as_deref() == Some(root);
            let protects_external = snapshot.external_program.as_ref().is_some_and(|plan| plan.requires_source() || archive.purpose == "delete" || matches!(archive.state.as_str(), "archiving" | "restoring"));
            if (bound && (reserve_all || snapshot.external_program.is_none())) || reconstructs || (external && (reserve_all || protects_external)) {
                return Ok(Some(format!(
                    "程序仍被归档或未完成的删除任务“{}”（{}）引用；请先恢复或清理该归档/任务，再更新、修改或卸载程序。", archive.instance_name.as_deref().unwrap_or("未命名实例"), archive.id,
                )));
            }
            if !reserve_all && external && let Some(plan) = snapshot.external_program {
                // A full archive releases its source only while its actual
                // recovery bytes remain intact. Metadata alone is insufficient.
                let archived_root = crate::instance_archive_files::archive_path(paths, &archive.leaf)?;
                tokio::task::spawn_blocking(move || super::verify(&archived_root, &plan)).await
                    .map_err(|error| StorageError::BlockingTaskFailed { operation: "verifying full archive before changing its source", message: error.to_string() })??;
            }
        }
        tx.rollback().await?;
        Ok(None)
    }.await;
    pool.close().await;
    result
}
