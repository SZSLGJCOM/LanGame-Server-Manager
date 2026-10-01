use super::*;

// Keep disk/network shutdown work bounded while allowing another instance to
// receive its save request when one game is waiting for its grace period.
const MAX_CONCURRENT_SHUTDOWNS: usize = 4;
const MAX_CONCURRENT_BACKUPS: usize = 2;

pub(super) async fn run<L: StorageContextTaskLease>(
    app: &tauri::AppHandle,
    storage: &StorageBootstrap,
    lease: &L,
    instance_ids: Vec<String>,
    shutdown_by_module: HashMap<String, ModuleShutdownSpec>,
) -> Vec<(String, String)> {
    let strategies = std::sync::Arc::new(shutdown_by_module);
    let mut pending = instance_ids.into_iter();
    let mut workers = tokio::task::JoinSet::new();
    let mut failures = Vec::new();
    let mut stopped_instances = Vec::new();
    loop {
        while workers.len() < MAX_CONCURRENT_SHUTDOWNS {
            let Some(instance_id) = pending.next() else {
                break;
            };
            let app = app.clone();
            let storage = storage.clone();
            let lease = lease.clone();
            let strategies = std::sync::Arc::clone(&strategies);
            workers.spawn(async move {
                let state = app.state::<DesktopState>();
                let result = shutdown_one_running_instance_for_app_exit(
                    &state,
                    &storage,
                    &lease,
                    &instance_id,
                    &strategies,
                )
                .await;
                (instance_id, result)
            });
        }
        let Some(result) = workers.join_next().await else {
            break;
        };
        let failure = match result {
            Ok((_, Ok(stopped))) => {
                stopped_instances.extend(stopped);
                continue;
            }
            Ok((instance_id, Err(error))) => (instance_id, error),
            Err(error) => (String::from("shutdown worker"), error.to_string()),
        };
        append_desktop_app_log(
            storage,
            "error",
            "app.exit.instance_shutdown_failed",
            &failure.1,
            json!({ "instance_id": failure.0 }),
        );
        failures.push(failure);
    }

    // A stopped world's archive must never occupy a slot needed to deliver
    // another running world's save/stop commands. Start disk-heavy backups only
    // after every admitted instance has completed its stop attempt.
    let mut pending_backups = stopped_instances
        .into_iter()
        .filter(|details| details.auto_backup_on_stop);
    let mut backups = tokio::task::JoinSet::new();
    loop {
        while backups.len() < MAX_CONCURRENT_BACKUPS {
            let Some(details) = pending_backups.next() else {
                break;
            };
            let app = app.clone();
            let storage = storage.clone();
            let lease = lease.clone();
            backups.spawn(async move {
                let _storage_lease = lease;
                let state = app.state::<DesktopState>();
                let _instance_lock = state.acquire_instance_mutation(&details.summary.id).await;
                super::commands_instance_stop::backup_after_stop(
                    &storage,
                    &details,
                    InstanceShutdownSource::AppExit,
                )
                .await;
            });
        }
        let Some(result) = backups.join_next().await else {
            break;
        };
        if let Err(error) = result {
            let error = error.to_string();
            append_desktop_app_log(
                storage,
                "error",
                "instance.stop.auto_backup_failed",
                &error,
                json!({ "source": "app_exit" }),
            );
            failures.push((String::from("backup worker"), error));
        }
    }
    failures
}
