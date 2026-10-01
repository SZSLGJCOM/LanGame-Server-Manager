use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use super::{
    InstanceFilePatchResult, MAX_FILE_BYTES, PreparedInstanceTextPatch, invalid, sha256,
    validate_relative_file, validate_runtime_marker,
};
use crate::StorageError;
use crate::atomic_file::compare_and_swap_file_atomically;

const MAX_BACKUPS: usize = 64;

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Retain each ancestor without delete sharing on Windows so a directory cannot
/// be replaced by a junction between validation and the file operation.
pub(crate) fn guard_directories(path: &Path) -> Result<Vec<File>, StorageError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(invalid(
            path,
            "Managed directories must be absolute without traversal.",
        ));
    }
    let mut guards = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let metadata = fs::symlink_metadata(directory)
            .map_err(|error| invalid(directory, error.to_string()))?;
        if !metadata.is_dir() || is_link(&metadata) {
            return Err(invalid(
                directory,
                "Managed directories cannot be links or reparse points.",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
            };
            options
                .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let guard = options
            .open(directory)
            .map_err(|error| invalid(directory, error.to_string()))?;
        let metadata = guard
            .metadata()
            .map_err(|error| invalid(directory, error.to_string()))?;
        if !metadata.is_dir() || is_link(&metadata) {
            return Err(invalid(
                directory,
                "Opened directory is not a plain directory.",
            ));
        }
        guards.push(guard);
    }
    Ok(guards)
}

pub(crate) fn read_bytes(path: &Path) -> Result<Vec<u8>, StorageError> {
    read_bytes_with_limit(path, MAX_FILE_BYTES)
}

pub(crate) fn read_bytes_with_limit(path: &Path, maximum: usize) -> Result<Vec<u8>, StorageError> {
    let maximum = maximum.min(MAX_FILE_BYTES);
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "File has no parent directory."))?;
    let _guards = guard_directories(parent)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(invalid(
            path,
            "Mod files must be existing ordinary files, not links or reparse points.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ);
    }
    let file = options
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > maximum as u64 {
        return Err(invalid(
            path,
            "File must be ordinary text within the remaining read budget (at most 256 KiB).",
        ));
    }
    let mut bytes = Vec::new();
    file.take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(path, error.to_string()))?;
    if bytes.len() > maximum {
        return Err(invalid(
            path,
            "File exceeds the remaining read budget (at most 256 KiB).",
        ));
    }
    Ok(bytes)
}

pub(super) fn apply_patch(
    prepared: &PreparedInstanceTextPatch,
) -> Result<InstanceFilePatchResult, StorageError> {
    apply_patch_with_readback(prepared, read_bytes)
}

fn apply_patch_with_readback(
    prepared: &PreparedInstanceTextPatch,
    readback: impl FnOnce(&Path) -> Result<Vec<u8>, StorageError>,
) -> Result<InstanceFilePatchResult, StorageError> {
    validate_relative_file(&prepared.preview.file)?;
    let path = prepared.root.join(&prepared.preview.file);
    let parent = path
        .parent()
        .ok_or_else(|| invalid(&path, "Mod file has no parent."))?;
    let _guards = guard_directories(parent)?;
    validate_runtime_marker(&prepared.root, &prepared.preview.file)?;
    if read_bytes(&path)? != prepared.original {
        return Err(invalid(
            &path,
            "The Mod file changed after confirmation preview; no patch was applied.",
        ));
    }
    let backup_id = create_backup(prepared)?;
    let retained_backup_error = |message: String| {
        invalid(
            &path,
            format!(
                "{message} Original retained in backupId={backup_id}; inspect the file before retrying."
            ),
        )
    };
    // The caller holds the lifecycle permit and storage mutation lease. This CAS
    // also rejects an external edit visible at the atomic writer's source read.
    validate_runtime_marker(&prepared.root, &prepared.preview.file)
        .map_err(|error| retained_backup_error(error.to_string()))?;
    if read_bytes(&path).map_err(|error| retained_backup_error(error.to_string()))?
        != prepared.original
    {
        return Err(retained_backup_error(String::from(
            "The source changed while backing up; no patch was applied.",
        )));
    }
    let changed =
        compare_and_swap_file_atomically(&path, &prepared.original, &prepared.replacement)
            .map_err(|error| retained_backup_error(format!("Atomic patch failed: {error}.")))?;
    if !changed {
        return Err(retained_backup_error(String::from(
            "Source content no longer matches the confirmed patch.",
        )));
    }
    let actual = readback(&path).map_err(|error| {
        retained_backup_error(format!(
            "Patch write returned, but read-back failed: {error}."
        ))
    })?;
    if actual != prepared.replacement {
        return Err(retained_backup_error(String::from(
            "Patch read-back differs from the confirmed result.",
        )));
    }
    Ok(InstanceFilePatchResult {
        file: prepared.preview.file.clone(),
        source_sha256: prepared.preview.source_sha256.clone(),
        result_sha256: sha256(&actual),
        backup_id,
        read_back_verified: true,
    })
}

