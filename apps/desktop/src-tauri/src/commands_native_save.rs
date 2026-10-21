//! Native administrative save requests. These adapters report submission only.
use super::commands_stdin_dispatch::{
    RuntimeStdinDispatchConfirmation, dispatch_managed_stdin_command,
};
use super::*;
use crate::astroneer_console::{Query, startup as console};
use crate::live_players::file_ipc::unreal_save_process::{self as process, GameProfile};

fn selected_world<'a>(games: &'a Value, statistics: &Value) -> Result<&'a str, String> {
    let active = games
        .get("activeSaveName")
        .and_then(Value::as_str)
        .filter(|name| {
            !name.trim().is_empty()
                && name.chars().count() <= 256
                && !name.chars().any(char::is_control)
        })
        .ok_or("ASTRONEER has no selected current world; no save was sent.")?;
    if !games.get("gameList").is_some_and(Value::is_array)
        || statistics.get("saveGameName").and_then(Value::as_str) != Some(active)
    {
        return Err("ASTRONEER current-world queries disagree; no save was sent.".into());
    }
    Ok(active)
}

pub(super) async fn astroneer(
    state: &tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let _mutation = mutation.ok_or("Saving requires the instance operation permit.")?;
    if details.summary.module_id != "astroneer" {
        return Err("ASTRONEER save requires its declared module.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    let primary = run
        .processes
        .iter()
        .find(|process| {
            process.is_primary
                && process.status == "running"
                && process.pid.is_some_and(|pid| pid > 0)
        })
        .ok_or("The current managed process is unavailable.")?;
    let pid = primary
        .pid
        .ok_or("The current managed PID is unavailable.")?;
    let identity = primary
        .process_identity
        .as_ref()
        .ok_or("The managed process identity is unavailable.")?;
    ensure_current(state, details, &primary.process_key, pid)?;
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance runtime directory is unavailable.")?;
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let native = process::capture(GameProfile::Astroneer, &root, pid, identity)?;
    let settings: Value = serde_json::from_str(&details.settings_json)
        .map_err(|_| "ASTRONEER settings are invalid.")?;
    let password = settings
        .get("console_password")
        .and_then(Value::as_str)
        .filter(|password| {
            !password.trim().is_empty()
                && password.chars().count() <= 128
                && !password.chars().any(char::is_control)
        })
        .ok_or("Set a valid ASTRONEER console password before saving.")?;
    let mut ports = details.ports.iter().filter(|port| port.name == "console");
    let port = ports.next().ok_or("ASTRONEER has no console port.")?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
        return Err("ASTRONEER console port is invalid or ambiguous.".into());
    }
    let games = console::query(
        port.port,
        password,
        Query::Games,
        native.pid,
        &native.identity,
    )
    .await?;
    let statistics = console::query(
        port.port,
        password,
        Query::Statistics,
        native.pid,
        &native.identity,
    )
    .await?;
    let world = selected_world(&games, &statistics)?;
    console::save_once(
        port.port,
        password,
        world,
        native.pid,
        &native.identity,
        || {
            ensure_current(state, details, &primary.process_key, pid)?;
            process::verify(GameProfile::Astroneer, &root, pid, identity, &native)
        },
    )
    .await?;
    Ok(requested(
        details,
        &primary.process_key,
        pid,
        submitted_at_unix_ms,
        "ASTRONEER native DSSaveGame request submitted; storage completion is not verified.".into(),
    ))
}

fn ensure_current(
    state: &DesktopState,
    details: &InstanceDetails,
    process_key: &str,
    pid: u32,
) -> Result<(), String> {
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    let mut runtime = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?;
    if !runtime
        .matches_running_process(&details.summary.id, run.run_id, process_key, pid)
        .map_err(|error| error.to_string())?
    {
        return Err("The managed server generation changed; the save was not retried.".into());
    }
    Ok(())
}

fn requested(
    details: &InstanceDetails,
    process_key: &str,
    pid: u32,
    submitted_at_unix_ms: u128,
    response: String,
) -> InstanceRuntimeCommandResult {
    InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: process_key.into(),
        display_name: format!("{} native save request", details.summary.module_id),
        pid,
        // Keep the declared logical action in receipts. Protocol-specific
        // commands belong to their transport and must not change this contract.
        command: "save".into(),
        response_text: Some(response),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Requested),
        submitted_at_unix_ms,
    }
}

