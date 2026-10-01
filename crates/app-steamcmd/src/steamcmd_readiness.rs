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
struct RuntimeFingerprint {
    root: String,
    files: Vec<FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    let path = root.join(RECORD);
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > RECORD_LIMIT {
        return false;
    }
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<RuntimeFingerprint>(&bytes).ok())
        .is_some_and(|record| record == current)
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
        record_verified(&status).unwrap();
        assert!(managed_steamcmd_status(&settings).ready);
        fs::write(root.join("steamclient64.dll"), b"changed runtime").unwrap();
        assert!(!managed_steamcmd_status(&settings).ready);
        record_verified(&managed_steamcmd_status(&settings)).unwrap();
        invalidate(&managed_steamcmd_status(&settings)).unwrap();
        assert!(!managed_steamcmd_status(&settings).ready);
        fs::remove_dir_all(root).unwrap();
    }
}
