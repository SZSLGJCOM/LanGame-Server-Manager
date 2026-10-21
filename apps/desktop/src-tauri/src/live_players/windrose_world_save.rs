//! Fixed-build Windrose checkpoint bridge. Only the native bool true receipt
//! plus unchanged world ownership confirms this request; unknown is never retried.
use super::transport;
use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use crate::state::{DesktopState, StorageContextTaskLease, spawn_blocking_storage_context_task};
use app_core::{InstanceDetails, InstanceStatus, ProcessIdentity};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::OwnedMutexGuard;
#[path = "windrose_save_installation.rs"]
mod installation;
use super::unreal_save_process as process;
const PROFILE: process::GameProfile = process::GameProfile::Windrose;
const DLL: &[u8] = include_bytes!(concat!(
    env!("OUT_DIR"),
    "/windrose-save-control/LgsmWindroseSaveControl.dll"
));
fn payload() -> Vec<u8> {
    [
        include_bytes!("../../../../../modules/windrose/control/LgsmWorldSave/Scripts/main.lua")
            .as_slice(),
        b"\n",
        include_bytes!("unreal_save_protocol.lua"),
    ]
    .concat()
}
fn check_module(details: &InstanceDetails) -> Result<(), String> {
    if details.summary.module_id != "windrose" {
        return Err("This save bridge only supports Windrose.".into());
    }
    Ok(())
}
fn now() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .map_err(|_| "The system clock is invalid.".into())
}
fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}
fn instance_root(details: &InstanceDetails) -> Result<&Path, String> {
    Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("The instance configuration has no private runtime root.".into())
}
fn runtime_root(details: &InstanceDetails) -> Result<PathBuf, String> {
    app_storage::resolve_instance_runtime_root(instance_root(details)?)
        .map_err(|error| error.to_string())
}
fn root(details: &InstanceDetails) -> Result<PathBuf, String> {
    if app_storage::instance_program_mode(instance_root(details)?)
        .map_err(|error| error.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("Windrose saving requires this instance's exclusive program directory.".into());
    }
    fs::canonicalize(runtime_root(details)?).map_err(|_| {
        "The instance program directory is unavailable for automatic save preparation.".into()
    })
}
struct Context {
    root: PathBuf,
    pid: u32,
    identity: ProcessIdentity,
    process_key: String,
    native: process::NativeProcess,
}
fn context(details: &InstanceDetails) -> Result<Context, String> {
    check_module(details)?;
    if !matches!(details.summary.status, InstanceStatus::Running)
        || details.summary.active_process_count != 1
    {
        return Err("Native world saving requires one current managed running process.".into());
    }
    let run = details
        .active_run
        .as_ref()
        .ok_or("The managed run is unavailable.")?;
    if run.process_count != 1 || run.processes.len() != 1 {
        return Err("The managed run is ambiguous.".into());
    }
    let process = &run.processes[0];
    let pid = process
        .pid
        .filter(|pid| *pid > 0)
        .ok_or("The managed PID is unavailable.")?;
    let identity = process
        .process_identity
        .clone()
        .filter(|id| id.creation_time > 0)
        .ok_or("The managed process identity is unavailable.")?;
    if !process.is_primary
        || process.status != "running"
        || process.exit_code.is_some()
        || process.crash_flag
        || process.run_id != run.run_id
        || process.session_id != run.session_id
        || run.pid != Some(pid)
    {
        return Err("The current native process context is invalid.".into());
    }
    let root = root(details)?;
    let image = fs::canonicalize(&identity.image_path)
        .map_err(|_| "The current executable is unavailable.")?;
    let shipping = root.join("R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe");
    let bootstrap = root.join("WindroseServer.exe");
    if image != fs::canonicalize(shipping).map_err(|_| "The fixed native executable is missing.")?
        && !fs::canonicalize(bootstrap).is_ok_and(|path| path == image)
    {
        return Err("The managed image does not belong to this private game runtime.".into());
    }
    let native = process::capture(PROFILE, &root, pid, &identity)?;
    Ok(Context {
        native,
        root,
        pid,
        identity,
        process_key: process.process_key.clone(),
    })
}

