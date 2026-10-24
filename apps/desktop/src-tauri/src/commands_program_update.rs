use super::super::commands_install_progress::verification_install_progress;
use super::*;

/// The complete caller-owned mutation lease survives the blocking baseline work.
pub(in crate::commands) trait ProgramUpdateGuard: Send + 'static {
    fn install_guard(&self) -> &app_steamcmd::GameInstallLifecycleGuard;
}

impl ProgramUpdateGuard for app_steamcmd::GameInstallLifecycleGuard {
    fn install_guard(&self) -> &app_steamcmd::GameInstallLifecycleGuard {
        self
    }
}

impl ProgramUpdateGuard for LibraryProgramMutation {
    fn install_guard(&self) -> &app_steamcmd::GameInstallLifecycleGuard {
        &self.install
    }
}

impl ProgramUpdateGuard for InstanceProgramMutation {
    fn install_guard(&self) -> &app_steamcmd::GameInstallLifecycleGuard {
        self.install()
    }
}

pub(in crate::commands) struct ProgramUpdateRequest<'a, G> {
    pub storage: &'a StorageBootstrap,
    pub descriptor: &'a ModuleDescriptor,
    pub module: &'a ModuleDetails,
    pub root: &'a Path,
    pub operation: &'a StorageContextOperationGuard,
    pub guard: G,
    pub validate: bool,
    pub cancellation: &'a app_steamcmd::InstallCancellation,
    pub account_name: Option<&'a str>,
}

