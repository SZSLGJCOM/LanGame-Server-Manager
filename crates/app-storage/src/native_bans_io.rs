//! Bounded reads shared by native ban projections. No reader creates directories
//! or files. Windows handles reject links and prevent directory/file replacement.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Component, Path};

use crate::StorageError;
use crate::instance_file_patch::io::{guard_directories, is_link};

pub(super) fn invalid(module: &str, path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: module.to_owned(),
        path: path.to_owned(),
        message: message.into(),
    }
}

/// Missing native optional directories are an empty observation. Existing
/// ancestors are still checked and retained so absence never bypasses link guards.
pub(super) fn guard_optional_directory(
    path: &Path,
    module: &str,
) -> Result<Option<Vec<File>>, StorageError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid(
            module,
            path,
            "Native ban paths must be absolute without traversal.",
        ));
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let guards = guard_directories(ancestor)?;
                return Ok((ancestor == path).then_some(guards));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(invalid(module, ancestor, error.to_string())),
        }
    }
    Err(invalid(
        module,
        path,
        "No existing ancestor for the native ban path.",
    ))
}

pub(super) fn read_optional_native_file(
    path: &Path,
    module: &str,
    maximum: usize,
) -> Result<Option<Vec<u8>>, StorageError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(module, path, "Native ban file has no parent."))?;
    let Some(_guards) = guard_optional_directory(parent, module)? else {
        return Ok(None);
    };
    let before = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
    };
    if !before.is_file() || is_link(&before) || before.len() > maximum as u64 {
        return Err(invalid(
            module,
            path,
            "Native ban data must be a bounded regular file without links or reparse points.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        options
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_owned(),
            source,
        })?;
    let opened = file
        .metadata()
        .map_err(|_| invalid(module, path, "Cannot inspect opened native ban data."))?;
    if !opened.is_file() || is_link(&opened) || opened.len() > maximum as u64 {
        return Err(invalid(
            module,
            path,
            "Opened native ban data is not a bounded regular file.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != opened.dev() || before.ino() != opened.ino() {
            return Err(invalid(
                module,
                path,
                "Native ban data changed while opening it.",
            ));
        }
    }
    let mut bytes = Vec::new();
    (&file)
        .take(maximum.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadConfig {
            path: path.to_owned(),
            source,
        })?;
    let after = file
        .metadata()
        .map_err(|_| invalid(module, path, "Cannot recheck native ban data."))?;
    let current = fs::symlink_metadata(path)
        .map_err(|_| invalid(module, path, "Native ban data changed while reading it."))?;
    if bytes.len() > maximum
        || bytes.len() as u64 != opened.len()
        || after.len() != opened.len()
        || after.modified().ok() != opened.modified().ok()
        || !current.is_file()
        || is_link(&current)
        || current.len() != opened.len()
        || current.modified().ok() != opened.modified().ok()
    {
        return Err(invalid(
            module,
            path,
            "Native ban data changed or exceeded its read limit.",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if current.dev() != opened.dev() || current.ino() != opened.ino() {
            return Err(invalid(
                module,
                path,
                "Native ban data was replaced while reading it.",
            ));
        }
    }
    Ok(Some(bytes))
}

pub(super) fn decode(bytes: &[u8], module: &str, path: &Path) -> Result<String, StorageError> {
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err(invalid(
                module,
                path,
                "Native ban UTF-16LE data has an incomplete code unit.",
            ));
        }
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&units)
            .map_err(|_| invalid(module, path, "Native ban data contains invalid UTF-16LE."))
    } else {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            invalid(
                module,
                path,
                "Native ban data is not valid UTF-8 or BOM-marked UTF-16LE.",
            )
        })
    }
}
