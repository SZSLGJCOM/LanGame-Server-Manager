use std::path::Path;

use app_core::InstanceStatus;
use sqlx::SqliteConnection;

use crate::instance_archive_store::{self as store, Snapshot, invalid};
use crate::storage_db::fetch_instance_record;
use crate::{StorageError, StoragePaths};

/// Run before filesystem work and again in the final short write transaction.
/// The instance mutation lease retains ownership between these checks, while
/// its existing row reserves paths and ports until the archive commits.
pub(super) async fn validate_archiving(
    connection: &mut SqliteConnection,
    paths: &StoragePaths,
    instance_id: &str,
    original: &Path,
    snapshot: &Snapshot,
) -> Result<(), StorageError> {
    let current = fetch_instance_record(&mut *connection, instance_id).await?;
    if current.summary.active_process_count > 0
        || matches!(
            current.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        )
        || crate::runtime::load_active_instance_run(&mut *connection, instance_id)
            .await?
            .is_some()
    {
        return Err(StorageError::ActiveInstanceDeletion {
            id: instance_id.to_owned(),
        });
    }
    if store::capture(connection, instance_id, snapshot.config_sha256.clone())
        .await?
        .tables
        != snapshot.tables
    {
        return Err(invalid(
            original,
            "Instance database changed after archive admission; recovery requires review.",
        ));
    }
    store::ensure_instance_root_unshared(connection, paths, original, instance_id).await?;
    store::ensure_unreferenced(connection, paths, &paths.archives_root).await
}
