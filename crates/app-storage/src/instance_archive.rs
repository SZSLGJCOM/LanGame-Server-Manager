use std::path::Path;

use crate::instance_archive_files as files;
use crate::instance_archive_models::*;
use crate::instance_archive_store::{self as store, invalid};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
    acquire_instance_settings_read_lock,
};
use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths};

#[path = "instance_archive_cleanup.rs"]
mod cleanup;
#[path = "instance_archive_config.rs"]
mod config;
#[path = "instance_archive_details.rs"]
mod details;
pub use details::{InstanceArchiveDetails, read_instance_archive_details};
#[path = "instance_archive_external.rs"]
pub(crate) mod external;
#[path = "instance_archive_program.rs"]
pub(crate) mod program;
pub use external::{
    InstanceRemovalPlan, ensure_program_archive_dependencies, inspect_instance_removal,
};
#[cfg(test)]
#[path = "instance_archive_read_probe.rs"]
pub(crate) mod read_probe;
#[cfg(test)]
#[path = "instance_archive_test_gate.rs"]
pub(crate) mod test_gate;
#[path = "instance_archive_transactions.rs"]
mod transactions;
pub(crate) use cleanup::delete_instance_locked;
pub(crate) use transactions::archive_instance as archive_instance_locked;
pub(crate) use transactions::validate_archived_files;

pub(crate) fn inventory_lock(paths: &StoragePaths) -> Result<InstanceSettingsLock, StorageError> {
    acquire_instance_settings_mutation_lock(paths, "archive-inventory")
}

