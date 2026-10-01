use crate::managed_console_log::owned_fs::{identity, reject_links};
use app_core::{ActiveInstanceRun, InstanceStatus, RuntimeHealth};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PATH: &str = "Moria/Saved/Config/Status.json";
const MAX_STATUS_BYTES: u64 = 64 * 1024;
const WINDOWS_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

/// Moria's documented hosting state is a native JSON file, not a log marker.
/// The active registry supplies ownership; file age binds this reused path to
/// that exact primary process generation. Never return the file's join data.
pub(super) fn analyze(
    status: &InstanceStatus,
    install_root: &Path,
    active_run: Option<&ActiveInstanceRun>,
) -> Option<RuntimeHealth> {
    if !matches!(status, InstanceStatus::Running | InstanceStatus::Starting) {
        return None;
    }
    let pending = || {
        health(
            "starting",
            "starting_tasks",
            "Waiting for Return to Moria to report its current hosting state.",
        )
    };
    let Some(created) = current_primary_creation(install_root, active_run) else {
        return Some(pending());
    };
    Some(match read_running_status(install_root, created) {
        Ok(true) => health(
            "ready",
            "ready_signal",
            "Return to Moria reports that the current server session is running.",
        ),
        Ok(false) => pending(),
        Err(_) => health(
            "warning",
            "log_read_failed",
            "Return to Moria hosting status could not be read safely.",
        ),
    })
}

fn current_primary_creation(install_root: &Path, run: Option<&ActiveInstanceRun>) -> Option<u64> {
    let run = run?;
    if run.run_id <= 0
        || run.process_count != run.processes.len()
        || run.session_id.as_deref().is_none_or(str::is_empty)
    {
        return None;
    }
    let mut primary = run.processes.iter().filter(|process| process.is_primary);
    let process = primary.next()?;
    if primary.next().is_some()
        || process.run_id != run.run_id
        || process.session_id != run.session_id
        || process.pid != run.pid
        || process.pid.is_none_or(|pid| pid == 0)
        || process.status != "running"
        || process.exit_code.is_some()
        || process.crash_flag
    {
        return None;
    }
    let process_identity = process.process_identity.as_ref()?;
    let created = process_identity
        .creation_time
        .checked_sub(WINDOWS_UNIX_EPOCH_TICKS)?;
    // The stored process must belong to this installation. In particular an
    // unrelated active run cannot make an old copied Status.json authoritative.
    let image = Path::new(&process_identity.image_path);
    reject_links(image).ok()?;
    let root = install_root.canonicalize().ok()?;
    if !image.canonicalize().ok()?.starts_with(&root) {
        return None;
    }
    Some(created)
}

fn read_running_status(install_root: &Path, created: u64) -> io::Result<bool> {
    reject_links(install_root)?;
    let path = install_root.join(STATUS_PATH);
    reject_links(&path)?;
    let resolved = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if !resolved.starts_with(install_root.canonicalize()?) {
        return Err(io::Error::other(
            "native hosting status escaped its installation",
        ));
    }
    let mut file = File::open(&resolved)?;
    let before = file.metadata()?;
    if !before.is_file() || before.len() > MAX_STATUS_BYTES {
        return Err(io::Error::other(
            "native hosting status exceeds its regular-file limit",
        ));
    }
    let file_identity = identity(&file)?;
    let modified = before.modified()?;
    // Keep the full 100 ns creation precision. A file from just before launch
    // must not become current merely because both times round to one second.
    let modified_ticks = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| io::Error::other("invalid native hosting status timestamp"))?
        .as_nanos()
        / 100;
    if modified_ticks <= u128::from(created) || modified > SystemTime::now() {
        return Ok(false);
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 > MAX_STATUS_BYTES {
        return Err(io::Error::other(
            "native hosting status exceeds its size limit",
        ));
    }
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || bytes.len() as u64 != after.len()
    {
        return Ok(false);
    }
    // The native writer can replace its file while it is being sampled. Do not
    // publish the old handle's running value after a replacement or link change.
    reject_links(&path)?;
    let current = File::open(&path)?;
    let current_metadata = current.metadata()?;
    if identity(&current)? != file_identity
        || current_metadata.len() != after.len()
        || current_metadata.modified()? != after.modified()?
    {
        return Ok(false);
    }
    let json = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    Ok(serde_json::from_slice::<serde_json::Value>(json)
        .ok()
        .and_then(|value| {
            value
                .get("Status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|status| status == "running"))
}

fn health(status: &str, code: &str, summary: &str) -> RuntimeHealth {
    RuntimeHealth {
        status: status.into(),
        summary: summary.into(),
        matched_line: None,
        reason: super::runtime_health_reason(
            code,
            &if code == "log_read_failed" {
                vec![("error", summary.into())]
            } else {
                Vec::new()
            },
        ),
    }
}

#[cfg(test)]
#[path = "runtime_returntomoria_health_tests.rs"]
mod tests;
