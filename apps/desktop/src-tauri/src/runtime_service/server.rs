use super::security::{MAX_CONNECTIONS, MAX_CONTROL_CONNECTIONS, MAX_WORK_REQUESTS};
use super::{events, security, wire};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::net::windows::named_pipe::NamedPipeServer;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

fn acquire_request_slot(
    slots: &Arc<Semaphore>,
    command: &str,
) -> Result<Option<OwnedSemaphorePermit>, String> {
    if matches!(
        command,
        "runtime_service_shutdown"
            | "runtime_service_tray_exit"
            | "runtime_service_status"
            | "runtime_service_events"
            | "cancel_storage_usage_scan"
            | "cancel_knowledge_sync"
            | "assistant_cancel_connection_check"
            | "read_knowledge_status"
    ) {
        return Ok(None);
    }
    slots.clone().try_acquire_owned().map(Some)
        .map_err(|_| String::from("Runtime service is busy; no operation was started. Retry after an active task finishes."))
}

#[cfg(feature = "desktop-reliability")]
pub(super) fn start(app: &tauri::AppHandle, endpoint: security::Endpoint) -> Result<(), String> {
    let first = tauri::async_runtime::block_on(async { security::create_pipe(&endpoint, true) })
        .map_err(|e| format!("Cannot start the current-user runtime service: {e}"))?;
    start_with_pipe(app, endpoint, first)
}

fn start_with_pipe(
    app: &tauri::AppHandle,
    endpoint: security::Endpoint,
    first: NamedPipeServer,
) -> Result<(), String> {
    let control_endpoint = endpoint.control();
    let control =
        tauri::async_runtime::block_on(async { security::create_pipe(&control_endpoint, true) })
            .map_err(|e| format!("Cannot claim the runtime service control channel: {e}"))?;
    events::install(app);
    super::log_service_event(
        "info",
        "runtime.service.started",
        "Current-user runtime service is ready",
    );
    for (endpoint, pipe, control_only) in
        [(endpoint, first, false), (control_endpoint, control, true)]
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = accept(app.clone(), endpoint, pipe, control_only).await {
                super::log_service_event("error", "runtime.service.control_channel_failed", &error);
                // A service without a control channel must not leave unmanaged servers.
                crate::commands::request_app_exit_shutdown(app);
            }
        });
    }
    Ok(())
}

async fn accept(
    app: tauri::AppHandle,
    endpoint: security::Endpoint,
    mut pipe: NamedPipeServer,
    control_only: bool,
) -> Result<(), String> {
    // Shutdown has its own secured name and admission. Ordinary connections,
    // including peers that have not sent a request, cannot consume its slots.
    let slots = Arc::new(Semaphore::new(if control_only {
        MAX_CONTROL_CONNECTIONS
    } else {
        MAX_CONNECTIONS
    }));
    let work_slots = Arc::new(Semaphore::new(MAX_WORK_REQUESTS));
    loop {
        let slot = slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| e.to_string())?;
        if let Err(error) = pipe.connect().await {
            // A peer may disconnect between opening the name and ConnectNamedPipe.
            // Replace only that connection while retaining the secured name.
            if matches!(error.raw_os_error(), Some(109 | 232 | 233)) {
                pipe = security::create_replacement_pipe(&endpoint)
                    .await
                    .map_err(|e| e.to_string())?;
                continue;
            }
            return Err(error.to_string());
        }
        // Keep an instance of the secured name open throughout replacement.
        let next = security::create_replacement_pipe(&endpoint)
            .await
            .map_err(|e| e.to_string())?;
        let connected = std::mem::replace(&mut pipe, next);
        let app = app.clone();
        let work_slots = Arc::clone(&work_slots);
        tauri::async_runtime::spawn_blocking(move || {
            let _slot = slot;
            tauri::async_runtime::block_on(handle(app, connected, work_slots, control_only));
        });
    }
}

fn command_allowed_on_channel(control_only: bool, command: &str) -> bool {
    !control_only || security::is_control_command(command)
}

