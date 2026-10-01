use super::*;
use crate::state::AutostartBatch;

pub(super) const AUTOSTART_SOURCE: &str = "autostart";
pub(super) const AUTOSTART_CANCELLED: &str = "Autostart cancelled.";
pub(super) const AUTOSTART_ALREADY_ACTIVE: &str =
    "Autostart skipped because the instance is already running or starting.";

/// The heartbeat only dispatches the batch. Launch/update waits run in one
/// storage-owned worker, so runtime reconciliation stays responsive.
pub(super) fn spawn_pending_autostart(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let state = app_handle.state::<DesktopState>();
    let Some(batch) = state.autostart.take()? else {
        return Ok(());
    };
    let operation = state.begin_storage_context_operation("instance autostart")?;
    let app_handle = app_handle.clone();
    // The storage lease outlives the detached task and blocks path replacement
    // and final shutdown until in-flight launch cleanup completes.
    spawn_storage_context_task(&operation, async move {
        let state = app_handle.state::<DesktopState>();
        if let Err(error) = run_autostart_batch(Some(&app_handle), &state, &batch).await {
            append_desktop_app_log(
                &batch.storage,
                "error",
                "instance.autostart.failed",
                &error,
                json!({ "source": AUTOSTART_SOURCE }),
            );
        }
    });
    Ok(())
}

pub(super) async fn run_autostart_batch(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    batch: &AutostartBatch,
) -> Result<(), String> {
    run_autostart_batch_with(state, batch, |instance| async move {
        start_autostart_instance(app_handle, state, &batch.storage, &instance).await
    })
    .await
}

async fn run_autostart_batch_with<F, Fut>(
    state: &tauri::State<'_, DesktopState>,
    batch: &AutostartBatch,
    mut start: F,
) -> Result<(), String>
where
    F: FnMut(InstanceSummary) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    ensure_storage_context_snapshot_current(state, &batch.storage, "instance autostart")?;
    for instance in &batch.instances {
        if state.shutdown_in_progress.load(Ordering::SeqCst) {
            for pending in &batch.instances {
                state.autostart.cancel(&pending.id)?;
            }
            break;
        }
        let job_id = new_background_job_id("instance-autostart", &instance.id);
        insert_background_job(
            state,
            &batch.storage,
            BackgroundJob {
                id: job_id.clone(),
                kind: JobKind::StartInstance,
                label: format!("Autostart {}", instance.name),
                status: JobStatus::Running,
                progress_percent: 0.0,
                install_progress: None,
                cancellable: false,
                cancel_requested: false,
                target_id: Some(instance.id.clone()),
                detail: Some(String::from("Starting automatically with LGSM.")),
                output_excerpt: None,
            },
        )?;
        let result = if state.autostart.is_eligible(&instance.id)? {
            start(instance.clone()).await
        } else {
            Err(String::from(AUTOSTART_CANCELLED))
        };
        state.autostart.cancel(&instance.id)?;
        let (status, detail) = match &result {
            Ok(()) => (
                JobStatus::Completed,
                String::from("Started automatically with LGSM."),
            ),
            Err(error) if is_autostart_skip(error) => (
                JobStatus::Cancelled,
                if error == AUTOSTART_CANCELLED {
                    error.clone()
                } else {
                    String::from(AUTOSTART_ALREADY_ACTIVE)
                },
            ),
            Err(_) if state.shutdown_in_progress.load(Ordering::SeqCst) => {
                (JobStatus::Cancelled, String::from(AUTOSTART_CANCELLED))
            }
            Err(error) => (JobStatus::Failed, error.clone()),
        };
        let failed = matches!(status, JobStatus::Failed);
        append_desktop_app_log(
            &batch.storage,
            if failed { "error" } else { "info" },
            if failed {
                "instance.autostart.failed"
            } else {
                "instance.autostart.finished"
            },
            &detail,
            json!({ "source": AUTOSTART_SOURCE, "instance_id": instance.id,
                "status": status }),
        );
        update_background_job(state, &job_id, |job| {
            job.status = status;
            job.progress_percent = 100.0;
            job.detail = Some(detail.clone());
            job.output_excerpt = failed.then_some(detail);
        })?;
    }
    Ok(())
}

async fn start_autostart_instance(
    app_handle: Option<&tauri::AppHandle>,
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance: &InstanceSummary,
) -> Result<(), String> {
    if !state.autostart.is_eligible(&instance.id)? {
        return Err(String::from(AUTOSTART_CANCELLED));
    }
    // Fresh reconciliation recognizes retained processes from the previous
    // desktop session; the normal reservation and mutation lock close races
    // against manual starts and edits after this read.
    reconcile_runtime_state(state).await?;
    start_instance_process_after_reconcile(
        app_handle,
        state,
        storage,
        instance.id.clone(),
        AUTOSTART_SOURCE,
        None,
    )
    .await
    .map(|_| ())
}

fn is_autostart_skip(error: &str) -> bool {
    if matches!(error, AUTOSTART_CANCELLED | AUTOSTART_ALREADY_ACTIVE) {
        return true;
    }
    serde_json::from_str::<Value>(error)
        .ok()
        .and_then(|value| value.get("code").and_then(Value::as_str).map(str::to_owned))
        .is_some_and(|code| code == "instance_already_running")
}

#[tauri::command]
pub async fn update_instance_autostart(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    autostart: bool,
) -> Result<InstanceDetails, String> {
    let operation = state.begin_storage_context_operation("instance autostart update")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    update_instance_autostart_with(&state, &storage, &operation, instance_id, autostart).await
}

async fn update_instance_autostart_with(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    operation: &StorageContextOperationGuard,
    instance_id: String,
    autostart: bool,
) -> Result<InstanceDetails, String> {
    // A start owns the mutation lock throughout prestart updates and launch.
    // Cancel first so disabling can stop that launch before waiting to persist.
    // A failed save remains an error; it does not rearm this session's start.
    if !autostart {
        state.autostart.cancel(&instance_id)?;
    }
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance_lock = state.acquire_instance_mutation(&instance_id).await;
    let paths = storage.paths.clone();
    let update_id = instance_id.clone();
    let details = spawn_storage_context_task(operation, async move {
        let _instance_lock = instance_lock;
        app_storage::update_instance_autostart(&paths, &update_id, autostart).await
    })
    .await
    .map_err(|error| format!("autostart update task failed: {error}"))?
    .map_err(|error| logged_error_message(storage, error.to_string()))?;
    let instances = list_instances(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    update_state_instances(state, instances)?;
    append_desktop_app_log(
        storage,
        "info",
        "instance.autostart.updated",
        "Instance autostart preference saved",
        json!({ "instance_id": instance_id, "autostart": autostart }),
    );
    Ok(details)
}

#[cfg(test)]
#[path = "commands_autostart_tests.rs"]
mod tests;
