use super::*;
use app_core::valheim_world::ValheimWorldRules;

#[tauri::command]
pub async fn read_valheim_world_rules(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    world_name: String,
) -> Result<ValheimWorldRules, String> {
    let operation = state.begin_storage_context_operation("Valheim world rules read")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    spawn_storage_context_task(&operation, async move {
        app_storage::read_valheim_world_rules(&storage.paths, &instance_id, &world_name)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Valheim world rules task failed: {error}"))?
}
