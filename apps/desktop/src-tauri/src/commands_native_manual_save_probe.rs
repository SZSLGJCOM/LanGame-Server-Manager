//! One opt-in save through the same declared action used by maintenance/cards.
//! Transport delivery and pending receipts are never promoted to durable saves.
use super::*;
use app_core::{ActiveInstanceRun, InstanceProcessState, ProcessIdentity};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "commands_native_save_checkpoint.rs"]
mod checkpoint;

pub(super) async fn before_start<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    checkpoint::before_start(runtime, details, cycle).await?;
    checkpoint::restored_world::before_start(runtime, details, cycle).await
}

pub(super) async fn after_stop<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    checkpoint::after_stop(runtime, running, cycle).await
}

type Attempt = (String, i64, String);
static ATTEMPTS: OnceLock<Mutex<HashSet<Attempt>>> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
struct ProcessGeneration {
    key: String,
    run_id: i64,
    pid: u32,
    session: Option<String>,
    identity: ProcessIdentity,
}

#[derive(Debug, PartialEq, Eq)]
struct RunGeneration {
    run_id: i64,
    session: Option<String>,
    processes: Vec<ProcessGeneration>,
}

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    cycle: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() != Ok("true") {
        return Ok(());
    }
    let expect_rejection = save_guard_probe::expected(&running.summary.module_id)?;
    let _operation = runtime
        .state
        .begin_storage_context_operation("isolated native manual-save acceptance")?;
    ensure_storage_context_snapshot_current(
        runtime.state,
        runtime.storage,
        "isolated native manual-save acceptance",
    )?;
    let fixture = runtime.package.root.canonicalize()?;
    let config = Path::new(&running.config_file_path).canonicalize()?;
    // Startup may have detached a formerly shared program for this instance.
    // Resolve the live binding rather than the pre-start fixture root.
    let program = creation::program_root(running)?.canonicalize()?;
    if !config.starts_with(&fixture)
        || !program.starts_with(&fixture)
        || runtime.descriptor.summary.id != running.summary.module_id
    {
        return Err(
            "Native manual save requires the disposable fixture program and instance".into(),
        );
    }
    let action = runtime
        .descriptor
        .runtime
        .player_actions
        .iter()
        .find(|action| action.id == "save_world")
        .ok_or("Native manual save requires the module's declared save_world action")?;
    if action.target_required || !action.role_values.is_empty() || action.destructive {
        return Err(
            "Native manual save must be a target-free, non-destructive declared action".into(),
        );
    }
    let expected = generation(running)?;
    require_current(runtime, running, &expected).await?;
    if cycle == "restart" && checkpoint::enabled()? {
        checkpoint::verify(runtime, running).await?;
        require_current(runtime, running, &expected).await?;
        return Ok(());
    }
    if checkpoint::enabled()?
        && matches!(
            cycle,
            "after_restore" | "after_archive_restore" | "after_program_maintenance"
        )
    {
        checkpoint::moria::verify_loaded(runtime, running, cycle).await?;
        checkpoint::terraria::verify_loaded(runtime, running, cycle).await?;
        checkpoint::necesse::verify_loaded(runtime, running, cycle).await?;
        checkpoint::restored_world::verify_loaded(runtime, running, cycle).await?;
        require_current(runtime, running, &expected).await?;
    }
    let windrose = if running.summary.module_id == "windrose" {
        let primary = primary_process(running)?;
        let pid = primary.pid.ok_or("Windrose has no managed bootstrap PID")?;
        let identity = primary
            .process_identity
            .as_ref()
            .ok_or("Windrose has no managed bootstrap identity")?;
        Some(crate::live_players::file_ipc::unreal_save_process::capture(
            crate::live_players::file_ipc::unreal_save_process::GameProfile::Windrose,
            &program,
            pid,
            identity,
        )?)
    } else {
        None
    };
    let dragon_observation = checkpoint::dragon::prepare(runtime, running, cycle).await?;
    let requested_observation = checkpoint::requested::prepare(runtime, running, cycle).await?;
    let moria_observation = checkpoint::moria::prepare(runtime, running, cycle).await?;
    let terraria_observation = checkpoint::terraria::prepare(runtime, running, cycle).await?;
    let necesse_observation = checkpoint::necesse::prepare(runtime, running, cycle).await?;
    let input = serde_json::from_value(json!({
        "instanceId": running.summary.id,
        "command": action.command_template,
        "runtimeActionId": "save_world",
    }))?;
    // Record admission before the side effect. A failure/unknown result must not
    // permit a repeated call for this instance, run and lifecycle cycle.
    admit_once(&running.summary.id, expected.run_id, cycle)?;
    if let Some(observation) = &moria_observation {
        observation.before_save().await?;
    }
    let before = unix_ms()?;
    let result = match crate::commands::send_instance_runtime_command(runtime.state.clone(), input)
        .await
    {
        Ok(result) if !expect_rejection => result,
        Ok(_) => {
            return Err("The no-client fixture unexpectedly accepted a save instead of rejecting before invocation".into());
        }
        Err(error) if expect_rejection => {
            save_guard_probe::verify(runtime, running, &program, cycle, &error, before)?;
            require_current(runtime, running, &expected).await?;
            return Ok(());
        }
        Err(error) => {
            eprintln!(
                "NATIVE_MANUAL_SAVE module={} cycle={} phase=dispatch_failed attempted_once=true retried=false completion=unknown",
                running.summary.module_id,
                safe_cycle(cycle),
            );
            let logs = diagnostics::failed_run_logs(running, &program, &fixture).unwrap_or_else(
                |detail| {
                    eprintln!("NATIVE_DIAGNOSTIC phase=manual_save log_read={detail}");
                    Vec::new()
                },
            );
            runtime::print_native_failure("manual_save", &error, &logs, runtime.settings, &fixture);
            return Err(error.into());
        }
    };
    let after = unix_ms()?;
    let route = validate_receipt(
        running,
        &result,
        action,
        windrose.as_ref().map(|native| native.pid),
        before,
        after,
    )?;
    let completion = match result.save_completion {
        Some(WorldSaveCompletion::Requested) => "requested",
        Some(WorldSaveCompletion::Confirmed) => "confirmed",
        Some(WorldSaveCompletion::Unconfirmed) => "unconfirmed",
        None => {
            return Err("The formal save receipt omitted its native save-completion status".into());
        }
    };
    if std::env::var("LANGAME_NATIVE_SAVE_REQUIRE_COMPLETED").as_deref() == Ok("true")
        && completion != "confirmed"
    {
        // The result text is generated by the fixed transport, not a game log.
        // Preserve its automated verification reason before failing acceptance.
        eprintln!(
            "NATIVE_MANUAL_SAVE module={} cycle={} completion={} pending={} reason={}",
            running.summary.module_id,
            safe_cycle(cycle),
            completion,
            result.write_confirmation_pending,
            result
                .response_text
                .as_deref()
                .unwrap_or("verification reason unavailable")
        );
        return Err("This native acceptance requires actual request-correlated save completion; a sent command is insufficient".into());
    }
    require_current(runtime, running, &expected).await?;
    if let Some(native) = windrose.as_ref() {
        let primary = primary_process(running)?;
        crate::live_players::file_ipc::unreal_save_process::verify(
            crate::live_players::file_ipc::unreal_save_process::GameProfile::Windrose,
            &program,
            primary.pid.ok_or("Windrose lost its bootstrap PID")?,
            primary
                .process_identity
                .as_ref()
                .ok_or("Windrose lost its bootstrap identity")?,
            native,
        )?;
    }
    println!(
        "NATIVE_MANUAL_SAVE module={} cycle={} phase=receipt attempted_once=true retried=false instance_matches=true run_matches=true session_matches=true managed_generations_alive=true route={} response_present={} response_bytes={} write_confirmation_pending={} save_completion={} durable_write=not_assessed restart_readback=not_assessed",
        running.summary.module_id,
        safe_cycle(cycle),
        route,
        result.response_text.is_some(),
        result.response_text.as_ref().map_or(0, String::len),
        result.write_confirmation_pending,
        completion,
    );
    if checkpoint::enabled()? {
        if matches!(
            running.summary.module_id.as_str(),
            "runescapedragonwilds" | "vrising" | "returntomoria" | "terraria" | "necesse"
        ) {
            if result.save_completion != Some(WorldSaveCompletion::Requested) {
                return Err("Reflected native save must retain its Requested receipt".into());
            }
            if let Some(observation) = dragon_observation {
                observation.wait().await?;
                require_current(runtime, running, &expected).await?;
            } else if let Some(observation) = requested_observation {
                observation.wait().await?;
                require_current(runtime, running, &expected).await?;
            } else if let Some(observation) = moria_observation {
                observation.wait().await?;
                require_current(runtime, running, &expected).await?;
            } else if let Some(observation) = terraria_observation {
                observation.wait().await?;
                require_current(runtime, running, &expected).await?;
            } else if let Some(observation) = necesse_observation {
                observation.wait().await?;
                require_current(runtime, running, &expected).await?;
            } else if cycle == "initial" {
                return Err(
                    "Requested save checkpoint has no independent native observation".into(),
                );
            }
            // The initial snapshot owns the exact restart proof. Later lifecycle
            // saves retain their normal Requested receipt without recapturing it.
        } else if result.save_completion != Some(WorldSaveCompletion::Confirmed) {
            return Err("Exact checkpoint acceptance requires native completion evidence before capture; command delivery alone is insufficient".into());
        }
        checkpoint::capture(runtime, running, cycle).await?;
    }
    Ok(())
}

