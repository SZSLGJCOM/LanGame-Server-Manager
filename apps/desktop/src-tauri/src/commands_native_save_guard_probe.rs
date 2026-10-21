//! Opt-in rejection acceptance for the two games without local player clients.
//! A rejected preflight is guard evidence, never a save receipt.
use super::*;
use crate::commands::commands_managed_save_control::{guard_pin_directories, guard_read_regular};
use serde::Deserialize;

const RIMWORLD_ERROR: &str = "RimWorld Together has no connected clients to request a save from.";
const ABIOTIC_ERROR: &str =
    "The current world, native signature, or save queue is not ready. No manual save was called.";

fn configured(module: &str, value: Option<&str>) -> Result<bool, String> {
    match value {
        None | Some("false") => Ok(false),
        Some("true") if matches!(module, "rimworld" | "abioticfactor") => Ok(true),
        _ => Err("The no-client save guard requires an explicit true flag and RimWorld or Abiotic Factor.".into()),
    }
}

pub(super) fn expected(module: &str) -> Result<bool, String> {
    let value = std::env::var("LANGAME_NATIVE_SAVE_NOT_READY")
        .map(Some)
        .or_else(|error| match error {
            std::env::VarError::NotPresent => Ok(None),
            _ => Err("The no-client save guard flag is invalid.".to_owned()),
        })?;
    configured(module, value.as_deref())
}

fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

