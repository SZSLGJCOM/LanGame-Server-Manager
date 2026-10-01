use super::*;

#[derive(Clone, Copy)]
pub(super) enum Retirement {
    Archive,
    Delete,
}

impl Retirement {
    fn name(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Delete => "delete",
        }
    }

    fn running_error(self) -> &'static str {
        match self {
            Self::Archive => "Stop the server before archiving the instance.",
            Self::Delete => "Stop the server before deleting the instance.",
        }
    }
}

#[tauri::command]
pub async fn archive_instance_record<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    instance_id: String,
) -> Result<app_core::InstanceArchiveResult, String> {
    retire_instance(
        app,
        instance_id,
        Retirement::Archive,
        |paths, id, _install| async move { app_storage::archive_instance(&paths, &id).await },
    )
    .await
}

#[tauri::command]
pub async fn delete_instance_record<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    instance_id: String,
) -> Result<app_core::InstanceDeletionResult, String> {
    retire_instance(
        app,
        instance_id,
        Retirement::Delete,
        |paths, id, install| async move {
            let mut result = app_storage::delete_instance(&paths, &id).await?;
            result.program_cleanup =
                super::commands_mods::after_instance_deletion(&paths, &result.module_id, &install)
                    .await;
            Ok(result)
        },
    )
    .await
}

pub(super) async fn retire_instance<R, T, F, Fut>(
    app: tauri::AppHandle<R>,
    instance_id: String,
    action: Retirement,
    retire: F,
) -> Result<T, String>
where
    R: tauri::Runtime,
    T: Serialize + Send + 'static,
    F: FnOnce(
            app_storage::StoragePaths,
            String,
            std::sync::Arc<app_steamcmd::GameInstallLifecycleGuard>,
        ) -> Fut
        + Send
        + 'static,
    Fut: std::future::Future<Output = Result<T, app_storage::StorageError>> + Send + 'static,
{
    let state = app.state::<DesktopState>();
    let operation = state.begin_storage_context_operation("instance retirement")?;
    let lease = state.storage_management.acquire()?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    let event = format!("instance.{}", action.name());
    append_desktop_app_log(
        &storage,
        "info",
        &format!("{event}.request"),
        "Instance retirement requested",
        json!({ "instance_id": instance_id, "operation": action.name() }),
    );
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let instance = state.acquire_instance_mutation(&instance_id).await;
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
        append_desktop_app_log(
            &storage,
            "error",
            &format!("{event}.blocked_running"),
            action.running_error(),
            json!({ "instance_id": instance_id }),
        );
        return Err(action.running_error().into());
    }
    // A retried deletion may already have removed its active database row. The
    // storage transaction validates the durable intent as well as process state.
    let install = std::sync::Arc::new(
        super::commands_storage_lifecycle::acquire_retirement(&storage.paths, &instance_id).await?,
    );
    let paths = storage.paths.clone();
    let worker_id = instance_id.clone();
    let worker_app = app.clone();
    spawn_storage_context_task(&operation, async move {
        let _guards = (lease, instance, install.clone());
        let result = retire(paths, worker_id, install).await;
        let state = worker_app.state::<DesktopState>();

        // Filesystem cleanup can fail after the active record is removed. Reflect
        // that committed state, so the UI can show its pending deletion for retry.
        state.live_player_registry.invalidate_instance(&instance_id);
        let refreshed = match list_instances(&storage.paths).await {
            Ok(instances) => update_state_instances(&state, instances),
            Err(error) => Err(error.to_string()),
        };
        let result = result.map_err(|error| {
            let message = error.to_string();
            append_desktop_app_log(
                &storage,
                "error",
                &format!("{event}.failed"),
                &message,
                json!({ "instance_id": instance_id, "refresh_error": refreshed.as_ref().err() }),
            );
            logged_error_message(&storage, message)
        })?;
        append_desktop_app_log(
            &storage,
            "info",
            &format!("{event}.success"),
            "Instance retirement completed",
            json!({ "instance_id": instance_id, "result": result }),
        );
        refreshed.map_err(|error| {
            format!(
                "Instance {} completed, but refreshing the server list failed: {error}",
                action.name()
            )
        })?;
        Ok(result)
    })
    .await
    .map_err(|error| format!("Instance {} task failed: {error}", action.name()))?
}
