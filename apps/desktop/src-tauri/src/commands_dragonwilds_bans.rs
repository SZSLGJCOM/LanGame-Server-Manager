use super::*;
use app_core::DragonwildsBanRoster;

#[tauri::command]
pub async fn read_instance_dragonwilds_bans(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<DragonwildsBanRoster, String> {
    let operation = state.begin_storage_context_operation("Dragonwilds saved ban roster read")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let permit = state.acquire_instance_mutation(&instance_id).await;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if instance.summary.module_id != "runescapedragonwilds" {
        return Err("The saved ban roster requires a Dragonwilds instance.".into());
    }
    spawn_roster_read(&operation, permit, move || {
        let root = app_storage::resolve_instance_runtime_root(
            super::commands_program_storage::instance_root(&instance)?,
        )
        .map_err(|error| error.to_string())?;
        app_storage::read_dragonwilds_ban_roster(&root, &instance.summary.id)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|_| "The Dragonwilds saved ban roster worker failed.".to_owned())?
}

fn spawn_roster_read<T: Send + 'static>(
    operation: &StorageContextOperationGuard,
    permit: tokio::sync::OwnedMutexGuard<()>,
    read: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    spawn_blocking_storage_context_task(operation, move || {
        // A cancelled UI/LAN waiter cannot release either the instance permit
        // or storage context while runtime-path resolution or file I/O continues.
        let _permit = permit;
        read()
    })
}

#[cfg(test)]
#[path = "commands_dragonwilds_bans_tests.rs"]
mod tests;
