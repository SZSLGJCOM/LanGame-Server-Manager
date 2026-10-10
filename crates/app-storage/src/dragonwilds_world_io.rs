use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::StorageError;
use crate::instance_file_patch::io::{guard_directories, is_link};

pub(super) const MAX_WORLD_BYTES: usize = 128 * 1024 * 1024;
const MAX_DIRECTORY_ENTRIES: usize = 256;

pub(super) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::WriteConfig {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.into()),
    }
}

pub(super) fn latest_world(
    root: &Path,
    default_world_name: &str,
) -> Result<Option<PathBuf>, StorageError> {
    if !root
        .try_exists()
        .map_err(|error| invalid(root, error.to_string()))?
    {
        return Ok(None);
    }
    let _guards = guard_directories(root)?;
    let entries = fs::read_dir(root).map_err(|error| invalid(root, error.to_string()))?;
    let mut latest: Option<(i64, PathBuf)> = None;
    let mut ambiguous = false;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(invalid(
                root,
                "Too many entries in the world save directory (maximum 256).",
            ));
        }
        let entry = entry.map_err(|error| invalid(root, error.to_string()))?;
        let path = entry.path();
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sav"))
        {
            continue;
        }
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| invalid(&path, error.to_string()))?;
        if !metadata.is_file() || is_link(&metadata) {
            return Err(invalid(
                &path,
                "World saves must be ordinary files, without links or reparse points.",
            ));
        }
        let info = read_world_metadata(&path)?;
        if !info.name.eq_ignore_ascii_case(default_world_name) {
            continue;
        }
        let modified = info.saved_at_ticks;
        if let Some((last_modified, last_path)) = &latest {
            if *last_modified == modified {
                ambiguous = true;
                continue;
            }
            if *last_modified > modified {
                continue;
            }
            if last_path == &path {
                continue;
            }
        }
        ambiguous = false;
        latest = Some((modified, path));
    }
    if ambiguous {
        return Err(invalid(
            root,
            "Several matching worlds have the same native save timestamp. The server's choice cannot be identified safely.",
        ));
    }
    Ok(latest.map(|(_, path)| path))
}

fn read_world_metadata(
    path: &Path,
) -> Result<crate::dragonwilds_save::metadata::WorldMetadata, StorageError> {
    let mut file = open_world(path)?;
    let mut header = [0; 16];
    file.read_exact(&mut header)
        .map_err(|error| invalid(path, error.to_string()))?;
    if &header[..4] != b"SAVE" || &header[8..12] != b"INFO" {
        return Err(invalid(
            path,
            "World metadata is not a supported Dragonwilds SAVE/INFO file.",
        ));
    }
    let length = u32::from_le_bytes(
        header[4..8]
            .try_into()
            .map_err(|_| invalid(path, "Invalid SAVE header."))?,
    ) as u64
        + 8;
    if length
        != file
            .metadata()
            .map_err(|error| invalid(path, error.to_string()))?
            .len()
    {
        return Err(invalid(
            path,
            "World save length changed or its metadata is truncated.",
        ));
    }
    let info_length = u32::from_le_bytes(
        header[12..16]
            .try_into()
            .map_err(|_| invalid(path, "Invalid INFO header."))?,
    ) as usize;
    if info_length > 1024 * 1024 || info_length as u64 > length.saturating_sub(16) {
        return Err(invalid(
            path,
            "World INFO exceeds the 1 MiB metadata limit.",
        ));
    }
    let mut info = vec![0; info_length];
    file.read_exact(&mut info)
        .map_err(|error| invalid(path, error.to_string()))?;
    crate::dragonwilds_save::metadata::read_world_metadata_info(&info)
        .map_err(|message| invalid(path, message))
}

fn open_world(path: &Path) -> Result<File, StorageError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "World save has no parent directory."))?;
    let _guards = guard_directories(parent)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_WORLD_BYTES as u64 {
        return Err(invalid(
            path,
            "World save must be an ordinary file no larger than 128 MiB.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        // A read-only snapshot must not block the game's own atomic autosave.
        // The codec detects partial/inconsistent snapshots; commits separately
        // require a stopped instance and compare the exact original bytes.
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    let file: File = options
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    let metadata = file
        .metadata()
        .map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > MAX_WORLD_BYTES as u64 {
        return Err(invalid(
            path,
            "Opened world save is not an ordinary bounded file.",
        ));
    }
    Ok(file)
}

pub(super) fn read_world(path: &Path) -> Result<Vec<u8>, StorageError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "World save has no parent directory."))?;
    let _guards = guard_directories(parent)?;
    let file = open_world(path)?;
    let mut bytes = Vec::new();
    file.take((MAX_WORLD_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(path, error.to_string()))?;
    if bytes.len() > MAX_WORLD_BYTES {
        return Err(invalid(path, "World save exceeded the 128 MiB read limit."));
    }
    Ok(bytes)
}

pub(super) fn world_file_name(path: &Path) -> Result<String, StorageError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(String::from)
        .ok_or_else(|| invalid(path, "World filename must be valid UTF-8."))
}
