use super::commands_install_progress::{
    InstallationJobLease, apply_install_progress, complete_install_progress, fail_install_progress,
    queued_install_progress,
};
use super::*;

/// Runs after the game update and before process creation, regardless of the
/// game's pinned-version policy. The start worker retains the program leases
/// while runtime downloads and any atomic system installer finish.
pub(super) async fn prepare(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    mut module: ModuleDetails,
    reservation: &RuntimeStartReservationLease,
    log_path: Option<&str>,
) -> Result<(ModuleDetails, Vec<ProcessLaunchPlan>), String> {
    let root =
        PathBuf::from(super::commands_runtime_lifecycle::private_runtime_install_root(instance)?);
    // A missing Minecraft JRE is repairable without changing server.jar. Other
    // incomplete game installations must not trigger unrelated runtime installs.
    if !matches!(
        module.summary.id.as_str(),
        "minecraft" | "necesse" | "projectzomboid"
    ) {
        ensure_module_installed_for_start(&module)?;
    } else if module.summary.id == "minecraft" && !root.join("server.jar").is_file() {
        return Err("Minecraft 服务端程序缺失，请先完成服务端安装。".into());
    }
    let job_id = new_background_job_id("instance-runtime-environment", &instance.summary.id);
    let lease = InstallationJobLease::begin(state, job_id.clone())?;
    insert_background_job(
        state,
        storage,
        BackgroundJob {
            warnings: Vec::new(),
            id: job_id.clone(),
            kind: JobKind::ValidateGame,
            label: format!("准备 {} 运行环境", instance.summary.name),
            status: JobStatus::Running,
            progress_percent: 0.0,
            install_progress: Some(queued_install_progress()),
            cancellable: true,
            cancel_requested: false,
            target_id: Some(instance.summary.id.clone()),
            detail: Some("正在检查所需运行环境…".into()),
            output_excerpt: None,
        },
    )?;
    let cancellation = lease.cancellation().clone();
    let mut progress_error = None;
    let mut last_message = String::new();
    let mut on_progress = |update: InstallProgressUpdate| {
        let result = lease.update(|job| apply_install_progress(job, &update));
        if let Err(error) = result {
            progress_error.get_or_insert(error);
            cancellation.cancel();
        }
        let message = format!("{}\n{}", update.detail, update.output_excerpt);
        if last_message != message {
            if let Some(path) = log_path
                && let Err(error) =
                    append_startup_console_line(path, &update.detail, &update.output_excerpt)
            {
                progress_error.get_or_insert(error);
                cancellation.cancel();
            }
            last_message = message;
        }
    };
    let mut preparation = Box::pin(async {
        let initial_plans =
            build_process_launch_plans_for_instance(&storage.settings, &module, instance)?;
        let initial_executables = runtime_executables(&module, &root, &initial_plans);
        let settings: Value =
            serde_json::from_str(&instance.settings_json).map_err(|error| error.to_string())?;
        let uses_tmodloader = settings
            .get("server_runtime")
            .and_then(Value::as_str)
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("tmodloader"));
        // Java/.NET host probes can themselves need VC++. Prepare the native
        // dependencies first, rather than misdiagnosing a load failure as a
        // damaged bundled runtime.
        app_steamcmd::prepare_windows_runtime_dependencies(
            &state.install_locks,
            &storage.settings,
            &instance.summary.module_id,
            uses_tmodloader,
            &initial_executables,
            cancellation.clone(),
            &mut on_progress,
        )
        .await
        .map_err(|error| error.to_string())?;
        let portable = app_steamcmd::prepare_portable_runtime(
            &state.install_locks,
            &storage.settings,
            &instance.summary.module_id,
            &root,
            &initial_executables,
            cancellation.clone(),
            &mut on_progress,
        )
        .await
        .map_err(|error| error.to_string())?;
        if let Some(runtime) = &portable {
            apply_portable_executable(&storage.settings, &root, &mut module, runtime)?;
        }
        ensure_module_installed_for_start(&module)?;
        let mut plans =
            build_process_launch_plans_for_instance(&storage.settings, &module, instance)?;
        if let Some(runtime) = portable {
            for plan in &mut plans {
                for (key, value) in &runtime.environment {
                    plan.launch_plan
                        .environment
                        .retain(|existing, _| !existing.eq_ignore_ascii_case(key));
                    plan.launch_plan
                        .environment
                        .insert(key.clone(), value.clone());
                }
            }
        }
        ensure_launch_plans_ready(&plans)?;
        let new_executables = runtime_executables(&module, &root, &plans)
            .into_iter()
            .filter(|path| !initial_executables.contains(path))
            .collect::<Vec<_>>();
        if !new_executables.is_empty() {
            app_steamcmd::prepare_windows_runtime_dependencies(
                &state.install_locks,
                &storage.settings,
                &instance.summary.module_id,
                true, // The original vanilla XNA requirement was already checked.
                &new_executables,
                cancellation.clone(),
                &mut on_progress,
            )
            .await
            .map_err(|error| error.to_string())?;
        }
        Ok::<_, String>((module, plans))
    });
    let result = tokio::select! {
        biased;
        _ = reservation.cancelled() => {
            cancellation.cancel();
            preparation.as_mut().await
        },
        result = &mut preparation => result,
    };
    // Drop the progress callback's borrows before final job publication.
    drop(preparation);
    let result = if cancellation.is_cancelled() || reservation.is_cancelled() {
        Err(progress_error.unwrap_or_else(|| "运行环境准备已取消，实例未启动。".into()))
    } else {
        progress_error.map_or(result, Err)
    };
    lease.update(|job| match &result {
        Ok(_) => {
            complete_install_progress(job);
            job.detail = Some("所需运行环境已检查并就绪。".into());
        }
        Err(error) => {
            fail_install_progress(job, 1.0);
            if cancellation.is_cancelled() || reservation.is_cancelled() {
                job.status = JobStatus::Cancelled;
            }
            job.detail = Some(error.clone());
        }
    })?;
    if let Some(path) = log_path {
        append_startup_console_line(
            path,
            match &result {
                Ok(_) => "所需运行环境已检查并就绪。",
                Err(_) => "运行环境准备失败，实例未启动。",
            },
            result.as_ref().err().map(String::as_str).unwrap_or(""),
        )?;
    }
    result
}

