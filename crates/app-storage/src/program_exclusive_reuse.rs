use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::program_runtime::invalid;
use crate::{StorageError, StoragePaths};

/// Metadata/default-file inspection shared by the preview and authoritative
/// creation. The latter holds the module lease and verifies all program bytes
/// (or their unchanged verified identities) before writing the new binding.
pub(crate) async fn unused_library_can_be_reused(
    paths: &StoragePaths,
    root: &Path,
    module_id: &str,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<bool, StorageError> {
    crate::instance_creation_io::check_creation_cancelled(cancellation.as_deref())?;
    let used = super::library_was_exclusively_used(root)?;
    // Reusing a retired exclusive library must not change a source reserved by
    // an archive, including a pending delete whose active DB row is already gone.
    if used
        && crate::instance_archive::external::program_has_archive_reservation(paths, root).await?
    {
        return Ok(false);
    }
    let worker_root = root.to_owned();
    let instances_root = paths.instances_root.clone();
    let module_id = module_id.to_owned();
    tokio::task::spawn_blocking(move || {
        if !used {
            return super::unused_library_is_fresh(
                &worker_root,
                &module_id,
                cancellation.as_deref(),
            );
        }
        if crate::program_library_retention::retained_library_program_source(
            &worker_root,
            &module_id,
        )? {
            return Ok(false);
        }
        let previous =
            crate::program_runtime::previous_exclusive_instance(&worker_root, &module_id)?;
        let previous_root = instances_root.join(previous);
        match std::fs::symlink_metadata(&previous_root) {
            // Losing only the database relation is not a completed retirement.
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: previous_root,
                    source,
                });
            }
        }
        crate::program_seed::retired_library_is_clean(
            &worker_root,
            &module_id,
            false,
            cancellation.as_deref(),
        )
    })
    .await
    .map_err(|error| {
        invalid(
            root,
            format!("installation reuse inspection failed: {error}"),
        )
    })?
}
