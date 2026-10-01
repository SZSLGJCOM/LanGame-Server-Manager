use super::{
    SeedSource, SeedStage, SelectedSeed, copy_manifest, excluded, invalid, package_exclusions,
    read_manifest, require_absent, validate_manifest, write_manifest,
};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use app_modules::ModuleDescriptor;

use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::{contains, normalize_path};
use crate::{ProgramInstallScope, StorageError, StoragePaths};

pub(super) async fn select(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    target: &Path,
    cancellation: &Arc<AtomicBool>,
) -> Result<Option<SelectedSeed>, StorageError> {
    let mut sources = Vec::new();
    if let Some(library) =
        crate::read_library_program_install(paths, &descriptor.summary.id).await?
    {
        let root = normalize_path(&library.install_root)?;
        if root != target && root.is_dir() {
            sources.push(SeedSource {
                root,
                current_version: library.current_version,
            });
        }
    }
    for library in crate::plan_module_library_cleanup(paths, descriptor, false)
        .await?
        .installations
    {
        if library.install_state == app_core::InstallState::NotInstalled {
            continue;
        }
        let root = normalize_path(&library.install_root)?;
        if root != target && root.is_dir() && !sources.iter().any(|source| source.root == root) {
            sources.push(SeedSource {
                root,
                current_version: library.current_version,
            });
        }
    }
    sources.extend(
        crate::read_module_instance_installs(paths, &descriptor.summary.id)
            .await?
            .into_iter()
            .filter(|record| {
                record.runtime_mode == "independent"
                    && record.install.scope == ProgramInstallScope::Instance
                    && record.install.install_state == app_core::InstallState::Installed
                    && record.install.owner_instance_id.as_deref()
                        == Some(record.instance_id.as_str())
            })
            .map(|record| SeedSource {
                root: record.install.install_root,
                current_version: record.install.current_version,
            }),
    );
    let worker_target = target.to_owned();
    let worker_descriptor = descriptor.clone();
    let token = Arc::clone(cancellation);
    let mut selected = tokio::task::spawn_blocking(move || {
        select_candidates(&worker_target, &worker_descriptor, &sources, Some(&token))
    })
    .await
    .map_err(|error| invalid(target, format!("clean seed selection failed: {error}")))??;
    if selected.as_ref().is_some_and(|seed| seed.complete) {
        // A complete live source never needs archive catalog admission.
        return Ok(selected);
    }
    let result =
        select_archived_candidates(paths, descriptor, target, cancellation, &mut selected).await;
    match result {
        Ok(()) => Ok(selected),
        Err(error) => {
            Err(
                tokio::task::spawn_blocking(move || cleanup_after_error(selected, error))
                    .await
                    .map_err(|error| {
                        invalid(target, format!("seed cleanup worker failed: {error}"))
                    })?,
            )
        }
    }
}

async fn select_archived_candidates(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    target: &Path,
    cancellation: &Arc<AtomicBool>,
    selected: &mut Option<SelectedSeed>,
) -> Result<(), StorageError> {
    check_creation_cancelled(Some(cancellation))?;
    // Only read candidate identities from the database, then lease one source.
    // Mutations take catalog -> source; a copier never acquires the catalog.
    let archives =
        crate::archived_programs::archived_program_source_ids(paths, &descriptor.summary.id)
            .await?;
    for archive_id in archives {
        check_creation_cancelled(Some(cancellation))?;
        let Some((source, lease)) =
            crate::archived_programs::lease_archived_program_source(paths, &archive_id).await?
        else {
            continue;
        };
        if source.module_id != descriptor.summary.id {
            continue;
        }
        let worker_target = target.to_owned();
        let worker_descriptor = descriptor.clone();
        let token = Arc::clone(cancellation);
        let previous = selected.take();
        *selected = tokio::task::spawn_blocking(move || {
            continue_selection(
                &worker_target,
                &worker_descriptor,
                &[SeedSource {
                    root: source.install_root,
                    current_version: source.current_version,
                }],
                Some(&token),
                previous,
                Some(lease),
            )
        })
        .await
        .map_err(|error| invalid(target, format!("archived seed selection failed: {error}")))??;
        if selected.as_ref().is_some_and(|seed| seed.complete) {
            break;
        }
    }
    Ok(())
}

