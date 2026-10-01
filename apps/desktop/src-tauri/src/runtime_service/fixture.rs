//! Opt-in native acceptance: a real service, two independent IPC
//! clients, and a synthetic game behind the existing Necesse module contract.
#[path = "fixture_client.rs"]
mod client;
#[path = "fixture_config.rs"]
mod config;
#[path = "fixture_network.rs"]
mod network;
#[path = "fixture_process.rs"]
mod process;
#[path = "fixture_tray_exit.rs"]
mod tray_exit;

pub(crate) use network::try_fixture_firewall_boundary;
pub(super) use tray_exit::exit_grace_timeout;

pub(super) fn exit_helper_endpoint() -> Result<Option<super::security::Endpoint>, String> {
    exit_helper_config()?
        .map(|config| super::security::Endpoint::isolated(&config.nonce).map_err(|e| e.to_string()))
        .transpose()
}

pub(super) fn record_exit_helper(service_pid: u32, deadline_tick_ms: u64) -> Result<(), String> {
    let Some(config) = exit_helper_config()? else {
        return Ok(());
    };
    let helper_pid = std::process::id();
    let helper_identity = app_runtime::inspect_process_identity(helper_pid)
        .map_err(|e| e.to_string())?
        .ok_or("Exit helper lost its process identity")?;
    config::write_new(
        &config.root.join("data/exit-handoff-owned.json"),
        &serde_json::json!({
            "nonce":config.nonce,"helper_pid":helper_pid,"helper_identity":helper_identity,
            "service_pid":service_pid,"deadline_tick_ms":deadline_tick_ms,
        }),
    )
}

pub(super) fn block_shutdown_storage_until_save(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(config) = exit_helper_config()? else {
        return Ok(());
    };
    let state = app.state::<crate::state::DesktopState>();
    if state.is_final_exit_requested() {
        return Ok(());
    }
    let lease =
        state.begin_storage_context_operation("fixture write awaiting the first server save")?;
    write_new(
        &config.root.join("data/shutdown-storage-blocker.json"),
        &json!({
            "nonce":config.nonce,"storage_lease_admitted":true,
        }),
    )?;
    tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(config.scenario.exit_grace().unwrap(), async {
            lease.cancelled().await;
            let requested = config.root.join("data/game-save-requested.json");
            while !config.root.join("data/game-save-requested.ready").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let save: Value = read_json(&requested)?;
            if save["nonce"] != config.nonce
                || save["received"] != true
                || save["save_commands"] != 1
            {
                return Err("Storage blocker observed invalid save evidence".to_owned());
            }
            // Publish while still holding the real lease. Only observed save
            // delivery releases it; elapsed time can never make this test pass.
            write_new(
                &config.root.join("data/shutdown-storage-released.json"),
                &json!({
                    "nonce":config.nonce,"cancellation_requested":true,
                    "save_received_while_lease_held":true,"save_commands":save["save_commands"],
                }),
            )
        })
        .await;
        drop(lease);
        if !matches!(result, Ok(Ok(()))) {
            eprintln!("Fixture storage blocker did not observe shutdown save: {result:?}");
        }
    });
    Ok(())
}

fn exit_helper_config() -> Result<Option<config::Config>, String> {
    let Some(root) = std::env::var_os("LANGAME_RUNTIME_SERVICE_FIXTURE_ROOT") else {
        return Ok(None);
    };
    let root = std::path::PathBuf::from(root);
    let config: config::Config = config::read_json(&root.join(config::MARKER))?;
    config.verify()?;
    if config.root != root
        || !config.scenario.is_tray_exit()
        || std::env::var("LANGAME_RUNTIME_SERVICE_FIXTURE_NONCE")
            .ok()
            .as_deref()
            != Some(config.nonce.as_str())
    {
        return Err("Exit helper fixture namespace does not match its verified marker".into());
    }
    Ok(Some(config))
}

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tauri::Manager;

use config::{Config, read_json, write_new};
use process::OwnedChild;

const OWNER: &str = "--runtime-service-fixture";
const SERVICE: &str = "--runtime-service-fixture-service";
const CLIENT_START: &str = "--runtime-service-fixture-client-start";
const CLIENT_STOP: &str = "--runtime-service-fixture-client-stop";

pub(super) fn run_if_requested() -> bool {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = if let Some(game) = process::run_game_if_requested() {
        game
    } else {
        let Some(role) = args.first().and_then(|value| value.to_str()) else {
            return false;
        };
        if ![OWNER, SERVICE, CLIENT_START, CLIENT_STOP].contains(&role) {
            return false;
        }
        if args.len() != 2 {
            Err("Invalid runtime service fixture arguments".into())
        } else {
            let path = Path::new(&args[1]);
            Config::load(path).and_then(|config| match role {
                OWNER => run_owner(&config, path),
                SERVICE => run_service(config),
                CLIENT_START => client::run(config, path, true),
                CLIENT_STOP => client::run(config, path, false),
                _ => Err("Unknown fixture role".into()),
            })
        }
    };
    if let Err(error) = result {
        eprintln!("Runtime service fixture failed: {error}");
        std::process::exit(1);
    }
    true
}

fn run_owner(config: &Config, path: &Path) -> Result<(), String> {
    config.create()?;
    config.initialize_environment()?;
    let result = run_owned_children(config, path);
    if let Err(error) = &result {
        write_new(&config.root.join("failure.json"), &json!({"error":error}))?;
    }
    result
}

