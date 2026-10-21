use super::*;
use app_storage::SaveFileListing;

/// Read-only listing of the instance saves directory, or of one of its backups.
/// Both roots are resolved from stored instance state; callers never pass paths.
#[tauri::command]
pub async fn list_instance_save_files(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    backup_id: Option<String>,
) -> Result<SaveFileListing, String> {
    let _operation = state.begin_storage_context_operation("save file listing")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    app_storage::list_instance_save_files(&storage.paths, &instance_id, backup_id.as_deref())
        .await
        .map_err(|error| error.to_string())
}