fn frame<'a>(body: &'a [u8], header: &str, keys: &[&str]) -> Result<Vec<&'a str>, String> {
    let text = std::str::from_utf8(body).map_err(|_| "The guard frame is not UTF-8.")?;
    if body.len() > 2048
        || body
            .iter()
            .any(|byte| *byte != b'\n' && !(32..=126).contains(byte))
    {
        return Err("The guard frame is not bounded ASCII.".into());
    }
    let mut lines = text
        .strip_suffix('\n')
        .ok_or("The guard frame is incomplete.")?
        .split('\n');
    if lines.next() != Some(header) {
        return Err("The guard frame header is invalid.".into());
    }
    let values = keys
        .iter()
        .map(|key| {
            lines
                .next()
                .and_then(|line| {
                    line.strip_prefix(key)
                        .and_then(|tail| tail.strip_prefix('='))
                })
                .ok_or_else(|| "The guard frame fields are invalid.".to_owned())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if lines.next().is_some() {
        return Err("The guard frame has extra fields.".into());
    }
    Ok(values)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeRequest {
    protocol: u32,
    request_id: String,
    requested_at: u64,
    action: String,
    boot_id: String,
    world_token: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeResponse {
    protocol: u32,
    request_id: String,
    timestamp: u64,
    action: String,
    boot_id: String,
    world_token: String,
    ready: bool,
    invoked: bool,
    call_returned: bool,
    write_confirmed: bool,
    launch_token: String,
    error: Option<String>,
}

fn abiotic_rejected(
    session: &[u8],
    request: &[u8],
    response: &[u8],
    earliest: u128,
    latest: u128,
) -> Result<(), String> {
    let session = frame(session, "LGSM_UNREAL_SAVE_1", &["module", "run"])?;
    let request: ProbeRequest =
        serde_json::from_slice(request).map_err(|_| "The guard probe request is invalid.")?;
    let response: ProbeResponse =
        serde_json::from_slice(response).map_err(|_| "The guard probe response is invalid.")?;
    let launch = session[1]
        .strip_prefix("LGSM_UWS_")
        .filter(|value| nonce(value));
    let requested_at = u128::from(request.requested_at);
    let timestamp = u128::from(response.timestamp);
    if latest < earliest
        || session[0] != "abioticfactor"
        || launch.is_none()
        || request.protocol != 1
        || !nonce(&request.request_id)
        || request.action != "saveprobe"
        || !request.boot_id.is_empty()
        || !request.world_token.is_empty()
        || !(earliest / 1000..=latest / 1000).contains(&requested_at)
        || response.protocol != 1
        || response.request_id != request.request_id
        || response.action != "saveprobe"
        || !nonce(&response.boot_id)
        || !response.world_token.is_empty()
        || response.launch_token != session[1]
        || !(requested_at..=requested_at + 6).contains(&timestamp)
        || timestamp > latest / 1000
        || response.ready
        || response.invoked
        || response.call_returned
        || response.write_confirmed
        || response.error.as_deref() != Some("world_unavailable")
    {
        return Err(
            "Abiotic Factor did not return this launch's current inert readiness probe.".into(),
        );
    }
    // The formal adapter returns before invoke_once when preflight fails. The
    // still-current published action must be saveprobe, never a save frame.
    Ok(())
}

pub(super) fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    running: &InstanceDetails,
    program: &Path,
    cycle: &str,
    error: &str,
    earliest_unix_ms: u128,
) -> Result<(), Box<dyn std::error::Error>> {
    let module = running.summary.module_id.as_str();
    if !expected(module)?
        || !matches!(cycle, "initial" | "restart")
        || runtime.descriptor.summary.id != module
    {
        return Err(
            "The no-client guard was not explicitly admitted for this lifecycle cycle.".into(),
        );
    }
    let fixture = runtime.package.root.canonicalize()?;
    let program = program.canonicalize()?;
    if !program.starts_with(&fixture)
        || !Path::new(&running.config_file_path)
            .canonicalize()?
            .starts_with(&fixture)
    {
        return Err("The no-client guard requires the disposable instance and program.".into());
    }
    let run = running
        .active_run
        .as_ref()
        .ok_or("The guard has no current run.")?;
    let mut primary = run
        .processes
        .iter()
        .filter(|process| process.is_primary && process.status == "running");
    let process = primary
        .next()
        .ok_or("The guard has no running primary process.")?;
    let pid = process
        .pid
        .filter(|pid| *pid > 0)
        .ok_or("The guard has no current PID.")?;
    let identity = process
        .process_identity
        .as_ref()
        .ok_or("The guard has no process generation.")?;
    if primary.next().is_some()
        || run.pid != Some(pid)
        || run.run_id != process.run_id
        || run.session_id != process.session_id
        || identity.creation_time == 0
        || !app_runtime::process_matches_identity(pid, identity)?
    {
        return Err("The guard process generation changed or is ambiguous.".into());
    }
    let _pins = guard_pin_directories(&program)?;
    if module == "rimworld" {
        // The native adapter queries the complete connected-client table and
        // rejects zero clients before sending any forcesave. No retired hook
        // session/request/response files are part of this path.
        if error != RIMWORLD_ERROR {
            return Err("RimWorld did not return the expected no-client rejection.".into());
        }
    } else {
        if error != ABIOTIC_ERROR {
            return Err("Abiotic Factor did not return the expected readiness rejection.".into());
        }
        let directory = program.join("langame_world_save");
        let _exchange_pin = guard_pin_directories(&directory)?;
        let session = guard_read_regular(&directory.join("session.txt"), 2048)?;
        let request = guard_read_regular(&directory.join("request.json"), 1024)?;
        let response = guard_read_regular(&directory.join("response.json"), 4096)?;
        let latest = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        abiotic_rejected(&session, &request, &response, earliest_unix_ms, latest)?;
    }
    if !app_runtime::process_matches_identity(pid, identity)? {
        return Err("The guard process changed while its rejection was checked.".into());
    }
    println!(
        "NATIVE_SAVE_GUARD module={module} cycle={cycle} phase=rejected attempted_once=true retried=false rejected_before_invoke=true formal_receipt=false"
    );
    Ok(())
}

#[test]
fn native_no_client_guard_flag_is_closed_and_explicit() {
    for module in ["rimworld", "abioticfactor"] {
        assert!(configured(module, Some("true")).unwrap());
        assert!(!configured(module, None).unwrap());
        assert!(!configured(module, Some("false")).unwrap());
        for value in ["", "TRUE", "1", " false"] {
            assert!(configured(module, Some(value)).is_err());
        }
    }
    assert!(configured("returntomoria", Some("true")).is_err());
}

#[test]
fn native_no_client_guard_abiotic_accepts_only_fresh_inert_probe_for_this_launch() {
    let id = "a".repeat(32);
    let boot = "b".repeat(32);
    let token = format!("LGSM_UWS_{}", "c".repeat(32));
    let session = format!("LGSM_UNREAL_SAVE_1\nmodule=abioticfactor\nrun={token}\n");
    let request = json!({"protocol":1,"request_id":id,"requested_at":100,"action":"saveprobe","boot_id":"","world_token":""});
    let response = json!({"protocol":1,"request_id":id,"timestamp":100,"action":"saveprobe","boot_id":boot,
        "world_token":"","ready":false,"invoked":false,"call_returned":false,"write_confirmed":false,"launch_token":token,"error":"world_unavailable"});
    let check = |s: &str, q: &Value, r: &Value| {
        abiotic_rejected(
            s.as_bytes(),
            &serde_json::to_vec(q).unwrap(),
            &serde_json::to_vec(r).unwrap(),
            100000,
            100900,
        )
    };
    check(&session, &request, &response).unwrap();
    for (key, value) in [
        ("ready", json!(true)),
        ("invoked", json!(true)),
        ("call_returned", json!(true)),
        ("write_confirmed", json!(true)),
        ("action", json!("save")),
        ("request_id", json!("d".repeat(32))),
        (
            "launch_token",
            json!(format!("LGSM_UWS_{}", "d".repeat(32))),
        ),
        ("world_token", json!(boot)),
        ("error", json!("api_unavailable")),
        ("timestamp", json!(99)),
        ("timestamp", json!(101)),
        ("extra", json!(true)),
    ] {
        let mut changed = response.clone();
        changed[key] = value;
        assert!(check(&session, &request, &changed).is_err(), "{key}");
    }
    for (key, value) in [
        ("requested_at", json!(99)),
        ("requested_at", json!(101)),
        ("action", json!("save")),
        ("boot_id", json!(boot)),
    ] {
        let mut changed = request.clone();
        changed[key] = value;
        assert!(check(&session, &changed, &response).is_err(), "{key}");
    }
    let mut missing = response.clone();
    missing.as_object_mut().unwrap().remove("invoked");
    assert!(check(&session, &request, &missing).is_err());
    assert!(abiotic_rejected(session.as_bytes(), b"{}", b"{}", 100000, 100900).is_err());
}
