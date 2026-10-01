use super::*;

#[derive(Debug)]
pub(in crate::commands) enum CreationProgramPreparation {
    Ready(String),
    NeedsRepair(String),
}

pub(in crate::commands) async fn prepare_creation_program(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    operation: &StorageContextOperationGuard,
    guard: &app_steamcmd::GameInstallLifecycleGuard,
    library_root: &Path,
    source: app_core::InstanceProgramSource,
) -> Result<CreationProgramPreparation, String> {
    descriptor
        .install
        .as_ref()
        .ok_or_else(|| String::from("module has no program installation contract"))?;
    let registered = app_storage::read_program_install_owner(&storage.paths, library_root)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(record) = &registered
        && (record.module_id != descriptor.summary.id
            || record.scope != app_storage::ProgramInstallScope::Library)
    {
        return Err("Selected program directory does not belong to this game's library.".into());
    }
    guard
        .ensure_scope(&descriptor.summary.id, library_root)
        .map_err(|error| steamcmd_error_message(&error))?;
    if registered
        .as_ref()
        .is_none_or(|record| record.install_state == InstallState::NotInstalled)
    {
        // Creation may reuse surviving instances or archives after uninstall,
        // but an absent program source requires an explicit library install.
        let plan = app_storage::inspect_instance_program_creation(
            &storage.paths,
            descriptor,
            None,
            source,
            Some(operation.cancellation_token()),
        )
        .await
        .map_err(|error| error.to_string())?;
        if !plan.can_create {
            return Err(plan
                .reason
                .unwrap_or_else(|| String::from("请先下载并验证服务器安装。")));
        }
    }
    app_storage::recover_interrupted_program_adoptions(
        &storage.paths,
        &descriptor.summary.id,
        library_root,
    )
    .await
    .map_err(|error| error.to_string())?;

    let current_version = if library_root.exists() {
        if app_storage::library_program_acquisition_is_trusted(library_root, descriptor)
            .map_err(|error| error.to_string())?
        {
            return Ok(CreationProgramPreparation::NeedsRepair(String::from(
                "服务器程序尚未下载完成，请先在游戏库完成安装或校验，再创建实例；本次没有启动下载。",
            )));
        }
        let root = library_root.to_string_lossy();
        let module =
            map_module_details_with_install_state(&storage.settings, descriptor, Some(&root));
        if module.summary.install_state != InstallState::Installed
            || registered
                .as_ref()
                .is_some_and(|record| record.install_state != InstallState::Installed)
        {
            return Ok(CreationProgramPreparation::NeedsRepair(String::from(
                "本地服务器程序不完整，请先在游戏库安装或校验；已有文件已保留，本次没有启动下载。",
            )));
        }
        // Storage admission verifies the source immediately before copying.
        // Do not add a second full-package hash pass to this fast path.
        registered
            .as_ref()
            .and_then(|record| record.current_version.clone())
    } else {
        if source == app_core::InstanceProgramSource::Local {
            return Err(String::from(
                "没有可导入的本地程序库目录；归档中的实例请先恢复，或使用已验证程序创建。",
            ));
        }
        // Seed selection prefers validated live sources and leases an archive
        // only if needed. A separate global catalog scan would block unrelated
        // creation and duplicate the authoritative source checks.
        let seed = prepare_creation_seed(storage, descriptor, library_root, operation)
            .await
            .map_err(|error| error.to_string())?;
        let root = seed.install_root.to_string_lossy().into_owned();
        let module =
            map_module_details_with_install_state(&storage.settings, descriptor, Some(&root));
        let complete =
            !seed.requires_validation && module.summary.install_state == InstallState::Installed;
        // A normalized seed path must not create a second registry identity.
        sync_game_installs(
            &storage.paths,
            &[GameInstallSyncRecord {
                module_id: descriptor.summary.id.clone(),
                install_root: library_root.to_string_lossy().into_owned(),
                install_state: if complete {
                    InstallState::Installed
                } else {
                    InstallState::Incomplete
                },
                current_version: seed.current_version.clone(),
                mark_verified: complete,
            }],
        )
        .await
        .map_err(|error| error.to_string())?;
        if !complete {
            return Ok(CreationProgramPreparation::NeedsRepair(String::from(
                "现有实例或归档中的程序缺少完整校验清单，或程序文件已修改；已有文件已保留，本次没有启动下载。可恢复原实例，或先在游戏库安装或校验服务器程序。",
            )));
        }
        seed.current_version
    };
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: library_root.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version,
            mark_verified: false,
        }],
    )
    .await
    .map_err(|error| error.to_string())?;
    match app_steamcmd::read_program_install_revision(
        Path::new(&storage.settings.servers_root),
        &descriptor.summary.id,
        library_root,
    ) {
        Ok(revision) => Ok(CreationProgramPreparation::Ready(revision.to_string())),
        Err(error @ app_steamcmd::SteamCmdError::PackageRevisionPending { .. }) => Ok(
            CreationProgramPreparation::NeedsRepair(steamcmd_error_message(&error)),
        ),
        Err(error) => Err(steamcmd_error_message(&error)),
    }
}

pub(in crate::commands) async fn prepare_creation_seed(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    target: &Path,
    operation: &StorageContextOperationGuard,
) -> Result<app_storage::CleanLibrarySeed, app_storage::StorageError> {
    if app_storage::is_instance_program_acquisition(&storage.paths, &descriptor.summary.id, target)?
    {
        app_storage::prepare_instance_program_seed_at(
            &storage.paths,
            descriptor,
            target,
            Some(operation.cancellation_token()),
        )
        .await
    } else {
        app_storage::prepare_clean_library_seed_at(
            &storage.paths,
            descriptor,
            target,
            Some(operation.cancellation_token()),
        )
        .await
    }
}