fn run_owned_children(config: &Config, path: &Path) -> Result<(), String> {
    // The first client uses the production detached-service launcher. This
    // independent owner retains that client's outer Job through both client
    // exits, so the fixture can clean all descendants without breakaway.
    let mut first = OwnedChild::spawn(config, CLIENT_START, path)?;
    first.wait(90)?;
    let first_report: Value = read_json(&config.root.join("client-start.json"))?;
    let service_pid = first_report["service_pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Missing service PID")?;
    let service_identity: app_core::ProcessIdentity =
        serde_json::from_value(first_report["service_identity"].clone())
            .map_err(|e| e.to_string())?;
    let game_pid = first_report["game_pid"]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or("Missing game PID")?;
    let game_identity: app_core::ProcessIdentity =
        serde_json::from_value(first_report["game_identity"].clone()).map_err(|e| e.to_string())?;
    // Observe after the first client has fully exited, not merely disconnected.
    std::thread::sleep(Duration::from_secs(2));
    if !app_runtime::process_matches_identity(service_pid, &service_identity)
        .map_err(|e| e.to_string())?
        || !app_runtime::process_matches_identity(game_pid, &game_identity)
            .map_err(|e| e.to_string())?
    {
        return Err("Closing the first client terminated the service or managed game".into());
    }
    if config.scenario.is_tray_exit() {
        return tray_exit::run(config, path, &first_report, &mut first);
    }
    let mut second = OwnedChild::spawn(config, CLIENT_STOP, path)?;
    second.wait(90)?;
    let second_report: Value = read_json(&config.root.join("client-stop.json"))?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while app_runtime::process_matches_identity(service_pid, &service_identity)
        .map_err(|e| e.to_string())?
        || app_runtime::process_matches_identity(game_pid, &game_identity)
            .map_err(|e| e.to_string())?
    {
        if Instant::now() >= deadline {
            return Err("Explicit service shutdown left a managed process alive".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if first_report["service_pid"] != second_report["service_pid"]
        || first_report["game_pid"] != second_report["game_pid"]
    {
        return Err("Reconnect did not preserve service/game process identity".into());
    }
    let saved: Value = read_json(&config.root.join("data/game-saved.json"))?;
    let stopped: Value = read_json(&config.root.join("data/game-stopped.json"))?;
    if saved["saved"] != true || stopped["saved_before_stop"] != true {
        return Err("Managed save/stop evidence is missing".into());
    }
    let instance_id = first_report["instance_id"]
        .as_str()
        .ok_or("Missing instance identity")?;
    let storage = app_storage::bootstrap_storage().map_err(|e| e.to_string())?;
    tauri::async_runtime::block_on(async {
        let details = app_storage::read_instance_details(&storage.paths, instance_id)
            .await
            .map_err(|e| e.to_string())?;
        if !matches!(details.summary.status, app_core::InstanceStatus::Stopped)
            || details.active_run.is_some()
        {
            return Err("Stopped fixture state did not persist after service exit".to_owned());
        }
        let world: Value =
            read_json(&std::path::Path::new(&details.saves_path).join("fixture-world.json"))?;
        if world["saved"] != true || world["nonce"] != config.nonce {
            return Err("Saved fixture world is not readable after service exit".into());
        }
        if app_storage::list_instance_backups(&storage.paths, instance_id)
            .await
            .map_err(|e| e.to_string())?
            .is_empty()
        {
            return Err("App-exit save backup was not created".into());
        }
        Ok::<(), String>(())
    })?;
    second.finish()?;
    first.finish()?;
    let firewall: Value = read_json(&config.root.join("data/firewall-boundary.json"))?;
    if firewall["mode"] != "synthetic_loopback_external_boundary"
        || firewall["native_firewall_exercised"] != false
        || firewall["nonce"] != config.nonce
        || firewall["instance_id"] != instance_id
    {
        return Err("Synthetic firewall boundary evidence mismatch".into());
    }
    write_new(
        &config.root.join("report.json"),
        &json!({
            "schema_version":1, "passed":true, "client_kind":"native tray client and separate headless reconnect client",
            "game_kind":"synthetic managed Necesse protocol process", "service_pid":service_pid,
            "game_pid":game_pid, "client_pids":[first.0.pid,second.0.pid],
            "started_by_first_client":true,"survived_first_client_exit":true,"reconnected_same_processes":true,
            "save_before_stop":true,"shutdown_receipt_acknowledged":true,
            "service_exited":true,"game_exited":true,"owned_process_trees_joined":true,
            "stopped_state_persisted":true,"world_saved_and_backed_up":true,
            "log_continuity":second_report["log_continuity"],
            "native_firewall_exercised":false,
            "first_client":first_report,"second_client":second_report
        }),
    )
}

fn run_service(config: Config) -> Result<(), String> {
    config.initialize_environment()?;
    let endpoint = super::security::Endpoint::isolated(&config.nonce).map_err(|e| e.to_string())?;
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    let ready = config.root.join("service-ready.json");
    let app = tauri::Builder::default()
        .setup(move |app| {
            app.manage(crate::state::DesktopState::default());
            app.manage(network::LoopbackFixture(config));
            super::server::start(app.handle(), endpoint).map_err(std::io::Error::other)?;
            crate::commands::spawn_runtime_heartbeat(app.handle().clone());
            write_new(&ready, &json!({"pid":std::process::id()})).map_err(std::io::Error::other)?;
            Ok(())
        })
        .build(context)
        .map_err(|e| e.to_string())?;
    let exit = app.run_return(|app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event
            && !crate::commands::app_exit_shutdown_completed(app)
        {
            api.prevent_exit();
        }
    });
    if exit != 0 {
        return Err(format!("Fixture service exit {exit}"));
    }
    Ok(())
}