/// Validate bytes while making one private copy. A usable next manifest replaces
/// the incomplete staging tree before copying; package versions never mix.
pub(super) fn select_candidates(
    target: &Path,
    descriptor: &ModuleDescriptor,
    sources: &[SeedSource],
    cancellation: Option<&AtomicBool>,
) -> Result<Option<SelectedSeed>, StorageError> {
    continue_selection(target, descriptor, sources, cancellation, None, None)
}

pub(super) fn continue_selection(
    target: &Path,
    descriptor: &ModuleDescriptor,
    sources: &[SeedSource],
    cancellation: Option<&AtomicBool>,
    mut selected: Option<SelectedSeed>,
    mut source_lease: Option<crate::instance_settings_lock::InstanceSettingsLock>,
) -> Result<Option<SelectedSeed>, StorageError> {
    let result = (|| {
        check_creation_cancelled(cancellation)?;
        normalize_path(target)?;
        require_absent(target)?;
        let exclusions = package_exclusions(descriptor)?;
        for source in sources {
            check_creation_cancelled(cancellation)?;
            let source_root = normalize_path(&source.root)?;
            if contains(&source_root, target) || contains(target, &source_root) {
                return Err(invalid(&source.root, "source and library seed overlap"));
            }
            let Some(mut manifest) = read_manifest(&source.root)? else {
                continue;
            };
            if manifest.module_id != descriptor.summary.id {
                return Err(invalid(
                    &source.root,
                    "clean package belongs to another module",
                ));
            }
            manifest.files.retain(|key, _| {
                !excluded(key, &exclusions, descriptor.summary.id == "dontstarve")
            });
            manifest
                .directories
                .retain(|key| !excluded(key, &exclusions, descriptor.summary.id == "dontstarve"));
            // A copied package gets new file identities. The copied bytes are
            // verified below; source-only metadata cannot certify the new root.
            manifest.verified_files.clear();
            validate_manifest(&manifest, &source.root)?;
            if let Some(previous) = selected.take() {
                previous.discard()?;
            }
            let mut stage = SeedStage::for_target(target)?;
            let copied = (|| {
                let complete = copy_manifest(&source.root, &stage.path, &manifest, cancellation)?;
                write_manifest(&stage.path, &manifest)?;
                Ok::<_, StorageError>(complete)
            })();
            let complete = match copied {
                Ok(complete) => complete,
                Err(error) => {
                    return match stage.cleanup() {
                        Ok(()) => Err(error),
                        Err(cleanup) => Err(invalid(
                            &stage.path,
                            format!("{error}; owned seed cleanup failed: {cleanup}"),
                        )),
                    };
                }
            };
            selected = Some(SelectedSeed {
                stage: Some(stage),
                complete,
                current_version: complete.then(|| source.current_version.clone()).flatten(),
                _archive_lease: source_lease.take(),
            });
            if complete {
                break;
            }
        }
        Ok::<_, StorageError>(())
    })();
    match result {
        Ok(()) => Ok(selected),
        Err(error) => Err(cleanup_after_error(selected, error)),
    }
}

pub(super) fn cleanup_after_error(
    selected: Option<SelectedSeed>,
    error: StorageError,
) -> StorageError {
    if let Some(selected) = selected {
        let path = selected
            .stage
            .as_ref()
            .map(|stage| stage.path.clone())
            .unwrap_or_default();
        if let Err(cleanup) = selected.discard() {
            return invalid(
                &path,
                format!("{error}; owned seed cleanup failed: {cleanup}"),
            );
        }
    }
    error
}

impl SelectedSeed {
    /// Explicit completion runs on a blocking worker and reports cleanup errors.
    fn discard(mut self) -> Result<(), StorageError> {
        let _archive_lease = self._archive_lease.take();
        match self.stage.take() {
            Some(mut stage) => stage.cleanup(),
            None => Ok(()),
        }
    }
}

impl Drop for SelectedSeed {
    fn drop(&mut self) {
        let Some(stage) = self.stage.take() else {
            return;
        };
        let archive_lease = self._archive_lease.take();
        let cleanup = move || {
            // A dropped async waiter must not traverse a large staging tree on
            // its executor or release the archive lease before cleanup finishes.
            let _archive_lease = archive_lease;
            drop(stage);
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(cleanup);
        } else {
            cleanup();
        }
    }
}
