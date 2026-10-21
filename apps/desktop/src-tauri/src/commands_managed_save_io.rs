//! Pinned ordinary-file boundaries for explicit native save extensions.
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path};
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
    let before = fs::symlink_metadata(path)
        .map_err(|_| "Cannot inspect the native save control directory.")?;
    if !before.is_dir() || reparse(&before) {
        return Err("native save control requires ordinary directories.".into());
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
        .map_err(|_| "Cannot pin the native save control directory.")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native save control directory.")?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("native save control requires ordinary directories.".into());
    }
    Ok(file)
}

/// Keep every ancestor pinned without DELETE sharing until the exchange ends.
pub(crate) fn pin_directories(path: &Path) -> Result<Vec<File>, String> {
    if !local_absolute(path) {
        return Err("native save control requires a local absolute directory.".into());
    }
    let ancestors: Vec<_> = path.ancestors().collect();
    ancestors.into_iter().rev().map(pin_directory).collect()
}

fn open_regular(path: &Path, pinned: bool) -> Result<Option<File>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || reparse(&metadata) => {
            return Err("native save control requires an ordinary file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("The native save control file is unavailable.".into()),
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
        Err(_) => return Err("The native save control file is unavailable.".into()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| "Cannot inspect the native save control file.")?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err("native save control requires an ordinary file.".into());
    }
    Ok(Some(file))
}

fn read_bounded(file: &mut File, limit: usize) -> Result<Vec<u8>, String> {
    if file
        .metadata()
        .map_err(|_| "Cannot inspect the native save control file.")?
        .len()
        > limit as u64
    {
        return Err("The native save control file exceeds its read limit.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read the native save control file.")?;
    if bytes.len() > limit {
        return Err("The native save control file exceeds its read limit.".into());
    }
    Ok(bytes)
}

pub(crate) fn read_optional(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    open_regular(path, false)?
        .map(|mut file| read_bounded(&mut file, limit))
        .transpose()
}

pub(crate) fn read_regular(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    read_optional(path, limit)?.ok_or_else(|| "The native save control file is missing.".into())
}

pub(in crate::commands) fn pin_regular(
    path: &Path,
    limit: usize,
) -> Result<(File, Vec<u8>), String> {
    let mut file = open_regular(path, true)?.ok_or("The native save control file is missing.")?;
    let bytes = read_bounded(&mut file, limit)?;
    Ok((file, bytes))
}

pub(crate) fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || reparse(&metadata) => {
            return Err("Refusing to replace a nonordinary native save control file.".into());
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("Cannot inspect the native save control destination.".into()),
    }
    let parent = path
        .parent()
        .ok_or("native save control destination has no parent.")?;
    let _guards = pin_directories(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| "Cannot create the native save control temporary file.")?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| "Cannot atomically publish the native save control file.".into())
}

/// Only the live request mailbox opts into replacing a held reader's old file.
pub(crate) fn publish_request(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if path.file_name().and_then(|name| name.to_str()) != Some("request.txt") {
        return Err("The native save request destination is invalid.".into());
    }
    let parent = path
        .parent()
        .ok_or("native save control destination has no parent.")?;
    let _guards = pin_directories(parent)?;
    let temporary = parent.join(format!(".{}.pending", uuid::Uuid::new_v4().simple()));
    let mut pending = crate::native_control_publish::PendingFile::create(&temporary, bytes)
        .map_err(|_| "Cannot create the native save control temporary file.")?;
    pending
        .replace(path)
        .map_err(|_| "Cannot atomically publish the native save control file.".into())
}

#[cfg(all(test, windows))]
#[test]
fn live_request_replaces_an_open_peer_snapshot_without_changing_its_bytes() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = std::env::temp_dir().join(format!("managed-request-{}", uuid::Uuid::new_v4()));
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
    assert!(publish_request(&root.join("session.txt"), b"forbidden").is_err());
    drop(peer);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}

/// Installer publication: an absent leaf stays absent until exclusive create,
/// and an existing ordinary leaf must still contain the inspected bytes.
pub(in crate::commands) fn replace_owned(
    path: &Path,
    expected: Option<&[u8]>,
    bytes: &[u8],
) -> Result<(), String> {
    let _pins = pin_directories(
        path.parent()
            .ok_or("The save extension file has no parent.")?,
    )?;
    if read_optional(path, 64 * 1024 * 1024)?.as_deref() != expected {
        return Err("A save extension destination changed; it was preserved.".into());
    }
    match expected {
        None => app_storage::create_file_atomically(path, bytes).map_err(|_| {
            "The save extension destination already exists or could not be created.".into()
        }),
        Some(expected) => {
            if app_storage::compare_and_swap_file_atomically(path, expected, bytes)
                .map_err(|_| "Cannot update the owned save extension file.")?
            {
                Ok(())
            } else {
                Err("A save extension destination changed; it was preserved.".into())
            }
        }
    }
}

/// Retired extensions are removed through the same handle that verified their
/// content. A late rename, replacement or write cannot change the deletion target.
pub(super) fn remove_owned(path: &Path, expected: &str) -> Result<(), String> {
    let _parents = pin_directories(path.parent().ok_or("Invalid retired extension path.")?)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) if !metadata.is_file() || reparse(&metadata) => {
            return Err(
                "A retired save extension is not an ordinary file; it was preserved.".into(),
            );
        }
        Ok(_) => {}
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x8000_0000 | 0x0001_0000)
            .share_mode(0x1)
            .custom_flags(0x0020_0000);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Cannot lock the retired save extension.")?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file()
        || reparse(&metadata)
        || digest(&read_bounded(&mut file, 16 * 1024 * 1024)?) != expected
    {
        return Err("A retired save extension changed; it was preserved.".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::*;
        let disposition = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfoEx,
                (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                std::mem::size_of_val(&disposition) as u32,
            )
        } == 0
        {
            return Err(format!(
                "Cannot retire the owned save extension: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        drop(file);
        fs::remove_file(path).map_err(|error| error.to_string())
    }
}
