use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::{Context, Target, io, response};

pub(super) fn tick() -> Result<u64, String> {
    #[cfg(windows)]
    {
        Ok(unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() })
    }
    #[cfg(not(windows))]
    {
        Err("Enshrouded native control is available on Windows only.".into())
    }
}

pub(super) fn verify_process(context: &Context) -> Result<(), String> {
    if app_runtime::process_matches_identity(context.pid, &context.identity)
        .map_err(|_| "Cannot verify the native server process.")?
    {
        Ok(())
    } else {
        Err("The native server process changed or stopped.".into())
    }
}

pub(super) fn open(context: &Context) -> Result<(File, File), String> {
    open_with_executable(context, None)
}

pub(super) fn open_with_executable(
    context: &Context,
    executable: Option<&str>,
) -> Result<(File, File), String> {
    verify_process(context)?;
    let root = io::pin_directory(&context.root)?;
    let directory = io::pin_directory(&context.root.join(io::DIRECTORY))?;
    let owner: serde_json::Value =
        serde_json::from_slice(&io::read_regular(&context.root.join(io::OWNER), 4096)?)
            .map_err(|_| "The native extension ownership record is invalid.")?;
    let payload = io::read_regular(
        &context.root.join("dbghelp.dll"),
        crate::enshrouded_extension::MAX_DLL,
    )?;
    let hash = io::digest(&payload);
    let executable = match executable {
        Some(executable) => executable.to_owned(),
        None => io::digest(&io::read_regular(
            Path::new(&context.identity.image_path),
            32 * 1024 * 1024,
        )?),
    };
    crate::enshrouded_extension::verify_installed(&context.root, &executable, &payload)?;
    if owner.get("schema").and_then(|v| v.as_u64()) != Some(1)
        || owner.get("module_id").and_then(|v| v.as_str()) != Some("enshrouded")
        || owner.get("plugin_sha256").and_then(|v| v.as_str()) != Some(hash.as_str())
        || io::read_regular(&context.root.join(io::DIRECTORY).join("enable"), 128)? != io::ENABLE
    {
        return Err(
            "The verified Enshrouded native extension is not installed for this build.".into(),
        );
    }
    Ok((root, directory))
}

fn packet(context: &Context) -> Result<response::Packet, String> {
    verify_process(context)?;
    let bytes = io::read_regular(
        &context.root.join(io::DIRECTORY).join("players.json"),
        io::MAX_BYTES,
    )?;
    let packet = response::parse(context, &bytes, tick()?)?;
    verify_process(context)?;
    Ok(packet)
}

pub(super) fn read_packet(context: &Context) -> Result<response::Packet, String> {
    let _guards = open(context)?;
    packet(context)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    protocol: u32,
    pid: u32,
    creation_time: String,
    run_id: String,
    request_id: String,
    action: String,
    account_id: String,
    status: String,
    issue: String,
}

pub(super) fn request_body(
    action: &str,
    request_id: &str,
    target: &Target,
    expires: u64,
) -> String {
    format!(
        "LGSM_ENSHROUDED_CONTROL_1\nrequest_id={request_id}\nrun_id={}\naction={action}\nsnapshot_sequence={}\nsession_handle={}\ngeneration={}\naccount_id={}\nexpires_at_tick={expires}\n",
        target.run_id, target.sequence, target.handle, target.generation, target.account
    )
}

fn read_reply(
    context: &Context,
    request_id: &str,
    target: &Target,
    action: &str,
) -> Result<bool, String> {
    let bytes = match io::read_regular(
        &context.root.join(io::DIRECTORY).join("response.json"),
        4096,
    ) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(false),
    };
    let reply: Reply = match serde_json::from_slice(&bytes) {
        Ok(reply) => reply,
        Err(_) => return Ok(false),
    };
    if reply.request_id != request_id {
        return Ok(false);
    }
    if reply.protocol != 1
        || reply.pid != context.pid
        || response::decimal(&reply.creation_time)? != context.identity.creation_time
        || reply.run_id != target.run_id
        || reply.action != action
        || response::decimal(&reply.account_id)? != target.account
    {
        return Err("The native action reply does not match this request and process.".into());
    }
    match reply.status.as_str() {
        "dispatched" if reply.issue == "none" => Ok(true),
        "rejected" => Err(match reply.issue.as_str() {
            "native_target_immune" => "The native server protects this player from moderation.",
            "request_expired" => "The native player selection expired. Refresh players before trying again.",
            "target_session_changed" | "snapshot_not_current" | "native_context_changed" => "The native player identity changed or is incomplete. Refresh players before trying again.",
            "request_queue_busy" => "The native server is already processing a moderation request.",
            _ => "The native server rejected this moderation request.",
        }.into()),
        _ => Err("The native server returned an unrecognized moderation result.".into()),
    }
}

