//! Ordinary-file boundaries shared by the Valheim installer and live adapter.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const DIRECTORY: &str = ".langame-valheim-control";
pub(crate) const OWNER: &str = ".langame-valheim-control.json";
pub(crate) const PLUGIN: &str = "BepInEx/plugins/LanGame.Valheim.Control.dll";
pub(crate) const SESSION: &str = "session.json";
pub(crate) const MAX_PLUGIN_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn bundled_payload() -> &'static [u8] {
    #[cfg(windows)]
    {
        include_bytes!(concat!(
            env!("OUT_DIR"),
            "/valheim-control/LanGame.Valheim.Control.dll"
        ))
    }
    #[cfg(not(windows))]
    {
        &[]
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owner {
    pub protocol: u32,
    pub module_id: String,
    pub plugin_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_plugin_sha256: Option<String>,
}

// Deliberately no Debug: the session contains the control credential.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Session {
    pub protocol: u32,
    pub token: String,
}

pub(crate) fn parse_session(bytes: &[u8]) -> Result<Session, String> {
    if bytes.len() > 4096 {
        return Err("The Valheim control session exceeds its limit.".into());
    }
    let session: Session =
        serde_json::from_slice(bytes).map_err(|_| "The Valheim control session is invalid.")?;
    if session.protocol != 1 || !nonce(&session.token) {
        return Err("The Valheim control session is invalid.".into());
    }
    Ok(session)
}

pub(crate) fn verify_owner(bytes: &[u8], payload: &[u8]) -> Result<(), String> {
    if bytes.len() > 4096 {
        return Err("The Valheim extension ownership record exceeds its limit.".into());
    }
    let owner: Owner = serde_json::from_slice(bytes)
        .map_err(|_| "The Valheim extension ownership record is invalid.")?;
    if payload.is_empty()
        || owner.protocol != 1
        || owner.module_id != "valheim"
        || owner.pending_plugin_sha256.is_some()
        || owner.plugin_sha256.as_deref() != Some(digest(payload).as_str())
    {
        return Err("The Valheim extension ownership record does not match this payload.".into());
    }
    Ok(())
}

pub(crate) fn nonce(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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

fn local_absolute(path: &Path) -> bool {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return false;
        };
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) {
            return false;
        }
        let Some(text) = path.to_str() else {
            return false;
        };
        let offset = if text.starts_with(r"\\?\") { 6 } else { 2 };
        if text.get(offset..).is_none_or(|tail| tail.contains(':')) {
            return false;
        }
    }
    true
}

pub(crate) fn pin_directory(path: &Path) -> Result<File, String> {
    let before =
        fs::symlink_metadata(path).map_err(|_| "Cannot inspect the Valheim control directory.")?;
    if !before.is_dir() || reparse(&before) {
        return Err("Valheim control requires ordinary directories.".into());
    }
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
        .map_err(|_| "Cannot pin the Valheim control directory.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the Valheim control directory.")?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("Valheim control requires ordinary directories.".into());
    }
    Ok(file)
}

/// Keep every ancestor pinned without DELETE sharing until the exchange ends.
pub(crate) fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    if !local_absolute(path) {
        return Err("Valheim control requires a local absolute directory.".into());
    }
    let ancestors: Vec<_> = path.ancestors().collect();
    ancestors.into_iter().rev().map(pin_directory).collect()
}

fn open_regular(path: &Path, pinned: bool) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || reparse(&metadata) => {
            return Err("Valheim control requires an ordinary file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("The Valheim control file is unavailable.".into()),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .custom_flags(0x0020_0000)
            .share_mode(if pinned { 0x1 } else { 0x1 | 0x2 | 0x4 });
    }
    #[cfg(not(windows))]
    let _ = pinned;
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("The Valheim control file is unavailable.".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the Valheim control file.")?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err("Valheim control requires an ordinary file.".into());
    }
    Ok(Some(file))
}

fn read_bounded(file: &mut File, limit: usize) -> Result<Vec<u8>, String> {
    if file
        .metadata()
        .map_err(|_| "Cannot inspect the Valheim control file.")?
        .len()
        > limit as u64
    {
        return Err("The Valheim control file exceeds its read limit.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the Valheim control file.")?;
    if bytes.len() > limit {
        return Err("The Valheim control file exceeds its read limit.".into());
    }
    Ok(bytes)
}

pub(crate) fn read_optional(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    open_regular(path, false)?
        .map(|mut file| read_bounded(&mut file, limit))
        .transpose()
}

pub(crate) fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    read_optional(path, limit)?.ok_or_else(|| "The Valheim control file is missing.".into())
}

pub(crate) fn pin_regular(path: &Path, limit: usize) -> Result<(File, Vec<u8>), String> {
    let mut file = open_regular(path, true)?.ok_or("The Valheim control file is missing.")?;
    let bytes = read_bounded(&mut file, limit)?;
    Ok((file, bytes))
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || reparse(&metadata) => {
            return Err("Refusing to replace a nonordinary Valheim control file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Cannot inspect the Valheim control destination.".into()),
    }
    let parent = path
        .parent()
        .ok_or("Valheim control destination has no parent.")?;
    let _guards = pin_directories(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "Cannot create the Valheim control temporary file.")?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Cannot atomically publish the Valheim control file.".into())
}

pub(crate) fn publish_request(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.file_name().and_then(|name| name.to_str()) != Some("request.json") {
        return Err("The Valheim control request destination is invalid.".into());
    }
    let parent = path
        .parent()
        .ok_or("Valheim control destination has no parent.")?;
    let _guards = pin_directories(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut pending = crate::native_control_publish::PendingFile::create(&temporary, bytes)
        .map_err(|_| "Cannot create the Valheim control temporary file.")?;
    pending
        .replace(path)
        .map_err(|_| "Cannot atomically publish the Valheim control file.".into())
}

#[cfg(all(test, windows))]
#[test]
fn live_request_replaces_an_open_peer_snapshot_without_changing_its_bytes() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = std::env::temp_dir().join(format!("valheim-request-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let path = root.join("request.json");
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
    assert!(publish_request(&root.join(SESSION), b"forbidden").is_err());
    drop(peer);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}
