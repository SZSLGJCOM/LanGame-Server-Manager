use super::commands_runtime_lifecycle::{
    InstanceRunConflict, InstanceShutdownSource, dst_lifecycle, load_module_shutdown_specs,
    request_instance_graceful_shutdown, stop_active_instance_processes,
};
use super::*;

#[cfg(test)]
#[path = "commands_instance_stop_tests.rs"]
mod tests;

async fn cancel_startup(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance_id: &str,
    cancellation: Arc<crate::state::RuntimeStartControl>,
) -> Result<StopInstanceResult, String> {
    append_desktop_app_log(
        storage,
        "info",
        "instance.startup.cancel_requested",
        "Cancelling this startup and waiting for its owned workers and processes to exit.",
        json!({"instance_id": instance_id}),
    );
    // Cancellation is signalled before acquiring the mutation lock held by the
    // startup worker. Its last lease releases only after cleanup has settled.
    cancellation
        .wait_until_settled(Duration::from_secs(120))
        .await?;
    let _instance_lock = state.acquire_instance_mutation(instance_id).await;
    if state
        .pending_runtime_start_instance_ids()?
        .iter()
        .any(|id| id == instance_id)
    {
        return Err("A newer startup owns this instance; the cancelled request did not stop that replacement.".into());
    }
    if read_active_instance_run(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
        || state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .is_tracked(instance_id)
    {
        return Err("Startup cancellation could not confirm complete process cleanup; surviving processes remain managed.".into());
    }
    let instances = list_instances(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let summary = instances
        .iter()
        .find(|instance| instance.id == instance_id)
        .cloned()
        .ok_or("Cancelled instance is no longer present")?;
    update_desktop_state_instances(state, instances)?;
    append_desktop_app_log(
        storage,
        "info",
        "instance.startup.cancelled",
        "Startup cancelled and its resources released. No graceful world save was claimed.",
        json!({"instance_id": instance_id}),
    );
    Ok(StopInstanceResult {
        summary,
        outcome: app_core::StopInstanceOutcome::StartupCancelled,
        auto_backup_error: None,
        run_id: None,
        session_id: None,
        pid: None,
        log_path: None,
        exit_code: None,
        process_count: 0,
        processes: Vec::new(),
    })
}

// Both native Windows pipe writes and ConPTY input have a two-second budget.
// Wait only for an already accepted writer; the final exit watchdog still owns
// the overall deadline, including workers delayed before their native write.
const PENDING_STDIN_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const PENDING_STDIN_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Callers hold the instance mutation lock. Every stop uses the same ordered
/// save/graceful-stop path, regardless of whether a desktop is still attached.
pub(super) async fn stop_run<L: StorageContextTaskLease>(
    state: &DesktopState,
    storage: &StorageBootstrap,
    storage_lease: &L,
    details: &InstanceDetails,
    active_run: &ActiveInstanceRun,
    shutdown: Option<&ModuleShutdownSpec>,
    source: InstanceShutdownSource,
) -> Result<Vec<app_core::StoppedProcess>, String> {
    wait_for_pending_stdin(
        state,
        &details.summary.id,
        active_run.run_id,
        active_run.session_id.as_deref(),
    )
    .await?;
    request_instance_graceful_shutdown(state, storage, storage_lease, details, shutdown, source)
        .await?;
    if details.summary.module_id == "dontstarve" {
        dst_lifecycle::finish_confirmed_stop(state, storage, &details.summary.id, active_run).await
    } else {
        stop_active_instance_processes(
            state,
            storage,
            storage_lease,
            &details.summary.id,
            active_run,
            source,
        )
        .await
    }
}

async fn wait_for_pending_stdin(
    state: &DesktopState,
    instance_id: &str,
    run_id: i64,
    session_id: Option<&str>,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + PENDING_STDIN_DRAIN_TIMEOUT;
    loop {
        let pending = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .has_pending_command_dispatches(instance_id, run_id, session_id)
            .map_err(|error| error.to_string())?;
        if !pending {
            return Ok(());
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(format!(
                "Instance `{instance_id}` still has an accepted stdin write after two seconds; save/stop commands were not dispatched."
            ));
        }
        tokio::time::sleep_until((now + PENDING_STDIN_POLL_INTERVAL).min(deadline)).await;
    }
}

/// Back up only after stop, following the instance's existing operator policy.
/// Game save commands belong to stop_run and are independent of this archive.
pub(super) async fn backup_after_stop(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    source: InstanceShutdownSource,
) -> Option<String> {
    if !details.auto_backup_on_stop {
        return None;
    }
    match create_instance_auto_stop_backup_snapshot(&storage.paths, &details.summary.id).await {
        Ok(backup) => {
            append_desktop_app_log(
                storage,
                "info",
                "instance.stop.auto_backup_created",
                "Automatic save backup created after stop.",
                json!({
                    "instance_id": details.summary.id,
                    "instance_name": details.summary.name,
                    "source": source.as_str(),
                    "backup_id": backup.backup_id,
                    "backup_path": backup.backup_path,
                    "file_count": backup.file_count,
                    "total_bytes": backup.total_bytes,
                    "retention_count": details.backup_retention_count,
                }),
            );
            None
        }
        Err(error) => {
            let message = error.to_string();
            append_desktop_app_log(
                storage,
                "error",
                "instance.stop.auto_backup_failed",
                &message,
                json!({"instance_id": details.summary.id, "source": source.as_str()}),
            );
            Some(message)
        }
    }
}

async fn finish_manual_stop(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    refresh_summary: impl std::future::Future<Output = Result<InstanceSummary, String>>,
) -> Result<(InstanceSummary, Option<String>), String> {
    // The process has already stopped. A failed UI-state refresh must not skip
    // the operator's automatic backup policy or conceal a backup failure.
    let auto_backup_error =
        backup_after_stop(storage, details, InstanceShutdownSource::Manual).await;
    let summary = refresh_summary.await.map_err(|refresh_error| {
        let mut message =
            format!("The server stopped, but refreshing the server list failed: {refresh_error}");
        if let Some(error) = &auto_backup_error {
            message.push_str(&format!(". Automatic backup also failed: {error}"));
        }
        append_desktop_app_log(
            storage,
            "error",
            "instance.stop.refresh_failed",
            &message,
            json!({"instance_id": details.summary.id, "auto_backup_error": auto_backup_error}),
        );
        message
    })?;
    Ok((summary, auto_backup_error))
}

pub(super) async fn stop_instance(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    expected_instance: Option<InstanceDetails>,
    startup_attempt_id: Option<String>,
) -> Result<StopInstanceResult, String> {
    let storage_context_operation = state.begin_storage_context_operation("runtime stop")?;
    let cancellation = if expected_instance.is_none() {
        state.request_runtime_start_cancellation_for_attempt(
            &instance_id,
            startup_attempt_id.as_deref(),
        )?
    } else {
        None
    };
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(cancellation) = cancellation {
        return cancel_startup(&state, &storage, &instance_id, cancellation).await;
    }
    reconcile_runtime_state(&state).await?;
    let _instance_lock = state.acquire_instance_mutation(&instance_id).await;

    if let Some(attempt_id) = startup_attempt_id.as_deref() {
        state.validate_runtime_start_attempt(&instance_id, attempt_id)?;
        state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| "runtime restart scheduler lock poisoned")?
            .request_stop(&instance_id);
        state.autostart.cancel(&instance_id)?;
    }

    let active_run = read_active_instance_run(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| InstanceRunConflict::NotRunning.into_error(&instance_id))?;
    let details = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(expected) = expected_instance {
        if serde_json::to_value(&details).map_err(|error| error.to_string())?
            != serde_json::to_value(&expected).map_err(|error| error.to_string())?
        {
            return Err(
                "The server changed after the stop preview. Request a new confirmation.".into(),
            );
        }
        state
            .runtime_restart_scheduler
            .lock()
            .map_err(|_| String::from("runtime restart scheduler lock poisoned"))?
            .request_stop(&instance_id);
        state.autostart.cancel(&instance_id)?;
    }
    let shutdown_by_module = load_module_shutdown_specs(&storage, InstanceShutdownSource::Manual);
    let shutdown = shutdown_by_module.get(&details.summary.module_id);

    append_desktop_app_log(
        &storage,
        "info",
        "instance.stop.started",
        "Stopping managed instance with its declared shutdown strategy.",
        json!({
            "instance_id": instance_id.as_str(),
            "instance_name": details.summary.name.as_str(),
            "module_id": details.summary.module_id.as_str(),
            "has_shutdown_strategy": shutdown.is_some(),
        }),
    );

    let stop_result = super::commands_instance_stop::stop_run(
        &state,
        &storage,
        &storage_context_operation,
        &details,
        &active_run,
        shutdown,
        InstanceShutdownSource::Manual,
    )
    .await;
    let stopped_processes = match stop_result {
        Ok(stopped) => stopped,
        Err(error) => {
            append_desktop_app_log(
                &storage,
                "error",
                "instance.stop.failed",
                &error,
                json!({
                    "instance_id": instance_id.as_str(),
                    "instance_name": details.summary.name.as_str(),
                    "module_id": details.summary.module_id.as_str(),
                }),
            );
            return Err(error);
        }
    };

    let (summary, auto_backup_error) = finish_manual_stop(&storage, &details, async {
        let instances = list_instances(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        let summary =
            find_instance_summary(&instances, &instance_id).unwrap_or_else(|| InstanceSummary {
                id: instance_id.clone(),
                name: instance_id.clone(),
                module_id: String::new(),
                status: InstanceStatus::Stopped,
                active_process_count: 0,
                bind_ip: String::from("0.0.0.0"),
                port_count: 0,
                autostart: false,
            });
        update_state_instances(&state, instances)?;
        Ok(summary)
    })
    .await?;

    let primary_process = stopped_processes
        .iter()
        .find(|process| process.run_id == active_run.run_id)
        .or_else(|| stopped_processes.first())
        .cloned();

    let result = StopInstanceResult {
        summary,
        outcome: app_core::StopInstanceOutcome::Stopped,
        auto_backup_error,
        run_id: Some(active_run.run_id),
        session_id: active_run.session_id.clone(),
        pid: primary_process.as_ref().and_then(|process| process.pid),
        log_path: primary_process
            .as_ref()
            .and_then(|process| process.log_path.clone()),
        exit_code: primary_process
            .as_ref()
            .and_then(|process| process.exit_code),
        process_count: stopped_processes.len(),
        processes: stopped_processes,
    };

    append_desktop_app_log(
        &storage,
        "info",
        "instance.stop.completed",
        "Managed instance stopped successfully.",
        json!({
            "instance_id": instance_id.as_str(),
            "instance_name": details.summary.name.as_str(),
            "module_id": details.summary.module_id.as_str(),
            "process_count": result.process_count,
        }),
    );

    Ok(result)
}
