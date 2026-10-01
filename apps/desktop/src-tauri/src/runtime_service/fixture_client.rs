use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use app_core::{
    InstanceDetails, InstanceProvisioning, LogTailSnapshot, StartInstanceResult,
    UpdateInstanceInput,
};
use serde_json::{Value, json};
use tauri::Manager;

use super::config::{Config, read_json, same_path, write_new};
use crate::runtime_service::client::Client;
use crate::runtime_service::security::Endpoint;

#[path = "fixture_tray.rs"]
mod tray;

pub(super) fn run(config: Config, path: &std::path::Path, start: bool) -> Result<(), String> {
    let final_tray_exit = !start && config.scenario.is_tray_exit();
    if start && !config.scenario.is_tray_exit() {
        tray::initialize_environment(&config)?;
    }
    config.initialize_environment()?;
    let endpoint = Endpoint::isolated(&config.nonce).map_err(|e| e.to_string())?;
    let client = if start {
        crate::runtime_service::client::connect_or_start_with_arguments(
            endpoint,
            &[super::SERVICE.into(), path.as_os_str().to_owned()],
        )?
    } else {
        tauri::async_runtime::block_on(async { Client::connect(endpoint) })
            .map_err(|e| e.to_string())?
    };
    let result = Arc::new(Mutex::new(None));
    let result_writer = Arc::clone(&result);
    let complete = Arc::new(AtomicBool::new(false));
    let completed = Arc::clone(&complete);
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let app = tauri::Builder::default()
        .manage(client)
        .on_window_event(crate::handle_window_event)
        .setup(move |app| {
            if start && !config.scenario.is_tray_exit() {
                tray::setup(app, &config).map_err(std::io::Error::other)?;
            }
            if final_tray_exit {
                super::tray_exit::install_exit_grace(app, &config)
                    .map_err(std::io::Error::other)?;
            }
            let app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if final_tray_exit {
                    let requested = match reconnect_observations(&app, &config).await {
                        Ok(evidence) => super::tray_exit::record_request(&config, evidence),
                        Err(error) => Err(error),
                    };
                    let ready = requested.is_ok();
                    if let Ok(mut writer) = result_writer.lock() {
                        *writer = Some(requested);
                    }
                    completed.store(true, Ordering::SeqCst);
                    if ready {
                        // This is the production tray handler. The owner, not
                        // this pre-dispatch record, verifies actual completion.
                        crate::runtime_service::request_stop_and_exit(app.clone());
                        crate::runtime_service::request_stop_and_exit(app);
                    } else {
                        app.exit(1);
                    }
                    return;
                }
                let operation = if start {
                    start_game(&app, &config).await
                } else {
                    reconnect_and_stop(&app, &config).await
                };
                let filename = if start {
                    "client-start.json"
                } else {
                    "client-stop.json"
                };
                let operation =
                    operation.and_then(|value| write_new(&config.root.join(filename), &value));
                if let Ok(mut writer) = result_writer.lock() {
                    *writer = Some(operation);
                }
                completed.store(true, Ordering::SeqCst);
                app.exit(0);
            });
            Ok(())
        })
        .build(context)
        .map_err(|e| e.to_string())?;
    app.run_return(move |_, event| {
        if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
            // Keep a headless client alive, but do not mask a close handler that
            // incorrectly calls app.exit() before the tray checks finish.
            if code.is_none() && (final_tray_exit || !complete.load(Ordering::SeqCst)) {
                api.prevent_exit();
            }
        }
    });
    result
        .lock()
        .map_err(|_| "Fixture result lock poisoned")?
        .take()
        .ok_or("Fixture client exited without a result")?
}