async fn stdin_once(
    state: &DesktopState,
    details: &InstanceDetails,
    process_key: &str,
    command: &str,
) -> Result<(), String> {
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    let receipt = dispatch_managed_stdin_command(
        state,
        &details.summary.id,
        Some(process_key),
        command,
        Some(run.run_id),
    )
    .await?;
    if receipt.confirmation != RuntimeStdinDispatchConfirmation::Confirmed {
        return Err("RimWorld Together command delivery is unknown; it was not retried.".into());
    }
    Ok(())
}

pub(super) async fn rimworld(
    state: &tauri::State<'_, DesktopState>,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let _mutation = mutation.ok_or("Saving requires the instance operation permit.")?;
    if details.summary.module_id != "rimworld" {
        return Err("RimWorld Together save requires its declared module.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    let primary = run
        .processes
        .iter()
        .find(|process| {
            process.is_primary
                && process.status == "running"
                && process.pid.is_some_and(|pid| pid > 0)
        })
        .ok_or("The current managed process is unavailable.")?;
    let pid = primary
        .pid
        .ok_or("The current managed PID is unavailable.")?;
    ensure_current(state, details, &primary.process_key, pid)?;
    let path = Path::new(
        primary
            .log_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .ok_or("RimWorld Together has no readable managed console log.")?,
    );
    let online = crate::live_players::console_log::rimworld::connected_count(path, || {
        stdin_once(state, details, &primary.process_key, "list")
    })
    .await?;
    if online == 0 {
        return Err("RimWorld Together has no connected clients to request a save from.".into());
    }
    let names = crate::live_players::console_log::rimworld::usernames(path, || {
        stdin_once(state, details, &primary.process_key, "deeplist")
    })
    .await?;
    if names.is_empty() {
        return Err(
            "RimWorld Together returned no registered client usernames; no save was sent.".into(),
        );
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    for (index, name) in names.iter().enumerate() {
        ensure_current(state, details, &primary.process_key, pid)?;
        let command = format!("forcesave {name}");
        tokio::time::timeout_at(deadline, stdin_once(state, details, &primary.process_key, &command))
            .await
            .map_err(|_| format!("RimWorld Together save request deadline expired after {index} submitted requests; any in-flight request was not retried."))?
            .map_err(|error| {
                format!("RimWorld Together save delivery became unknown after {index} submitted requests; no request was retried: {error}")
            })?;
    }
    Ok(requested(details, &primary.process_key, pid, submitted_at_unix_ms,
        "Native forcesave requests submitted for registered usernames; the server skips offline clients. Client uploads and storage completion are not verified.".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_protocol_requests_keep_the_declared_logical_save_receipt() {
        for (module, native) in [("astroneer", "DSSaveGame"), ("rimworld", "forcesave")] {
            let details: InstanceDetails = serde_json::from_value(json!({
                "summary":{"id":"native-save","name":"Native save","module_id":module,
                    "status":"Running","bind_ip":"127.0.0.1","port_count":0,"autostart":false},
                "config_file_path":"D:/fixture/config/instance.json",
                "saves_path":"D:/fixture/data/saves", "auto_backup_on_stop":false,
                "backup_retention_count":0, "settings_json":"{}", "ports":[], "active_run":null
            }))
            .unwrap();
            let response = format!("Native {native} request submitted.");
            let receipt = requested(&details, "main", 42, 123, response.clone());
            assert_eq!(receipt.command, "save");
            assert_eq!(receipt.response_text.as_deref(), Some(response.as_str()));
            assert_eq!(receipt.instance_id, details.summary.id);
            assert_eq!((receipt.pid, receipt.submitted_at_unix_ms), (42, 123));
            assert!(matches!(
                receipt.save_completion,
                Some(WorldSaveCompletion::Requested)
            ));
            assert!(!receipt.write_confirmation_pending);
        }
    }

    #[test]
    fn astroneer_requires_agreeing_nonempty_current_world_queries() {
        let games = json!({"activeSaveName":"SAVE_1","gameList":[]});
        assert_eq!(
            selected_world(&games, &json!({"saveGameName":"SAVE_1"})).unwrap(),
            "SAVE_1"
        );
        for bad in [
            json!({}),
            json!({"saveGameName":"OTHER"}),
            json!({"saveGameName":""}),
        ] {
            assert!(selected_world(&games, &bad).is_err());
        }
        assert!(
            selected_world(
                &json!({"activeSaveName":"","gameList":[]}),
                &json!({"saveGameName":""})
            )
            .is_err()
        );
    }
}
