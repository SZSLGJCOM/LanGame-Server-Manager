use super::commands_storage::run_instance_mutation_to_completion;
use super::*;
use app_storage::{InstanceSaveImportPreview, InstanceSaveImportResult, InstanceSaveImportSupport};

#[tauri::command]
pub async fn read_instance_save_import_support(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceSaveImportSupport, String> {
    let _operation = state.begin_storage_context_operation("save import support")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    app_storage::read_instance_save_import_support(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn preview_instance_save_import(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    source_path: String,
) -> Result<InstanceSaveImportPreview, String> {
    let operation = state.begin_storage_context_operation("save import preview")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let id = instance_id.clone();
    run_instance_mutation_to_completion(
        state.inner(),
        &operation,
        &instance_id,
        move || async move {
            app_storage::preview_instance_save_import(
                &storage.paths,
                &id,
                Path::new(source_path.trim()),
            )
            .await
            .map_err(|error| error.to_string())
        },
    )
    .await
}

#[tauri::command]
pub async fn import_instance_save(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
    source_path: String,
    expected_fingerprint: String,
) -> Result<InstanceSaveImportResult, String> {
    let operation = state.begin_storage_context_operation("native save import")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let id = instance_id.clone();
    let install_locks = state.install_locks.clone();
    run_instance_mutation_to_completion(state.inner(), &operation, &instance_id, move || async move {
        // Acquire the same complete program-root lease used by archive/removal,
        // then revalidate it. The task owns both permits through compensation.
        let _install = super::commands_storage_lifecycle::acquire_retirement(
            &install_locks, &storage.paths, &id,
        ).await?;
        let result = app_storage::import_instance_save(&storage.paths, &id,
            Path::new(source_path.trim()), &expected_fingerprint)
            .await.map_err(|error| error.to_string())?;
        for warning in &result.warnings {
            append_desktop_app_log(&storage, "warning", "instance.save_import.cleanup_warning", warning,
                json!({ "instance_id": result.instance_id, "safeguard_backup_path": result.safeguard_backup_path }));
        }
        append_desktop_app_log(&storage, "info", "instance.save_import.completed", "Native saves imported",
            json!({ "instance_id": result.instance_id, "copied_file_count": result.copied_file_count,
                "copied_total_bytes": result.copied_total_bytes,
                "safeguard_backup_path": result.safeguard_backup_path,
                "safeguard_backup_paths": result.safeguard_backup_paths }));
        Ok(result)
    }).await
}
