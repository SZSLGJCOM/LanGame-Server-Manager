use super::super::commands_install_progress::{InstallationJobLease, apply_install_progress};
use super::*;

pub(in crate::commands) struct CreationProgramRequest<'a> {
    pub storage: &'a StorageBootstrap,
    pub descriptor: &'a ModuleDescriptor,
    pub operation: &'a StorageContextOperationGuard,
    pub guard: &'a app_steamcmd::GameInstallLifecycleGuard,
    pub input: CreateInstanceInput,
    pub mode: Option<app_core::InstanceProgramMode>,
    pub source: app_core::InstanceProgramSource,
    pub program_root: &'a Path,
    pub repair_root: &'a Path,
    pub job: &'a InstallationJobLease,
}

/// Repair is one bounded continuation of creation. Only a fresh manager-owned
/// library may receive official files; existing programs remain untouched and
/// the repaired source survives deletion of its independent instance copies.
pub(in crate::commands) async fn create_with_program_repair<F, Fut>(
    request: CreationProgramRequest<'_>,
    install: F,
) -> Result<InstanceProvisioning, String>
where
    F: FnOnce(ModuleDetails, PathBuf, app_steamcmd::InstallCancellation) -> Fut,
    Fut: std::future::Future<Output = Result<ModuleInstallResult, String>>,
{
    let CreationProgramRequest {
        storage,
        descriptor,
        operation,
        guard,
        input,
        mode,
        source,
        program_root,
        repair_root,
        job,
    } = request;
    check_cancelled(operation, job)?;
    let acquired_for_instance = app_storage::is_instance_program_acquisition(
        &storage.paths,
        &descriptor.summary.id,
        program_root,
    )
    .map_err(creation_error_message)?;
    let options = app_storage::InstanceCreationOptions {
        prefer_existing_install: true,
        program_mode: mode,
        require_clean_program: source == app_core::InstanceProgramSource::Verified,
        use_local_program: source == app_core::InstanceProgramSource::Local,
        cancellation: Some(operation.cancellation_token()),
        ..Default::default()
    };
    let pending_at_start =
        app_storage::library_program_acquisition_is_trusted(program_root, descriptor)
            .map_err(|error| error.to_string())?;
    let prepared =
        prepare_creation_program(storage, descriptor, operation, guard, program_root, source).await;
    match prepared {
        Ok(CreationProgramPreparation::Ready(revision)) if !acquired_for_instance => {
            let result = app_storage::create_instance_with_options(
                &storage.paths,
                descriptor,
                input.clone(),
                app_storage::InstanceCreationOptions {
                    source_generation: Some(revision),
                    program_install_root: Some(program_root.to_owned()),
                    ..options.clone()
                },
            )
            .await;
            match result {
                Ok(created) => return Ok(created.provisioning),
                Err(app_storage::StorageError::CleanLibraryProgramRequired { .. })
                    if source == app_core::InstanceProgramSource::Verified => {}
                Err(error) => return Err(creation_error_message(error)),
            }
        }
        // Recover completed acquisitions as persistent library sources instead
        // of transferring the only repaired program into a deletable instance.
        Ok(CreationProgramPreparation::Ready(_)) => {}
        Ok(CreationProgramPreparation::NeedsRepair(error)) => {
            if source == app_core::InstanceProgramSource::Local {
                return Err(error);
            }
            // An interrupted installation can leave its executable in place.
            // Repair in a fresh root without clearing the old revision marker.
        }
        Err(error) => {
            if error == app_storage::StorageError::InstanceCreationCancelled.to_string() {
                return Err("installation_cancelled".into());
            }
            // Permission, ownership and recovery failures are not repaired by
            // redownloading, even when the executable is also missing.
            return Err(error);
        }
    }
    check_cancelled(operation, job)?;
    job.update(|job| {
        job.status = JobStatus::Running;
        job.detail = Some("正在准备原版程序；将复用已验证文件并补齐缺失内容…".into());
    })?;
    let original = program_root.to_owned();
    // A previous failed download may have been modified outside the manager.
    // Its acquisition marker cannot certify those extra files. Only continue
    // a seed allocated during this request; otherwise seed a fresh target.
    let resume = !pending_at_start
        && app_storage::library_program_acquisition_is_trusted(&original, descriptor)
            .map_err(|error| error.to_string())?;
    let target = if acquired_for_instance {
        repair_root.to_owned()
    } else if !original.exists() || resume {
        original.clone()
    } else {
        repair_root.to_owned()
    };
    let resume = resume && target == original;
    guard
        .ensure_scope(&descriptor.summary.id, &target)
        .map_err(|error| steamcmd_error_message(&error))?;
    let (needs_install, seed_version) = if resume {
        (true, None)
    } else {
        let seed = super::creation::prepare_creation_seed(storage, descriptor, &target, operation)
            .await
            .map_err(creation_error_message)?;
        let root_text = target.to_string_lossy();
        (
            seed.requires_validation
                || map_module_details_with_install_state(
                    &storage.settings,
                    descriptor,
                    Some(&root_text),
                )
                .summary
                .install_state
                    != InstallState::Installed,
            seed.current_version,
        )
    };
    // Failed acquisitions stay registered; later requests may reuse only bytes
    // that still match an official allowlist in another fresh acquisition.
    // Never replace the old registration or rebind its existing instances.
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: target.to_string_lossy().into_owned(),
            install_state: InstallState::Incomplete,
            current_version: None,
            mark_verified: false,
        }],
    )
    .await
    .map_err(|error| error.to_string())?;
    append_desktop_app_log(
        storage,
        "info",
        "instance.create.program_repair",
        "Preparing original server program",
        json!({
            "module_id": descriptor.summary.id, "source_root": original, "program_root": target, "requires_install": needs_install,
        }),
    );
    let root_text = target.to_string_lossy();
    let module =
        map_module_details_with_install_state(&storage.settings, descriptor, Some(&root_text));
    let version = if needs_install {
        let acquisition = app_storage::read_library_program_acquisition(&target, descriptor)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| {
                "Original program repair requires a manager-owned acquisition.".to_owned()
            })?;
        check_cancelled(operation, job)?;
        let result = install(module, target.clone(), job.cancellation().clone()).await?;
        check_cancelled(operation, job)?;
        if result.install_state != InstallState::Installed || !result.executable_exists {
            return Err(
                "Official program verification did not complete; no server was created.".into(),
            );
        }
        app_storage::restore_library_program_acquisition(&acquisition)
            .map_err(|error| error.to_string())?;
        let root = target.clone();
        let descriptor = descriptor.clone();
        let cancellation = operation.cancellation_token();
        spawn_blocking_storage_context_task(operation, move || {
            app_storage::record_library_program_baseline(
                &root,
                &descriptor,
                true,
                Some(&cancellation),
            )
            .map_err(creation_error_message)
        })
        .await
        .map_err(|error| format!("original program baseline worker failed: {error}"))??;
        result.current_version
    } else {
        seed_version
    };
    check_cancelled(operation, job)?;
    let retained_paths = storage.paths.clone();
    let retained_root = target.clone();
    let retained_descriptor = descriptor.clone();
    spawn_blocking_storage_context_task(operation, move || {
        app_storage::retain_library_program_source(
            &retained_paths,
            &retained_root,
            &retained_descriptor,
        )
        .map_err(creation_error_message)
    })
    .await
    .map_err(|error| format!("retaining library source failed: {error}"))??;
    sync_game_installs(
        &storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: target.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: version,
            mark_verified: true,
        }],
    )
    .await
    .map_err(|error| error.to_string())?;
    job.update(|job| {
        job.status = JobStatus::Running;
        job.detail = Some("原版程序已就绪，正在创建服务器…".into());
    })?;
    let revision = app_steamcmd::read_program_install_revision(
        Path::new(&storage.settings.servers_root),
        &descriptor.summary.id,
        &target,
    )
    .map_err(|error| steamcmd_error_message(&error))?;
    check_cancelled(operation, job)?;
    app_storage::create_instance_with_options(
        &storage.paths,
        descriptor,
        input,
        app_storage::InstanceCreationOptions {
            source_generation: Some(revision.to_string()),
            prefer_existing_install: false,
            program_install_root: Some(target),
            ..options
        },
    )
    .await
    .map(|created| created.provisioning)
    .map_err(creation_error_message)
}

fn check_cancelled(
    operation: &StorageContextOperationGuard,
    job: &InstallationJobLease,
) -> Result<(), String> {
    if operation.cancellation_token().load(Ordering::SeqCst) || job.cancellation().is_cancelled() {
        Err("installation_cancelled".into())
    } else {
        Ok(())
    }
}

fn creation_error_message(error: app_storage::StorageError) -> String {
    match error {
        app_storage::StorageError::InstanceCreationCancelled => "installation_cancelled".into(),
        error => error.to_string(),
    }
}

pub(in crate::commands) fn report_progress(
    job: &InstallationJobLease,
    update: &InstallProgressUpdate,
) {
    // Ready means the installer settled; creation still owns the job and lease.
    let _ = job.update(|job| apply_install_progress(job, update));
}

#[cfg(test)]
#[path = "commands_program_repair_tests.rs"]
mod tests;
