use super::commands_runtime_lifecycle::{
    InstanceShutdownSource, dst_lifecycle, request_instance_graceful_shutdown,
    stop_active_instance_processes,
};
use super::*;

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
    wait_for_pending_stdin(state, &details.summary.id, active_run.run_id).await?;
    request_instance_graceful_shutdown(state, storage, details, shutdown, source).await?;
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
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + PENDING_STDIN_DRAIN_TIMEOUT;
    loop {
        let pending = state
            .runtime_supervisor
            .lock()
            .map_err(|_| String::from("runtime supervisor lock poisoned"))?
            .has_pending_command_dispatches(instance_id, run_id)
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
) {
    if !details.auto_backup_on_stop {
        return;
    }
    match create_instance_auto_stop_backup_snapshot(&storage.paths, &details.summary.id).await {
        Ok(backup) => append_desktop_app_log(
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
        ),
        Err(error) => append_desktop_app_log(
            storage,
            "error",
            "instance.stop.auto_backup_failed",
            &error.to_string(),
            json!({"instance_id": details.summary.id, "source": source.as_str()}),
        ),
    }
}
