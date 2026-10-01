//! Production tray-exit acceptance in a verified, disposable, headless namespace.
use std::path::Path;
use std::time::{Duration, Instant};

use app_runtime::ProcessExitTarget;
use serde_json::{Value, json};
use tauri::Manager;

use super::config::{Config, Scenario, read_json, same_path, write_new};
use super::process::OwnedChild;

struct FixtureExitGrace(Duration);

pub(super) fn install_exit_grace(app: &mut tauri::App, config: &Config) -> Result<(), String> {
    config.verify()?;
    let owner = config.root.parent().ok_or("Missing fixture owner")?;
    same_path(
        owner.parent().ok_or("Missing fixture TEMP parent")?,
        &std::env::temp_dir(),
    )?;
    if config.root.file_name().and_then(|name| name.to_str()) != Some("fixture")
        || !owner
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("langame-runtime-service-"))
    {
        return Err("Short exit deadlines require the dedicated TEMP fixture namespace".into());
    }
    let grace = config
        .scenario
        .exit_grace()
        .ok_or("The normal acceptance scenario cannot change the exit deadline")?;
    app.manage(FixtureExitGrace(grace));
    Ok(())
}

/// This capability can only be installed by the feature-gated fixture client.
/// Production callers and environment variables cannot select a shorter budget.
pub(in crate::runtime_service) fn exit_grace_timeout(app: &tauri::AppHandle) -> Option<Duration> {
    app.try_state::<FixtureExitGrace>().map(|grace| grace.0)
}