fn safe_cycle(cycle: &str) -> &'static str {
    match cycle {
        "initial" => "initial",
        "restart" => "restart",
        "preflight" => "preflight",
        _ => "other",
    }
}

fn unix_ms() -> Result<u128, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

fn admit_once(instance: &str, run_id: i64, cycle: &str) -> Result<(), &'static str> {
    let mut attempts = ATTEMPTS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map_err(|_| "Native manual-save admission lock is poisoned")?;
    if attempts.len() >= 4096 {
        return Err("Native manual-save admission limit exceeded");
    }
    if !attempts.insert((instance.to_owned(), run_id, cycle.to_owned())) {
        return Err(
            "Native manual save was already attempted for this instance/run/cycle; not retried",
        );
    }
    Ok(())
}

fn generation(details: &InstanceDetails) -> Result<RunGeneration, &'static str> {
    if !matches!(details.summary.status, app_core::InstanceStatus::Running) {
        return Err("Native manual-save instance is not running");
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("Native manual save requires an active run")?;
    generation_from_run(run)
}

fn generation_from_run(run: &ActiveInstanceRun) -> Result<RunGeneration, &'static str> {
    let mut processes = Vec::new();
    let mut pids = HashSet::new();
    let mut run_ids = HashSet::new();
    let mut primary_count = 0;
    for process in run
        .processes
        .iter()
        .filter(|process| process.status == "running")
    {
        let pid = process
            .pid
            .filter(|pid| *pid > 0)
            .ok_or("Native save process has no PID")?;
        // Storage assigns one row/run ID per process. The aggregate run uses
        // its primary row ID; sibling shards share the session, not that ID.
        if process.run_id <= 0
            || process.session_id != run.session_id
            || !run_ids.insert(process.run_id)
            || !pids.insert(pid)
            || processes
                .iter()
                .any(|entry: &ProcessGeneration| entry.key == process.process_key)
        {
            return Err("Native manual-save managed process/run/session is ambiguous");
        }
        if process.is_primary {
            primary_count += 1;
            if process.run_id != run.run_id || process.pid != run.pid {
                return Err("Native manual-save primary does not match the aggregate run");
            }
        }
        processes.push(ProcessGeneration {
            key: process.process_key.clone(),
            run_id: process.run_id,
            pid,
            session: process.session_id.clone(),
            identity: process
                .process_identity
                .clone()
                .ok_or("Native save process has no OS generation identity")?,
        });
    }
    if processes.is_empty() || primary_count != 1 {
        return Err("Native manual save requires exactly one running primary process");
    }
    processes.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(RunGeneration {
        run_id: run.run_id,
        session: run.session_id.clone(),
        processes,
    })
}

