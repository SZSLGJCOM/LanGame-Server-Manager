use super::*;
use app_storage::{
    InstanceArchiveDetails, InstanceArchiveList, InstanceArchivePurgeResult,
    InstanceArchiveRestoreResult, StorageUsageReport,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageScanInput {
    pub scan_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstanceArchiveInput {
    pub archive_id: String,
}

fn validate_id(value: &str) -> Result<(), String> {
    uuid::Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| "Invalid storage operation identifier".into())
}

#[tauri::command]
pub async fn scan_storage_usage(
    state: tauri::State<'_, DesktopState>,
    input: StorageScanInput,
) -> Result<StorageUsageReport, String> {
    validate_id(&input.scan_id)?;
    let operation = state.begin_storage_context_operation("storage usage scan")?;
    let lease = state.storage_management.acquire()?;
    let cancellation = operation.cancellation_token();
    lease.register_scan(input.scan_id.clone(), cancellation.clone())?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    spawn_storage_context_task(&operation, async move {
        let _lease = lease;
        initialize_database(&storage.paths)
            .await
            .map_err(|error| error.to_string())?;
        app_storage::scan_storage_usage(&storage.paths, input.scan_id, cancellation)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Storage scan task failed: {error}"))?
}

#[tauri::command]
pub fn cancel_storage_usage_scan(
    state: tauri::State<'_, DesktopState>,
    input: StorageScanInput,
) -> Result<bool, String> {
    validate_id(&input.scan_id)?;
    state.storage_management.cancel(&input.scan_id)
}

// Recovery observes the same instance-before-install lock order as startup and
// updates. The worker retains these locks even if the requesting UI disconnects.
pub(super) async fn recover_archives_with(
    state: &DesktopState,
    storage: &StorageBootstrap,
    operation: &crate::state::StorageContextOperationGuard,
) -> Result<(), String> {
    recover_archives_with_pending(state, storage, operation, || async {
        app_storage::pending_instance_archive_ids(&storage.paths)
            .await
            .map_err(|error| error.to_string())
    })
    .await
}

async fn recover_archives_with_pending<P, F>(
    state: &DesktopState,
    storage: &StorageBootstrap,
    operation: &crate::state::StorageContextOperationGuard,
    mut pending: P,
) -> Result<(), String>
where
    P: FnMut() -> F,
    F: std::future::Future<Output = Result<Vec<String>, String>>,
{
    if pending().await?.is_empty() {
        return Ok(());
    }
    let lease = state.storage_management.acquire()?;
    // Archiving can leave a pending operation between the initial read and the
    // lease. Lock the refreshed set before recovery reads its own pending list.
    let ids = pending().await?;
    if ids.is_empty() {
        return Ok(());
    }
    let mut locks = Vec::with_capacity(ids.len());
    for id in &ids {
        locks.push(
            state
                .try_acquire_instance_mutation(id)
                .await
                .ok_or_else(|| format!("Instance {id} is busy; archive recovery must wait."))?,
        );
        ensure_not_running(state, id)?;
    }
    let install = super::commands_storage_lifecycle::acquire_recovery(&storage.paths, &ids).await?;
    let paths = storage.paths.clone();
    spawn_storage_context_task(operation, async move {
        let _guards = (lease, locks, install);
        app_storage::recover_instance_archives(&paths)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Archive recovery task failed: {error}"))?
}

fn ensure_not_running(state: &DesktopState, id: &str) -> Result<(), String> {
    let tracked = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "Runtime supervisor lock poisoned")?
        .is_tracked(id);
    if tracked
        || state
            .pending_runtime_start_instance_ids()?
            .iter()
            .any(|pending| pending == id)
    {
        return Err(format!(
            "Stop or cancel the start of instance {id} before changing its archive."
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn list_instance_archives(
    state: tauri::State<'_, DesktopState>,
) -> Result<InstanceArchiveList, String> {
    let operation = state.begin_storage_context_operation("instance archive listing")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    recover_archives_with(&state, &storage, &operation).await?;
    let lease = state.storage_management.acquire()?;
    spawn_storage_context_task(&operation, async move {
        let _lease = lease;
        app_storage::list_instance_archives(&storage.paths)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Archive listing task failed: {error}"))?
}

#[tauri::command]
pub async fn read_instance_archive_details(
    state: tauri::State<'_, DesktopState>,
    input: InstanceArchiveInput,
) -> Result<InstanceArchiveDetails, String> {
    validate_id(&input.archive_id)?;
    let operation = state.begin_storage_context_operation("instance archive details preview")?;
    let lease = state.storage_management.acquire()?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    spawn_storage_context_task(&operation, async move {
        let _lease = lease;
        app_storage::read_instance_archive_details(&storage.paths, &input.archive_id)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Archive details preview task failed: {error}"))?
}

async fn archive_instance_lock(
    state: &DesktopState,
    resources: &app_storage::InstanceStorageResources,
) -> Result<Option<tokio::sync::OwnedMutexGuard<()>>, String> {
    let Some(instance_id) = &resources.instance_id else {
        return Ok(None);
    };
    let lock = state
        .try_acquire_instance_mutation(instance_id)
        .await
        .ok_or_else(|| String::from("This instance has another operation in progress."))?;
    ensure_not_running(state, instance_id)?;
    Ok(Some(lock))
}

#[tauri::command]
pub async fn restore_instance_archive<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    input: InstanceArchiveInput,
) -> Result<InstanceArchiveRestoreResult, String> {
    restore_instance_archive_with(app, input, |paths, id| async move {
        app_storage::restore_instance_archive(&paths, &id).await
    })
    .await
}

async fn restore_instance_archive_with<R, F, Fut>(
    app: tauri::AppHandle<R>,
    input: InstanceArchiveInput,
    restore: F,
) -> Result<InstanceArchiveRestoreResult, String>
where
    R: tauri::Runtime,
    F: FnOnce(app_storage::StoragePaths, String) -> Fut + Send + 'static,
    Fut: std::future::Future<
            Output = Result<InstanceArchiveRestoreResult, app_storage::StorageError>,
        > + Send
        + 'static,
{
    validate_id(&input.archive_id)?;
    let state = app.state::<DesktopState>();
    let operation = state.begin_storage_context_operation("instance archive restoration")?;
    let lease = state.storage_management.acquire()?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let resources = app_storage::read_instance_archive_resources(&storage.paths, &input.archive_id)
        .await
        .map_err(|error| error.to_string())?;
    let instance = archive_instance_lock(&state, &resources).await?;
    let install = super::commands_storage_lifecycle::acquire_archive(
        &storage.paths,
        &input.archive_id,
        &resources,
    )
    .await?;
    let paths = storage.paths.clone();
    let worker_app = app.clone();
    spawn_storage_context_task(&operation, async move {
        let _guards = (lease, instance, install);
        let result = restore(paths, input.archive_id)
            .await
            .map_err(|error| error.to_string())?;
        let state = worker_app.state::<DesktopState>();
        state
            .live_player_registry
            .invalidate_instance(&result.instance_id);
        let refreshed = match list_instances(&storage.paths).await {
            Ok(instances) => update_state_instances(&state, instances),
            Err(error) => Err(error.to_string()),
        };
        append_desktop_app_log(
            &storage,
            "info",
            "instance.archive.restored",
            "Instance archive restored",
            json!({ "archive_id": result.archive_id, "instance_id": result.instance_id,
                "refresh_error": refreshed.as_ref().err() }),
        );
        refreshed.map_err(|error| {
            format!("Instance archive restored, but refreshing the server list failed: {error}")
        })?;
        Ok(result)
    })
    .await
    .map_err(|error| format!("Archive restoration task failed: {error}"))?
}

#[tauri::command]
pub async fn purge_instance_archive(
    state: tauri::State<'_, DesktopState>,
    input: InstanceArchiveInput,
) -> Result<InstanceArchivePurgeResult, String> {
    validate_id(&input.archive_id)?;
    let operation = state.begin_storage_context_operation("instance archive permanent cleanup")?;
    let lease = state.storage_management.acquire()?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let resources =
        app_storage::read_instance_archive_cleanup_resources(&storage.paths, &input.archive_id)
            .await
            .map_err(|error| error.to_string())?;
    let instance = archive_instance_lock(&state, &resources).await?;
    let install = super::commands_storage_lifecycle::acquire_archive_cleanup(
        &storage.paths,
        &input.archive_id,
        &resources,
    )
    .await?;
    spawn_storage_context_task(&operation, async move {
        let _guards = (lease, instance, install);
        let result = app_storage::purge_instance_archive(&storage.paths, &input.archive_id)
            .await
            .map_err(|error| error.to_string())?;
        append_desktop_app_log(
            &storage,
            "info",
            "instance.archive.purged",
            "Instance archive permanently cleared",
            json!({ "archive_id": result.archive_id }),
        );
        Ok(result)
    })
    .await
    .map_err(|error| format!("Archive cleanup task failed: {error}"))?
}

#[cfg(test)]
#[path = "commands_storage_management_tests.rs"]
mod tests;