fn apply_portable_executable(
    settings: &AppSettings,
    root: &Path,
    module: &mut ModuleDetails,
    runtime: &app_steamcmd::PortableRuntime,
) -> Result<(), String> {
    if let Some(executable) = &runtime.executable {
        if !executable.is_absolute() || !executable.is_file() {
            return Err("已准备的运行环境没有可用的绝对程序路径。".into());
        }
        module
            .process
            .as_mut()
            .ok_or("模块没有启动程序定义")?
            .executable = executable.to_string_lossy().into_owned();
        let probe = probe_module_install_state_with_override(
            settings,
            &module.summary.id,
            module.summary.steam_app_id,
            module.install.as_ref(),
            module.process.as_ref(),
            Some(&root.to_string_lossy()),
        );
        module.summary.install_state = probe.install_state;
    }
    Ok(())
}

fn runtime_executables(
    module: &ModuleDetails,
    root: &Path,
    plans: &[ProcessLaunchPlan],
) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = plans
        .iter()
        .map(|plan| PathBuf::from(&plan.launch_plan.executable_path))
        .collect();
    // Script wrappers such as Romestead still have an independently declared
    // package executable. Inspect that binary without evaluating the script.
    if let Some(relative) = module
        .install
        .as_ref()
        .and_then(|install| install.verification_path.as_ref())
    {
        let path = Path::new(relative);
        if !path.is_absolute()
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        {
            paths.push(root.join(path));
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

#[cfg(test)]
#[path = "commands_runtime_environment_tests.rs"]
mod tests;
