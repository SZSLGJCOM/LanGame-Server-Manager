//! Anonymous native downloads own only private worker resources. Publication
//! later uses the instance lifecycle lease and never writes the main runtime.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use app_core::AppSettings;

use crate::install_resources::{ResourceLocks, canonical_resource_path, scope_error};
use crate::{
    GameInstallLifecycleGuard, InstallCancellation, InstallDeadline, InstallLockContext,
    InstallProgressUpdate, SteamCmdError, SteamWorkshopDownloadRequest,
    SteamWorkshopDownloadResult, VerifiedWorkshopCache,
};

pub struct WorkshopPrefetchRequest<'a> {
    pub runtime_root: &'a Path,
    /// A nonexistent directory unique per attempt, disjoint from the runtime.
    pub download_root: &'a Path,
    pub consumer_app_id: u32,
    pub ids: &'a [String],
}

/// Immutable private snapshot. The caller bounds ticket count and removes this
/// attempt's directory only after every reference to the prepared result ends.
pub struct PreparedWorkshopDownload {
    verified: VerifiedWorkshopCache,
    source: Arc<ResourceLocks>,
}

impl PreparedWorkshopDownload {
    pub fn download_result(&self) -> &SteamWorkshopDownloadResult {
        self.verified.download_result()
    }
}

/// Cancellation must be signalled and this future awaited, so native process
/// cleanup completes before the caller reuses the runtime slot.
pub async fn prefetch_workshop_items<F: FnMut(InstallProgressUpdate)>(
    settings: &AppSettings,
    request: WorkshopPrefetchRequest<'_>,
    lock_context: &InstallLockContext,
    cancellation: &InstallCancellation,
    mut on_progress: F,
) -> Result<PreparedWorkshopDownload, SteamCmdError> {
    cancellation
        .scope(async {
            let deadline =
                InstallDeadline::new("Steam Workshop prefetch", crate::WORKSHOP_DOWNLOAD_TIMEOUT);
            let ids = crate::normalize_steam_workshop_item_ids(request.ids);
            if request.consumer_app_id == 0 || ids.is_empty() {
                return Err(SteamCmdError::MissingWorkshopItemIds);
            }
            let (runtime_root, download_root) = private_roots(settings, &request)?;
            let source = Arc::new(
                ResourceLocks::acquire(
                    lock_context,
                    &[],
                    std::slice::from_ref(&download_root),
                    deadline,
                )
                .await?,
            );
            let runtime = Arc::new(
                ResourceLocks::acquire(
                    lock_context,
                    &[],
                    std::slice::from_ref(&runtime_root),
                    deadline,
                )
                .await?,
            );
            let parent = download_root.parent().ok_or_else(|| {
                scope_error(
                    &download_root,
                    "private Workshop download root has no parent",
                )
            })?;
            crate::dst_workshop_cache_fs::ensure_plain_directory(parent)
                .map_err(|error| io_error(parent, std::io::Error::other(error)))?;
            // Claim this attempt exclusively; existing data can never become a
            // private native download target without the instance/module lease.
            std::fs::create_dir(&download_root).map_err(|error| io_error(&download_root, error))?;
            crate::prepare_configured_steamcmd_root(&runtime_root)?;
            let cleanup_root = runtime_root.clone();
            let cleanup_lease = Arc::clone(&runtime);
            let error_root = runtime_root.clone();
            // No native process has started. Recover this private worker's
            // abandoned Workshop cache before allowing another attempt.
            tokio::task::spawn_blocking(move || {
                cleanup_worker_cache(&cleanup_root, &cleanup_lease)
            })
            .await
            .map_err(|error| io_error(&error_root, std::io::Error::other(error)))??;
            let worker_settings = AppSettings {
                steamcmd_root: runtime_root.to_string_lossy().into_owned(),
                ..settings.clone()
            };
            let steamcmd = crate::steamcmd_prepare::prepare_private_runtime_locked(
                &worker_settings,
                deadline.limited_to(crate::steamcmd_prepare::STEAMCMD_PREPARE_TIMEOUT),
                |update| on_progress(InstallProgressUpdate::preparing_steamcmd(update)),
            )
            .await?;
            let result = crate::download_workshop_items_with_runtime(
                &steamcmd,
                SteamWorkshopDownloadRequest {
                    consumer_app_id: request.consumer_app_id,
                    install_root: &download_root,
                    ids: &ids,
                    account_name: None,
                },
                deadline,
                |mut update| {
                    if !update.is_raw_output() {
                        update.progress_percent *= 0.94;
                    }
                    on_progress(update);
                },
            )
            .await;
            let result = result.and_then(|result| {
                deadline.check_cancelled()?;
                Ok(result)
            });
            if result.is_ok() {
                snapshot_progress(
                    &mut on_progress,
                    95.0,
                    "Verifying the private Workshop snapshot...",
                );
            }
            let cancellation = cancellation.clone();
            let error_root = download_root.clone();
            // Move both leases into the IO worker. Dropping IPC cannot expose an
            // unfinished copy or let another native process rewrite its source.
            let prepared = tokio::task::spawn_blocking(move || -> Result<_, SteamCmdError> {
                let result = result.and_then(|result| {
                    let verified =
                        VerifiedWorkshopCache::capture_native_download(result, &cancellation)
                            .map_err(|error| io_error(&download_root, error))?;
                    check_cancelled(&cancellation)?;
                    crate::dst_workshop_cache::deploy_prefetched_cache(
                        &verified,
                        &download_root.join("steamapps/workshop"),
                    )
                    .map_err(|error| io_error(&download_root, std::io::Error::other(error)))?;
                    check_cancelled(&cancellation)?;
                    Ok(PreparedWorkshopDownload {
                        verified: verified.relocated_to(&download_root),
                        source: Arc::clone(&source),
                    })
                });
                // Every successful snapshot is independent of this cache. A
                // failed/cancelled native call is also reaped before returning,
                // except for the explicit unconfirmed-process error below.
                finish_worker_attempt(&runtime_root, &runtime, result)
            })
            .await
            .map_err(|error| io_error(&error_root, std::io::Error::other(error)))??;
            snapshot_progress(
                &mut on_progress,
                96.0,
                "Workshop snapshot ready for instance deployment.",
            );
            Ok(prepared)
        })
        .await
}