pub(super) fn execute(
    context: &Context,
    action: &str,
    selected: Option<Target>,
    account: u64,
) -> Result<(), String> {
    let _guards = open(context)?;
    let initial = packet(context)?;
    let now = tick()?;
    let target = selected.unwrap_or(Target {
        run_id: initial.run_id.clone(),
        sequence: 0,
        tick: now,
        handle: 0,
        generation: 0,
        account,
    });
    if target.run_id != initial.run_id {
        return Err("The native server session changed. Refresh before trying again.".into());
    }
    if action != "unban"
        && (!initial.complete
            || !initial.players.iter().any(|p| {
                p.session_handle == target.handle
                    && p.generation == target.generation.to_string()
                    && p.account_id == target.account.to_string()
                    && !p.moderation_immune
            }))
    {
        return Err("The selected native player is unavailable, protected, or changed.".into());
    }
    let expires = target
        .tick
        .checked_add(15_000)
        .ok_or("Invalid native snapshot time.")?
        .min(now.saturating_add(5_000));
    if now >= expires {
        return Err("The native player selection expired. Refresh before trying again.".into());
    }
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let body = request_body(action, &request_id, &target, expires);
    let destination = context.root.join(io::DIRECTORY).join("request.txt");
    verify_process(context)?;
    io::publish_request(&destination, body.as_bytes())?;
    // Never resend after an unknown outcome. The extension also deduplicates IDs.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut dispatched = false;
    while Instant::now() < deadline {
        verify_process(context).map_err(
            |_| "The server stopped after submission; the moderation outcome is unknown.",
        )?;
        if !dispatched {
            dispatched = read_reply(context, &request_id, &target, action)?;
        }
        if dispatched {
            let current = match packet(context) {
                Ok(packet) => packet,
                Err(_) => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            };
            if current.run_id != target.run_id {
                return Err(
                    "The native session changed after dispatch; the moderation outcome is unknown."
                        .into(),
                );
            }
            let departed = current.complete
                && !current.players.iter().any(|p| {
                    if action == "ban" {
                        p.account_id == target.account.to_string()
                    } else {
                        p.session_handle == target.handle
                            && p.generation == target.generation.to_string()
                    }
                });
            let confirmed = match action {
                "kick" => departed,
                "ban" => {
                    departed
                        && native_banned(&context.root, target.account).is_ok_and(|banned| banned)
                }
                "unban" => native_banned(&context.root, target.account).is_ok_and(|banned| !banned),
                _ => false,
            };
            if confirmed {
                verify_process(context)?;
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(if dispatched {
        "The native action was dispatched, but player departure or canonical ban-file persistence could not be confirmed. Refresh the roster before any further action."
    } else {
        "No confirmed native reply arrived. The outcome is unknown; refresh the roster before any further action."
    }.into())
}

pub(super) fn native_banned(root: &Path, account: u64) -> Result<bool, String> {
    let bytes = io::read_regular(&root.join("enshrouded_server.json"), 256 * 1024)?;
    let document: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| "The canonical Enshrouded configuration is invalid.")?;
    let entries = document
        .get("bannedAccounts")
        .and_then(|value| value.as_array())
        .ok_or("The canonical Enshrouded bannedAccounts array is missing.")?;
    let mut ids = HashSet::new();
    for entry in entries {
        let id = entry
            .get("accountId")
            .and_then(|value| value.as_u64())
            .ok_or("The canonical native ban contains an invalid accountId.")?;
        if !ids.insert(id) {
            return Err("The canonical native ban contains duplicate account IDs.".into());
        }
    }
    Ok(ids.contains(&account))
}

#[cfg(test)]
pub(super) fn encode_request(
    action: &str,
    request_id: &str,
    target: &Target,
    expires: u64,
) -> String {
    request_body(action, request_id, target, expires)
}
