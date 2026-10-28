use super::*;
use app_core::scum_custom_zones::{ScumCustomZonesSnapshot, WriteScumCustomZoneDamageInput};

#[tauri::command]
pub async fn read_scum_custom_zones(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<ScumCustomZonesSnapshot, String> {
    let operation = state.begin_storage_context_operation("SCUM custom zones read")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance_lock = state.acquire_instance_mutation(&instance_id).await;
    spawn_storage_context_task(&operation, async move {
        let _instance_lock = instance_lock;
        app_storage::read_scum_custom_zones(&storage.paths, &instance_id)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Custom-zone read task failed: {error}"))?
}

#[tauri::command]
pub async fn write_scum_custom_zone_damage(
    state: tauri::State<'_, DesktopState>,
    input: WriteScumCustomZoneDamageInput,
) -> Result<ScumCustomZonesSnapshot, String> {
    let operation = state.begin_storage_context_operation("SCUM custom-zone damage write")?;
    let storage = state.bootstrap_storage()?;
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
            return Err("Stop the server before changing custom zones.".into());
        }
    }
    // Cancellation cannot release either the instance mutation permit or storage
    // context while the database transaction and pre-change backup are in flight.
    spawn_storage_context_task(&operation, async move {
        let _instance_lock = instance_lock;
        app_storage::write_scum_custom_zone_damage(&storage.paths, input)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Custom-zone write task failed: {error}"))?
}
