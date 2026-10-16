//! Instance-local Dragonwilds control. Callers retain the instance lifecycle
//! permit through the worker, including cancellation and receipt validation.
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::{InstanceDetails, ProcessIdentity};
use serde::{Deserialize, Serialize};

use super::cache::LivePlayerCollectionResult;
use super::file_ipc::{BridgeError, Game, transport};

#[path = "dragonwilds_control_response.rs"]
mod response;

#[path = "dragonwilds_control_installation.rs"]
pub(crate) mod installation;

pub(crate) fn bundled_payload() -> (&'static [u8], &'static [u8]) {
    let script = include_bytes!(
        "../../../../../modules/runescapedragonwilds/control/LgsmPlayerControl/Scripts/main.lua"
    );
    #[cfg(windows)]
    let bridge = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/dragonwilds-player-query/LgsmPlayerControl.dll"
    ));
    #[cfg(not(windows))]
    let bridge: &[u8] = &[];
    (script, bridge)
}

pub(crate) fn is_prepared(details: &InstanceDetails) -> Result<bool, String> {
    let context = context(details)?;
    let root = Game::Dragonwilds
        .install_root(Path::new(&context.identity.image_path))
        .map_err(|_| "The Dragonwilds control installation cannot be located.")?;
    let (script, bridge) = bundled_payload();
    installation::is_complete(&root, script, bridge)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    boot: String,
    session: String,
    token: String,
    account: String,
}

fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl Target {
    fn parse(value: &str) -> Result<Self, String> {
        let fields = value.split(':').collect::<Vec<_>>();
        if fields.len() != 4
            || !nonce(fields[0])
            || !nonce(fields[2])
            || !nonce(fields[3])
            || fields[1].is_empty()
            || (fields[1].len() > 1 && fields[1].starts_with('0'))
            || !fields[1].bytes().all(|byte| byte.is_ascii_digit())
            || !fields[1].parse::<i32>().is_ok_and(|value| value >= 0)
        {
            return Err(
                "Dragonwilds moderation requires a current authoritative player selection.".into(),
            );
        }
        Ok(Self {
            boot: fields[0].into(),
            session: fields[1].into(),
            token: fields[2].into(),
            account: fields[3].into(),
        })
    }
}

#[derive(Serialize)]
struct Request<'a> {
    protocol: u32,
    request_id: &'a str,
    requested_at: u64,
    action: &'a str,
    boot_id: &'a str,
    session_id: &'a str,
    session_token: &'a str,
    account_id: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    protocol: u32,
    request_id: String,
    timestamp: u64,
    action: String,
    boot_id: String,
    session_id: String,
    session_token: String,
    account_id: String,
    complete: bool,
    applied: bool,
    error: Option<String>,
}

struct Context {
    pid: u32,
    identity: ProcessIdentity,
}

fn context(details: &InstanceDetails) -> Result<Context, String> {
    if details.summary.module_id != "runescapedragonwilds" {
        return Err("Dragonwilds control requires the Dragonwilds module.".into());
    }
    let mut processes = details
        .active_run
        .as_ref()
        .ok_or("The server is stopped.")?
        .processes
        .iter()
        .filter(|process| process.is_primary && process.status == "running");
    let process = processes
        .next()
        .ok_or("The Dragonwilds server process is unavailable.")?;
    if processes.next().is_some() {
        return Err("The Dragonwilds server process is ambiguous.".into());
    }
    let identity = process
        .process_identity
        .clone()
        .ok_or("The server process identity is unavailable.")?;
    Game::Dragonwilds
        .install_root(Path::new(&identity.image_path))
        .map_err(|_| "The Dragonwilds executable does not match its control contract.")?;
    Ok(Context {
        pid: process.pid.ok_or("The server process ID is unavailable.")?,
        identity,
    })
}

fn now() -> Result<u64, String> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "The system clock is unavailable.")?
        .as_millis();
    u64::try_from(value).map_err(|_| "The system timestamp is out of range.".into())
}

