use super::*;

/// Reconcile external folder removals only at inventory refresh. Runtime reads
/// and explicit retirement keep their own lifecycle and do not remove records.
pub(super) async fn reconcile_missing_instances(
    state: &DesktopState,
    storage: &StorageBootstrap,
) -> Result<(), String> {
    let operation = state.begin_storage_context_operation("missing instance reconciliation")?;
    let candidates = app_storage::list_missing_instance_candidates(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    if candidates.is_empty() {
        return Ok(());
    }
    let Ok(lease) = state.storage_management.acquire() else {
        // Archive/restore and scans own the inventory until their worker ends.
        return Ok(());
    };
    let lease = Arc::new(lease);
    for instance_id in candidates {
        let Some(instance) = state.try_acquire_instance_mutation(&instance_id).await else {
            continue;
        };
        let tracked = state
            .runtime_supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned")?
            .is_tracked(&instance_id);
        if tracked
            || state
                .pending_runtime_start_instance_ids()?
                .contains(&instance_id)
        {
            continue;
        }
        let worker_storage = storage.clone();
        let worker_id = instance_id.clone();
        let worker_lease = Arc::clone(&lease);
        let players = state.live_player_registry.clone();
        // Keep the desktop leases with the database mutation if a renderer
        // disconnects. The storage layer rechecks absence and ownership itself.
        let result = spawn_storage_context_task(&operation, async move {
            let _guards = (worker_lease, instance);
            let removed = app_storage::reconcile_missing_instance(&worker_storage.paths, &worker_id).await?;
            if removed {
                players.invalidate_instance(&worker_id);
                append_desktop_app_log(
                    &worker_storage,
                    "info",
                    "instance.reconciled_missing_directory",
                    "Removed an inactive instance record after its managed directory was deleted externally",
                    json!({ "instance_id": worker_id }),
                );
            }
            Ok::<_, app_storage::StorageError>(removed)
        })
        .await
        .map_err(|error| format!("Missing instance reconciliation task failed: {error}"))?;
        match result {
            Ok(_) | Err(app_storage::StorageError::InstanceSettingsLocked { .. }) => {}
            Err(error) => {
                append_desktop_app_log(
                    storage,
                    "warn",
                    "instance.reconciliation.retained",
                    "Retained an instance record because safe removal could not be confirmed",
                    json!({ "instance_id": instance_id, "reason": error.to_string() }),
                );
            }
        }
    }
    Ok(())
}