pub(crate) fn program_source_read_lock(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceSettingsLock, StorageError> {
    store::validate_id(archive_id)?;
    acquire_instance_settings_read_lock(paths, &format!("archive-source-{archive_id}"))
}

pub(crate) fn program_source_mutation_lock(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceSettingsLock, StorageError> {
    store::validate_id(archive_id)?;
    acquire_instance_settings_mutation_lock(paths, &format!("archive-source-{archive_id}"))
}

pub async fn list_instance_archives(
    paths: &StoragePaths,
) -> Result<InstanceArchiveList, StorageError> {
    let _inventory_lock = inventory_lock(paths)?;
    let owned = paths.clone();
    let entries = tokio::task::spawn_blocking(move || files::inventory(&owned))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "reading instance archives",
            message: error.to_string(),
        })??;
    let pool = connect_pool(paths).await?;
    let result = async {
        for entry in entries {
            sqlx::query("INSERT INTO instance_archives (archive_id, archive_leaf, directory_identity_json, state, problem, instances_root_identity_json, archive_parent_identity_json) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(archive_leaf) WHERE state NOT IN ('restored', 'purged') DO NOTHING")
                .bind(uuid::Uuid::new_v4().to_string()).bind(entry.leaf).bind(&entry.identity)
                .bind(if entry.identity.is_some() { "missing_metadata" } else { "unrecognized" })
                .bind(entry.problem).bind(entry.instances_identity).bind(entry.parent_identity).execute(&pool).await?;
        }
        let mut summaries = Vec::new();
        let mut pending_deletions = Vec::new();
        let mut library_manifests = std::collections::BTreeMap::<String, Result<String, String>>::new();
        for id in store::archive_ids(&pool).await? {
            let archive = store::load(&pool, &id).await?;
            if archive.purpose == "delete" {
                pending_deletions.push(PendingInstanceDeletion {
                    operation_id: archive.id, instance_id: archive.instance_id.unwrap_or_default(), instance_name: archive.instance_name.unwrap_or_default(), module_id: archive.module_id.unwrap_or_default(),
                    deleted_instance_root: archive.original_root.unwrap_or_default(), started_at_unix_ms: archive.deleted_at.and_then(|value| value.try_into().ok()),
                    can_retry: matches!(archive.state.as_str(), "archiving" | "purging"), issues: archive.problem.into_iter().collect(),
                });
                continue;
            }
            let mut issues = archive.problem.iter().cloned().collect::<Vec<_>>();
            let state = serde_json::from_value(serde_json::Value::String(archive.state.clone()))?;
            let root = files::archive_path(paths, &archive.leaf)?;
            let checked_paths = paths.clone();
            let checked_archive = archive.clone();
            let inspected = tokio::task::spawn_blocking(move || {
                let _parents = files::guard_parents(&checked_paths, checked_archive.instances_identity.as_deref(), checked_archive.parent_identity.as_deref())?;
                let root = files::archive_path(&checked_paths, &checked_archive.leaf)?;
                let exists = files::verify_identity(&root, checked_archive.identity.as_deref())?;
                let restoration = transactions::validate_archived_files(&checked_paths, &checked_archive);
                Ok::<_, StorageError>((exists, restoration))
            }).await.map_err(|error| StorageError::BlockingTaskFailed { operation: "inspecting instance archive", message: error.to_string() })?;
            let (owned, restorable) = match inspected {
                Ok((exists, restoration)) => {
                    if let Err(error) = &restoration { issues.push(error.to_string()); }
                    ((exists && archive.identity.is_some()) || (archive.state == "purging" && archive.instances_identity.is_some() && archive.parent_identity.is_some()), restoration.is_ok())
                }
                Err(error) => { issues.push(error.to_string()); (false, false) }
            };
            let mut tx = pool.begin().await?;
            let unreferenced = store::ensure_unreferenced(&mut tx, paths, &root).await;
            tx.rollback().await?;
            if let Err(error) = &unreferenced { issues.push(error.to_string()); }
            let can_purge = owned && unreferenced.is_ok() && matches!(state, InstanceArchiveState::Archived | InstanceArchiveState::MissingMetadata | InstanceArchiveState::Unrecognized | InstanceArchiveState::Purging);
            let mut can_restore = restorable && matches!(state, InstanceArchiveState::Archived | InstanceArchiveState::Restoring);
            if can_restore && let Err(error) = transactions::check_restore_conflicts(&pool, paths, &archive).await {
                issues.push(error.to_string()); can_restore = false;
            }
            if state == InstanceArchiveState::MissingMetadata { issues.push(String::from("This archive predates recovery metadata. Its files are retained, but database settings and associations cannot be reconstructed safely.")); }
            let snapshot = store::snapshot(&archive).ok();
            let program = snapshot.as_ref().and_then(|snapshot| snapshot.program.as_ref());
            let external_program = snapshot.as_ref().and_then(|snapshot| snapshot.external_program.as_ref());
            let library_binding = snapshot.as_ref().and_then(|snapshot| snapshot.tables["game_installs"].first()).is_some_and(|install| install["scope"] == "library");
            let requires_program = program.is_some() || external_program.map_or(library_binding, |plan| plan.requires_source());
            if let Some(plan) = program {
                if !library_manifests.contains_key(&plan.module_id) {
                    let mut connection = pool.acquire().await?;
                    let library = program::library(&mut connection, &plan.module_id).await?;
                    drop(connection);
                    let available = if let Some((root, _)) = library {
                        let module = plan.module_id.clone();
                        tokio::task::spawn_blocking(move || program::manifest_fingerprint(&root, &module))
                            .await.map_err(|error| StorageError::BlockingTaskFailed { operation: "checking archive program manifest", message: error.to_string() })?
                            .map_err(|error| error.to_string())
                    } else { Err("Install or repair the exact archived program version before restoring; no download was started.".into()) };
                    library_manifests.insert(plan.module_id.clone(), available);
                }
                match &library_manifests[&plan.module_id] {
                    Ok(actual) if actual == &plan.package_fingerprint => {}
                    Ok(_) => { can_restore = false; issues.push("The installed library manifest does not match this archive's exact program version.".into()); }
                    Err(error) => { can_restore = false; issues.push(error.clone()); }
                }
            }
            issues.sort(); issues.dedup();
            summaries.push(InstanceArchiveSummary {
                archive_id: archive.id, instance_id: archive.instance_id, instance_name: archive.instance_name,
                module_id: archive.module_id, deleted_at_unix_ms: archive.deleted_at.and_then(|time| time.try_into().ok()),
                archived_instance_root: root.to_string_lossy().into_owned(), previous_instance_root: archive.original_root,
                preserved_external_saves_path: archive.external_saves, state, can_restore, can_purge, issues,
                program_storage: if requires_program { "reconstructable" } else { "full" }.into(),
                omitted_program_bytes: program.map_or_else(|| external_program.map_or(0, |plan| plan.omitted_bytes()), |plan| plan.bytes()), omitted_program_files: program.map_or_else(|| external_program.map_or(0, |plan| plan.omitted_files()), |plan| plan.files.len()),
                required_program_fingerprint: program.map(|plan| plan.package_fingerprint.clone()).or_else(|| external_program.filter(|plan| plan.requires_source()).map(|plan| plan.fingerprint.clone())), required_program_version: if requires_program { program.and_then(|plan| plan.current_version.clone()).or_else(|| snapshot.as_ref().and_then(|snapshot| snapshot.tables["game_installs"].first()).and_then(|install| install["current_version"].as_str().map(str::to_owned))) } else { None },
                program_retention_reason: snapshot.as_ref().and_then(|snapshot| snapshot.program_retention_reason.clone()), external_saves_backup_id: snapshot.and_then(|snapshot| snapshot.external_saves_backup_id),
            });
        }
        Ok(InstanceArchiveList { archives: summaries, pending_deletions, issues: Vec::new() })
    }.await;
    pool.close().await;
    result
}

