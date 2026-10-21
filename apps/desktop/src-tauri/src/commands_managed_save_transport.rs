use super::{
    DIRECTORY, InstanceDetails, InstanceRuntimeCommandResult, Profile, io, payload, profile, root,
};
use crate::commands::WorldSaveCompletion;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
struct Lease(PathBuf);
impl Lease {
    fn acquire(root: &Path) -> Result<Self, String> {
        let key =
            dunce::canonicalize(root).map_err(|_| "Cannot identify the save control directory.")?;
        if !ACTIVE
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "Save control lock was poisoned.")?
            .insert(key.clone())
        {
            return Err("Another save request is already being processed.".into());
        }
        Ok(Self(key))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        ACTIVE
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

fn lines(bytes: &[u8], count: usize) -> Result<Vec<&str>, String> {
    if bytes.len() > 2048 || bytes.iter().any(|c| *c != b'\n' && !matches!(c, 32..=126)) {
        return Err("The native save frame is invalid.".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "The native save frame is invalid.")?;
    let lines: Vec<_> = text
        .strip_suffix('\n')
        .ok_or("The native save frame is incomplete.")?
        .split('\n')
        .collect();
    if lines.len() != count || lines[0] != "LGSM_SAVE_1" {
        return Err("The native save frame has an unsupported shape.".into());
    }
    Ok(lines)
}
fn value<'a>(line: &'a str, key: &str) -> Result<&'a str, String> {
    line.strip_prefix(&format!("{key}="))
        .ok_or_else(|| "The native save frame has an invalid field.".into())
}
fn session(bytes: &[u8], game: Profile) -> Result<(String, String), String> {
    let fields = lines(bytes, 4)?;
    let run = value(fields[2], "run")?;
    let token = value(fields[3], "token")?;
    if value(fields[1], "module")? != game.module || !io::nonce(run) || !io::nonce(token) {
        return Err("The prepared save control session is invalid.".into());
    }
    Ok((run.into(), token.into()))
}
#[derive(Debug, PartialEq)]
pub(super) enum Reply {
    Old,
    Dispatching,
    Dispatched,
    Failed(String),
    Completed,
    Rejected(String),
    Unknown,
}
pub(super) fn reply(
    bytes: &[u8],
    game: Profile,
    run: &str,
    nonce: &str,
    pid: u32,
) -> Result<Reply, String> {
    let fields = lines(bytes, 7)?;
    let returned_run = value(fields[2], "run")?;
    let returned_nonce = value(fields[3], "nonce")?;
    let returned_pid = value(fields[4], "pid")?;
    let status = value(fields[5], "status")?;
    let issue = value(fields[6], "issue")?;
    if value(fields[1], "module")? != game.module
        || !io::nonce(returned_run)
        || !io::nonce(returned_nonce)
        || returned_pid
            .parse::<u32>()
            .ok()
            .filter(|v| *v > 0)
            .is_none_or(|v| v.to_string() != returned_pid)
        || !matches!(
            status,
            "dispatching" | "dispatched" | "completed" | "rejected" | "unknown" | "failed"
        )
        || !matches!(
            issue,
            "none"
                | "unauthorized"
                | "unsupported_operation"
                | "nonce_conflict"
                | "request_expired"
                | "native_rejected"
                | "api_unavailable"
                | "unsupported_native_build"
                | "world_not_ready"
                | "world_ambiguous"
                | "save_busy"
                | "target_changed"
                | "native_outcome_unknown"
                | "campaign_not_ready"
                | "campaign_round_running"
                | "campaign_submarine_switch_pending"
                | "world_context_changed"
                | "vrising_write_failed"
        )
        || (issue == "vrising_write_failed" && (game.module != "vrising" || status != "failed"))
    {
        return Err("The native save reply is invalid.".into());
    }
    if returned_nonce != nonce {
        return Ok(Reply::Old);
    }
    if returned_run != run || returned_pid != pid.to_string() {
        return Err("The native save reply belongs to another process or run.".into());
    }
    match (status, issue) {
        ("dispatching", "none") => Ok(Reply::Dispatching),
        ("dispatched", "none") => Ok(Reply::Dispatched),
        ("completed", "none") => Ok(Reply::Completed),
        ("failed", "vrising_write_failed") if game.module == "vrising" => {
            Ok(Reply::Failed(issue.into()))
        }
        ("rejected", reason) if reason != "none" && reason != "native_outcome_unknown" => {
            Ok(Reply::Rejected(reason.into()))
        }
        ("unknown", "native_outcome_unknown") => Ok(Reply::Unknown),
        _ => Err("The native save reply has contradictory status fields.".into()),
    }
}
fn unknown() -> String {
    "Save verification failed: the program did not obtain a complete native write result for this request. Success was not reported and the save was not resent.".into()
}
fn now() -> Result<u128, String> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|v| v.as_millis())
        .map_err(|_| "The system clock is invalid.".into())
}

