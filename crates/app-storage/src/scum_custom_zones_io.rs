use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::StorageError;
use crate::instance_file_patch::io::{guard_directories, is_link};
use crate::managed_console_log::owned_fs::identity;

const VERIFIED_IMAGE: &str = "070fcad918ba04eb9797db29674cadbd6691427b936fe354bca23b008c8c9971";

pub(super) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::WriteConfig {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.into()),
    }
}

fn open_regular(path: &Path, write: bool) -> Result<File, StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| invalid(path, error.to_string()))?;
    if !metadata.is_file() || is_link(&metadata) || (write && metadata.permissions().readonly()) {
        return Err(invalid(
            path,
            "SCUM data must be ordinary files; read-only databases cannot be edited.",
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
        // Retain the actual file without delete sharing while SQLite opens it.
        options
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    }
    let file = options
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    if !file
        .metadata()
        .is_ok_and(|value| value.is_file() && !is_link(&value))
    {
        return Err(invalid(
            path,
            "The opened SCUM file is not an ordinary file.",
        ));
    }
    Ok(file)
}

pub(super) fn verified_layout(install: &Path) -> Result<bool, StorageError> {
    let path = install.join("SCUM/Binaries/Win64/SCUMServer.exe");
    if !path
        .try_exists()
        .map_err(|error| invalid(&path, error.to_string()))?
    {
        return Ok(false);
    }
    let _directories = guard_directories(
        path.parent()
            .ok_or_else(|| invalid(&path, "Missing program directory."))?,
    )?;
    let mut file = open_regular(&path, false)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let length = file
            .read(&mut buffer)
            .map_err(|error| invalid(&path, error.to_string()))?;
        if length == 0 {
            break;
        }
        hash.update(&buffer[..length]);
    }
    let digest: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(digest == VERIFIED_IMAGE)
}

pub(super) struct DatabaseAnchor {
    pub path: PathBuf,
    pub identity: String,
    _file: File,
    _directories: Vec<File>,
}

impl DatabaseAnchor {
    pub fn open(path: &Path, write: bool) -> Result<Self, StorageError> {
        let parent = path
            .parent()
            .ok_or_else(|| invalid(path, "Missing database directory."))?;
        let directories = guard_directories(parent)?;
        let file = open_regular(path, write)?;
        let identity = serde_json::to_string(
            &identity(&file).map_err(|error| invalid(path, error.to_string()))?,
        )?;
        let result = Self {
            path: path.to_path_buf(),
            identity,
            _file: file,
            _directories: directories,
        };
        result.check_auxiliary_files()?;
        Ok(result)
    }

    pub fn check_auxiliary_files(&self) -> Result<(), StorageError> {
        // SCUM uses WAL. SQLite consumes these fixed siblings; none may redirect
        // an otherwise valid instance database through a reparse point.
        for suffix in ["-wal", "-shm", "-journal"] {
            let path = PathBuf::from(format!(
                "{}{suffix}",
                self.path.as_os_str().to_string_lossy()
            ));
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() && !is_link(&metadata) => {}
                Ok(_) => {
                    return Err(invalid(
                        &path,
                        "SQLite auxiliary files cannot be links or reparse points.",
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(invalid(&path, error.to_string())),
            }
        }
        Ok(())
    }
}