async fn handle(
    app: tauri::AppHandle,
    mut pipe: NamedPipeServer,
    work_slots: Arc<Semaphore>,
    control_only: bool,
) {
    let request = tokio::time::timeout(
        Duration::from_secs(15),
        wire::read_frame::<wire::Request>(&mut pipe),
    )
    .await;
    let mut exit_after_response = false;
    let result = match request {
        Ok(Ok(request))
            if request.protocol == wire::PROTOCOL
                && !command_allowed_on_channel(control_only, &request.command) =>
        {
            Err("The runtime control channel accepts only status and shutdown requests".into())
        }
        Ok(Ok(request))
            if request.protocol == wire::PROTOCOL
                && request.command == "runtime_service_tray_exit" =>
        {
            accept_final_exit(&app, &request.args)
        }
        Ok(Ok(request))
            if request.protocol == wire::PROTOCOL
                && request.command == "runtime_service_shutdown" =>
        {
            // Updates still require completed saves before installing anything.
            let result =
                crate::commands::commands_runtime_lifecycle::prepare_runtime_service_shutdown(&app)
                    .await;
            exit_after_response = result.is_ok();
            result.map(|()| json!({ "shutdown_completed": true, "pid": std::process::id() }))
        }
        Ok(Ok(request)) if request.protocol == wire::PROTOCOL => {
            // Once received, an operation belongs to the service. Closing the UI
            // does not cancel a save, install, or stop half way through.
            match acquire_request_slot(&work_slots, &request.command) {
                Ok(_work_slot) => {
                    let command = request.command.clone();
                    complete_dispatch(&command, Duration::from_secs(1800), dispatch(&app, request))
                        .await
                }
                Err(error) => Err(error),
            }
        }
        Ok(Ok(_)) => Err(
            "Runtime service protocol mismatch; restart the service using this installation".into(),
        ),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("Runtime service request timed out".into()),
    };
    let _ = tokio::time::timeout(
        Duration::from_secs(15),
        wire::write_response(&mut pipe, &wire::Response { result }),
    )
    .await;
    if exit_after_response {
        // Give the client a bounded chance to read the success receipt before
        // closing the pipe. A disconnected UI must never keep a stopped service alive.
        let _ = tokio::time::timeout(Duration::from_secs(15), wire::read_frame::<bool>(&mut pipe))
            .await;
        app.exit(0);
    }
}

fn accept_final_exit(app: &tauri::AppHandle, args: &Value) -> Result<Value, String> {
    let deadline = args["deadline_tick_ms"]
        .as_u64()
        .ok_or_else(|| String::from("Final exit requires its original deadline"))?;
    #[cfg(feature = "desktop-reliability")]
    super::fixture::block_shutdown_storage_until_save(app)?;
    super::exit_deadline::arm_service(deadline)?;
    // Seal admission before returning ownership. This flag does not claim the
    // shutdown CAS, so an ordinary owner already draining can finish that work.
    if app
        .state::<crate::state::DesktopState>()
        .request_final_exit()
    {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match crate::commands::commands_runtime_lifecycle::prepare_runtime_service_tray_exit(
                &app,
            )
            .await
            {
                Ok(()) => app.exit(0),
                Err(error) => {
                    super::log_service_event("error", "app.exit.background_stop_failed", &error)
                }
            }
        });
    }
    Ok(json!({ "shutdown_accepted": true, "pid": std::process::id() }))
}

fn requires_owned_completion(command: &str) -> bool {
    matches!(
        command,
        "operate_ark_cluster"
            | "create_ark_cluster_backup"
            | "list_ark_cluster_backups"
            | "restore_ark_cluster_backup"
            | "read_pending_ark_cluster_restore"
            | "recover_ark_cluster_restore"
            | "archive_instance_record"
            | "delete_instance_record"
            | "list_instance_archives"
            | "restore_instance_archive"
            | "purge_instance_archive"
    )
}

async fn complete_dispatch<F>(
    command: &str,
    deadline: Duration,
    operation: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    // Each maintenance stage owns its bounds and rollback. A generic IPC
    // observation deadline must not abandon the remaining cluster members or
    // drop a transaction between staging, promotion and recovery.
    if requires_owned_completion(command) {
        return operation.await;
    }
    match tokio::time::timeout(deadline, operation).await {
        Ok(result) => result,
        Err(_) => Err("Runtime service operation exceeded its 30-minute deadline; inspect its task status before retrying".into()),
    }
}

async fn dispatch(app: &tauri::AppHandle, request: wire::Request) -> Result<Value, String> {
    if let Some(operation) =
        super::local_commands::dispatch(app, &request.command, request.args.clone())
    {
        return operation.await;
    }
    match request.command.as_str() {
        "uninstall_steamcmd" => serde_json::to_value(
            crate::commands::uninstall_steamcmd(app.state::<crate::state::DesktopState>()).await?,
        )
        .map_err(|e| e.to_string()),
        "preview_dontstarve_world_start" => {
            let id = request
                .args
                .get("instanceId")
                .or_else(|| request.args.get("instance_id"))
                .and_then(Value::as_str)
                .ok_or("Missing instance ID")?;
            serde_json::to_value(
                crate::commands::commands_dst_world_state::preview_dontstarve_world_start(
                    app.state::<crate::state::DesktopState>(),
                    id.into(),
                )
                .await?,
            )
            .map_err(|e| e.to_string())
        }
        "runtime_service_status" => {
            let state = app.state::<crate::state::DesktopState>();
            Ok(
                json!({ "pid": std::process::id(), "protocol": wire::PROTOCOL,
                "shutdown_in_progress": state.shutdown_in_progress.load(std::sync::atomic::Ordering::SeqCst) }),
            )
        }
        "runtime_service_events" => app.state::<events::Events>().after(
            request.args["after"].as_u64().unwrap_or(0),
            request.args["generation"].as_str(),
        ),
        _ => crate::lan_host::dispatch_command(app, &request.command, request.args).await,
    }
}

