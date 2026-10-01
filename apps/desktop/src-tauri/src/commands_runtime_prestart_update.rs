use super::commands_install_progress::{
    InstallationJobLease, apply_install_progress, complete_install_progress, fail_install_progress,
    queued_install_progress,
};
use super::commands_program_storage::{ProgramUpdateRequest, install_program_with_baseline};
use super::*;

#[cfg(test)]
#[path = "commands_runtime_prestart_policy_tests.rs"]
mod tests;

pub(super) async fn acquire_prestart_program_guard(
    module_id: &str,
    root: &Path,
    reservation: &RuntimeStartReservationLease,
) -> Result<app_steamcmd::GameInstallLifecycleGuard, String> {
    let roots = [root.to_owned()];
    tokio::select! {
        biased;
        _ = reservation.cancelled() => Err("启动已取消，实例未启动。".into()),
        guard = app_steamcmd::acquire_game_install_lifecycle(module_id, &roots) => {
            guard.map_err(|error| steamcmd_error_message(&error))
        }
    }
}

pub(super) fn should_run_prestart_update(module: &ModuleDetails) -> bool {
    matches!(module.summary.install_state, InstallState::Installed)
        && module_supports_automatic_prestart_update(module)
}

pub(super) fn module_supports_automatic_prestart_update(module: &ModuleDetails) -> bool {
    module.install.as_ref().is_some_and(|install| {
        install.download_url_windows.is_none()
            && (module.summary.steam_app_id.unwrap_or(0) > 0
                || matches!(install.source, Some(InstallSource::MinecraftJava)))
    })
}

pub(super) struct PrestartUpdateLogContext<'a> {
    pub source: &'a str,
    pub console_log_path: Option<&'a str>,
}

pub(super) struct PrestartUpdateRequest<'a> {
    pub descriptor: &'a ModuleDescriptor,
    pub module: &'a ModuleDetails,
    pub instance: &'a InstanceDetails,
    pub install_guard: app_steamcmd::GameInstallLifecycleGuard,
    pub reservation: &'a RuntimeStartReservationLease,
    pub context: PrestartUpdateLogContext<'a>,
}

fn report(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    context: &PrestartUpdateLogContext<'_>,
    outcome: &str,
    message: &str,
) {
    if let Some(path) = context.console_log_path {
        let _ = append_prestart_update_console_line(path, message, "");
    }
    append_desktop_app_log(
        storage,
        if outcome == "failed" { "error" } else { "info" },
        &format!("instance.prestart_update.{outcome}"),
        message,
        json!({"source": context.source, "instance_id": instance.summary.id,
            "module_id": instance.summary.module_id, "outcome": outcome}),
    );
}

