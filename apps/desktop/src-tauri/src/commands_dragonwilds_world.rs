use super::*;
use app_core::dragonwilds_world::{
    DragonwildsWorldSettingsSnapshot, WriteDragonwildsWorldSettingsInput,
};

#[tauri::command]
pub async fn read_dragonwilds_world_settings(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<DragonwildsWorldSettingsSnapshot, String> {
    let _operation = state.begin_storage_context_operation("Dragonwilds world settings read")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    app_storage::read_dragonwilds_world_settings(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn write_dragonwilds_world_settings(
    state: tauri::State<'_, DesktopState>,
    input: WriteDragonwildsWorldSettingsInput,
) -> Result<DragonwildsWorldSettingsSnapshot, String> {
    let operation = state.begin_storage_context_operation("Dragonwilds world settings write")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance_lock = state.acquire_instance_mutation(&input.instance_id).await;
    {
        let supervisor = state
            .runtime_supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned.".to_owned())?;
        if supervisor.is_tracked(&input.instance_id) {
            return Err("Stop the server before changing world settings.".into());
        }
    }
    // Cancellation cannot release the lifecycle permit while backup/publication
    // is still running in the storage worker.
    spawn_storage_context_task(&operation, async move {
        let _instance_lock = instance_lock;
        app_storage::write_dragonwilds_world_settings(&storage.paths, input)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("World settings task failed: {error}"))?
}
