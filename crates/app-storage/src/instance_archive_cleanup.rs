use std::path::Path;

use app_core::InstanceDeletionResult;
use sqlx::SqlitePool;

use crate::instance_archive_files as files;
use crate::instance_archive_models::InstanceArchivePurgeResult;
use crate::instance_archive_store::{self as store, Archive, invalid};
use crate::instance_settings_lock::InstanceSettingsLock;
use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths};

pub(super) async fn purge(
    pool: &SqlitePool,
    paths: &StoragePaths,
    archive: &Archive,
    _inventory_lock: &InstanceSettingsLock,
) -> Result<InstanceArchivePurgeResult, StorageError> {
    let source_lock = crate::instance_archive::program_source_mutation_lock(paths, &archive.id)?;
    let lock = &source_lock;
    if archive.state == "purged" {
        return Ok(InstanceArchivePurgeResult {
            archive_id: archive.id.clone(),
            purged: true,
        });
    }
    if !matches!(
        archive.state.as_str(),
        "archived" | "missing_metadata" | "unrecognized" | "purging"
    ) {
        return Err(invalid(
            Path::new(&archive.leaf),
            "Operation is not available for permanent cleanup.",
        ));
    }
    let root = files::archive_path(paths, &archive.leaf)?;
    let checked_paths = paths.clone();
    let checked = archive.clone();
    let worker_root = root.clone();
    lock.spawn_blocking(move || {
        let _parents = files::guard_parents(&checked_paths, checked.instances_identity.as_deref(), checked.parent_identity.as_deref())?;
        if files::plain_directory(&worker_root)? {
            if !files::verify_identity(&worker_root, checked.identity.as_deref())? { return Err(invalid(&worker_root, "Cleanup directory disappeared.")); }
            // A committed deletion already owns its durable purging intent.
            // Keep the complete tree preflight immediately before removal below;
            // only new purge intents need another scan before changing state.
            if checked.state == "purging" { Ok(()) }
            else { files::preflight_tree(&worker_root, checked.identity.as_deref().unwrap()) }
        } else if checked.state == "purging" && checked.instances_identity.is_some() && checked.parent_identity.is_some() { Ok(()) }
        else { Err(invalid(&worker_root, "Cleanup directory is missing without a verified pending deletion; recovery metadata was preserved.")) }
    }).await.map_err(|error| worker_error("checking permanent cleanup ownership", error))??;
    store::set_state(pool, &archive.id, "purging", None).await?;
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let cleanup = async {
        store::ensure_unreferenced(&mut tx, paths, &root).await?;
        let checked_paths = paths.clone();
        let checked = archive.clone();
        lock.spawn_blocking(move || {
            let _parents = files::guard_parents(
                &checked_paths,
                checked.instances_identity.as_deref(),
                checked.parent_identity.as_deref(),
            )?;
            if !files::plain_directory(&root)? {
                if checked.state == "purging"
                    && checked.instances_identity.is_some()
                    && checked.parent_identity.is_some()
                {
                    return Ok(());
                }
                return Err(invalid(
                    &root,
                    "Cleanup directory disappeared before removal; metadata was retained.",
                ));
            }
            files::purge_tree(
                &root,
                checked
                    .identity
                    .as_deref()
                    .ok_or_else(|| invalid(&root, "Cleanup directory ownership is missing."))?,
            )
        })
        .await
        .map_err(|error| worker_error("clearing instance-owned files", error))?
    }
    .await;
    match cleanup {
        Ok(()) => {
            sqlx::query("UPDATE instance_archives SET state='purged',problem=NULL,snapshot_json=NULL,snapshot_sha256=NULL,restore_staging_identity_json=NULL,updated_at=CURRENT_TIMESTAMP WHERE archive_id=?1")
                .bind(&archive.id).execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(InstanceArchivePurgeResult {
                archive_id: archive.id.clone(),
                purged: true,
            })
        }
        Err(error) => {
            sqlx::query("UPDATE instance_archives SET problem=?2 WHERE archive_id=?1")
                .bind(&archive.id)
                .bind(error.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Err(error)
        }
    }
}

pub(crate) async fn delete_instance_locked(
    paths: &StoragePaths,
    instance_id: &str,
    lock: &InstanceSettingsLock,
) -> Result<InstanceDeletionResult, StorageError> {
    if let Some(result) =
        crate::instance_reconciliation::remove_missing_instance_locked(paths, instance_id, lock)
            .await?
    {
        return Ok(result);
    }
    let pool = connect_pool(paths).await?;
    let result = async {
        let pending: Option<String> = sqlx::query_scalar("SELECT archive_id FROM instance_archives WHERE instance_id=?1 AND purpose='delete' AND state NOT IN ('restored','purged')")
            .bind(instance_id).fetch_optional(&pool).await?;
        let id = if let Some(id) = pending {
            let archive = store::load(&pool, &id).await?;
            if archive.state == "archiving" { super::transactions::finish_archiving(&pool, paths, &archive, lock).await?; }
            id
        } else { super::archive_instance_locked(paths, instance_id, lock, "delete").await?.archive_id };
        let archive = store::load(&pool, &id).await?;
        let response = InstanceDeletionResult {
            program_cleanup: Default::default(),
            instance_id: archive.instance_id.clone().ok_or_else(|| invalid(Path::new(&archive.leaf), "Deletion instance identity is missing."))?,
            instance_name: archive.instance_name.clone().unwrap_or_default(), module_id: archive.module_id.clone().unwrap_or_default(),
            deleted_at_unix_ms: archive.deleted_at.unwrap_or_default().max(0) as u128,
            deleted_instance_root: archive.original_root.clone().unwrap_or_default(), preserved_external_saves_path: archive.external_saves.clone(),
        };
        purge(&pool, paths, &archive, lock).await?;
        Ok(response)
    }.await;
    pool.close().await;
    result
}

fn worker_error(operation: &'static str, error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation,
        message: error.to_string(),
    }
}
