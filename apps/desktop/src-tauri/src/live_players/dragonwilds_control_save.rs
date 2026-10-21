//! One owned native dispatch followed only by request-bound completion queries.
use super::super::file_ipc::unreal_save_process::{self as native_process, GameProfile};
use super::super::file_ipc::{Game, transport};
use super::{context, installation, is_prepared, nonce, now};
use crate::commands::{InstanceRuntimeCommandResult, WorldSaveCompletion};
use crate::state::{DesktopState, StorageContextTaskLease, spawn_blocking_storage_context_task};
use app_core::InstanceDetails;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::OwnedMutexGuard;

const LIFETIME: Duration = Duration::from_secs(30);
const EXCHANGE_BUDGET: Duration = Duration::from_secs(6); // Transport is bounded to five seconds.
const UNKNOWN: &str = "Dragonwilds could not verify completion of this save within its automatic check. Success was not reported; the save was not resent.";
#[derive(Serialize)]
struct Request<'a> {
    protocol: u32,
    request_id: &'a str,
    requested_at: u64,
    action: &'a str,
    boot_id: &'a str,
    world_token: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_id: Option<&'a str>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Ready,
    Dispatching,
    Completed,
    Failed,
    Rejected,
    Unknown,
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
    save_id: String,
    ready: bool,
    invoked: bool,
    call_returned: bool,
    write_confirmed: bool,
    native_status: Status,
    native_pid: String,
    native_creation_time: String,
    run: String,
    error: Option<String>,
}
struct Reference<'a> {
    run: &'a str,
    pid: u32,
    creation_time: u64,
}
fn decimal(value: &str) -> Option<u64> {
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}
fn known_issue(value: &str) -> bool {
    matches!(
        value,
        "save_busy"
            | "native_profile_unavailable"
            | "native_identity_unavailable"
            | "native_target_changed"
            | "native_target_unavailable"
            | "native_provider_unsupported"
            | "native_slot_unavailable"
            | "native_write_failed"
            | "native_completion_timeout"
            | "native_closure_unavailable"
            | "native_run_changed"
            | "native_nonce_conflict"
            | "native_request_unknown"
            | "native_request_capacity"
            | "dispatcher_unavailable"
            | "dispatcher_timeout"
            | "api_unavailable"
            | "world_unavailable"
            | "world_ambiguous"
            | "roster_limit"
            | "roster_incomplete"
            | "identity_unavailable"
            | "request_expired"
            | "target_changed"
            | "native_rejected"
            | "effect_unconfirmed"
            | "persistence_unavailable"
            | "collection_failed"
            | "save_call_unknown"
    )
}
fn rejected(error: Option<&str>) -> bool {
    matches!(
        error,
        Some(
            "save_busy"
                | "target_changed"
                | "api_unavailable"
                | "world_unavailable"
                | "world_ambiguous"
                | "request_expired"
                | "dispatcher_timeout"
                | "dispatcher_unavailable"
                | "roster_limit"
                | "roster_incomplete"
                | "identity_unavailable"
                | "persistence_unavailable"
                | "native_profile_unavailable"
                | "native_identity_unavailable"
                | "native_target_changed"
                | "native_target_unavailable"
                | "native_provider_unsupported"
                | "native_slot_unavailable"
                | "native_run_changed"
                | "native_nonce_conflict"
                | "native_request_capacity"
        )
    )
}
fn parse(body: &[u8], request: &Request<'_>, reference: &Reference<'_>) -> Result<Reply, ()> {
    if body.len() > 2048
        || !body.is_ascii()
        || !nonce(request.request_id)
        || !nonce(reference.run)
        || reference.pid == 0
        || reference.creation_time == 0
    {
        return Err(());
    }
    let reply: Reply = serde_json::from_slice(body).map_err(|_| ())?;
    if reply.protocol != 1
        || reply.request_id != request.request_id
        || reply.action != request.action
        || reply.save_id != request.save_id.unwrap_or("")
        || reply.run != reference.run
        || !nonce(&reply.boot_id)
        || reply.timestamp < request.requested_at
        || reply.timestamp > request.requested_at.saturating_add(6)
        || reply
            .error
            .as_deref()
            .is_some_and(|value| !known_issue(value))
        || (request.action != "saveprobe"
            && (reply.boot_id != request.boot_id || reply.world_token != request.world_token))
    {
        return Err(());
    }
    let identity = decimal(&reply.native_pid) == Some(u64::from(reference.pid))
        && decimal(&reply.native_creation_time) == Some(reference.creation_time);
    let outer_rejection = reply.native_status == Status::Unknown
        && !reply.ready
        && !reply.invoked
        && !reply.call_returned
        && !reply.write_confirmed
        && rejected(reply.error.as_deref())
        && reply.native_pid.is_empty()
        && reply.native_creation_time.is_empty();
    if outer_rejection {
        return Ok(reply);
    }
    if !identity || !nonce(&reply.world_token) {
        return Err(());
    }
    let valid = match reply.native_status {
        Status::Ready => {
            request.action == "saveprobe"
                && reply.ready
                && !reply.invoked
                && !reply.call_returned
                && !reply.write_confirmed
                && reply.error.is_none()
        }
        Status::Completed => {
            request.action != "saveprobe"
                && reply.ready
                && reply.invoked
                && reply.call_returned
                && reply.write_confirmed
                && reply.error.is_none()
        }
        Status::Dispatching => {
            request.action != "saveprobe"
                && reply.ready
                && reply.invoked
                && !reply.write_confirmed
                && reply.error.is_none()
        }
        Status::Failed => {
            request.action != "saveprobe"
                && reply.ready
                && reply.invoked
                && !reply.write_confirmed
                && reply.error.as_deref() == Some("native_write_failed")
        }
        Status::Rejected => {
            !reply.ready
                && !reply.invoked
                && !reply.call_returned
                && !reply.write_confirmed
                && rejected(reply.error.as_deref())
        }
        Status::Unknown => !reply.write_confirmed && reply.error.is_some(),
    };
    if valid { Ok(reply) } else { Err(()) }
}
fn probe(body: &[u8], request: &Request<'_>, reference: &Reference<'_>) -> Result<Reply, String> {
    let reply = parse(body, request, reference)
        .map_err(|_| "Dragonwilds returned an invalid native preflight; no save was requested.")?;
    if reply.native_status != Status::Ready {
        return Err(format!(
            "Dragonwilds save preflight rejected: {}. No manual save was called.",
            reply.error.as_deref().unwrap_or("invalid_state")
        ));
    }
    Ok(reply)
}
/// Save is published once. Every subsequent exchange is a new read-only query
/// carrying that Save's identifier. Dropping the UI future retains this worker.
fn complete_once(
    request: &Request<'_>,
    reference: &Reference<'_>,
    deadline: Instant,
    mut exchange: impl FnMut(&Request<'_>) -> Result<Vec<u8>, String>,
    mut pause: impl FnMut(),
) -> Result<(), String> {
    let mut response = exchange(request)
        .ok()
        .and_then(|body| parse(&body, request, reference).ok());
    loop {
        if Instant::now() >= deadline {
            return Err(UNKNOWN.into());
        }
        if let Some(reply) = response {
            match reply.native_status {
                Status::Completed => return Ok(()),
                Status::Dispatching => {}
                _ => {
                    return Err(format!(
                        "Dragonwilds save verification failed: {}. Success was not reported; the save was not resent.",
                        reply.error.as_deref().unwrap_or("invalid_state")
                    ));
                }
            }
        }
        if deadline.saturating_duration_since(Instant::now()) <= EXCHANGE_BUDGET {
            return Err(UNKNOWN.into());
        }
        pause();
        if deadline.saturating_duration_since(Instant::now()) <= EXCHANGE_BUDGET {
            return Err(UNKNOWN.into());
        }
        let query_id = uuid::Uuid::new_v4().simple().to_string();
        let query = Request {
            protocol: 1,
            request_id: &query_id,
            requested_at: now()? / 1000,
            action: "savequery",
            boot_id: request.boot_id,
            world_token: request.world_token,
            save_id: Some(request.request_id),
        };
        response = exchange(&query)
            .ok()
            .and_then(|body| parse(&body, &query, reference).ok());
    }
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
    let reference = Reference {
        run: &run,
        pid: native.pid,
        creation_time: native.identity.creation_time,
    };
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
        save_id: None,
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
        save_id: None,
    };
    complete_once(
        &request,
        &reference,
        Instant::now() + LIFETIME,
        &mut exchange,
        || std::thread::sleep(Duration::from_millis(150)),
    )?;
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: process.process_key.clone(),
        display_name: "Dragonwilds native world save".into(),
        pid: context.pid,
        command: "save".into(),
        response_text: Some("Save completed.".into()),
        write_confirmation_pending: false,
        save_completion: Some(WorldSaveCompletion::Confirmed),
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
