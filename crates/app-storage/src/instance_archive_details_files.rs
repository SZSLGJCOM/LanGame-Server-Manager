use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use app_core::{InstanceBackupKind, InstanceBackupResult};

use super::{InstanceArchiveBackups, InstanceArchiveLog};
use crate::instance_archive_files::native;
use crate::instance_archive_store::SavedRow;
use crate::instance_isolation::paths::contains;
use crate::private_runtime_refresh::validated_relative_path;

const MAX_LOG_BYTES: u64 = 64 * 1024;
const MAX_BACKUP_ENTRIES: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;
const MAX_TOTAL_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;

pub(super) fn read_log(
    root: &Path,
    original: &Path,
    rows: &[(i64, &SavedRow)],
) -> InstanceArchiveLog {
    let mut result = InstanceArchiveLog::default();
    let Some((_, row)) = rows.iter().find(|(_, row)| !row["log_path"].is_null()) else {
        return result;
    };
    let read = || -> Result<(String, String, bool), String> {
        let source = row["log_path"]
            .as_str()
            .ok_or_else(|| "Archived log path is not text.".to_owned())?;
        let source = Path::new(source);
        // Never resolve or open the historical path: it may now belong to a
        // different instance. Only its safe relative suffix selects retained bytes.
        if !source.is_absolute() || !contains(original, source) {
            return Err(
                "The historical log is outside the archived instance; it was not read.".into(),
            );
        }
        let relative = source
            .components()
            .skip(original.components().count())
            .collect::<std::path::PathBuf>();
        let relative_text = relative.to_string_lossy().replace('\\', "/");
        let relative =
            validated_relative_path(&relative_text).map_err(|error| error.to_string())?;
        let path = root.join(relative);
        let mut file = native::open_verified_file(&path, false)
            .map_err(|error| format!("Cannot read archived log {relative_text}: {error}"))?;
        let length = file
            .reader()
            .metadata()
            .map_err(|error| error.to_string())?
            .len();
        let truncated = length > MAX_LOG_BYTES;
        file.reader()
            .seek(SeekFrom::Start(length.saturating_sub(MAX_LOG_BYTES)))
            .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        file.reader()
            .take(MAX_LOG_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok((relative_text, text, truncated))
    };
    match read() {
        Ok((relative, text, truncated)) => {
            result.relative_path = Some(relative);
            result.text = text;
            result.truncated = truncated;
        }
        Err(error) => result.issues.push(error),
    }
    result
}

pub(super) fn read_backups(root: &Path, instance_id: &str) -> InstanceArchiveBackups {
    let mut result = InstanceArchiveBackups::default();
    let directory = root.join("backups");
    let _guard = match native::open(&directory, true, false) {
        Ok(guard) => guard,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return result,
        Err(error) => {
            result
                .issues
                .push(format!("Cannot read archived backups: {error}"));
            return result;
        }
    };
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) => {
            result
                .issues
                .push(format!("Cannot list archived backups: {error}"));
            return result;
        }
    };
    let mut remaining_bytes = MAX_TOTAL_MANIFEST_BYTES;
    for (index, entry) in entries.take(MAX_BACKUP_ENTRIES + 1).enumerate() {
        if index == MAX_BACKUP_ENTRIES {
            result.truncated = true;
            result.issues.push("Backup inventory exceeds 256 directory entries; only the first 256 were inspected.".into());
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                result
                    .issues
                    .push(format!("Cannot inspect archived backup entry: {error}"));
                continue;
            }
        };
        let path = entry.path();
        let label = entry.file_name().to_string_lossy().into_owned();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                result
                    .issues
                    .push(format!("Cannot inspect backup {label}: {error}"));
                continue;
            }
        };
        if metadata.is_file() {
            result.issues.push(format!(
                "Retained backup entry {label} has no directory manifest and was not listed."
            ));
            continue;
        }
        match read_backup(&path, instance_id, &label, &mut remaining_bytes) {
            Ok(backup) => result.entries.push(backup),
            Err(BackupReadError::Issue(error)) => result
                .issues
                .push(format!("Cannot read backup {label}: {error}")),
            Err(BackupReadError::Budget) => {
                result.truncated = true;
                result.issues.push("Backup metadata exceeds the combined 2 MiB read limit; remaining manifests were not read.".into());
                break;
            }
        }
    }
    result
        .entries
        .sort_by_key(|backup| std::cmp::Reverse(backup.created_at_unix_ms));
    result
}

enum BackupReadError {
    Issue(String),
    Budget,
}

fn read_backup(
    path: &Path,
    instance_id: &str,
    label: &str,
    remaining_bytes: &mut u64,
) -> Result<InstanceBackupResult, BackupReadError> {
    let issue = |error: std::io::Error| BackupReadError::Issue(error.to_string());
    let _directory = native::open(path, true, false).map_err(issue)?;
    let mut manifest =
        native::open_verified_file(&path.join("backup.json"), false).map_err(issue)?;
    let length = manifest.reader().metadata().map_err(issue)?.len();
    if length > MAX_MANIFEST_BYTES {
        return Err(BackupReadError::Issue(
            "Backup manifest exceeds 256 KiB.".into(),
        ));
    }
    if length > *remaining_bytes {
        return Err(BackupReadError::Budget);
    }
    let mut bytes = Vec::new();
    manifest
        .reader()
        .take(MAX_MANIFEST_BYTES.min(*remaining_bytes) + 1)
        .read_to_end(&mut bytes)
        .map_err(issue)?;
    if bytes.len() as u64 > *remaining_bytes {
        return Err(BackupReadError::Budget);
    }
    *remaining_bytes -= bytes.len() as u64;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(BackupReadError::Issue(
            "Backup manifest exceeds 256 KiB.".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| BackupReadError::Issue(error.to_string()))?;
    let mut backup: InstanceBackupResult = serde_json::from_value(value.clone())
        .map_err(|error| BackupReadError::Issue(error.to_string()))?;
    if backup.backup_id != label || backup.instance_id != instance_id {
        return Err(BackupReadError::Issue(
            "Backup manifest does not belong to this archive and directory.".into(),
        ));
    }
    // Match the normal backup reader's historical kind inference, without
    // trusting any recorded absolute path as filesystem authority.
    if value.get("backup_kind").is_none() {
        let id = backup.backup_id.to_ascii_lowercase();
        backup.backup_kind = if id.starts_with("pre-restore-") {
            InstanceBackupKind::PreRestore
        } else if id.starts_with("auto-stop-") {
            InstanceBackupKind::AutoStop
        } else {
            InstanceBackupKind::Manual
        };
    }
    backup.backup_path = path.to_string_lossy().into_owned();
    Ok(backup)
}