/// Library, instance maintenance and startup updates share the same trust
/// transition. The returned guard must remain owned through result persistence.
pub(in crate::commands) async fn install_program_with_baseline<G, P>(
    request: ProgramUpdateRequest<'_, G>,
    on_progress: P,
) -> Result<(ModuleInstallResult, G), app_steamcmd::SteamCmdError>
where
    G: ProgramUpdateGuard,
    P: FnMut(InstallProgressUpdate) + Send,
{
    let started_at = std::time::Instant::now();
    let on_progress = std::sync::Mutex::new(on_progress);
    let ProgramUpdateRequest {
        storage,
        descriptor,
        module,
        root,
        operation,
        guard,
        validate,
        cancellation,
        account_name,
    } = request;
    let metadata_error = |detail| app_steamcmd::SteamCmdError::InstallationVerificationFailed {
        module_id: descriptor.summary.id.clone(),
        operation: "reading original program inventory".into(),
        detail,
    };
    guard
        .install_guard()
        .ensure_scope(&descriptor.summary.id, root)?;
    if cancellation.is_cancelled() || operation.cancellation_token().load(Ordering::Acquire) {
        return Err(app_steamcmd::SteamCmdError::InstallCancelled {
            operation: "preparing program acquisition".into(),
        });
    }
    let source_was_empty = program_directory_is_empty(root).map_err(metadata_error)?;
    let owner = app_storage::read_program_install_owner(&storage.paths, root)
        .await
        .map_err(|error| metadata_error(error.to_string()))?;
    if owner
        .as_ref()
        .is_some_and(|record| record.module_id != descriptor.summary.id)
    {
        return Err(metadata_error(
            "Program directory belongs to another game.".into(),
        ));
    }
    let library = owner
        .as_ref()
        .is_none_or(|record| record.scope == app_storage::ProgramInstallScope::Library);
    let replacing_archive = !source_was_empty
        && descriptor
            .install
            .as_ref()
            .is_some_and(|install| install.download_url_windows.is_some())
        && !app_steamcmd::has_retained_install_data(root);
    let replacement = if replacing_archive {
        if owner.is_none() {
            return Err(metadata_error(
                "Existing archive installation has no managed program owner.".into(),
            ));
        }
        let checked_root = root.to_owned();
        let checked_descriptor = descriptor.clone();
        let has_baseline = run_baseline_worker(
            operation,
            cancellation,
            &descriptor.summary.id,
            |_| {},
            move |_, _| {
                app_storage::archive_program_has_baseline(&checked_root, &checked_descriptor)
                    .map_err(|error| error.to_string())
            },
        )
        .await?;
        if !has_baseline {
            return Err(metadata_error("Archive update requires its original verified package inventory. Preserve this directory and install into a separate empty directory before migrating saves and configuration.".into()));
        }
        let protected = super::super::commands_mods::collect_protected_install_data_paths(
            &storage.paths,
            descriptor,
            root,
        )
        .await
        .map_err(metadata_error)?;
        let paths = protected
            .into_iter()
            .map(|item| {
                item.path.ok_or_else(|| {
                    metadata_error(
                        "Protected data cannot be separated from the program directory.".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Some((root.to_owned(), paths))
    } else {
        None
    };
    let steam_source = descriptor
        .summary
        .steam_app_id
        .filter(|id| *id != 0)
        .filter(|_| {
            descriptor
                .install
                .as_ref()
                .is_some_and(|install| install.download_url_windows.is_none())
        });
    let retained_paths = if steam_source.is_some() && !source_was_empty {
        let resolved = super::super::commands_mods::collect_protected_install_data_paths(
            &storage.paths,
            descriptor,
            root,
        )
        .await
        .map_err(metadata_error)?;
        let mut paths = resolved
            .into_iter()
            .map(|item| {
                item.path.ok_or_else(|| {
                    metadata_error(
                        "Protected data cannot be separated from the program directory.".into(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let checked_root = root.to_owned();
        let checked_descriptor = descriptor.clone();
        paths.extend(
            run_baseline_worker(
                operation,
                cancellation,
                &descriptor.summary.id,
                |_| {},
                move |_, _| {
                    app_storage::program_update_retained_paths(&checked_root, &checked_descriptor)
                        .map_err(|error| error.to_string())
                },
            )
            .await?,
        );
        paths
    } else {
        Vec::new()
    };
    let steam_app_id = steam_source.filter(|_| library);
    if steam_app_id.is_some() && source_was_empty {
        // Keep acquisition ownership from the first attempt, including failures
        // after Steam has already written part of the server payload.
        app_storage::begin_empty_library_program_acquisition(root, descriptor)
            .map_err(|error| metadata_error(error.to_string()))?;
    }
    let needs_steam_inventory = steam_app_id
        .map(|app_id| {
            app_storage::library_program_acquisition_is_trusted(root, descriptor)
                .map_err(|error| error.to_string())
                .and_then(|pending| {
                    root.join(".langame-clean-package.json")
                        .try_exists()
                        .map(|has_inventory| (pending || !has_inventory).then_some(app_id))
                        .map_err(|error| error.to_string())
                })
        })
        .transpose()
        .map_err(metadata_error)?
        .flatten();
    let previous_version = owner.and_then(|record| record.current_version);
    let baseline = LibraryBaselineRecorder::new(operation, descriptor, cancellation);
    let result = app_steamcmd::install_or_update_module_at_with_login_and_callbacks(
        app_steamcmd::SteamModuleInstallRequest {
            settings: &storage.settings,
            module,
            install_root: root,
            guard: guard.install_guard(),
            // Empty first acquisitions have no old bytes to repair and still require
            // exact official depot verification below. Existing partial or unknown
            // payloads must be repaired before their inventory can be certified.
            validate: should_validate_program_acquisition(
                validate,
                needs_steam_inventory.is_some(),
                source_was_empty,
            ),
            cancellation,
            account_name,
            retained_paths: &retained_paths,
        },
        app_steamcmd::ModuleInstallCallbacks {
            on_progress: |update| {
                report_program_install_progress(&on_progress, started_at, update);
            },
            prepare_fresh_payload: |root| {
                let replacement = replacement.clone();
                let baseline = &baseline;
                let on_progress = &on_progress;
                async move {
                    let progress = |progress| {
                        report_program_install_progress(
                            on_progress,
                            started_at,
                            verification_install_progress(progress),
                        );
                    };
                    if let Some((source, protected)) = replacement {
                        baseline
                            .prepare_archive_replacement(root, source, protected, progress)
                            .await
                    } else {
                        baseline.prepare_fresh_payload(root, progress).await
                    }
                }
            },
        },
    )
    .await?;
    let same_version = previous_version
        .as_ref()
        .is_some_and(|version| result.current_version.as_ref() == Some(version));
    let guard = if let Some(app_id) = needs_steam_inventory {
        let version = result
            .current_version
            .clone()
            .ok_or_else(|| metadata_error("Steam acquisition has no installed build.".into()))?;
        baseline
            .finish_steam_acquisition(
                guard,
                root.to_owned(),
                storage.paths.steamcmd_root.clone(),
                app_id,
                version,
                |progress| {
                    report_program_install_progress(
                        &on_progress,
                        started_at,
                        verification_install_progress(progress),
                    );
                },
            )
            .await?
    } else {
        baseline
            .finish(
                guard,
                root.to_owned(),
                source_was_empty,
                same_version,
                |progress| {
                    report_program_install_progress(
                        &on_progress,
                        started_at,
                        verification_install_progress(progress),
                    );
                },
            )
            .await?
    };
    Ok((result, guard))
}

fn should_validate_program_acquisition(
    explicit_validation: bool,
    needs_steam_inventory: bool,
    source_was_empty: bool,
) -> bool {
    explicit_validation || (needs_steam_inventory && !source_was_empty)
}

fn report_program_install_progress<P: FnMut(InstallProgressUpdate)>(
    on_progress: &std::sync::Mutex<P>,
    started_at: std::time::Instant,
    mut update: InstallProgressUpdate,
) {
    if let Some(progress) = update.install_progress.as_mut() {
        // Native completion precedes inventory publication and database persistence.
        // The owning job announces Ready only after those steps succeed.
        if progress.phase == app_core::InstallPhase::Ready {
            return;
        }
        progress.elapsed_seconds = started_at.elapsed().as_secs();
    }
    on_progress
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)(update);
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{InstallPhase, InstallVerificationProgress, InstallVerificationStage};
    use std::time::{Duration, Instant};

    #[test]
    fn new_empty_acquisition_needs_official_inventory_without_native_revalidation() {
        assert!(!should_validate_program_acquisition(false, true, true));
    }

    #[test]
    fn failed_retry_and_existing_payload_without_inventory_keep_native_validation() {
        assert!(should_validate_program_acquisition(false, true, false));
        // A normally registered installation retains the ordinary update policy.
        assert!(!should_validate_program_acquisition(false, false, false));
    }

    #[test]
    fn explicitly_requested_validation_always_reaches_the_native_installer() {
        for needs_inventory in [false, true] {
            for source_was_empty in [false, true] {
                assert!(should_validate_program_acquisition(
                    true,
                    needs_inventory,
                    source_was_empty
                ));
            }
        }
    }

    #[test]
    fn baseline_progress_keeps_outer_elapsed_and_native_ready_waits_for_persistence() {
        let reports = std::sync::Mutex::new(Vec::new());
        let on_progress = std::sync::Mutex::new(|update| reports.lock().unwrap().push(update));
        let started_at = Instant::now() - Duration::from_secs(12);
        report_program_install_progress(
            &on_progress,
            started_at,
            InstallProgressUpdate::stage(InstallPhase::Ready, "native install complete"),
        );
        assert!(reports.lock().unwrap().is_empty());
        report_program_install_progress(
            &on_progress,
            started_at,
            verification_install_progress(InstallVerificationProgress {
                stage: InstallVerificationStage::SteamFiles,
                completed_bytes: 128,
                total_bytes: 256,
                completed_files: 1,
                total_files: 2,
            }),
        );
        let reports = reports.lock().unwrap();
        let progress = reports[0].install_progress.as_ref().unwrap();
        assert_eq!(progress.phase, InstallPhase::Verifying);
        assert_eq!(progress.percent, Some(50.0));
        assert!(progress.elapsed_seconds >= 12);
        assert!(progress.verification.is_some());
    }
}
