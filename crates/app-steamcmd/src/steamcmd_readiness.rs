use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use super::{SteamCmdError, SteamCmdOwnership, SteamCmdStatus};

const RECORD: &str = ".langame-steamcmd-ready.json";
const RECORD_LIMIT: u64 = 16 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RuntimeFingerprint {
    root: String,
    files: Vec<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct FileFingerprint {
    name: String,
    length: u64,
    modified_ns: u128,
}

fn external_verifications() -> &'static Mutex<HashMap<String, RuntimeFingerprint>> {
    static CACHE: OnceLock<Mutex<HashMap<String, RuntimeFingerprint>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn fingerprint(root: &Path) -> Option<RuntimeFingerprint> {
    let canonical = fs::canonicalize(root).ok()?;
    let mut files = Vec::new();
    for name in ["steamcmd.exe", "steamclient.dll", "steamclient64.dll"] {
        let metadata = match fs::metadata(canonical.join(name)) {
            Ok(metadata) if metadata.is_file() => metadata,
            _ if name != "steamcmd.exe" => continue,
            _ => return None,
        };
        files.push(FileFingerprint {
            name: name.to_owned(),
            length: metadata.len(),
            modified_ns: metadata
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
        });
    }
    Some(RuntimeFingerprint {
        root: canonical.to_string_lossy().to_ascii_lowercase(),
        files,
    })
}

pub(super) fn ready(root: &Path, ownership: SteamCmdOwnership) -> bool {
    let Some(current) = fingerprint(root) else {
        return false;
    };
    if ownership != SteamCmdOwnership::Managed {
        return external_verifications()
            .lock()
            .ok()
            .and_then(|cache| cache.get(&current.root).cloned())
            .is_some_and(|record| record == current);
    }
    read_record(root).is_some_and(|record| record == current)
}

fn read_record(root: &Path) -> Option<RuntimeFingerprint> {
    let path = root.join(RECORD);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return None;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > RECORD_LIMIT {
        return None;
    }
    fs::read(&path)
        .ok()
        .filter(|bytes| bytes.len() <= RECORD_LIMIT as usize)
        .and_then(|bytes| serde_json::from_slice::<RuntimeFingerprint>(&bytes).ok())
        .filter(valid_record)
}

fn valid_record(record: &RuntimeFingerprint) -> bool {
    let mut previous = None;
    for file in &record.files {
        let index = match file.name.as_str() {
            "steamcmd.exe" => 0,
            "steamclient.dll" => 1,
            "steamclient64.dll" => 2,
            _ => return false,
        };
        if file.length == 0 || file.modified_ns == 0 || previous.is_some_and(|last| index <= last) {
            return false;
        }
        previous = Some(index);
    }
    record
        .files
        .first()
        .is_some_and(|file| file.name == "steamcmd.exe")
}

/// A known managed runtime may have updated itself since its last verification.
/// Missing, foreign, malformed and external records still require the explicit
/// dependency action; this predicate never establishes readiness on its own.
pub(super) fn needs_reverification(root: &Path, ownership: SteamCmdOwnership) -> bool {
    if ownership != SteamCmdOwnership::Managed {
        return false;
    }
    let (Some(current), Some(record)) = (fingerprint(root), read_record(root)) else {
        return false;
    };
    record.root == current.root && record != current
}

pub(super) fn invalidate(status: &SteamCmdStatus) -> Result<(), SteamCmdError> {
    let root = Path::new(&status.root);
    if let Some(current) = fingerprint(root)
        && let Ok(mut cache) = external_verifications().lock()
    {
        cache.remove(&current.root);
    }
    if status.ownership == SteamCmdOwnership::Managed {
        let path = root.join(RECORD);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(SteamCmdError::SteamCmdReadinessIo { path, source }),
        }
    }
    Ok(())
}

pub(super) fn record_verified(status: &SteamCmdStatus) -> Result<(), SteamCmdError> {
    let root = Path::new(&status.root);
    let current = fingerprint(root).ok_or_else(|| SteamCmdError::PrepareSteamCmd {
        output_excerpt: String::from(
            "Verified SteamCMD runtime changed or disappeared before readiness could be recorded.",
        ),
    })?;
    if status.ownership != SteamCmdOwnership::Managed {
        let mut cache =
            external_verifications()
                .lock()
                .map_err(|_| SteamCmdError::PrepareSteamCmd {
                    output_excerpt: String::from("SteamCMD verification cache is unavailable."),
                })?;
        if cache.len() >= 32 {
            cache.clear();
        }
        cache.insert(current.root.clone(), current);
        return Ok(());
    }
    let path = root.join(RECORD);
    let bytes =
        serde_json::to_vec(&current).map_err(|source| SteamCmdError::SteamCmdReadinessIo {
            path: path.clone(),
            source: std::io::Error::other(source),
        })?;
    // Readiness is advisory: a partial record is rejected and never treated as ready.
    fs::write(&path, bytes).map_err(|source| SteamCmdError::SteamCmdReadinessIo { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppSettings, managed_steamcmd_status, prepare_configured_steamcmd_root};
    use std::path::PathBuf;

    #[test]
    fn leftover_executable_is_unverified_and_any_runtime_change_invalidates_readiness() {
        let root: PathBuf = std::env::temp_dir().join(format!(
            "langame-readiness-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        prepare_configured_steamcmd_root(&root).unwrap();
        fs::write(root.join("steamcmd.exe"), b"fixture executable").unwrap();
        let settings = AppSettings {
            steamcmd_root: root.to_string_lossy().into_owned(),
            ..AppSettings::default()
        };
        let status = managed_steamcmd_status(&settings);
        assert!(status.executable_exists);
        assert!(!status.ready);
        assert!(!needs_reverification(&root, status.ownership));
        record_verified(&status).unwrap();
        assert!(managed_steamcmd_status(&settings).ready);
        assert!(!needs_reverification(&root, status.ownership));
        fs::write(root.join("steamclient64.dll"), b"changed runtime").unwrap();
        assert!(!managed_steamcmd_status(&settings).ready);
        assert!(needs_reverification(&root, status.ownership));
        assert!(!needs_reverification(&root, SteamCmdOwnership::External));
        record_verified(&managed_steamcmd_status(&settings)).unwrap();
        invalidate(&managed_steamcmd_status(&settings)).unwrap();
        assert!(!managed_steamcmd_status(&settings).ready);
        assert!(!needs_reverification(&root, status.ownership));
        fs::remove_dir_all(root).unwrap();
    }
}
