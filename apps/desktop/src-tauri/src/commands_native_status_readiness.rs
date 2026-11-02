use crate::runtime_log_stream::file_identity::{LogFileIdentity, log_identity};
use app_core::ProcessIdentity;
use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const STATUS_PATH: &str = "Moria/Saved/Config/Status.json";
const MAX_STATUS_BYTES: u64 = 64 * 1024;
const WINDOWS_UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    identity: LogFileIdentity,
    modified: SystemTime,
    length: u64,
    fingerprint: u64,
    running: bool,
}

pub(super) struct Baseline(Option<Snapshot>);

enum Observation {
    Missing,
    Updating,
    Present(Snapshot),
}

pub(super) fn capture(install_root: &Path) -> Result<Baseline, String> {
    match read_snapshot(install_root)? {
        Observation::Missing => Ok(Baseline(None)),
        Observation::Present(snapshot) => Ok(Baseline(Some(snapshot))),
        Observation::Updating => Err("cannot baseline a changing prestart status file".into()),
    }
}

#[cfg(test)]
pub(super) fn ready(install_root: &Path, baseline: &Baseline) -> Result<bool, String> {
    ready_since(install_root, baseline, None)
}

pub(super) fn ready_for_process(
    install_root: &Path,
    baseline: &Baseline,
    identity: &ProcessIdentity,
) -> Result<bool, String> {
    let created = identity
        .creation_time
        .checked_sub(WINDOWS_UNIX_EPOCH_TICKS)
        .ok_or("native status readiness requires a valid process creation token")?;
    ready_since(install_root, baseline, Some(created))
}

fn ready_since(
    install_root: &Path,
    baseline: &Baseline,
    created_ticks: Option<u64>,
) -> Result<bool, String> {
    let Observation::Present(current) = read_snapshot(install_root)? else {
        return Ok(false);
    };
    // The copied package may already contain Status=running. Require a changed
    // file generation, writer timestamp or content after the pre-launch baseline.
    if !current.running || baseline.0.as_ref() == Some(&current) {
        return Ok(false);
    }
    let Some(created) = created_ticks else {
        return Ok(true);
    };
    // Preserve Windows' 100 ns process-token precision: a newly copied file
    // with an older writer timestamp is not this process's hosting state.
    let modified_ticks = current
        .modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "native status writer timestamp is invalid")?
        .as_nanos()
        / 100;
    Ok(modified_ticks >= u128::from(created) && current.modified <= SystemTime::now())
}

fn read_snapshot(install_root: &Path) -> Result<Observation, String> {
    read_snapshot_with(install_root, || {})
}

/// The seam is immediately after the bounded read, so tests can deterministically
/// replace/truncate the native file while its original handle remains open.
fn read_snapshot_with(
    install_root: &Path,
    after_read: impl FnOnce(),
) -> Result<Observation, String> {
    let path = install_root.join(STATUS_PATH);
    let resolved = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Missing);
        }
        Err(_) => return Err("native status path cannot be resolved".into()),
    };
    let root = install_root
        .canonicalize()
        .map_err(|_| "native install root unavailable")?;
    if !resolved.starts_with(&root) {
        return Err("native status escaped its installation".into());
    }
    let mut file = match File::open(&resolved) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Missing);
        }
        Err(_) => return Err("native status cannot be read".into()),
    };
    let before = file
        .metadata()
        .map_err(|_| "native status metadata unavailable")?;
    if !before.is_file() || before.len() > MAX_STATUS_BYTES {
        return Err("native status exceeds its regular-file size limit".into());
    }
    let identity = match log_identity(&path, &file) {
        Ok(identity) => identity,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Updating);
        }
        Err(_) => return Err("native status file identity unavailable".into()),
    };
    let modified = before
        .modified()
        .map_err(|_| "native status writer timestamp unavailable")?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "native status read failed")?;
    after_read();
    let after = file
        .metadata()
        .map_err(|_| "native status metadata unavailable")?;
    if bytes.len() as u64 > MAX_STATUS_BYTES {
        return Err("native status exceeds its size limit".into());
    }
    if before.len() != after.len()
        || modified
            != after
                .modified()
                .map_err(|_| "native status writer timestamp unavailable")?
        || bytes.len() as u64 != after.len()
    {
        return Ok(Observation::Updating);
    }
    // A stable old handle can remain readable after an atomic replacement.
    // Only the file still at the current path may contribute hosting evidence.
    let current_path = match path.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Updating);
        }
        Err(_) => return Err("native status path cannot be rechecked".into()),
    };
    if current_path != resolved || !current_path.starts_with(&root) {
        return Ok(Observation::Updating);
    }
    let current = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Updating);
        }
        Err(_) => return Err("native status cannot be reopened".into()),
    };
    let current_identity = match log_identity(&path, &current) {
        Ok(identity) => identity,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Observation::Updating);
        }
        Err(_) => return Err("native status current file identity unavailable".into()),
    };
    let current_metadata = current
        .metadata()
        .map_err(|_| "native status current metadata unavailable")?;
    if current_identity != identity
        || !current_metadata.is_file()
        || current_metadata.len() != after.len()
        || current_metadata
            .modified()
            .map_err(|_| "native status current writer timestamp unavailable")?
            != modified
    {
        return Ok(Observation::Updating);
    }
    // The installed 21872765 writer emits a UTF-8 BOM before its JSON object.
    let json_bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let running = serde_json::from_slice::<serde_json::Value>(json_bytes)
        .ok()
        .and_then(|value| {
            value
                .get("Status")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .is_some_and(|status| status == "running");
    // This bounded content fingerprint detects rewrites, not an authentication
    // boundary. Never include the status payload (which contains join data) in diagnostics.
    let mut hash = DefaultHasher::new();
    bytes.hash(&mut hash);
    Ok(Observation::Present(Snapshot {
        identity,
        modified,
        length: after.len(),
        fingerprint: hash.finish(),
        running,
    }))
}

#[cfg(test)]
#[path = "commands_native_status_readiness_tests.rs"]
mod tests;