async fn require_current<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    expected_details: &InstanceDetails,
    expected: &RunGeneration,
) -> Result<(), Box<dyn std::error::Error>> {
    let current =
        read_instance_details(&runtime.storage.paths, &expected_details.summary.id).await?;
    if current.summary.id != expected_details.summary.id
        || current.summary.module_id != expected_details.summary.module_id
        || generation(&current)? != *expected
    {
        return Err("Native manual-save instance/run/session changed".into());
    }
    for process in &expected.processes {
        let actual = app_runtime::inspect_process_identity(process.pid)?;
        if actual.as_ref() != Some(&process.identity) {
            return Err("Native manual-save managed process exited or changed generation".into());
        }
    }
    Ok(())
}

fn primary_process(details: &InstanceDetails) -> Result<&InstanceProcessState, &'static str> {
    let mut primary = details
        .active_run
        .as_ref()
        .ok_or("Native manual save has no active run")?
        .processes
        .iter()
        .filter(|process| process.is_primary && process.status == "running");
    let first = primary
        .next()
        .ok_or("Native manual save has no running primary process")?;
    if primary.next().is_some() {
        return Err("Native manual save has ambiguous primary processes");
    }
    Ok(first)
}

fn validate_receipt(
    running: &InstanceDetails,
    result: &InstanceRuntimeCommandResult,
    action: &app_core::ModulePlayerActionSpec,
    windrose_pid: Option<u32>,
    before: u128,
    after: u128,
) -> Result<&'static str, &'static str> {
    let expected_command =
        crate::commands::commands_runtime_actions::render_runtime_action_command(
            action, None, None, false,
        )
        .map_err(|_| "Native manual-save action cannot render its declared command")?;
    if result.instance_id != running.summary.id
        || result.command != expected_command
        || !(before..=after).contains(&result.submitted_at_unix_ms)
    {
        return Err("Native manual-save receipt does not match the submitted instance/action/time");
    }
    let run = running
        .active_run
        .as_ref()
        .ok_or("Native manual save has no active run")?;
    let processes = run
        .processes
        .iter()
        .filter(|process| process.status == "running")
        .collect::<Vec<_>>();
    let declared_target = action
        .process_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());
    if let Some(native_pid) = windrose_pid {
        let primary = primary_process(running)?;
        if running.summary.module_id == "windrose"
            && action.transport == "windrose_save"
            && declared_target.is_none_or(|key| key == primary.process_key)
            && result.pid == native_pid
            && result.process_key == primary.process_key
        {
            return Ok("managed_shipping_generation");
        }
        return Err("Windrose manual-save receipt has a different Shipping process");
    }
    if running.summary.module_id == "dontstarve" {
        if action.transport == "stdin"
            && declared_target == Some("master")
            && result.process_key == "master"
            && processes
                .iter()
                .any(|process| process.process_key == "master" && process.pid == Some(result.pid))
        {
            return Ok("native_master_cluster_request");
        }
        return Err("DST save receipt has a different Master process");
    }
    // Existing network/aggregate results deliberately have no PID. Their fixed
    // transport label is checked, while the run/session/OS generation checks
    // above and below dispatch establish the actual managed process boundary.
    if result.pid == 0
        && declared_target.is_none()
        && result.process_key == action.transport
        && matches!(
            action.transport.as_str(),
            "source_rcon"
                | "humanitz_rcon"
                | "palworld_rest"
                | "websocket_rcon"
                | "telnet"
                | "battleye_rcon"
        )
    {
        return Ok("declared_network_transport_current_run");
    }
    let target = match declared_target {
        Some(key) => {
            let mut selected = processes
                .iter()
                .copied()
                .filter(|process| process.process_key == key);
            let first = selected
                .next()
                .ok_or("Declared native save target is not running")?;
            if selected.next().is_some() {
                return Err("Declared native save target is ambiguous");
            }
            first
        }
        None => primary_process(running)?,
    };
    if result.pid > 0 && result.process_key == target.process_key && target.pid == Some(result.pid)
    {
        return Ok("managed_process_generation");
    }
    Err("Native manual-save receipt has an unrelated process or transport")
}