pub(super) fn save(
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
) -> Result<InstanceRuntimeCommandResult, String> {
    let game = profile(&details.summary.module_id)?;
    let command = crate::commands::commands_world_save::NativeSave::from_transport(&format!(
        "{}_save",
        game.module
    ))
    .ok_or("The managed save action contract is unavailable.")?
    .contract()
    .1;
    let runtime = root(details)?;
    let _lease = Lease::acquire(&runtime)?;
    let mut pins = payload::verify_prepared(&runtime, game)?;
    let run = details
        .active_run
        .as_ref()
        .ok_or("The server is stopped.")?;
    let mut candidates = run
        .processes
        .iter()
        .filter(|p| p.is_primary && p.status == "running" && p.run_id == run.run_id);
    let process = candidates
        .next()
        .ok_or("The managed primary server process is unavailable.")?;
    if candidates.next().is_some() {
        return Err("The managed primary server process is ambiguous.".into());
    }
    let pid = process
        .pid
        .filter(|v| *v != 0)
        .ok_or("The managed server PID is unavailable.")?;
    let identity = process
        .process_identity
        .as_ref()
        .filter(|i| i.creation_time != 0)
        .ok_or("The managed server process identity is unavailable.")?;
    let image = Path::new(&identity.image_path);
    if !image.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        Path::new(game.image)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|expected| n.eq_ignore_ascii_case(expected))
    }) {
        return Err("The managed image is not this module's dedicated server.".into());
    }
    pins.extend(super::process::execution_pins(
        details, &runtime, game, image,
    )?);
    let verify = || -> Result<(), String> {
        if app_runtime::process_matches_identity(pid, identity)
            .map_err(|_| "Cannot verify the current server process.")?
        {
            Ok(())
        } else {
            Err("The managed server process changed or stopped.".into())
        }
    };
    verify()?;
    let directory = runtime.join(DIRECTORY);
    let (session_pin, bytes) = io::pin_regular(&directory.join("session.txt"), 2048)?;
    pins.push(session_pin);
    let (session_run, token) = session(&bytes, game)?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let expires = now()? + 5000;
    let request = format!(
        "LGSM_SAVE_1\nrun={session_run}\nnonce={nonce}\ntoken={token}\nop=save\nexpires={expires}\n"
    );
    super::process::ensure_instance_scope(details, &runtime, game, Some(pid))?;
    verify()?;
    // Exactly one publication, with a five-second native admission deadline.
    // Every failure after this point is unknown unless the correlated peer
    // definitively rejected it. No resend and no mtime-based success guess.
    io::publish_request(&directory.join("request.txt"), request.as_bytes())
        .map_err(|_| unknown())?;
    let deadline = Instant::now() + Duration::from_secs(125);
    while Instant::now() < deadline {
        verify().map_err(|_| unknown())?;
        if let Some(bytes) =
            io::read_optional(&directory.join("response.txt"), 2048).map_err(|_| unknown())?
        {
            match reply(&bytes, game, &session_run, &nonce, pid).map_err(|_| unknown())? {
                Reply::Old | Reply::Dispatching => {}
                Reply::Unknown => return Err(unknown()),
                Reply::Failed(reason) => return Err(match reason.as_str() {
                    "vrising_write_failed" => "The game reported a native writer failure for this request's serialized world. The save operation did not complete successfully; it was not retried.".into(),
                    _ => "The native game reported a write failure. This request could not certify a complete world save; it was not retried.".into(),
                }),
                Reply::Rejected(reason) => {
                    return Err(format!(
                        "The native save request was rejected ({reason}); no save completion was claimed."
                    ));
                }
                state @ (Reply::Dispatched | Reply::Completed) => {
                    let completed = state == Reply::Completed;
                    verify().map_err(|_| unknown())?;
                    return Ok(InstanceRuntimeCommandResult {
                        instance_id:details.summary.id.clone(),process_key:process.process_key.clone(),display_name:format!("{} native save control",game.module),pid,command:command.into(),
                        response_text:Some(if completed && game.module=="barotrauma" {
                            "The game completed writing the campaign checkpoint for this request while between rounds."
                        }else if completed {
                            "The game confirmed completion of this request's current-world save."
                        }else if game.module=="barotrauma" {
                            "The native campaign checkpoint save was invoked once while between rounds. Disk completion has not been confirmed; this does not save an ongoing round."
                        }else if game.module=="sonsoftheforest" {
                            "The native current-world save interface was invoked once. Disk completion has not been confirmed; do not treat this receipt as a completed save or blindly repeat the request."
                        }else{
                            "The native current-world save request was accepted. Disk completion has not been confirmed; do not treat this receipt as a completed save or blindly repeat the request."
                        }.into()),
                        write_confirmation_pending:!completed,submitted_at_unix_ms,
                        save_completion:Some(if completed {WorldSaveCompletion::Confirmed}else{WorldSaveCompletion::Unconfirmed}),
                    });
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(unknown())
}
