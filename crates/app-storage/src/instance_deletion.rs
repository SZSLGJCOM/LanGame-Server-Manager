use app_core::{InstanceArchiveResult, InstanceDeletionResult};

use crate::instance_archive::{archive_instance_locked, delete_instance_locked, inventory_lock};
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::{StorageError, StoragePaths};

pub async fn delete_instance(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceDeletionResult, StorageError> {
    let inventory = inventory_lock(paths)?;
    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let transaction_lock = settings_lock.clone();
    settings_lock
        .complete_mutation("deleting instance", async move {
            let _inventory = inventory;
            delete_instance_locked(&paths, &instance_id, &transaction_lock).await
        })
        .await
}

pub async fn archive_instance(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceArchiveResult, StorageError> {
    let inventory = inventory_lock(paths)?;
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let id = instance_id.to_owned();
    lock.complete_mutation("archiving instance", async move {
        let _inventory = inventory;
        archive_instance_locked(&paths, &id, &worker_lock, "archive").await
    })
    .await
}
