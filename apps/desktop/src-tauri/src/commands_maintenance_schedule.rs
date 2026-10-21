use super::commands_runtime_lifecycle::{InstanceShutdownSource, load_module_shutdown_specs};
use super::*;
use app_core::maintenance_schedule::{
    DailyMaintenanceAction, DailyMaintenanceSchedule, UpdateDailyMaintenanceScheduleInput,
};
use app_storage::maintenance_schedule::{self as schedules, ClaimedDailyMaintenance};

#[tauri::command]
pub async fn read_instance_maintenance_schedule(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    action: DailyMaintenanceAction,
) -> Result<DailyMaintenanceSchedule, String> {
    let _operation = state.begin_storage_context_operation("daily maintenance policy read")?;
    let storage = state.bootstrap_storage()?;
    schedules::read_daily_maintenance_schedule(&storage.paths, &instance_id, action)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_instance_maintenance_schedule(
    state: tauri::State<'_, DesktopState>,
    input: UpdateDailyMaintenanceScheduleInput,
) -> Result<DailyMaintenanceSchedule, String> {
    let _operation = state.begin_storage_context_operation("daily maintenance policy update")?;
    let storage = state.bootstrap_storage()?;
    schedules::update_daily_maintenance_schedule(&storage.paths, &input)
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn spawn_due_maintenance(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<DesktopState>();
    if state.shutdown_in_progress.load(Ordering::SeqCst) {
        return Ok(());
    }
    let (date, time) = local_clock()?;
    let Some(worker) = state.daily_maintenance.try_start(&date, &time)? else {
        return Ok(());
    };
    let operation = state.begin_storage_context_operation("daily maintenance")?;
    let storage = state.bootstrap_storage()?;
    let handle = app_handle.clone();
    spawn_storage_context_task(&operation.clone(), async move {
        let _worker = worker;
        let state = handle.state::<DesktopState>();
        let result = async {
            schedules::interrupt_abandoned_daily_maintenance(&storage.paths, &_worker.owner_session)
                .await.map_err(|error| error.to_string())?;
            let claims = schedules::claim_due_daily_maintenance(&storage.paths, &_worker.local_date, &_worker.due_minutes,
                &_worker.owner_session).await.map_err(|error| error.to_string())?;
            for claim in claims {
                // The storage claim orders backup before restart for the same
                // instance. Each operation finishes before the next one begins.
                let outcome = execute_claim(&handle, &state, &storage, &operation, &claim, &_worker).await;
                let (status, detail) = match outcome {
                    Ok(detail) => ("succeeded", detail),
                    Err(MaintenanceOutcome::Skipped(detail)) => ("skipped", detail),
                    Err(MaintenanceOutcome::Failed(detail)) => ("failed", detail),
                };
                if let Err(error) = schedules::complete_daily_maintenance(&storage.paths, &claim, status, &detail).await {
                    append_desktop_app_log(&storage, "error", "instance.maintenance.receipt_failed", &error.to_string(),
                        json!({"instance_id":claim.schedule.instance_id,"action":claim.schedule.action.as_str(),"result":status,"detail":detail}));
                }
                append_desktop_app_log(&storage, if status == "failed" { "error" } else { "info" },
                    "instance.maintenance.completed", &detail,
                    json!({"instance_id":claim.schedule.instance_id,"action":claim.schedule.action.as_str(),"status":status}));
            }
            Ok::<_, String>(())
        }.await;
        if let Err(error) = result {
            append_desktop_app_log(
                &storage,
                "error",
                "instance.maintenance.scan_failed",
                &error,
                json!({}),
            );
        }
    });
    Ok(())
}

#[derive(Debug)]
enum MaintenanceOutcome {
    Skipped(String),
    Failed(String),
}
impl From<String> for MaintenanceOutcome {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}

async fn execute_claim(
    handle: &tauri::AppHandle,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    claim: &ClaimedDailyMaintenance,
    worker: &crate::state::DailyMaintenanceWorker,
) -> Result<String, MaintenanceOutcome> {
    reconcile_runtime_state(state).await?;
    let instance_lock = acquire_claim_execution(state, storage, claim, worker).await?;
    match claim.schedule.action {
        DailyMaintenanceAction::Backup => {
            let backup = super::commands_storage::create_instance_backup_locked(
                storage,
                &claim.schedule.instance_id,
            )
            .await?;
            Ok(format!("Scheduled backup created: {}", backup.backup_id))
        }
        DailyMaintenanceAction::Restart => {
            restart_scheduled_instance(
                handle,
                state,
                storage,
                operation,
                &claim.schedule.instance_id,
                instance_lock,
            )
            .await
        }
    }
}

async fn acquire_claim_execution(
    state: &DesktopState,
    storage: &StorageBootstrap,
    claim: &ClaimedDailyMaintenance,
    worker: &crate::state::DailyMaintenanceWorker,
) -> Result<tokio::sync::OwnedMutexGuard<()>, MaintenanceOutcome> {
    let instance_lock = state
        .acquire_instance_mutation(&claim.schedule.instance_id)
        .await;
    // A save or another mutation may hold this lock for a long time. Recheck
    // cancellation only after that wait, immediately before accepting execution.
    if state.shutdown_in_progress.load(Ordering::SeqCst) {
        return Err(MaintenanceOutcome::Skipped(
            "Skipped because LGSM is shutting down.".into(),
        ));
    }
    let current = schedules::read_daily_maintenance_schedule(
        &storage.paths,
        &claim.schedule.instance_id,
        claim.schedule.action,
    )
    .await
    .map_err(|error| error.to_string())?;
    if !current.enabled || current.revision != claim.schedule.revision {
        return Err(MaintenanceOutcome::Skipped(
            "Skipped because the daily maintenance policy changed before execution.".into(),
        ));
    }
    if local_clock()?.0 != worker.local_date || !worker.window_is_current()? {
        return Err(MaintenanceOutcome::Skipped(
            "Skipped because the host was suspended or its local date changed before execution."
                .into(),
        ));
    }
    Ok(instance_lock)
}

struct PlannedRestartFlight<'a> {
    state: &'a DesktopState,
    entry: RuntimeRestartScheduleEntry,
}
impl Drop for PlannedRestartFlight<'_> {
    fn drop(&mut self) {
        if let Ok(mut scheduler) = self.state.runtime_restart_scheduler.lock() {
            scheduler.finish_restart(&self.entry);
        }
    }
}

