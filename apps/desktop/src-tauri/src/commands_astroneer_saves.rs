use super::*;
use app_core::astroneer_saves::AstroneerSaveCatalog;

#[tauri::command]
pub async fn read_astroneer_save_catalog(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<AstroneerSaveCatalog, String> {
    let operation = state.begin_storage_context_operation("ASTRONEER save selection read")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    spawn_storage_context_task(&operation, async move {
        app_storage::read_astroneer_save_catalog(&storage.paths, &instance_id)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("ASTRONEER save catalog task failed: {error}"))?
}