#[derive(Serialize)]
struct Request<'a> {
    protocol: u32,
    request_id: &'a str,
    requested_at: u64,
    action: &'a str,
    boot_id: &'a str,
    world_token: &'a str,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
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
    error: Option<String>,
    native_process_id: Option<u32>,
    native_creation_time: Option<String>,
}
fn parse(body: &[u8], request: &Request<'_>) -> Result<Reply, String> {
    let reply: Reply =
        serde_json::from_slice(body).map_err(|_| "The native save receipt is invalid.")?;
    if reply.protocol != 1
        || reply.request_id != request.request_id
        || reply.action != request.action
        || !nonce(&reply.boot_id)
        || reply.timestamp < request.requested_at
        || reply.timestamp > request.requested_at.saturating_add(6)
        || (reply.write_confirmed
            && (!reply.ready || !reply.invoked || !reply.call_returned || reply.error.is_some()))
        || (request.action == "save"
            && (reply.boot_id != request.boot_id || reply.world_token != request.world_token))
    {
        return Err("The native save receipt does not match this request.".into());
    }
    Ok(reply)
}
fn exchange(context: &Context, request: &Request<'_>) -> Result<Vec<u8>, String> {
    let body = serde_json::to_vec(request).map_err(|_| "Cannot encode the native save request.")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let directory = context.root.join("langame_world_save");
    let _pin = transport::open_directory(&context.root, &directory)
        .map_err(|_| "The prepared native exchange is unavailable.")?;
    process::verify(
        PROFILE,
        &context.root,
        context.pid,
        &context.identity,
        &context.native,
    )?;
    transport::publish_body(
        &context.root,
        &directory,
        request.request_id,
        &body,
        deadline,
    )
    .map_err(|_| "Native save publication failed or is uncertain; it was not retried.")?;
    loop {
        process::verify(
            PROFILE,
            &context.root,
            context.pid,
            &context.identity,
            &context.native,
        )?;
        if Instant::now() >= deadline {
            return Err("Native save receipt timed out; the request was not retried.".into());
        }
        if let Some(body) = transport::response_for_request(
            &context.root,
            &directory.join("response.json"),
            request.request_id,
        )
        .map_err(|_| "The native save receipt could not be read.")?
        {
            process::verify(
                PROFILE,
                &context.root,
                context.pid,
                &context.identity,
                &context.native,
            )?;
            return Ok(body);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
fn preflight(context: &Context) -> Result<Reply, String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &id,
        requested_at: now()?,
        action: "saveprobe",
        boot_id: "",
        world_token: "",
    };
    let reply = parse(&exchange(context, &request)?, &request)?;
    receipt_owner(context, &reply)?;
    if !reply.ready
        || !nonce(&reply.world_token)
        || reply.invoked
        || reply.call_returned
        || reply.write_confirmed
        || reply.error.is_some()
    {
        return Err("The current world, native signature, or save queue is not ready. No manual save was called.".into());
    }
    Ok(reply)
}
fn invoke_once(context: &Context, ready: &Reply) -> Result<(bool, &'static str), String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &id,
        requested_at: now()?,
        action: "save",
        boot_id: &ready.boot_id,
        world_token: &ready.world_token,
    };
    classify(exchange(context, &request).and_then(|body| {
        let reply = parse(&body, &request)?;
        receipt_owner(context, &reply)?;
        Ok(reply)
    }))
}
fn receipt_owner(context: &Context, reply: &Reply) -> Result<(), String> {
    if reply.ready
        && (reply.native_process_id != Some(context.native.pid)
            || reply.native_creation_time.as_deref()
                != Some(context.native.identity.creation_time.to_string().as_str()))
    {
        return Err(
            "The native save receipt does not belong to the current Shipping process generation."
                .into(),
        );
    }
    Ok(())
}
fn classify(reply: Result<Reply, String>) -> Result<(bool, &'static str), String> {
    match reply {
        Ok(reply)
            if reply.ready
                && reply.invoked
                && reply.call_returned
                && reply.write_confirmed
                && reply.error.is_none() =>
        {
            Ok((
                true,
                "The native current-world checkpoint completed successfully. Windrose returned true for this single SaveBackupsSync call.",
            ))
        }
        Ok(reply)
            if !reply.ready
                && !reply.invoked
                && !reply.call_returned
                && !reply.write_confirmed
                && matches!(
                    reply.error.as_deref(),
                    Some(
                        "api_unavailable"
                            | "world_unavailable"
                            | "world_ambiguous"
                            | "save_busy"
                            | "target_changed"
                            | "request_expired"
                            | "dispatcher_timeout"
                            | "dispatcher_unavailable"
                    )
                ) =>
        {
            Err("The native save was rejected before invocation. No manual save was called.".into())
        }
        _ => Ok((
            false,
            "The native save was attempted once, but its checkpoint result is unknown. Check the native log and current save before another request; it was not retried.",
        )),
    }
}
fn stopped(details: &InstanceDetails) -> Result<(), String> {
    if !matches!(
        details.summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) || details.summary.active_process_count != 0
        || details.active_run.as_ref().is_some_and(|run| {
            run.process_count != 0
                || run
                    .processes
                    .iter()
                    .any(|process| process.status == "running")
        })
    {
        return Err("Stop Windrose before preparing its optional native save extension.".into());
    }
    Ok(())
}
fn owned_worker<L: StorageContextTaskLease, T: Send + 'static>(
    lease: &L,
    mutation: OwnedMutexGuard<()>,
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    spawn_blocking_storage_context_task(lease, move || {
        let _mutation = mutation;
        operation()
    })
}
pub(crate) fn validate_preparation(details: &InstanceDetails) -> Result<PathBuf, String> {
    check_module(details)?;
    stopped(details)?;
    let root = root(details)?;
    process::stopped(PROFILE, &root)?;
    Ok(root)
}
pub(crate) fn ensure_prepared(details: &InstanceDetails) -> Result<(), String> {
    installation::install(&validate_preparation(details)?, &payload(), DLL)
}
pub(crate) async fn save_world(
    state: &DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let mutation = mutation.ok_or("Native saving requires the instance mutation reservation.")?;
    let lease = state.begin_storage_context_operation("Windrose immediate native checkpoint")?;
    let details = details.clone();
    owned_worker(&lease, mutation, move || {
        let context = context(&details)?;
        let _pins = installation::validate(&context.root)?;
        if !installation::complete(&context.root, &payload(), DLL)? {
            return Err("The owned Windrose save extension needs preparation while the instance is stopped.".into());
        }
        let ready = preflight(&context)?;
        let (confirmed, message) = invoke_once(&context, &ready)?;
        Ok(InstanceRuntimeCommandResult { instance_id: details.summary.id.clone(), process_key: context.process_key,
            display_name: "Windrose native world checkpoint".into(), pid: context.native.pid, command: "save".into(),
            response_text: Some(message.into()), write_confirmation_pending: !confirmed,
            save_completion: Some(if confirmed { WorldSaveCompletion::Confirmed } else { WorldSaveCompletion::Unconfirmed }),
            submitted_at_unix_ms })
    }).await.map_err(|_| "Windrose save worker failed; invocation or completion may be unknown. The request was not retried.".to_owned())?
}
#[cfg(test)]
#[path = "windrose_world_save_tests.rs"]
mod tests;
