use app_core::InstanceDetails;
use serde_json::{Map, Value};

use crate::atomic_file::read_file_to_string;
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
};
use crate::instances::commit_instance_transaction;
use crate::storage_db::connect_pool;
use crate::templates::ManagedConfigMutation;
use crate::{StorageError, StoragePaths, read_instance_details};

/// Startup policy is independent of game configuration and may change while the
/// instance is running. The instance lease also prevents an older settings save
/// from overwriting the manager configuration mirror during this transaction.
pub async fn update_instance_autostart(
    paths: &StoragePaths,
    instance_id: &str,
    autostart: bool,
) -> Result<InstanceDetails, StorageError> {
    let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let transaction_lock = settings_lock.clone();
    settings_lock
        .complete_mutation("updating instance autostart", async move {
            update_autostart_transaction(&paths, &instance_id, autostart, &transaction_lock).await
        })
        .await
}

async fn update_autostart_transaction(
    paths: &StoragePaths,
    instance_id: &str,
    autostart: bool,
    settings_lock: &InstanceSettingsLock,
) -> Result<InstanceDetails, StorageError> {
    let mut details = read_instance_details(paths, instance_id).await?;
    let pool = connect_pool(paths).await?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE instances SET autostart = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
    )
    .bind(instance_id)
    .bind(i64::from(autostart))
    .execute(&mut *tx)
    .await?;

    let config_path = std::path::PathBuf::from(&details.config_file_path);
    let module_id = details.summary.module_id.clone();
    let config_mutation = settings_lock
        .spawn_blocking(move || {
            let content =
                read_file_to_string(&config_path).map_err(|source| StorageError::ReadConfig {
                    path: config_path.clone(),
                    source,
                })?;
            let mut document: Map<String, Value> =
                serde_json::from_str(&content).map_err(|source| {
                    StorageError::InvalidConfigJson {
                        path: config_path.clone(),
                        source,
                    }
                })?;
            document.insert("autostart".to_owned(), Value::Bool(autostart));
            let mut mutation = ManagedConfigMutation::new(&module_id);
            mutation.write(&config_path, &serde_json::to_vec_pretty(&document)?)?;
            Ok::<_, StorageError>(mutation)
        })
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "updating instance autostart configuration",
            message: error.to_string(),
        })??;
    commit_instance_transaction(tx, config_mutation, settings_lock).await?;
    pool.close().await;
    details.summary.autostart = autostart;
    Ok(details)
}
