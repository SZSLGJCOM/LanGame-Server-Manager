use std::collections::hash_map::DefaultHasher;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::Path;
use std::time::SystemTime;

const STATUS_PATH: &str = "Moria/Saved/Config/Status.json";
const MAX_STATUS_BYTES: u64 = 64 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    modified: Option<SystemTime>,
    created: Option<SystemTime>,
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

pub(super) fn ready(install_root: &Path, baseline: &Baseline) -> Result<bool, String> {
    let Observation::Present(current) = read_snapshot(install_root)? else {
        return Ok(false);
    };
    // The copied package may already contain Status=running. Identical content
    // is valid only after the native writer has changed the file's metadata.
    Ok(current.running && baseline.0.as_ref() != Some(&current))
}

fn read_snapshot(install_root: &Path) -> Result<Observation, String> {
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
    if !resolved.starts_with(root) {
        return Err("native status escaped the disposable package".into());
    }
    let mut file = File::open(resolved).map_err(|_| "native status cannot be read")?;
    let before = file
        .metadata()
        .map_err(|_| "native status metadata unavailable")?;
    if !before.is_file() || before.len() > MAX_STATUS_BYTES {
        return Err("native status exceeds its regular-file size limit".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "native status read failed")?;
    let after = file
        .metadata()
        .map_err(|_| "native status metadata unavailable")?;
    if bytes.len() as u64 > MAX_STATUS_BYTES {
        return Err("native status exceeds its size limit".into());
    }
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || bytes.len() as u64 != after.len()
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
        modified: after.modified().ok(),
        created: after.created().ok(),
        length: after.len(),
        fingerprint: hash.finish(),
        running,
    }))
}

#[cfg(test)]
#[path = "commands_native_status_readiness_tests.rs"]
mod tests;
