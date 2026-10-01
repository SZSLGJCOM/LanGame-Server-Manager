use std::fs;
use std::path::Path;

use app_core::InstanceStatus;

use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::validate_managed_instance_root;
use crate::program_runtime::resolve_instance_runtime_root;
use crate::runtime::load_active_instance_run;
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

/// Restore only an owned rollback left by an interrupted package refresh.
/// The desktop caller also retains its instance mutation and storage-context leases.
pub async fn recover_interrupted_instance_runtime(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<(), StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = settings_lock.clone();
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    settings_lock
        .complete_mutation("recovering instance runtime", async move {
            let pool = connect_pool(&paths).await?;
            let result = async {
                // Hold a write reservation so another process cannot start this instance
                // between the active-run check and restoring the directory.
                let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
                let record = fetch_instance_record(&mut *tx, &instance_id).await?;
                let active = record.summary.active_process_count > 0
                    || matches!(
                        record.summary.status,
                        InstanceStatus::Starting
                            | InstanceStatus::Running
                            | InstanceStatus::Stopping
                    )
                    || load_active_instance_run(&mut *tx, &instance_id)
                        .await?
                        .is_some();
                let instance_root = record
                    .config_dir
                    .parent()
                    .unwrap_or(&record.config_dir)
                    .to_path_buf();
                worker_lock
                    .spawn_blocking(move || {
                        validate_managed_instance_root(&instance_root, &paths.instances_root)?;
                        let metadata = fs::symlink_metadata(&instance_root).map_err(|source| {
                            StorageError::ReadPath {
                                path: instance_root.clone(),
                                source,
                            }
                        })?;
                        if !metadata.is_dir()
                            || metadata.file_type().is_symlink()
                            || crate::private_runtime::is_reparse_point(&instance_root)?
                        {
                            return Err(StorageError::UnsafeManagedPath {
                                path: instance_root,
                                root: paths.instances_root,
                            });
                        }
                        if !active {
                            crate::program_detach::recover_detach(
                                &instance_root,
                                &record.runtime_mode,
                            )?;
                        }
                        recover_stopped_runtime(&instance_root, active)?;
                        crate::workshop_collection_removal::recover(&instance_root, active)?;
                        crate::instances::effective_instance_install_root(&record).map(|_| ())
                    })
                    .await
                    .map_err(|error| StorageError::BlockingTaskFailed {
                        operation: "recovering instance runtime",
                        message: error.to_string(),
                    })??;
                tx.commit().await?;
                Ok(())
            }
            .await;
            pool.close().await;
            result
        })
        .await
}

fn recover_stopped_runtime(instance_root: &Path, active: bool) -> Result<(), StorageError> {
    let runtime_root = instance_root.join("runtime");
    if path_exists(&runtime_root)? {
        return resolve_instance_runtime_root(instance_root).map(|_| ());
    }
    let rollback_root = instance_root.join("runtime.refresh-rollback");
    if !path_exists(&rollback_root)? {
        return resolve_instance_runtime_root(instance_root).map(|_| ());
    }
    if active {
        return Err(StorageError::PrivateRuntimeRefresh {
            path: runtime_root,
            message: String::from(
                "stop the instance before recovering its private runtime; existing files were preserved",
            ),
        });
    }
    crate::private_runtime_refresh::restore_private_runtime_rollback(
        &runtime_root,
        &rollback_root,
    )?;
    resolve_instance_runtime_root(instance_root).map(|_| ())
}

fn path_exists(path: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_path_buf(),
            source,
        }),
    }
}