#[test]
fn manual_save_generation_rejects_mixed_run_or_session_and_duplicate_pid() {
    let mut run: ActiveInstanceRun = serde_json::from_value(json!({
        "run_id": 3, "session_id": "session", "pid": 10, "log_path": null, "process_count": 1,
        "processes": [{ "run_id": 3, "session_id": "session", "process_key": "main", "display_name": "fixture", "pid": 10,
            "process_identity": {"creation_time": 20, "image_path": "D:/fixture/server.exe"},
            "status": "running", "started_at": null, "stopped_at": null, "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": true }]
    })).unwrap();
    generation_from_run(&run).unwrap();
    run.processes[0].session_id = Some("other".into());
    assert!(generation_from_run(&run).is_err());
    run.processes[0].session_id = run.session_id.clone();
    run.processes[0].run_id = 4;
    assert!(generation_from_run(&run).is_err());
    run.processes[0].run_id = run.run_id;
    let mut duplicate = run.processes[0].clone();
    duplicate.process_key = "another".into();
    run.processes.push(duplicate);
    assert!(generation_from_run(&run).is_err());
}

#[test]
fn manual_save_generation_tracks_distinct_shard_rows_in_one_session() {
    let mut run: ActiveInstanceRun = serde_json::from_value(json!({
        "run_id": 3, "session_id": "session", "pid": 10, "log_path": null, "process_count": 2,
        "processes": [
            { "run_id": 3, "session_id": "session", "process_key": "master", "display_name": "Master", "pid": 10,
                "process_identity": {"creation_time": 20, "image_path": "D:/fixture/server.exe"},
                "status": "running", "started_at": null, "stopped_at": null, "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": true },
            { "run_id": 4, "session_id": "session", "process_key": "caves", "display_name": "Caves", "pid": 11,
                "process_identity": {"creation_time": 21, "image_path": "D:/fixture/server.exe"},
                "status": "running", "started_at": null, "stopped_at": null, "exit_code": null, "crash_flag": false, "log_path": null, "is_primary": false }
        ]
    })).unwrap();
    let expected = generation_from_run(&run).unwrap();
    run.processes[1].run_id = 5;
    assert_ne!(generation_from_run(&run).unwrap(), expected);
    run.processes[1].run_id = 3;
    assert!(generation_from_run(&run).is_err());
    run.processes[1].run_id = 4;
    run.processes[1].session_id = Some("other-session".into());
    assert!(generation_from_run(&run).is_err());
    run.processes[1].session_id = run.session_id.clone();
    run.processes[1].is_primary = true;
    assert!(generation_from_run(&run).is_err());
    run.processes[1].is_primary = false;
    run.processes[0].is_primary = false;
    assert!(generation_from_run(&run).is_err());
}