async fn start_game(app: &tauri::AppHandle, config: &Config) -> Result<Value, String> {
    let client = app.state::<Client>();
    let status = client
        .request("runtime_service_status", Value::Null)
        .await?;
    let service_pid = status["pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Missing service PID")?;
    let service_identity = app_runtime::inspect_process_identity(service_pid)
        .map_err(|e| e.to_string())?
        .ok_or("Service exited before start")?;
    client.request("bootstrap", Value::Null).await?;
    client.request("ensure_storage_ready", Value::Null).await?;
    prepare_synthetic_install(config).await?;
    let created: InstanceProvisioning = serde_json::from_value(
        client
            .request(
                "create_instance_record",
                json!({"input":{"name":"Runtime service fixture","module_id":"necesse"}}),
            )
            .await?,
    )
    .map_err(|e| e.to_string())?;
    let id = created.summary.id;
    let details: InstanceDetails = serde_json::from_value(
        client
            .request(
                "read_instance_details_from_storage",
                json!({"instanceId":id}),
            )
            .await?,
    )
    .map_err(|e| e.to_string())?;
    // Reserve a real OS-selected loopback UDP port for this one fixture. The
    // production start path still resolves a collision after this socket drops.
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = socket.local_addr().map_err(|e| e.to_string())?.port();
    let mut ports = details.ports.clone();
    for binding in &mut ports {
        binding.port = port;
    }
    let update = UpdateInstanceInput {
        id: id.clone(),
        bind_ip: "127.0.0.1".into(),
        auto_backup_on_stop: true,
        backup_retention_count: details.backup_retention_count,
        settings_json: details.settings_json.clone(),
        ports,
    };
    client
        .request(
            "update_instance_record_if_current",
            json!({"input":update,"expectedSettingsJson":details.settings_json}),
        )
        .await?;
    drop(socket);
    let started: StartInstanceResult = serde_json::from_value(
        client
            .request("start_instance_process", json!({"instanceId":id}))
            .await?,
    )
    .map_err(|e| e.to_string())?;
    let identity = app_runtime::inspect_process_identity(started.pid)
        .map_err(|e| e.to_string())?
        .ok_or("Managed game exited before client close")?;
    same_path(
        std::path::Path::new(&identity.image_path),
        &config
            .root
            .join("runtime/instances")
            .join(&id)
            .join("runtime/jre/bin/java.exe"),
    )?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    while !config.root.join("data/game-ready.json").is_file() {
        if tokio::time::Instant::now() >= deadline {
            return Err("Synthetic game did not publish readiness".into());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let tray = if config.scenario.is_tray_exit() {
        json!({"headless":true})
    } else {
        tray::exercise(app, config, service_pid).await?
    };
    Ok(
        json!({"client_pid":std::process::id(),"service_pid":status["pid"],"instance_id":id,
        "service_identity":service_identity,"game_pid":started.pid,"game_identity":identity,"run_id":started.run_id,"started":true,"tray":tray}),
    )
}

async fn prepare_synthetic_install(config: &Config) -> Result<(), String> {
    config.verify()?;
    let storage = app_storage::bootstrap_storage().map_err(|e| e.to_string())?;
    if !storage.paths.app_data_root.starts_with(&config.root) {
        return Err("Synthetic install registration escaped fixture storage".into());
    }
    same_path(
        &storage.paths.games_root,
        &config.root.join("runtime/games"),
    )?;
    let program_root = storage.paths.games_root.join("necesse");
    let _install_guard = app_steamcmd::acquire_game_install_lifecycle(
        "necesse",
        std::slice::from_ref(&program_root),
    )
    .await
    .map_err(|e| e.to_string())?;
    let paths = storage.paths.clone();
    let source = program_root.clone();
    let descriptor = tokio::task::spawn_blocking(move || -> Result<_, String> {
        let descriptor = app_modules::discover_modules(&paths.modules_root)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|descriptor| descriptor.summary.id == "necesse")
            .ok_or("Synthetic package requires the Necesse module")?;
        // Config::create supplied only this fixture binary and a synthetic JAR
        // in a new owned directory. Hash those real bytes before production
        // creation checks integrity; this does not exercise official acquisition.
        app_storage::record_library_program_baseline(&source, &descriptor, true, None)
            .map_err(|e| e.to_string())?;
        // Keep the source separate so production creation makes an instance
        // copy, as required by the synthetic process and firewall boundaries.
        app_storage::retain_library_program_source(&paths, &source, &descriptor)
            .map_err(|e| e.to_string())?;
        Ok(descriptor)
    })
    .await
    .map_err(|e| format!("Synthetic package metadata worker failed: {e}"))??;
    app_storage::sync_modules(&storage.paths, std::slice::from_ref(&descriptor))
        .await
        .map_err(|e| e.to_string())?;
    app_storage::sync_game_installs(
        &storage.paths,
        &[app_storage::GameInstallSyncRecord {
            module_id: descriptor.summary.id,
            install_root: program_root.to_string_lossy().into_owned(),
            install_state: app_core::InstallState::Installed,
            current_version: Some("synthetic-runtime-service-fixture".into()),
            mark_verified: true,
        }],
    )
    .await
    .map_err(|e| e.to_string())
}

async fn reconnect_and_stop(app: &tauri::AppHandle, config: &Config) -> Result<Value, String> {
    let mut evidence = reconnect_observations(app, config).await?;
    crate::runtime_service::client::stop_service(app).await?;
    evidence["explicit_shutdown_completed"] = json!(true);
    Ok(evidence)
}

async fn reconnect_observations(app: &tauri::AppHandle, config: &Config) -> Result<Value, String> {
    let first: Value = read_json(&config.root.join("client-start.json"))?;
    let id = first["instance_id"]
        .as_str()
        .ok_or("First client did not record its instance")?;
    let client = app.state::<Client>();
    // Reproduce connect/close without a request, including the window between
    // CreateFile and ConnectNamedPipe. Only pipe-busy is retried, with a bound.
    let endpoint = Endpoint::isolated(&config.nonce).map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    for _ in 0..20 {
        loop {
            match crate::runtime_service::security::connect_verified(&endpoint) {
                Ok(pipe) => {
                    drop(pipe);
                    break;
                }
                Err(error)
                    if error.raw_os_error() == Some(231)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                Err(error) => {
                    return Err(format!(
                        "Disconnected probe reached an unavailable service: {error}"
                    ));
                }
            }
        }
        tokio::task::yield_now().await;
    }
    let status = client
        .request("runtime_service_status", Value::Null)
        .await?;
    if status["pid"] != first["service_pid"] {
        return Err("Reconnect reached a replacement service".into());
    }
    let details: InstanceDetails = serde_json::from_value(
        client
            .request(
                "read_instance_details_from_storage",
                json!({"instanceId":id}),
            )
            .await?,
    )
    .map_err(|e| e.to_string())?;
    let run = details
        .active_run
        .ok_or("Managed game disappeared after the first client exited")?;
    if json!(run.pid) != first["game_pid"] || json!(run.run_id) != first["run_id"] {
        return Err("Reconnect reached a different managed game run".into());
    }
    let log: LogTailSnapshot = serde_json::from_value(
        client
            .request(
                "read_instance_log_document_from_storage",
                json!({"instanceId":id,"maxLines":200,"runId":run.run_id}),
            )
            .await?,
    )
    .map_err(|e| e.to_string())?;
    if log.read_error.is_some()
        || log.source_path.is_none()
        || log.source_path != run.log_path
        || !log
            .lines
            .iter()
            .any(|line| line.contains("LGSM_SYNTHETIC_GAME_READY"))
    {
        return Err(format!(
            "Original run log did not survive client reconnection: {:?}",
            log.read_error
        ));
    }
    Ok(
        json!({"client_pid":std::process::id(),"service_pid":status["pid"],"game_pid":first["game_pid"],
        "service_identity":first["service_identity"],"game_identity":first["game_identity"],"run_id":run.run_id,
        "same_run":true,"log_continuity":true,"log_path":log.source_path,
        "disconnected_probes":20}),
    )
}