/// Caller retains the instance mutation lock. The program lease is returned only
/// after both the original-file inventory and the matching install record settle.
pub(super) async fn run_prestart_update_if_needed(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    request: PrestartUpdateRequest<'_>,
) -> Result<app_steamcmd::GameInstallLifecycleGuard, String> {
    let PrestartUpdateRequest {
        descriptor,
        module,
        instance,
        install_guard,
        reservation,
        context,
    } = request;
    let policy =
        app_core::InstanceProgramUpdatePolicy::from_settings_json(&instance.settings_json)?;
    if policy == app_core::InstanceProgramUpdatePolicy::Pinned {
        report(
            storage,
            instance,
            &context,
            "pinned",
            "当前版本已固定，使用现有服务器程序启动。",
        );
        return Ok(install_guard);
    }
    if !should_run_prestart_update(module) {
        let message = if module_supports_automatic_prestart_update(module) {
            "服务器程序尚未完整安装，无法进行启动前更新；请先完成安装。"
        } else {
            "此安装源不支持启动前自动更新，使用现有程序；需要更新时请手动维护。"
        };
        report(storage, instance, &context, "unavailable", message);
        return Ok(install_guard);
    }
    let root =
        PathBuf::from(super::commands_runtime_lifecycle::private_runtime_install_root(instance)?);
    let uses_library = app_storage::instance_uses_library_program(
        super::commands_program_storage::instance_root(instance)?,
    )
    .map_err(|error| error.to_string())?;
    let job_id = new_background_job_id("instance-prestart-update", &instance.summary.id);
    let lease = InstallationJobLease::begin(state, job_id.clone())?;
    insert_background_job(
        state,
        storage,
        BackgroundJob {
            id: job_id.clone(),
            kind: JobKind::ValidateGame,
            label: format!("启动前更新 {}", instance.summary.name),
            status: JobStatus::Running,
            progress_percent: 1.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(instance.summary.id.clone()),
            detail: Some("正在确认服务器程序版本…".into()),
            output_excerpt: None,
        },
    )?;
    report(
        storage,
        instance,
        &context,
        "started",
        "正在检查启动前更新…",
    );
    let cancellation = lease.cancellation();
    let update = async {
        // A read-only remote comparison avoids mutating an already-current shared
        // Minecraft install, including when another instance is running or pinned.
        if let Some(version) = app_steamcmd::current_program_version(
            &storage.settings,
            module,
            &root,
            &install_guard,
            cancellation,
        )
        .await
        .map_err(|error| steamcmd_error_message(&error))?
        {
            let message = format!("已确认服务器程序为当前发布版本 {version}。");
            return Ok((install_guard, message));
        }
        if uses_library {
            app_storage::ensure_library_program_target_available(&storage.paths, &root)
                .await
                .map_err(|error| error.to_string())?;
            super::commands_program_storage::ensure_shared_program_unused(
                state,
                storage,
                &instance.summary.module_id,
                &root,
                Some(&instance.summary.id),
            )
            .await
            .map_err(|reason| format!("启动前更新无法完成：{reason}"))?;
        }
        app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
            .await
            .map_err(|error| format!("启动前更新受归档版本保护：{error}"))?;
        let (result, guard) = install_program_with_baseline(
            ProgramUpdateRequest {
                storage,
                descriptor,
                module,
                root: &root,
                operation: reservation.storage_operation(),
                guard: install_guard,
                validate: false,
                cancellation,
            },
            |update: InstallProgressUpdate| {
                if let Some(path) = context.console_log_path {
                    let _ = append_prestart_update_console_line(path, &update.detail, "");
                }
                let _ = update_background_job(state, &job_id, |job| {
                    apply_install_progress(job, &update)
                });
            },
        )
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
        let record = GameInstallSyncRecord {
            module_id: result.module_id.clone(),
            install_root: result.install_root.clone(),
            install_state: result.install_state.clone(),
            current_version: result.current_version.clone(),
            mark_verified: true,
        };
        if uses_library {
            sync_game_installs(&storage.paths, &[record]).await
        } else {
            app_storage::sync_instance_game_install(&storage.paths, &instance.summary.id, &record)
                .await
        }
        .map_err(|error| error.to_string())?;
        let message = match result.current_version {
            Some(version) => format!("服务器程序已更新并核验，当前版本 {version}。"),
            None => "服务器程序更新与核验已完成。".into(),
        };
        Ok::<_, String>((guard, message))
    };
    tokio::pin!(update);
    let result = tokio::select! {
        biased;
        _ = reservation.cancelled() => {
            cancellation.cancel();
            update.await
        },
        result = &mut update => result,
    };
    match result {
        Ok((guard, message)) => {
            if cancellation.is_cancelled() || reservation.is_cancelled() {
                update_background_job(state, &job_id, |job| {
                    fail_install_progress(job, 1.0);
                    job.status = JobStatus::Cancelled;
                    job.detail = Some("启动前更新已取消，实例未启动。".into());
                })?;
                return Err("启动前更新已取消，实例未启动。".into());
            }
            update_background_job(state, &job_id, |job| {
                complete_install_progress(job);
                job.detail = Some(message.clone());
            })?;
            report(storage, instance, &context, "completed", &message);
            Ok(guard)
        }
        Err(error) => {
            update_background_job(state, &job_id, |job| {
                fail_install_progress(job, 1.0);
                if cancellation.is_cancelled() {
                    job.status = JobStatus::Cancelled;
                }
                job.detail = Some(error.clone());
            })?;
            report(storage, instance, &context, "failed", &error);
            Err(error)
        }
    }
}
