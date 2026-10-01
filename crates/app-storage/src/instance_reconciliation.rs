use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{InstanceDeletionResult, InstanceStatus};

use crate::instance_archive::inventory_lock;
use crate::instance_archive_files::{self as files, native};
use crate::instance_archive_store::{self as store, invalid};
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

const MAX_RECONCILIATION_INSTANCES: usize = 4096;

/// A read-only, bounded prefilter. Mutation callers must still acquire their
/// runtime lease and call `reconcile_missing_instance` to revalidate ownership.
pub async fn list_missing_instance_candidates(
    paths: &StoragePaths,
) -> Result<Vec<String>, StorageError> {
    let pool = connect_pool(paths).await?;
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT id,config_path FROM instances WHERE status NOT IN ('starting','running','stopping') \
         AND NOT EXISTS (SELECT 1 FROM instance_runs WHERE instance_id=instances.id AND status='running') \
         AND NOT EXISTS (SELECT 1 FROM instance_archives WHERE instance_id=instances.id AND state NOT IN ('restored','purged')) \
         ORDER BY id LIMIT 4097",
    ).fetch_all(&pool).await;
    pool.close().await;
    let rows = rows?;
    if rows.len() > MAX_RECONCILIATION_INSTANCES {
        return Err(invalid(
            &paths.instances_root,
            "Instance reconciliation inventory exceeds its limit.",
        ));
    }
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        let Ok(Some(_parent)) = guard_instances_root(&paths) else {
            return Ok(Vec::new());
        };
        let mut candidates = Vec::new();
        for (id, config) in rows {
            let Ok(root) = checked_instance_root(&paths, Path::new(&config)) else {
                continue;
            };
            if root_is_missing(&root).is_ok_and(|missing| missing) {
                candidates.push(id);
            }
        }
        Ok(candidates)
    })
    .await
    .map_err(worker_error)?
}

/// Remove only database registration after the whole managed instance directory
/// disappeared. Missing program files alone never qualify. No files are deleted.
pub async fn reconcile_missing_instance(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<bool, StorageError> {
    // Locks live beneath instances_root, so validate and pin the existing parent
    // before acquisition can create a lock directory inside it.
    let checked = paths.clone();
    let Some(parent) = tokio::task::spawn_blocking(move || guard_instances_root(&checked))
        .await
        .map_err(worker_error)??
    else {
        return Ok(false);
    };
    let inventory = inventory_lock(paths)?;
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let id = instance_id.to_owned();
    lock.complete_mutation("reconciling missing instance", async move {
        let _parent = parent;
        let _inventory = inventory;
        Ok(remove_missing_instance_locked(&paths, &id, &worker_lock)
            .await?
            .is_some())
    })
    .await
}

pub(crate) async fn remove_missing_instance_locked(
    paths: &StoragePaths,
    instance_id: &str,
    lock: &InstanceSettingsLock,
) -> Result<Option<InstanceDeletionResult>, StorageError> {
    let checked = paths.clone();
    let Some(_parent) = lock
        .spawn_blocking(move || guard_instances_root(&checked))
        .await
        .map_err(worker_error)??
    else {
        return Ok(None);
    };
    let pool = connect_pool(paths).await?;
    let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let record = match fetch_instance_record(&mut *tx, instance_id).await {
            Ok(record) => record,
            Err(StorageError::MissingInstance { .. }) => return Ok(None),
            Err(error) => return Err(error),
        };
        if record.summary.active_process_count > 0
            || matches!(record.summary.status, InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping)
            || crate::runtime::load_active_instance_run(&mut *tx, instance_id).await?.is_some()
        { return Ok(None); }
        let journal: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM instance_archives WHERE instance_id=?1 AND state NOT IN ('restored','purged'))",
        ).bind(instance_id).fetch_one(&mut *tx).await?;
        if journal { return Ok(None); }
        let checked = paths.clone();
        let config = record.config_dir.clone();
        let root = lock.spawn_blocking(move || checked_instance_root(&checked, &config))
            .await.map_err(worker_error)??;
        store::ensure_instance_root_unshared(&mut tx, paths, &root, instance_id).await?;
        let checked = root.clone();
        if !lock.spawn_blocking(move || root_is_missing(&checked)).await.map_err(worker_error)?? {
            return Ok(None);
        }
        let external = !contains(&normalize_path(&root)?, &normalize_path(&record.saves_dir)?);
        // The FK graph removes ports, runs, policies and instance-owned install
        // records. Library records and all filesystem contents remain untouched.
        sqlx::query("DELETE FROM instances WHERE id=?1").bind(instance_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Some(InstanceDeletionResult {
            program_cleanup: Default::default(),
            instance_id: instance_id.to_owned(),
            instance_name: record.summary.name,
            module_id: record.summary.module_id,
            deleted_at_unix_ms: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            deleted_instance_root: root.to_string_lossy().into_owned(),
            preserved_external_saves_path: external.then(|| record.saves_dir.to_string_lossy().into_owned()),
        }))
    }.await;
    pool.close().await;
    result
}

fn guard_instances_root(paths: &StoragePaths) -> Result<Option<native::OwnedNode>, StorageError> {
    if !files::plain_directory(&paths.instances_root)? {
        return Ok(None);
    }
    native::open(&paths.instances_root, true, false)
        .map(Some)
        .map_err(|source| StorageError::ReadPath {
            path: paths.instances_root.clone(),
            source,
        })
}

pub(crate) fn checked_instance_root(
    paths: &StoragePaths,
    config: &Path,
) -> Result<PathBuf, StorageError> {
    if config.file_name() != Some(std::ffi::OsStr::new("config")) {
        return Err(invalid(
            config,
            "Registered instance configuration directory is invalid.",
        ));
    }
    let original = config
        .parent()
        .ok_or_else(|| invalid(config, "Instance root is missing."))?;
    let root = normalize_path(original)?;
    let parent = normalize_path(&paths.instances_root)?;
    crate::instances::validate_managed_instance_root(&root, &parent)?;
    if root.file_name().is_some_and(|name| name == ".langame") {
        return Err(invalid(
            &root,
            "Application metadata cannot be an instance directory.",
        ));
    }
    Ok(root)
}

fn root_is_missing(root: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(root) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(source) => Err(StorageError::ReadPath {
            path: root.to_owned(),
            source,
        }),
    }
}

fn worker_error(error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation: "checking missing instance directories",
        message: error.to_string(),
    }
}