fn exchange(context: &Context, request: &Request<'_>) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(request)
        .map_err(|_| "The Dragonwilds control request could not be encoded.")?;
    transport::exchange_dragonwilds_control(context.pid, &context.identity, request.request_id, &bytes)
        .map_err(|error| match error {
            BridgeError::Timeout => "Dragonwilds did not confirm this request in time. Its outcome may be unknown; refresh before performing another action.".into(),
            BridgeError::Missing => "The Dragonwilds control extension is unavailable. Restart this instance to prepare it.".into(),
            BridgeError::Process => "The Dragonwilds server process changed; the old selection cannot be used.".into(),
            _ => "The local Dragonwilds control exchange failed.".into(),
        })
}

fn query(context: &Context, request_id: &str, observed_at: u64) -> Result<Vec<u8>, String> {
    exchange(
        context,
        &Request {
            protocol: 1,
            request_id,
            requested_at: observed_at / 1000,
            action: "query",
            boot_id: "",
            session_id: "",
            session_token: "",
            account_id: "",
        },
    )
}

fn query_worker<T: Send + 'static>(
    permit: tokio::sync::OwnedMutexGuard<()>,
    operation: impl FnOnce() -> T + Send + 'static,
) -> tokio::task::JoinHandle<T> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        operation()
    })
}

pub(crate) async fn collect(
    details: &InstanceDetails,
    request_id: &str,
    observed_at: u64,
    permit: tokio::sync::OwnedMutexGuard<()>,
) -> LivePlayerCollectionResult {
    let fail = |message: String| {
        Box::new(super::service::failed_snapshot(
            &details.summary.id,
            request_id.into(),
            app_core::ModulePlayerListSource::FileIpc,
            app_core::RuntimeLivePlayerIssueCode::ExtensionUnavailable,
            &message,
            false,
        ))
    };
    let context = context(details).map_err(fail)?;
    let request = request_id.to_owned();
    let body = query_worker(permit, move || query(&context, &request, observed_at))
        .await
        .map_err(|_| fail("The Dragonwilds query worker failed.".into()))?
        .map_err(fail)?;
    response::parse(&details.summary.id, request_id, observed_at, &body).map_err(fail)
}

pub(crate) struct Receipt {
    pub account_id: String,
    pub banned: Option<bool>,
}

fn confirm(body: &[u8], request: &Request<'_>) -> Result<(), String> {
    let reply: Reply = serde_json::from_slice(body)
        .map_err(|_| "Dragonwilds returned an invalid action receipt; the outcome is unknown.")?;
    if reply.protocol != 1
        || reply.request_id != request.request_id
        || reply.action != request.action
        || reply.boot_id != request.boot_id
        || reply.session_id != request.session_id
        || reply.session_token != request.session_token
        || reply.account_id != request.account_id
        || reply.timestamp < request.requested_at
        || reply.timestamp > request.requested_at.saturating_add(6)
    {
        return Err(
            "Dragonwilds did not confirm the requested action and target; the outcome is unknown."
                .into(),
        );
    }
    if !reply.complete || !reply.applied || reply.error.is_some() {
        return Err(match reply.error.as_deref() {
            Some("target_changed") if !reply.applied => "The selected Dragonwilds player or ban state changed. Refresh before another action.",
            Some("api_unavailable" | "dispatcher_unavailable") if !reply.applied => "The native Dragonwilds moderation interface is unavailable.",
            Some("persistence_unavailable") if !reply.applied => "The game ban list could not be validated; no moderation action was dispatched.",
            Some("request_expired" | "dispatcher_timeout") if !reply.applied => "The Dragonwilds request expired before dispatch. Refresh before another action.",
            _ => "The native Dragonwilds moderation action was not confirmed; its outcome may be unknown. Refresh before another action.",
        }.into());
    }
    Ok(())
}