pub(super) fn create_backup(prepared: &PreparedInstanceTextPatch) -> Result<String, StorageError> {
    let mut parent = prepared.root.clone();
    let mut guards = guard_directories(&parent)?;
    for component in ["data", ".langame", "file-patches"] {
        parent.push(component);
        match fs::create_dir(&parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(invalid(&parent, error.to_string())),
        }
        guards.extend(guard_directories(&parent)?);
    }
    // Published and incomplete backups both consume capacity. Never delete a
    // user's recovery material automatically to make room for another patch.
    let mut entries = fs::read_dir(&parent).map_err(|error| invalid(&parent, error.to_string()))?;
    for index in 0..=MAX_BACKUPS {
        match entries.next() {
            Some(Ok(_)) if index < MAX_BACKUPS => {}
            Some(Ok(_)) => {
                return Err(invalid(
                    &parent,
                    "Instance patch backup capacity (64) is full; preserve or remove reviewed backups before making another patch.",
                ));
            }
            Some(Err(error)) => return Err(invalid(&parent, error.to_string())),
            None if index < MAX_BACKUPS => break,
            None => {
                return Err(invalid(
                    &parent,
                    "Instance patch backup capacity (64) is full; preserve or remove reviewed backups before making another patch.",
                ));
            }
        }
    }
    let backup_id = uuid::Uuid::new_v4().to_string();
    let staging = parent.join(format!(".pending-{backup_id}"));
    fs::create_dir(&staging).map_err(|error| invalid(&staging, error.to_string()))?;
    let staging_guards = guard_directories(&staging)?;
    let content = serde_json::to_vec_pretty(&serde_json::json!({
        "version": 1, "instanceId": prepared.instance_id,
        "file": prepared.preview.file, "sourceSha256": prepared.preview.source_sha256,
        "resultSha256": prepared.preview.result_sha256, "originalFile": "original",
    }))
    .map_err(|error| invalid(&staging, error.to_string()))?;
    write_new_synced(&staging.join("original"), &prepared.original)?;
    write_new_synced(&staging.join("manifest.json"), &content)?;
    #[cfg(unix)]
    File::open(&staging)
        .and_then(|file| file.sync_all())
        .map_err(|error| invalid(&staging, error.to_string()))?;
    drop(staging_guards);
    fs::rename(&staging, parent.join(&backup_id))
        .map_err(|error| invalid(&staging, error.to_string()))?;
    #[cfg(unix)]
    File::open(&parent)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            invalid(
                &parent,
                format!(
                    "Backup published as backupId={backup_id}, but directory sync failed: {error}"
                ),
            )
        })?;
    drop(guards);
    Ok(backup_id)
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| invalid(path, error.to_string()))
}

#[cfg(test)]
pub(super) fn apply_with_failed_readback(
    prepared: &PreparedInstanceTextPatch,
) -> Result<InstanceFilePatchResult, StorageError> {
    apply_patch_with_readback(prepared, |path| {
        Err(invalid(path, "injected read-back failure"))
    })
}