fn snapshot_progress(
    on_progress: &mut impl FnMut(InstallProgressUpdate),
    percent: f32,
    detail: &str,
) {
    on_progress(InstallProgressUpdate {
        console_output: None,
        install_progress: None,
        progress_percent: percent,
        detail: detail.into(),
        output_excerpt: String::new(),
    });
}

fn cleanup_worker_cache(root: &Path, lease: &ResourceLocks) -> Result<(), SteamCmdError> {
    lease.ensure_root(root)?;
    crate::validate_steamcmd_ownership(root)?;
    let cache = root.join("steamapps/workshop");
    check_existing_ancestor(&cache)?;
    match std::fs::symlink_metadata(&cache) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(io_error(&cache, error)),
        Ok(_) => {}
    }
    // Never follow a link/reparse point, including one inside a partial cache.
    // The only deletable subtree is this leased, owned worker's Workshop data;
    // executable, package, config and bootstrap state are outside it.
    crate::dst_workshop_cache_fs::inspect_tree(&cache, 0)
        .map_err(|error| io_error(&cache, std::io::Error::other(error)))?;
    std::fs::remove_dir_all(&cache).map_err(|error| io_error(&cache, error))
}

fn finish_worker_attempt<T>(
    root: &Path,
    lease: &ResourceLocks,
    result: Result<T, SteamCmdError>,
) -> Result<T, SteamCmdError> {
    if matches!(
        &result,
        Err(SteamCmdError::InstallProcessCleanupFailed { .. }
            | SteamCmdError::SteamCmdPreparationCleanupFailed { .. })
    ) {
        // Do not delete data while the native process tree may still own it.
        return result;
    }
    match (result, cleanup_worker_cache(root, lease)) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(primary), Err(cleanup)) => Err(io_error(
            &root.join("steamapps/workshop"),
            std::io::Error::other(WorkerCleanupFailure { primary, cleanup }),
        )),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{primary}; private Workshop cache cleanup also failed: {cleanup}")]
struct WorkerCleanupFailure {
    #[source]
    primary: SteamCmdError,
    cleanup: SteamCmdError,
}