/// Runs on a bounded worker owned by the command's retained lifecycle permit.
pub(crate) fn execute_blocking(
    details: &InstanceDetails,
    command: &str,
) -> Result<Receipt, String> {
    if !is_prepared(details)? {
        return Err("The current Dragonwilds control extension is not prepared. Restart this instance before moderation.".into());
    }
    let context = context(details)?;
    let fields = command.split_ascii_whitespace().collect::<Vec<_>>();
    if fields.len() != 2 || !matches!(fields[0], "kick" | "ban" | "unban") {
        return Err("Invalid Dragonwilds moderation command.".into());
    }
    let action = fields[0];
    let target = if action == "unban" {
        if !nonce(fields[1]) {
            return Err("A complete Dragonwilds account ID is required to unban a player.".into());
        }
        let observed = now()?;
        let request_id = uuid::Uuid::new_v4().simple().to_string();
        let body = query(&context, &request_id, observed)?;
        response::parse(&details.summary.id, &request_id, observed, &body)?;
        let packet: serde_json::Value =
            serde_json::from_slice(&body).map_err(|_| "Invalid Dragonwilds control response.")?;
        let boot = packet
            .get("boot_id")
            .and_then(serde_json::Value::as_str)
            .ok_or("The control boot identity is unavailable.")?;
        Target {
            boot: boot.into(),
            session: String::new(),
            token: String::new(),
            account: fields[1].into(),
        }
    } else {
        Target::parse(fields[1])?
    };
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let request = Request {
        protocol: 1,
        request_id: &request_id,
        requested_at: now()? / 1000,
        action,
        boot_id: &target.boot,
        session_id: &target.session,
        session_token: &target.token,
        account_id: &target.account,
    };
    let body = exchange(&context, &request)?;
    confirm(&body, &request)?;
    Ok(Receipt {
        account_id: target.account,
        banned: match action {
            "ban" => Some(true),
            "unban" => Some(false),
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dragonwilds_query_cancellation_retains_the_instance_permit_until_worker_exit() {
        let mutex = std::sync::Arc::new(tokio::sync::Mutex::new(()));
        let permit = mutex.clone().lock_owned().await;
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::sync_channel(1);
        let (finished, done) = tokio::sync::oneshot::channel();
        let outer = tokio::spawn(async move {
            query_worker(permit, move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
            })
            .await
            .unwrap();
        });
        ready.await.unwrap();
        outer.abort();
        assert!(outer.await.unwrap_err().is_cancelled());
        assert!(mutex.try_lock().is_err());
        release.send(()).unwrap();
        let observer = mutex.clone();
        tokio::spawn(async move {
            let guard = observer.lock().await;
            drop(guard);
            finished.send(()).unwrap();
        });
        done.await.unwrap();
        assert!(mutex.try_lock().is_ok());
    }

    #[test]
    fn dragonwilds_control_target_is_not_an_operator_entered_name_or_id() {
        let id = "a".repeat(32);
        assert!(Target::parse(&format!("{id}:256:{id}:{id}")).is_ok());
        for invalid in [
            id.clone(),
            "a name".into(),
            format!("{id}:0256:{id}:{id}"),
            format!("{id}:2147483648:{id}:{id}"),
            format!("{id}:256:{id}:{id}:extra"),
        ] {
            assert!(Target::parse(&invalid).is_err());
        }
    }

    #[test]
    fn dragonwilds_control_receipt_requires_exact_operation_target_and_effect() {
        let id = "a".repeat(32);
        let request = Request {
            protocol: 1,
            request_id: &id,
            requested_at: 100,
            action: "kick",
            boot_id: &id,
            session_id: "256",
            session_token: &id,
            account_id: &id,
        };
        let packet = serde_json::json!({"protocol":1,"request_id":id,"timestamp":100,"action":"kick",
            "boot_id":id,"session_id":"256","session_token":id,"account_id":id,"complete":true,"applied":true,"error":null});
        assert!(confirm(&serde_json::to_vec(&packet).unwrap(), &request).is_ok());
        let mut rejected = packet.clone();
        rejected["complete"] = serde_json::json!(false);
        rejected["applied"] = serde_json::json!(false);
        rejected["error"] = serde_json::json!("target_changed");
        let error = confirm(&serde_json::to_vec(&rejected).unwrap(), &request).unwrap_err();
        assert!(error.contains("changed") && !error.contains("unknown"));
        rejected["error"] = serde_json::json!("effect_unconfirmed");
        assert!(
            confirm(&serde_json::to_vec(&rejected).unwrap(), &request)
                .unwrap_err()
                .contains("unknown")
        );
        for (key, value) in [
            ("action", serde_json::json!("ban")),
            ("account_id", serde_json::json!("b".repeat(32))),
            ("applied", serde_json::json!(false)),
            ("timestamp", serde_json::json!(107)),
            ("complete", serde_json::json!(false)),
        ] {
            let mut invalid = packet.clone();
            invalid[key] = value;
            assert!(confirm(&serde_json::to_vec(&invalid).unwrap(), &request).is_err());
        }
    }
}