#[test]
fn manual_save_admission_never_repeats_an_attempt_in_the_same_cycle() {
    let id = format!("manual-save-test-{}", uuid::Uuid::new_v4());
    admit_once(&id, 3, "initial").unwrap();
    assert!(admit_once(&id, 3, "initial").is_err());
    admit_once(&id, 4, "restart").unwrap();
}

#[test]
fn manual_save_receipt_rejects_wrong_route_generation_and_time() {
    let mut details: InstanceDetails = serde_json::from_value(json!({
        "summary": { "id": "manual-save", "name": "fixture", "module_id": "abioticfactor",
            "status": "Running", "bind_ip": "127.0.0.1", "port_count": 0, "autostart": false },
        "config_file_path": "D:/fixture/config/instance.json", "saves_path": "D:/fixture/worlds",
        "auto_backup_on_stop": false, "backup_retention_count": 0, "settings_json": "{}", "ports": [],
        "active_run": { "run_id": 3, "session_id": "session", "pid": 10, "log_path": null, "process_count": 1,
            "processes": [{ "run_id": 3, "session_id": "session", "process_key": "main", "display_name": "fixture", "pid": 10,
                "process_identity": {"creation_time": 20, "image_path": "D:/fixture/server.exe"},
                "status": "running", "started_at": null, "stopped_at": null, "exit_code": null,
                "crash_flag": false, "log_path": null, "is_primary": true }] }
    })).unwrap();
    let mut receipt = InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: "main".into(),
        display_name: "fixture".into(),
        pid: 10,
        command: "save".into(),
        response_text: Some("invoked; completion pending".into()),
        write_confirmation_pending: true,
        save_completion: Some(WorldSaveCompletion::Unconfirmed),
        submitted_at_unix_ms: 110,
    };
    let action: app_core::ModulePlayerActionSpec = serde_json::from_value(json!({
        "id": "save_world", "label": "save", "transport": "abioticfactor_save", "command_template": "save"
    })).unwrap();
    let check = |result: &InstanceRuntimeCommandResult| {
        validate_receipt(&details, result, &action, None, 100, 120)
    };
    assert_eq!(check(&receipt).unwrap(), "managed_process_generation");
    receipt.pid = 11;
    assert!(check(&receipt).is_err());
    receipt.pid = 10;
    receipt.process_key = "unrelated".into();
    assert!(check(&receipt).is_err());
    receipt.process_key = "main".into();
    receipt.submitted_at_unix_ms = 99;
    assert!(check(&receipt).is_err());
    receipt.submitted_at_unix_ms = 110;
    receipt.instance_id = "other".into();
    assert!(check(&receipt).is_err());
    receipt.instance_id = details.summary.id.clone();
    receipt.pid = 0;
    receipt.process_key = "abioticfactor_save".into();
    assert!(check(&receipt).is_err());
    receipt.process_key = "source_rcon".into();
    let mut network = action.clone();
    network.transport = "source_rcon".into();
    assert_eq!(
        validate_receipt(&details, &receipt, &network, None, 100, 120).unwrap(),
        "declared_network_transport_current_run"
    );
    // Both flags have the same routing guarantee. Neither is a generic claim
    // of durable write completion or future restart/readback.
    receipt.write_confirmation_pending = false;
    assert_eq!(
        validate_receipt(&details, &receipt, &network, None, 100, 120).unwrap(),
        "declared_network_transport_current_run"
    );

    let run = details.active_run.as_mut().unwrap();
    let mut auxiliary = run.processes[0].clone();
    auxiliary.process_key = "auxiliary".into();
    auxiliary.pid = Some(11);
    auxiliary.is_primary = false;
    run.processes.push(auxiliary);
    receipt.pid = 11;
    receipt.process_key = "auxiliary".into();
    // A valid PID in this same run is insufficient: the undeclared default is
    // the primary process, whereas an explicit action may select its auxiliary.
    assert!(validate_receipt(&details, &receipt, &action, None, 100, 120).is_err());
    let mut targeted = action.clone();
    targeted.process_key = Some("main".into());
    assert!(validate_receipt(&details, &receipt, &targeted, None, 100, 120).is_err());
    targeted.process_key = Some("auxiliary".into());
    assert_eq!(
        validate_receipt(&details, &receipt, &targeted, None, 100, 120).unwrap(),
        "managed_process_generation"
    );
    targeted.process_key = Some("missing".into());
    assert!(validate_receipt(&details, &receipt, &targeted, None, 100, 120).is_err());
    network.process_key = Some("main".into());
    receipt.pid = 0;
    receipt.process_key = "source_rcon".into();
    assert!(validate_receipt(&details, &receipt, &network, None, 100, 120).is_err());

    details.summary.module_id = "arksurvivalevolved".into();
    network.process_key = None;
    assert!(validate_receipt(&details, &receipt, &network, None, 100, 120).is_ok());
    details.summary.module_id = "palworld".into();
    let mut rest_save = action.clone();
    rest_save.transport = "palworld_rest".into();
    receipt.process_key = "palworld_rest".into();
    receipt.command = "{\"operation\":\"save\"}".into();
    assert!(validate_receipt(&details, &receipt, &rest_save, None, 100, 120).is_ok());
    for wrong in [
        "save",
        "{\"operation\":\"shutdown\"}",
        "{\"operation\":\"save\",\"unknown\":true}",
    ] {
        receipt.command = wrong.into();
        assert!(validate_receipt(&details, &receipt, &rest_save, None, 100, 120).is_err());
    }
    receipt.command = "save".into();
    details.summary.module_id = "windrose".into();
    let mut shipping = action.clone();
    shipping.transport = "windrose_save".into();
    receipt.process_key = "main".into();
    receipt.pid = 22;
    assert_eq!(
        validate_receipt(&details, &receipt, &shipping, Some(22), 100, 120).unwrap(),
        "managed_shipping_generation"
    );
    shipping.process_key = Some("auxiliary".into());
    assert!(validate_receipt(&details, &receipt, &shipping, Some(22), 100, 120).is_err());
    details.summary.module_id = "dontstarve".into();
    details.active_run.as_mut().unwrap().processes[0].process_key = "master".into();
    let mut shards = action.clone();
    shards.transport = "stdin".into();
    shards.process_key = Some("master".into());
    receipt.process_key = "master".into();
    receipt.pid = 10;
    assert_eq!(
        validate_receipt(&details, &receipt, &shards, None, 100, 120).unwrap(),
        "native_master_cluster_request"
    );
    receipt.process_key = "all".into();
    assert!(validate_receipt(&details, &receipt, &shards, None, 100, 120).is_err());
}