pub async fn restore_instance_archive(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceArchiveRestoreResult, StorageError> {
    store::validate_id(archive_id)?;
    let lock = inventory_lock(paths)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let id = archive_id.to_owned();
    lock.complete_mutation("restoring instance archive", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            let archive = store::load(&pool, &id).await?;
            let instance_id = archive.instance_id.as_deref().ok_or_else(|| {
                invalid(
                    Path::new(&archive.leaf),
                    "Archive has no recoverable instance identity.",
                )
            })?;
            let _instance_lock = acquire_instance_settings_mutation_lock(&paths, instance_id)?;
            transactions::restore(&pool, &paths, &archive, &worker_lock).await
        }
        .await;
        pool.close().await;
        result
    })
    .await
}

pub async fn purge_instance_archive(
    paths: &StoragePaths,
    archive_id: &str,
) -> Result<InstanceArchivePurgeResult, StorageError> {
    store::validate_id(archive_id)?;
    let lock = inventory_lock(paths)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let id = archive_id.to_owned();
    lock.complete_mutation("permanently clearing instance archive", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            let archive = store::load(&pool, &id).await?;
            if archive.purpose != "archive" { return Err(invalid(Path::new(&archive.leaf), "A pending instance deletion is not a recoverable archive; retry deleting its instance instead.")); }
            cleanup::purge(&pool, &paths, &archive, &worker_lock).await
        }.await;
        pool.close().await; result
    }).await
}

pub async fn recover_instance_archives(paths: &StoragePaths) -> Result<(), StorageError> {
    let lock = inventory_lock(paths)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    lock.complete_mutation("recovering instance archive transactions", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            for id in store::archive_ids(&pool).await? {
                let archive = store::load(&pool, &id).await?;
                if archive.purpose != "archive"
                    || !matches!(archive.state.as_str(), "archiving" | "restoring")
                {
                    continue;
                }
                let recovery = async {
                    let instance_id = archive.instance_id.as_deref().ok_or_else(|| {
                        invalid(
                            Path::new(&archive.leaf),
                            "Pending archive has no instance identity.",
                        )
                    })?;
                    let _instance_lock =
                        acquire_instance_settings_mutation_lock(&paths, instance_id)?;
                    if archive.state == "archiving" {
                        transactions::finish_archiving(&pool, &paths, &archive, &worker_lock).await
                    } else {
                        transactions::restore(&pool, &paths, &archive, &worker_lock)
                            .await
                            .map(|_| ())
                    }
                }
                .await;
                if let Err(error) = recovery {
                    sqlx::query("UPDATE instance_archives SET problem=?2 WHERE archive_id=?1")
                        .bind(&archive.id)
                        .bind(error.to_string())
                        .execute(&pool)
                        .await?;
                }
            }
            Ok(())
        }
        .await;
        pool.close().await;
        result
    })
    .await
}

pub async fn pending_instance_archive_ids(
    paths: &StoragePaths,
) -> Result<Vec<String>, StorageError> {
    let pool = connect_pool(paths).await?;
    let result = sqlx::query_scalar::<_, String>("SELECT DISTINCT instance_id FROM instance_archives WHERE purpose='archive' AND state IN ('archiving','restoring') AND instance_id IS NOT NULL ORDER BY instance_id LIMIT 4097")
        .fetch_all(&pool).await;
    pool.close().await;
    let ids = result?;
    if ids.len() > store::MAX_ARCHIVES {
        return Err(invalid(
            &paths.database_path,
            "Pending archive recovery exceeds 4096 instances; no recovery was attempted.",
        ));
    }
    Ok(ids)
}

#[cfg(test)]
#[path = "instance_archive_tests.rs"]
mod tests;
