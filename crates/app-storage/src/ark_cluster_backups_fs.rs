use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::invalid;
use crate::StorageError;
use crate::backups::{canonical_plain_directory, ensure_plain_directory, is_link_or_reparse};

pub(super) const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 64;
const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const CAPACITY_RESERVE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Entry {
    pub path: PathBuf,
    pub bytes: Option<u64>,
    pub sha256: Option<String>,
}

pub(super) fn inventory(root: &Path) -> Result<Vec<Entry>, StorageError> {
    plain_ancestors(root)?;
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: root.to_owned(),
                source,
            });
        }
        Ok(_) => {}
    }
    canonical_plain_directory(root, root)?;
    let mut entries = Vec::new();
    visit(root, root, 0, &mut entries, &mut 0)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn visit(
    root: &Path,
    dir: &Path,
    depth: usize,
    entries: &mut Vec<Entry>,
    total: &mut u64,
) -> Result<(), StorageError> {
    if depth > MAX_DEPTH {
        return Err(invalid(
            dir,
            "Cluster backup exceeds the 64-directory depth limit",
        ));
    }
    let children = fs::read_dir(dir).map_err(|source| StorageError::ReadDirectory {
        path: dir.to_owned(),
        source,
    })?;
    for child in children {
        let path = child
            .map_err(|source| StorageError::ReadDirectory {
                path: dir.to_owned(),
                source,
            })?
            .path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if is_link_or_reparse(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
            return Err(invalid(
                &path,
                "Cluster backups cannot follow links, junctions or special files",
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid(&path, "Snapshot entry escapes its root"))?
            .to_owned();
        if entries.len() >= MAX_ENTRIES {
            return Err(invalid(root, "Cluster backup exceeds 200000 entries"));
        }
        if metadata.is_dir() {
            entries.push(Entry {
                path: relative,
                bytes: None,
                sha256: None,
            });
            visit(root, &path, depth + 1, entries, total)?;
        } else {
            *total = total
                .checked_add(metadata.len())
                .filter(|size| *size <= MAX_BYTES)
                .ok_or_else(|| invalid(root, "Cluster backup exceeds the 1 TiB data limit"))?;
            let (bytes, sha256) = hash_file(&path)?;
            if bytes != metadata.len() {
                return Err(invalid(
                    &path,
                    "Snapshot source changed while it was being read",
                ));
            }
            entries.push(Entry {
                path: relative,
                bytes: Some(bytes),
                sha256: Some(sha256),
            });
        }
    }
    Ok(())
}

pub(super) fn hash_file(path: &Path) -> Result<(u64, String), StorageError> {
    let mut file = File::open(path).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > MAX_BYTES {
            return Err(invalid(path, "Snapshot file exceeds the data limit"));
        }
        hash.update(&buffer[..count]);
    }
    Ok((
        bytes,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

pub(super) fn copy_verified(
    source: &Path,
    destination: &Path,
    entries: &[Entry],
) -> Result<(), StorageError> {
    plain_ancestors(source)?;
    plain_ancestors(destination)?;
    ensure_plain_directory(destination)?;
    let mut seen = std::collections::HashSet::new();
    if entries.len() > MAX_ENTRIES {
        return Err(invalid(source, "Snapshot has too many entries"));
    }
    for entry in entries {
        if entry.path.as_os_str().is_empty()
            || entry
                .path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !seen.insert(entry.path.clone())
        {
            return Err(invalid(
                source,
                "Snapshot contains an invalid or duplicate relative path",
            ));
        }
        let from = source.join(&entry.path);
        let to = destination.join(&entry.path);
        if let Some(parent) = from.parent() {
            plain_ancestors(parent)?;
        }
        let metadata = fs::symlink_metadata(&from).map_err(|source| StorageError::ReadPath {
            path: from.clone(),
            source,
        })?;
        if is_link_or_reparse(&metadata) {
            return Err(invalid(&from, "Snapshot contains a link or junction"));
        }
        if let Some(expected_size) = entry.bytes {
            if !metadata.is_file() || metadata.len() != expected_size {
                return Err(invalid(
                    &from,
                    "Snapshot file size no longer matches its manifest",
                ));
            }
            if let Some(parent) = to.parent() {
                ensure_plain_directory(parent)?;
            }
            let input = File::open(&from).map_err(|source| StorageError::ReadPath {
                path: from.clone(),
                source,
            })?;
            let mut output = File::options()
                .write(true)
                .create_new(true)
                .open(&to)
                .map_err(|source| StorageError::CreatePath {
                    path: to.clone(),
                    source,
                })?;
            let copied = std::io::copy(
                &mut input.take(expected_size.saturating_add(1)),
                &mut output,
            )
            .map_err(|source| StorageError::CopyPath {
                from: from.clone(),
                to: to.clone(),
                source,
            })?;
            output
                .flush()
                .and_then(|_| output.sync_all())
                .map_err(|source| StorageError::WriteConfig {
                    path: to.clone(),
                    source,
                })?;
            let (bytes, digest) = hash_file(&to)?;
            if copied != expected_size
                || bytes != expected_size
                || entry.sha256.as_deref() != Some(digest.as_str())
            {
                return Err(invalid(
                    &from,
                    "Snapshot checksum failed or source changed during copying",
                ));
            }
        } else {
            if !metadata.is_dir() || entry.sha256.is_some() {
                return Err(invalid(
                    &from,
                    "Snapshot directory does not match its manifest",
                ));
            }
            ensure_plain_directory(&to)?;
        }
    }
    Ok(())
}

pub(super) fn plain_ancestors(path: &Path) -> Result<(), StorageError> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !is_link_or_reparse(&metadata) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(invalid(
                    ancestor,
                    "Snapshot paths cannot contain links, junctions or non-directory ancestors",
                ));
            }
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: ancestor.to_owned(),
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(super) fn require_capacity(root: &Path, bytes: u64) -> Result<(), StorageError> {
    let required = bytes
        .checked_add(CAPACITY_RESERVE)
        .ok_or_else(|| invalid(root, "Snapshot size exceeds capacity accounting"))?;
    let available = available_bytes(root)?;
    if available < required {
        return Err(invalid(
            root,
            format!(
                "Cluster snapshot needs {required} free bytes including 512 MiB headroom; {available} bytes are available"
            ),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn available_bytes(root: &Path) -> Result<u64, StorageError> {
    use std::os::windows::ffi::OsStrExt;
    // std::fs accepts extended-length paths, but the Win32 call does not add
    // that prefix itself. Preserve canonical UTF-16 paths, including UNC roots.
    let canonical = canonical_plain_directory(root, root)?;
    let mut path = canonical.as_os_str().encode_wide().collect::<Vec<_>>();
    if path.last() != Some(&(b'\\' as u16)) {
        path.push(b'\\' as u16);
    }
    path.push(0);
    let mut available = 0;
    // The terminated UTF-16 path and output pointer remain valid for this call.
    let success = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(StorageError::ReadPath {
            path: root.to_owned(),
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(available)
}

#[cfg(not(windows))]
fn available_bytes(root: &Path) -> Result<u64, StorageError> {
    Err(invalid(
        root,
        "Cluster snapshot capacity verification currently requires the supported Windows host",
    ))
}
