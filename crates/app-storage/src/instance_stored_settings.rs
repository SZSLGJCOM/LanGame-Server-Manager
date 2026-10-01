use app_core::InstanceSummary;

use crate::instance_isolation::paths::normalize_resource_path;
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

/// Lifecycle reconciliation needs the persisted restart policy even when the
/// game's program was removed. This does not authorize starting or writing it.
pub async fn read_instance_stored_settings(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<(InstanceSummary, String), StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await;
    pool.close().await;
    let record = record?;
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        crate::instance_reconciliation::checked_instance_root(&paths, &record.config_dir)?;
        let config = record.config_dir.join("instance.json");
        normalize_resource_path(&config)?;
        let settings = crate::instances::read_instance_settings_json(&config)?;
        Ok((record.summary, settings))
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "reading stored instance settings",
        message: error.to_string(),
    })?
}