pub(super) fn run() -> Result<(), String> {
    let endpoint = security::Endpoint::current().map_err(|e| e.to_string())?;
    // The existing process lease protects storage even before the pipe exists.
    let lease = crate::acquire_process_instance_lease().map_err(|e| e.to_string())?;
    if lease.is_secondary() {
        return Ok(());
    }
    // A per-temp-directory lease alone is insufficient: a second launch can
    // use another TEMP while targeting the same current-user data. Claim the
    // secured OS-wide endpoint before DesktopState can read or create storage.
    let first = tauri::async_runtime::block_on(async { security::create_pipe(&endpoint, true) })
        .map_err(|e| format!("Cannot claim the current-user runtime service: {e}"))?;
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .setup(move |app| {
            app.manage(crate::state::DesktopState::default());
            app.manage(crate::media_cache::MediaCacheState::new(
                app_storage::StoragePaths::default()
                    .app_data_root
                    .join("cache/media-service"),
            ));
            start_with_pipe(app.handle(), endpoint, first).map_err(std::io::Error::other)?;
            if crate::lan_host::is_lan_host_requested() {
                crate::lan_host::spawn_lan_host(
                    app.handle().clone(),
                    crate::lan_host::config_from_env().map_err(std::io::Error::other)?,
                )
                .map_err(std::io::Error::other)?;
            }
            if let Err(error) =
                crate::lan_directory::spawn_lan_directory_broadcaster(app.handle().clone())
            {
                eprintln!("LanGame LAN directory is unavailable: {error}");
            }
            crate::commands::spawn_runtime_heartbeat(app.handle().clone());
            crate::knowledge_runtime::spawn_knowledge_scheduler(app.handle().clone());
            crate::commands::commands_managed_save::spawn_managed_save_worker(app.handle().clone());
            Ok(())
        })
        .build(context)
        .map_err(|e| e.to_string())?;
    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !crate::commands::app_exit_shutdown_completed(handle)
        {
            api.prevent_exit();
        }
    });
    drop(lease);
    Ok(())
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[test]
    fn shutdown_and_status_remain_available_when_creation_fills_work_capacity() {
        let slots = Arc::new(Semaphore::new(MAX_WORK_REQUESTS));
        let mut work = (0..MAX_WORK_REQUESTS)
            .map(|_| acquire_request_slot(&slots, "create_instance_record").unwrap())
            .collect::<Vec<_>>();
        assert!(acquire_request_slot(&slots, "create_instance_record").is_err());
        for command in [
            "runtime_service_status",
            "runtime_service_shutdown",
            "runtime_service_tray_exit",
            "runtime_service_events",
            "cancel_storage_usage_scan",
            "assistant_cancel_connection_check",
        ] {
            assert!(acquire_request_slot(&slots, command).unwrap().is_none());
        }
        work.pop();
        assert!(
            acquire_request_slot(&slots, "create_instance_record")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn dedicated_control_channel_rejects_work_before_dispatch() {
        for command in [
            "create_instance_record",
            "runtime_service_events",
            "unknown",
        ] {
            assert!(!command_allowed_on_channel(true, command), "{command}");
            assert!(command_allowed_on_channel(false, command), "{command}");
        }
        for command in [
            "runtime_service_status",
            "runtime_service_shutdown",
            "runtime_service_tray_exit",
        ] {
            assert!(command_allowed_on_channel(true, command), "{command}");
        }
    }

    #[tokio::test]
    async fn aggregate_maintenance_finishes_after_the_ipc_observation_deadline() {
        for command in [
            "operate_ark_cluster",
            "create_ark_cluster_backup",
            "list_ark_cluster_backups",
            "restore_ark_cluster_backup",
            "read_pending_ark_cluster_restore",
            "recover_ark_cluster_restore",
            "archive_instance_record",
            "delete_instance_record",
            "list_instance_archives",
            "restore_instance_archive",
            "purge_instance_archive",
        ] {
            let (started, observed) = tokio::sync::oneshot::channel();
            let (finish, completing) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(complete_dispatch(command, Duration::ZERO, async move {
                started.send(()).map_err(|_| "No test observer")?;
                completing.await.map_err(|_| "Maintenance was abandoned")?;
                Ok(json!({"completed":true}))
            }));
            observed
                .await
                .expect("The real maintenance future must start");
            finish
                .send(())
                .expect("The pending maintenance future must remain owned");
            assert_eq!(
                task.await.unwrap().unwrap(),
                json!({"completed":true}),
                "{command}"
            );
        }
        assert!(
            complete_dispatch(
                "read_instance_details_from_storage",
                Duration::ZERO,
                std::future::pending::<Result<Value, String>>()
            )
            .await
            .is_err()
        );
    }
}