fn tick_ms() -> u64 {
    // Same boot-relative clock as the production UI/service deadline handshake.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

pub(super) fn record_request(config: &Config, mut evidence: Value) -> Result<(), String> {
    evidence["phase"] = json!("before_tray_handler_dispatch");
    evidence["nonce"] = json!(config.nonce);
    evidence["scenario"] = json!(config.scenario);
    evidence["request_tick_ms"] = json!(tick_ms());
    evidence["exit_grace_ms"] = json!(
        config
            .scenario
            .exit_grace()
            .ok_or("Missing tray fixture deadline")?
            .as_millis() as u64
    );
    // This is deliberately not a successful-exit receipt. The independent owner
    // retains process objects and validates saves after every process has exited.
    write_new(&config.root.join("client-stop-request.json"), &evidence)
}

pub(super) fn run(
    config: &Config,
    path: &Path,
    first_report: &Value,
    first: &mut OwnedChild,
) -> Result<(), String> {
    let service = capture(first_report, "service")?;
    let game = capture(first_report, "game")?;
    let mut second = OwnedChild::spawn(config, super::CLIENT_STOP, path)?;
    let request = wait_for_request(config, &mut second)?;
    if request["nonce"] != config.nonce
        || request["scenario"] != json!(config.scenario)
        || request["client_pid"] != second.0.pid
        || request["service_pid"] != first_report["service_pid"]
        || request["game_pid"] != first_report["game_pid"]
        || request["service_identity"] != first_report["service_identity"]
        || request["game_identity"] != first_report["game_identity"]
        || request["run_id"] != first_report["run_id"]
        || request["same_run"] != true
        || request["log_continuity"] != true
        || request["phase"] != "before_tray_handler_dispatch"
    {
        return Err("Pre-dispatch tray fixture evidence does not match the owned processes".into());
    }
    let requested_at = request["request_tick_ms"]
        .as_u64()
        .ok_or("Missing tray fixture request timestamp")?;
    let grace_ms = config.scenario.exit_grace().unwrap().as_millis() as u64;
    if request["exit_grace_ms"] != grace_ms || requested_at > tick_ms() {
        return Err("Tray fixture deadline evidence is invalid".into());
    }
    // The extra second is an observation allowance for this external process's
    // scheduling and file publication; actual elapsed time remains in the report.
    let observation_deadline = requested_at.saturating_add(grace_ms + 2_000 + 1_000);
    let grace_deadline = requested_at.saturating_add(grace_ms);
    let forced = config.scenario == Scenario::TrayExitHangSave;
    let mut client_exit = None;
    let mut client_exit_tick_ms = None;
    let mut background_observed_after_client_exit = false;
    let mut helper = None;
    let mut helper_exit_tick_ms = None;
    let mut helper_report = Value::Null;
    let mut service_exit_tick_ms = None;
    let mut game_exit_tick_ms = None;
    loop {
        if client_exit.is_none() {
            client_exit = second
                .0
                .child
                .as_mut()
                .ok_or("Tray fixture lost its client owner")?
                .try_wait()
                .map_err(|error| error.to_string())?;
        }
        let service_running = service.is_running().map_err(|error| error.to_string())?;
        let game_running = game.is_running().map_err(|error| error.to_string())?;
        let observed_at = tick_ms();
        if client_exit.is_some() && client_exit_tick_ms.is_none() {
            client_exit_tick_ms = Some(observed_at);
            if !service_running
                || !game_running
                || config.root.join("data/game-saved.json").exists()
            {
                return Err(
                    "Interface exit must precede background game saving and runtime exit".into(),
                );
            }
            background_observed_after_client_exit = true;
            helper_report = read_json(&config.root.join("data/exit-handoff-owned.json"))?;
            if helper_report["nonce"] != config.nonce
                || helper_report["service_pid"] != first_report["service_pid"]
            {
                return Err(
                    "Independent exit helper does not own the original fixture runtime".into(),
                );
            }
            helper = Some(capture(&helper_report, "helper")?);
            if !forced {
                write_new(
                    &config.root.join("data/allow-game-save.json"),
                    &json!({
                        "nonce":config.nonce,"client_exited":true,
                    }),
                )?;
                std::fs::File::options()
                    .write(true)
                    .create_new(true)
                    .open(config.root.join("data/allow-game-save.ready"))
                    .map_err(|error| error.to_string())?;
            }
        }
        if client_exit.is_none() && observed_at > requested_at.saturating_add(3_000) {
            return Err("Interface stayed alive waiting for background save/stop work".into());
        }
        if !service_running {
            service_exit_tick_ms.get_or_insert(observed_at);
        }
        if !game_running {
            game_exit_tick_ms.get_or_insert(observed_at);
        }
        let helper_running = helper
            .as_ref()
            .map(|target: &ProcessExitTarget| target.is_running())
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or(true);
        if !helper_running {
            helper_exit_tick_ms.get_or_insert(observed_at);
        }
        if forced && observed_at < grace_deadline && (!service_running || !game_running) {
            return Err(format!(
                "Hung-save grace period was cut short: service_running={service_running}, game_running={game_running}, elapsed_ms={}",
                observed_at.saturating_sub(requested_at),
            ));
        }
        if client_exit.is_some() && !service_running && !game_running && !helper_running {
            break;
        }
        if observed_at > observation_deadline {
            return Err(format!(
                "Tray exit missed its deadline: client_exited={}, service_running={service_running}, game_running={game_running}",
                client_exit.is_some(),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let completed_at = tick_ms();
    let elapsed_ms = completed_at.saturating_sub(requested_at);
    if completed_at > observation_deadline {
        return Err(format!(
            "All tray processes exited, but their observed completion exceeded the full deadline: elapsed_ms={elapsed_ms}"
        ));
    }
    let instance_id = first_report["instance_id"]
        .as_str()
        .ok_or("Missing fixture instance ID")?;
    let save_request: Value = read_json(&config.root.join("data/game-save-requested.json"))?;
    if save_request["received"] != true
        || save_request["nonce"] != config.nonce
        || save_request["pid"] != first_report["game_pid"]
        || save_request["save_commands"] != 1
    {
        return Err("Synthetic game did not confirm receipt of the save command".into());
    }
    let storage_blocker: Value =
        read_json(&config.root.join("data/shutdown-storage-released.json"))?;
    if storage_blocker["nonce"] != config.nonce
        || storage_blocker["cancellation_requested"] != true
        || storage_blocker["save_received_while_lease_held"] != true
        || storage_blocker["save_commands"] != 1
    {
        return Err("Unified save did not run before the unrelated storage lease drained".into());
    }
    let storage = app_storage::bootstrap_storage().map_err(|error| error.to_string())?;
    let state: Result<Value, String> = tauri::async_runtime::block_on(async {
        let details = app_storage::read_instance_details(&storage.paths, instance_id)
            .await
            .map_err(|error| error.to_string())?;
        let world = Path::new(&details.saves_path).join("fixture-world.json");
        let backups = app_storage::list_instance_backups(&storage.paths, instance_id)
            .await
            .map_err(|error| error.to_string())?;
        if forced {
            if world.exists()
                || config.root.join("data/game-saved.json").exists()
                || config.root.join("data/game-stopped.json").exists()
                || !backups.is_empty()
            {
                return Err("Hung-save scenario fabricated successful save/backup evidence".into());
            }
            if elapsed_ms < grace_ms {
                return Err("Hung-save process exited before the allowed grace period".into());
            }
            Ok(json!({"save_completed":false,"world_exists":false,"backups_created":0}))
        } else {
            let saved: Value = read_json(&config.root.join("data/game-saved.json"))?;
            let stopped: Value = read_json(&config.root.join("data/game-stopped.json"))?;
            let world: Value = read_json(&world)?;
            if saved["saved"] != true
                || stopped["saved_before_stop"] != true
                || world["saved"] != true
                || world["nonce"] != config.nonce
                || !matches!(details.summary.status, app_core::InstanceStatus::Stopped)
                || details.active_run.is_some()
                || backups.is_empty()
                || saved["save_commands"] != 1
                || stopped["save_commands"] != 1
            {
                return Err(
                    "Normal tray exit did not persist its confirmed save/stop/backup".into(),
                );
            }
            if !client_exit.as_ref().unwrap().success() {
                return Err("Normal tray-exit client reported failure".into());
            }
            if elapsed_ms >= grace_ms {
                return Err("Normal tray exit required the final watchdog cutoff".into());
            }
            Ok(json!({"save_completed":true,"stopped_state_persisted":true,
                "world_saved_and_backed_up":true,"backups_created":backups.len()}))
        }
    });
    let state = state?;
    let firewall: Value = read_json(&config.root.join("data/firewall-boundary.json"))?;
    if firewall["mode"] != "synthetic_loopback_external_boundary"
        || firewall["native_firewall_exercised"] != false
        || firewall["nonce"] != config.nonce
        || firewall["instance_id"] != instance_id
    {
        return Err("Synthetic firewall boundary evidence mismatch".into());
    }
    // Cleanup follows the observations, so it cannot manufacture a passing exit.
    second.finish()?;
    first.finish()?;
    write_new(
        &config.root.join("report.json"),
        &json!({
            "schema_version":3,"passed":true,"scenario":config.scenario,
            "client_kind":"two headless clients; production final tray handler",
            "game_kind":"synthetic managed Necesse protocol process",
            "service_pid":first_report["service_pid"],"game_pid":first_report["game_pid"],
            "client_pids":[first.0.pid,second.0.pid],"first_client":first_report,
            "request_evidence":request,"started_by_first_client":true,
            "survived_first_client_exit":true,"reconnected_same_processes":true,
            "production_tray_handler":true,"duplicate_tray_request":true,
            "save_command_received":true,"expected_forced_exit":forced,
            "client_exited":true,"client_exit_code":client_exit.unwrap().code(),
            "client_exit_tick_ms":client_exit_tick_ms,
            "observed_client_exit_elapsed_ms":client_exit_tick_ms.map(|tick| tick.saturating_sub(requested_at)),
            "background_running_after_client_exit":background_observed_after_client_exit,
            "exit_helper":helper_report,"helper_exited":true,"helper_exit_tick_ms":helper_exit_tick_ms,
            "save_command_count":save_request["save_commands"],
            "save_before_storage_drain":storage_blocker,
            "service_exited":true,"game_exited":true,"owned_process_trees_joined":true,
            "exit_grace_ms":grace_ms,"observed_exit_elapsed_ms":elapsed_ms,
            "service_exit_tick_ms":service_exit_tick_ms,"game_exit_tick_ms":game_exit_tick_ms,
            "observed_service_exit_elapsed_ms":service_exit_tick_ms.map(|tick| tick.saturating_sub(requested_at)),
            "observed_game_exit_elapsed_ms":game_exit_tick_ms.map(|tick| tick.saturating_sub(requested_at)),
            "native_settle_allowance_ms":2000,"external_observation_allowance_ms":1000,
            "save_state":state,"native_firewall_exercised":false,
        }),
    )
}

fn capture(report: &Value, role: &str) -> Result<ProcessExitTarget, String> {
    let pid = report[format!("{role}_pid")]
        .as_u64()
        .and_then(|pid| u32::try_from(pid).ok())
        .ok_or_else(|| format!("Missing {role} PID"))?;
    let identity: app_core::ProcessIdentity =
        serde_json::from_value(report[format!("{role}_identity")].clone())
            .map_err(|error| error.to_string())?;
    ProcessExitTarget::capture(pid, &identity)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Fixture {role} exited before final tray request"))
}

fn wait_for_request(config: &Config, second: &mut OwnedChild) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match read_json(&config.root.join("client-stop-request.json")) {
            Ok(report) => return Ok(report),
            Err(error) if Instant::now() >= deadline => return Err(error),
            Err(_) => {}
        }
        if second
            .0
            .child
            .as_mut()
            .ok_or("Fixture lost its client owner")?
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            return Err("Tray fixture client exited before recording its request".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
