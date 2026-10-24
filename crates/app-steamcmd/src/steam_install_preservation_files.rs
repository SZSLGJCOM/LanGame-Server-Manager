use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::steam_depot::{bounded, check_plain, invalid, open_read, relative};

const MAX_ENTRIES: usize = 200_000;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Entry {
    pub path: String,
    pub directory: bool,
    pub bytes: u64,
    pub sha256: String,
}

pub(super) fn pin_root(path: &Path) -> io::Result<(File, String)> {
    check_plain(path, true)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let id = file_identity(&file, true)?;
    if identity(path, true)? != id {
        return Err(invalid("program root changed during admission"));
    }
    Ok((file, id))
}

pub(super) fn identity(path: &Path, directory: bool) -> io::Result<String> {
    check_plain(path, directory)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    file_identity(&options.open(path)?, directory)
}

fn file_identity(file: &File, directory: bool) -> io::Result<String> {
    let metadata = file.metadata()?;
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(invalid("retained data has an unexpected entry type"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // SAFETY: the live File owns the handle; the API initializes info on success.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), info.as_mut_ptr()) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the successful call initialized every field.
        let info = unsafe { info.assume_init() };
        if info.dwFileAttributes & 0x400 != 0 || (!directory && info.nNumberOfLinks != 1) {
            return Err(invalid(
                "retained data contains a reparse point or hard link",
            ));
        }
        Ok(format!(
            "{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !directory && metadata.nlink() != 1 {
            return Err(invalid("retained data contains a hard link"));
        }
        Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(invalid(
            "retained data identity is unsupported on this platform",
        ))
    }
}

pub(super) fn ensure_directory(path: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        match fs::symlink_metadata(current) {
            Ok(_) => {
                check_plain(current, true)?;
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_owned());
                current = current
                    .parent()
                    .ok_or_else(|| invalid("directory has no existing ancestor"))?;
            }
            Err(error) => return Err(error),
        }
    }
    for directory in missing.into_iter().rev() {
        fs::create_dir(&directory)?;
        check_plain(&directory, true)?;
    }
    Ok(())
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    ensure_directory(path.parent().ok_or_else(|| invalid("file has no parent"))?)?;
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    output.write_all(bytes)?;
    output.sync_all()
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&bounded(path, 64 * 1024 * 1024)?).map_err(invalid)
}