fn private_roots(
    settings: &AppSettings,
    request: &WorkshopPrefetchRequest<'_>,
) -> Result<(PathBuf, PathBuf), SteamCmdError> {
    check_existing_ancestor(request.runtime_root)?;
    check_existing_ancestor(request.download_root)?;
    let runtime = canonical_resource_path(request.runtime_root)?;
    let download = canonical_resource_path(request.download_root)?;
    let overlap = |a: &Path, b: &Path| a.starts_with(b) || b.starts_with(a);
    if overlap(&runtime, &download) {
        return Err(scope_error(
            &download,
            "private Workshop runtime and download roots overlap",
        ));
    }
    for shared in crate::steamcmd_resource_roots(settings) {
        let shared = canonical_resource_path(&shared)?;
        if overlap(&shared, &runtime) || overlap(&shared, &download) {
            return Err(scope_error(
                &runtime,
                "private Workshop resources overlap the shared SteamCMD runtime",
            ));
        }
    }
    Ok((runtime, download))
}

fn check_existing_ancestor(path: &Path) -> Result<(), SteamCmdError> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => {
                return crate::steam_depot::check_plain(ancestor, true)
                    .map_err(|error| io_error(ancestor, error));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io_error(ancestor, error)),
        }
    }
    Err(scope_error(
        path,
        "private Workshop resource has no existing ancestor",
    ))
}

/// The caller holds the stopped instance/module lease and retains it through
/// later game-specific deployment. This function retains the same target lease
/// and the prepared source in its owned IO worker even if its future is dropped.
pub async fn publish_prefetched_workshop_items(
    prepared: Arc<PreparedWorkshopDownload>,
    target_install_root: PathBuf,
    game_guard: &GameInstallLifecycleGuard,
) -> Result<SteamWorkshopDownloadResult, SteamCmdError> {
    game_guard.0.ensure_root(&target_install_root)?;
    prepared.source.ensure_disjoint(&target_install_root)?;
    let target_lease = Arc::clone(&game_guard.0);
    let error_root = target_install_root.clone();
    tokio::task::spawn_blocking(move || {
        let _target_lease = target_lease;
        publish(&prepared.verified, &target_install_root)
    })
    .await
    .map_err(|error| io_error(&error_root, std::io::Error::other(error)))?
}

fn publish(
    verified: &VerifiedWorkshopCache,
    install: &Path,
) -> Result<SteamWorkshopDownloadResult, SteamCmdError> {
    crate::dst_workshop_cache::deploy_prefetched_cache(
        verified,
        &install.join("steamapps/workshop"),
    )
    .map_err(|error| io_error(install, std::io::Error::other(error)))?;
    let relocated = verified.clone().relocated_to(install);
    let result = relocated.download_result();
    let content = Path::new(&result.workshop_root);
    let cancellation = InstallCancellation::new();
    let inventory =
        crate::workshop_cache::read_installed_inventory(content, result.consumer_app_id, true)?;
    for item in &result.items {
        crate::workshop_cache_integrity::record_item(
            content,
            result.consumer_app_id,
            &item.item_id,
            &cancellation,
        )
        .map_err(|error| io_error(content, error))?;
        let installed = inventory.get(&item.item_id).ok_or_else(|| {
            io_error(
                content,
                std::io::Error::other("published Workshop item is missing installed metadata"),
            )
        })?;
        crate::workshop_instance_membership::record_native_success(
            install,
            result.consumer_app_id,
            &item.item_id,
            installed.manifest,
            &cancellation,
        )
        .map_err(|error| io_error(install, error))?;
    }
    Ok(result.clone())
}

fn check_cancelled(cancellation: &InstallCancellation) -> Result<(), SteamCmdError> {
    if cancellation.is_cancelled() {
        Err(SteamCmdError::InstallCancelled {
            operation: "Steam Workshop prefetch".into(),
        })
    } else {
        Ok(())
    }
}

fn io_error(path: &Path, source: std::io::Error) -> SteamCmdError {
    if source.kind() == std::io::ErrorKind::Interrupted {
        SteamCmdError::InstallCancelled {
            operation: "Steam Workshop prefetch".into(),
        }
    } else {
        SteamCmdError::WorkshopInventoryRead {
            path: path.to_owned(),
            source,
        }
    }
}

#[cfg(test)]
#[path = "workshop_prefetch_tests.rs"]
mod tests;
