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
}

/// Library, instance maintenance and startup updates share the same trust
/// transition. The returned guard must remain owned through result persistence.
pub(in crate::commands) async fn install_program_with_baseline<G, P>(
    request: ProgramUpdateRequest<'_, G>,
    mut on_progress: P,
) -> Result<(ModuleInstallResult, G), app_steamcmd::SteamCmdError>
where
    G: ProgramUpdateGuard,
    P: FnMut(InstallProgressUpdate),
{
    let ProgramUpdateRequest {
        storage,
        descriptor,
        module,
        root,
        operation,
        guard,
        validate,
        cancellation,
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
    let steam_app_id = descriptor
        .summary
        .steam_app_id
        .filter(|id| *id != 0)
        .filter(|_| {
            descriptor
                .install
                .as_ref()
                .is_some_and(|install| install.download_url_windows.is_none())
        })
        .filter(|_| library);
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
    let result = app_steamcmd::install_or_update_module_at_with_callbacks(
        &storage.settings,
        module,
        root,
        guard.install_guard(),
        // A failed acquisition may already contain its executable. Always
        // validate retries before certifying the exact official file inventory.
        validate || needs_steam_inventory.is_some(),
        cancellation,
        app_steamcmd::ModuleInstallCallbacks {
            on_progress: &mut on_progress,
            prepare_fresh_payload: |root| baseline.prepare_fresh_payload(root),
        },
    )
    .await?;
    let mut progress = InstallProgressUpdate::stage(
        app_core::InstallPhase::Verifying,
        "正在校验程序文件并准备后续实例所需的干净基线…",
    );
    progress.progress_percent = 99.0;
    on_progress(progress);
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
            )
            .await?
    } else {
        baseline
            .finish(guard, root.to_owned(), source_was_empty, same_version)
            .await?
    };
    Ok((result, guard))
}