async fn restart_scheduled_instance(
    handle: &tauri::AppHandle,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: &str,
    instance_lock: tokio::sync::OwnedMutexGuard<()>,
) -> Result<String, MaintenanceOutcome> {
    let details = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if !matches!(details.summary.status, InstanceStatus::Running) {
        return Err(MaintenanceOutcome::Skipped(
            "Skipped because the instance is not running.".into(),
        ));
    }
    let active = read_active_instance_run(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            MaintenanceOutcome::Skipped("Skipped because the instance is not running.".into())
        })?;
    let entry = state
        .runtime_restart_scheduler
        .lock()
        .map_err(|_| "runtime restart scheduler lock poisoned".to_owned())?
        .begin_planned_restart(
            instance_id,
            &details.summary.name,
            std::iter::once(active.run_id)
                .chain(active.processes.iter().map(|process| process.run_id)),
        )
        .ok_or_else(|| {
            MaintenanceOutcome::Skipped(
                "Skipped because a stop or another restart is already pending.".into(),
            )
        })?;
    let flight = PlannedRestartFlight { state, entry };
    if state.shutdown_in_progress.load(Ordering::SeqCst) {
        return Err(MaintenanceOutcome::Skipped(
            "Skipped because LGSM is shutting down.".into(),
        ));
    }
    let shutdowns = load_module_shutdown_specs(storage, InstanceShutdownSource::Manual);
    super::commands_instance_stop::stop_run(
        state,
        storage,
        operation,
        &details,
        &active,
        shutdowns.get(&details.summary.module_id),
        InstanceShutdownSource::Manual,
    )
    .await?;
    super::commands_instance_stop::backup_after_stop(
        storage,
        &details,
        InstanceShutdownSource::Manual,
    )
    .await;
    update_state_instances(
        state,
        list_instances(&storage.paths)
            .await
            .map_err(|error| error.to_string())?,
    )?;
    drop(instance_lock);
    if !state
        .runtime_restart_scheduler
        .lock()
        .map_err(|_| "runtime restart scheduler lock poisoned".to_owned())?
        .entry_is_current(&flight.entry)
    {
        return Err(MaintenanceOutcome::Skipped(
            "Restart cancelled by a newer runtime action; the instance remains stopped.".into(),
        ));
    }
    // The normal launch checks the same flight again under the instance lock
    // and before every process spawn. A user's Stop always wins the handoff.
    start_instance_process_after_reconcile(
        Some(handle),
        state,
        storage,
        instance_id.to_owned(),
        "auto_restart",
        None,
    )
    .await?;
    Ok("Scheduled restart completed.".into())
}

#[cfg(windows)]
fn local_clock() -> Result<(String, String), String> {
    let mut time = windows_sys::Win32::Foundation::SYSTEMTIME::default();
    // SAFETY: GetLocalTime fills the valid SYSTEMTIME output without retaining it.
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut time);
    }
    Ok((
        format!("{:04}-{:02}-{:02}", time.wYear, time.wMonth, time.wDay),
        format!("{:02}:{:02}", time.wHour, time.wMinute),
    ))
}

#[cfg(not(windows))]
fn local_clock() -> Result<(String, String), String> {
    Err("Daily maintenance requires the Windows host local clock.".into())
}

#[cfg(all(test, windows))]
#[path = "commands_maintenance_schedule_tests.rs"]
mod tests;
