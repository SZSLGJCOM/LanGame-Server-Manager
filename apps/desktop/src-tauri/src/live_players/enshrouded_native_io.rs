//! Bounded local exchange with the instance-owned Enshrouded extension.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

pub(crate) const EXE_SHA256: &str =
    "4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba";
pub(crate) const BUNDLED_EXE_SHA256: &[&str] = &[
    EXE_SHA256,
    "b627639ce4a3f8c940fe90afa1d36ed59db3ff9ced24e935890eb7c99b04d5bf",
    "d7baa768eb00aa895d9f5a1cd8747ccf66858c7b542f50983092b2e3261588ee",
];
pub(crate) const DIRECTORY: &str = ".langame-enshrouded-control";
pub(crate) const OWNER: &str = ".langame-enshrouded-control-owner.json";
const MANAGER_STATE: &str = ".langame-enshrouded-control-state.json";
pub(crate) const ENABLE: &[u8] = b"LGSM_ENSHROUDED_CONTROL_1\n";
pub(crate) const MAX_BYTES: usize = 64 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagerState {
    executable_sha256: String,
    extension_status: app_core::RuntimePlayerExtensionStatus,
}

pub(crate) fn publish_manager_state(
    root: &Path,
    executable: &str,
    status: app_core::RuntimePlayerExtensionStatus,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(&ManagerState {
        executable_sha256: executable.to_owned(),
        extension_status: status,
    })
    .map_err(|_| "Cannot encode extension readiness.")?;
    publish(&root.join(MANAGER_STATE), &bytes)
}

pub(crate) fn extension_status(
    root: &Path,
    pid: u32,
    creation_time: u64,
) -> Option<app_core::RuntimePlayerExtensionStatus> {
    use app_core::RuntimePlayerExtensionStatus as Status;
    let executable = read_regular(&root.join("enshrouded_server.exe"), 32 * 1024 * 1024)
        .map(|bytes| digest(&bytes))
        .ok();
    if let Some(status) = executable
        .as_deref()
        .and_then(|executable| manager_status_for_executable(root, executable))
    {
        return Some(status);
    }
    let native: Option<serde_json::Value> =
        read_regular(&root.join(DIRECTORY).join("status.json"), 4096)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if let Some(native) = native
        && native.get("pid").and_then(|v| v.as_u64()) == Some(pid as u64)
    {
        return match native.get("state").and_then(|v| v.as_str()) {
            Some("blocked") => Some(Status::UnsupportedGameBuild),
            Some("waiting") => Some(Status::Loading),
            Some("ready" | "incomplete") => None,
            _ => Some(Status::LoadFailed),
        };
    }
    if !root.join("dbghelp.dll").is_file() {
        return Some(Status::NotInstalled);
    }
    let filetime_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_nanos() / 100).ok())
        .and_then(|ticks| ticks.checked_add(116_444_736_000_000_000));
    if filetime_now.is_some_and(|now| now.saturating_sub(creation_time) < 100_000_000) {
        Some(Status::Loading)
    } else {
        Some(Status::LoadFailed)
    }
}

pub(super) fn manager_status_for_executable(
    root: &Path,
    executable: &str,
) -> Option<app_core::RuntimePlayerExtensionStatus> {
    use app_core::RuntimePlayerExtensionStatus as Status;
    let manager: Option<ManagerState> = read_regular(&root.join(MANAGER_STATE), 4096)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if let Some(state) = manager
        && state.executable_sha256 == executable
        && matches!(
            state.extension_status,
            Status::UnsupportedGameBuild | Status::NotInstalled
        )
    {
        return Some(state.extension_status);
    }
    None
}

pub(crate) fn bundled_payload() -> &'static [u8] {
    #[cfg(windows)]
    {
        include_bytes!(concat!(env!("OUT_DIR"), "/enshrouded-control/dbghelp.dll"))
    }
    #[cfg(not(windows))]
    {
        &[]
    }
}

pub(crate) fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(crate) fn pin_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x1)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "Cannot pin the native control directory.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native control directory.")?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("Native control requires an ordinary directory.".into());
    }
    Ok(file)
}

pub(crate) fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| "The native control file is unavailable.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native control file.")?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > limit as u64 {
        return Err("Native control requires a bounded ordinary file.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the native control file.")?;
    if bytes.len() > limit {
        return Err("The native control file exceeded its read budget.".into());
    }
    Ok(bytes)
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && (!metadata.is_file() || reparse(&metadata))
    {
        return Err("Refusing to replace a nonordinary native control file.".into());
    }
    let parent = path
        .parent()
        .ok_or("Native control destination has no parent.")?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "Cannot create the native control temporary file.")?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Cannot atomically publish the native control file.".into())
}

pub(crate) fn publish_request(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.file_name().and_then(|name| name.to_str()) != Some("request.txt") {
        return Err("The native control request destination is invalid.".into());
    }
    let parent = path
        .parent()
        .ok_or("Native control destination has no parent.")?;
    let _guard = pin_directory(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut pending = crate::native_control_publish::PendingFile::create(&temporary, bytes)
        .map_err(|_| "Cannot create the native control temporary file.")?;
    pending
        .replace(path)
        .map_err(|_| "Cannot atomically publish the native control file.".into())
}

#[cfg(all(test, windows))]
#[test]
fn live_request_replaces_an_open_peer_snapshot_without_changing_its_bytes() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = std::env::temp_dir().join(format!("enshrouded-request-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let path = root.join("request.txt");
    fs::write(&path, b"old").unwrap();
    let mut peer = OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(&path)
        .unwrap();
    publish_request(&path, b"new").unwrap();
    let mut old = Vec::new();
    peer.read_to_end(&mut old).unwrap();
    assert_eq!(old, b"old");
    assert_eq!(read_regular(&path, 16).unwrap(), b"new");
    assert!(publish_request(&root.join("enable"), b"forbidden").is_err());
    drop(peer);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}