pub(super) fn fingerprint(
    path: &Path,
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<(u64, String)> {
    let mut file = open_read(path)?;
    let before = file_identity(&file, false)?;
    let result = hash_reader(&mut file, check)?;
    if identity(path, false)? != before || file.metadata()?.len() != result.0 {
        return Err(invalid("retained file changed while it was read"));
    }
    Ok(result)
}

fn hash_reader(
    file: &mut File,
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<(u64, String)> {
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| invalid("retained file size overflow"))?;
        hash.update(&buffer[..count]);
    }
    Ok((
        size,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

pub(super) fn scan(
    root: &Path,
    selected: &[String],
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<Vec<Entry>> {
    let mut entries = BTreeMap::new();
    let mut pending = selected.to_vec();
    while let Some(key) = pending.pop() {
        check()?;
        relative(&key)?;
        if entries.contains_key(&key) {
            continue;
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(invalid("retained data exceeds its entry limit"));
        }
        let path = root.join(&key);
        let metadata = fs::symlink_metadata(&path)?;
        let directory = metadata.is_dir();
        identity(&path, directory)?;
        let (bytes, sha256) = if directory {
            for child in fs::read_dir(&path)? {
                let child = child?;
                let name = child
                    .file_name()
                    .into_string()
                    .map_err(|_| invalid("retained path is not UTF-8"))?;
                if !name.to_ascii_lowercase().starts_with(".langame-") {
                    pending.push(format!("{key}/{name}"));
                }
            }
            (0, String::new())
        } else {
            fingerprint(&path, check)?
        };
        entries.insert(
            key.clone(),
            Entry {
                path: key,
                directory,
                bytes,
                sha256,
            },
        );
    }
    Ok(entries.into_values().collect())
}

pub(super) fn copy_verified(
    source: &Path,
    target: &Path,
    expected: &Entry,
    check: &mut dyn FnMut() -> io::Result<()>,
) -> io::Result<()> {
    ensure_directory(
        target
            .parent()
            .ok_or_else(|| invalid("copy target has no parent"))?,
    )?;
    let mut input = open_read(source)?;
    let before = file_identity(&input, false)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    let mut buffer = [0; 64 * 1024];
    let mut written = 0_u64;
    loop {
        check()?;
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        written = written
            .checked_add(count as u64)
            .ok_or_else(|| invalid("retained copy size overflow"))?;
        if written > expected.bytes {
            return Err(invalid("retained file grew during copying"));
        }
        output.write_all(&buffer[..count])?;
    }
    output.sync_all()?;
    output.set_times(fs::FileTimes::new().set_modified(input.metadata()?.modified()?))?;
    drop(output);
    if identity(source, false)? != before
        || fingerprint(target, check)? != (expected.bytes, expected.sha256.clone())
    {
        return Err(invalid("retained data copy failed byte verification"));
    }
    Ok(())
}

pub(super) fn verify(root: &Path, entries: &[Entry]) -> io::Result<()> {
    for entry in entries {
        relative(&entry.path)?;
        let path = root.join(&entry.path);
        if entry.directory {
            identity(&path, true)?;
        } else if fingerprint(&path, &mut || Ok(()))? != (entry.bytes, entry.sha256.clone()) {
            return Err(invalid(format!(
                "retained data checksum mismatch: {}",
                entry.path
            )));
        }
    }
    Ok(())
}

pub(super) fn unique_sibling(path: &Path, label: &str) -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(
        ".langame-{label}-{}-{stamp}-{sequence}",
        std::process::id()
    ))
}

pub(super) fn restore(root: &Path, transaction: &Path, entries: &[Entry]) -> io::Result<()> {
    let originals = transaction.join("originals");
    verify(&originals, entries)?;
    for entry in entries {
        let target = root.join(&entry.path);
        if entry.directory {
            ensure_directory(&target)?;
            continue;
        }
        let exists = match fs::symlink_metadata(&target) {
            Ok(_) => {
                if fingerprint(&target, &mut || Ok(()))? == (entry.bytes, entry.sha256.clone()) {
                    continue;
                }
                true
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        ensure_directory(
            target
                .parent()
                .ok_or_else(|| invalid("restore target has no parent"))?,
        )?;
        let stage = unique_sibling(&target, "retained-restore");
        copy_verified(&originals.join(&entry.path), &stage, entry, &mut || Ok(()))?;
        if exists {
            // ReplaceFile preserves the exact displaced file atomically. It may
            // contain an external edit, so it is retained even after success.
            let changed = transaction.join("changed").join(&entry.path);
            ensure_directory(
                changed
                    .parent()
                    .ok_or_else(|| invalid("changed file has no parent"))?,
            )?;
            let changed = if changed.exists() {
                unique_sibling(&changed, "changed")
            } else {
                changed
            };
            identity(&target, false)?;
            if let Err(error) = replace_with_backup(&target, &stage, &changed) {
                // The recovery inventory remains authoritative. This unique
                // staging copy has no user references and need not be retried.
                if let Err(cleanup) = fs::remove_file(&stage)
                    && cleanup.kind() != io::ErrorKind::NotFound
                {
                    return Err(invalid(format!(
                        "retained replacement failed: {error}; staging cleanup failed: {cleanup}"
                    )));
                }
                return Err(error);
            }
        } else {
            // A hard-link publication fails if another target appears. Removing
            // our staging name leaves one ordinary file, without overwriting it.
            fs::hard_link(&stage, &target)?;
            fs::remove_file(&stage)?;
        }
    }
    verify(root, entries)
}

#[cfg(windows)]
fn replace_with_backup(target: &Path, stage: &Path, backup: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
    let wide = |path: &Path| {
        path.as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    if backup.try_exists()? {
        return Err(invalid("retained replacement backup already exists"));
    }
    let (target, stage, backup) = (wide(target), wide(stage), wide(backup));
    // SAFETY: all three terminated UTF-16 buffers remain live during the call.
    if unsafe {
        ReplaceFileW(
            target.as_ptr(),
            stage.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_with_backup(target: &Path, stage: &Path, backup: &Path) -> io::Result<()> {
    fs::hard_link(target, backup)?;
    fs::rename(stage, target)
}

pub(super) fn remove_originals(transaction: &Path, entries: &[Entry]) -> io::Result<()> {
    let root = transaction.join("originals");
    verify(&root, entries)?;
    for entry in entries.iter().filter(|entry| !entry.directory) {
        fs::remove_file(root.join(&entry.path))?;
    }
    // Remove only empty directories, including intermediate selection parents.
    let mut directories = std::collections::BTreeSet::new();
    for entry in entries {
        for ancestor in Path::new(&entry.path)
            .ancestors()
            .skip(usize::from(!entry.directory))
        {
            directories.insert(ancestor.to_owned());
        }
    }
    let mut directories: Vec<_> = directories.into_iter().collect();
    directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in directories {
        fs::remove_dir(root.join(path))?;
    }
    Ok(())
}
