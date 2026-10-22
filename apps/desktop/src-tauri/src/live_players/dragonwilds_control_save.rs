//! Submit the reflected native save once; a returned call is not disk confirmation.
use super::super::file_ipc::unreal_save_process::{self as native_process, GameProfile};
use super::super::file_ipc::{Game, transport};
use super::{context, installation, is_prepared, nonce, now};
use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use crate::state::{DesktopState, StorageContextTaskLease, spawn_blocking_storage_context_task};
use app_core::InstanceDetails;
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedMutexGuard;

const UNKNOWN: &str = "Dragonwilds did not acknowledge the native save request. Its outcome is unknown; the save was not resent.";
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
    run: String,
    error: Option<String>,
}
struct Reference<'a> {
    run: &'a str,
}
fn known_issue(value: &str) -> bool {
    matches!(
        value,
        "save_busy"
            | "dispatcher_unavailable"
            | "dispatcher_timeout"
            | "api_unavailable"
            | "world_unavailable"
            | "world_ambiguous"
            | "request_expired"
            | "request_limit"
            | "target_changed"
            | "collection_failed"
            | "save_call_unknown"
    )
}
fn parse(body: &[u8], request: &Request<'_>, reference: &Reference<'_>) -> Result<Reply, ()> {
    if body.len() > 2048 || !body.is_ascii() || !nonce(request.request_id) || !nonce(reference.run)
    {
        return Err(());
    }
    let reply: Reply = serde_json::from_slice(body).map_err(|_| ())?;
    if reply.protocol != 1
        || reply.request_id != request.request_id
        || reply.action != request.action
        || reply.run != reference.run
        || !nonce(&reply.boot_id)
        || reply.timestamp < request.requested_at
        || reply.timestamp > request.requested_at.saturating_add(6)
        || reply.write_confirmed
        || reply
            .error
            .as_deref()
            .is_some_and(|value| !known_issue(value))
        || (request.action != "saveprobe"
            && (reply.boot_id != request.boot_id || reply.world_token != request.world_token))
    {
        return Err(());
    }
    let valid = if let Some(error) = reply.error.as_deref() {
        !reply.call_returned
            && reply.ready == reply.invoked
            && (reply.invoked == (error == "save_call_unknown"))
            && (!reply.invoked || (request.action == "save" && nonce(&reply.world_token)))
    } else {
        reply.ready
            && nonce(&reply.world_token)
            && match request.action {
                "saveprobe" => !reply.invoked && !reply.call_returned,
                "save" => reply.invoked && reply.call_returned,
                _ => false,
            }
    };
    if valid { Ok(reply) } else { Err(()) }
}
fn probe(body: &[u8], request: &Request<'_>, reference: &Reference<'_>) -> Result<Reply, String> {
    let reply = parse(body, request, reference)
        .map_err(|_| "Dragonwilds returned an invalid save preflight; no save was requested.")?;
    if let Some(error) = &reply.error {
        return Err(format!(
            "Dragonwilds save preflight rejected: {error}. No manual save was called."
        ));
    }
    Ok(reply)
}
/// The authenticated transport verifies the current process before and after
/// publication. A missing or ambiguous acknowledgment must never resend Save.
fn dispatch_once(
    request: &Request<'_>,
    reference: &Reference<'_>,
    mut exchange: impl FnMut(&Request<'_>) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let body = exchange(request).map_err(|_| UNKNOWN.to_owned())?;
    let reply = parse(&body, request, reference).map_err(|_| UNKNOWN.to_owned())?;
    if let Some(error) = reply.error {
        return Err(if reply.invoked {
            UNKNOWN.to_owned()
        } else {
            format!(
                "Dragonwilds rejected the save request: {error}. No manual save was called; the save was not resent."
            )
        });
    }
    Ok(())
}
fn save_owned(
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
) -> Result<InstanceRuntimeCommandResult, String> {
    if !is_prepared(details)? {
        return Err("The current Dragonwilds extension is not prepared. Stop and start this instance to prepare it.".into());
    }
    let active = details
        .active_run
        .as_ref()
        .ok_or("The Dragonwilds managed run is unavailable.")?;
    if active.process_count != 1
        || active.processes.len() != 1
        || details.summary.active_process_count != 1
    {
        return Err("Dragonwilds saving requires one managed server process.".into());
    }
    let process = &active.processes[0];
    let context = context(details)?;
    if process.process_key != "main"
        || process.exit_code.is_some()
        || process.crash_flag
        || process.run_id != active.run_id
        || process.session_id != active.session_id
        || active.pid != Some(context.pid)
    {
        return Err("The Dragonwilds managed process metadata changed.".into());
    }
    let root = Game::Dragonwilds
        .install_root(std::path::Path::new(&context.identity.image_path))
        .map_err(|_| "The Dragonwilds native installation cannot be located.")?;
    let (run, _pins) = installation::session(&root)?;
    let native = native_process::capture(
        GameProfile::Dragonwilds,
        &root,
        context.pid,
        &context.identity,
    )?;
    let reference = Reference { run: &run };
    let mut exchange = |request: &Request<'_>| {
        native_process::verify(
            GameProfile::Dragonwilds,
            &root,
            context.pid,
            &context.identity,
            &native,
        )?;
        let body =
            serde_json::to_vec(request).map_err(|_| "Cannot encode Dragonwilds save request.")?;
        let reply = transport::exchange_dragonwilds_control(
            context.pid,
            &context.identity,
            request.request_id,
            &body,
        )
        .map_err(|_| "Dragonwilds native exchange failed.".to_owned())?;
        native_process::verify(
            GameProfile::Dragonwilds,
            &root,
            context.pid,
            &context.identity,
            &native,
        )?;
        Ok(reply)
    };
    let probe_id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &probe_id,
        requested_at: now()? / 1000,
        action: "saveprobe",
        boot_id: "",
        world_token: "",
    };
    let ready = probe(&exchange(&request)?, &request, &reference)?;
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &request_id,
        requested_at: now()? / 1000,
        action: "save",
        boot_id: &ready.boot_id,
        world_token: &ready.world_token,
    };
    dispatch_once(&request, &reference, &mut exchange)?;
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: process.process_key.clone(),
        display_name: "Dragonwilds native world save".into(),
        pid: context.pid,
        command: "save".into(),
        response_text: Some(
            "The native save request was sent. Disk completion has not been confirmed.".into(),
        ),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Requested),
        submitted_at_unix_ms,
    })
}
fn spawn_save_worker<L, T>(
    lease: &L,
    mutation: OwnedMutexGuard<()>,
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T>
where
    L: StorageContextTaskLease,
    T: Send + 'static,
{
    spawn_blocking_storage_context_task(lease, move || {
        let _mutation = mutation;
        operation()
    })
}
pub(crate) async fn save_world(
    state: &DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    let mutation =
        mutation.ok_or("Dragonwilds save requires the instance mutation reservation.")?;
    let lease = state.begin_storage_context_operation("Dragonwilds immediate world save")?;
    let details = details.clone();
    spawn_save_worker(&lease, mutation, move || {
        save_owned(&details, submitted_at_unix_ms)
    })
    .await
    .map_err(|_| {
        "Dragonwilds save verification worker failed; the save was not resent.".to_owned()
    })?
}
#[cfg(test)]
#[path = "dragonwilds_control_save_tests.rs"]
mod tests;
