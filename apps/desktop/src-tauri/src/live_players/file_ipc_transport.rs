use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use app_core::ProcessIdentity;

use super::{BridgeError, Game, MAX_BYTES, TIMEOUT, valid_nonce};

pub(super) fn verify_process(pid: u32, identity: &ProcessIdentity) -> Result<(), BridgeError> {
    if app_runtime::process_matches_identity(pid, identity).map_err(|_| BridgeError::Io)? {
        Ok(())
    } else {
        Err(BridgeError::Process)
    }
}

pub(super) fn file_error(error: std::io::Error) -> BridgeError {
    // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION during a short writer window.
    if cfg!(windows) && matches!(error.raw_os_error(), Some(32 | 33)) {
        return BridgeError::Busy;
    }
    match error.kind() {
        std::io::ErrorKind::NotFound => BridgeError::Missing,
        _ => BridgeError::Io,
    }
}

fn is_reparse(metadata: &fs::Metadata) -> bool {
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

fn check_path(root: &Path, path: &Path) -> Result<fs::Metadata, BridgeError> {
    let metadata = fs::symlink_metadata(path).map_err(file_error)?;
    if is_reparse(&metadata)
        || !fs::canonicalize(path)
            .map_err(file_error)?
            .starts_with(root)
    {
        return Err(BridgeError::Io);
    }
    Ok(metadata)
}

pub(super) fn open_directory(root: &Path, path: &Path) -> Result<File, BridgeError> {
    if !check_path(root, path)?.is_dir() {
        return Err(BridgeError::Io);
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
        // Keep this single extension-owned directory in place for the exchange
        // and cleanup. Attribute-only access does not enforce share modes;
        // directory-list access pins the directory but permits file replacement.
        options
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(file_error)?;
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_dir() || is_reparse(&metadata) {
        return Err(BridgeError::Io);
    }
    check_path(root, path)?;
    Ok(file)
}

pub(super) fn read_response(root: &Path, path: &Path) -> Result<Vec<u8>, BridgeError> {
    if !check_path(root, path)?.is_file() {
        return Err(BridgeError::Io);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).map_err(file_error)?;
    // Inspect the opened file, because an atomic rename can replace the leaf
    // between the path check and open. Never follow a replacement reparse point.
    let metadata = file.metadata().map_err(file_error)?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err(BridgeError::Io);
    }
    if metadata.len() > MAX_BYTES as u64 {
        return Err(BridgeError::Limit);
    }
    let mut body = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(file_error)?;
    if body.len() > MAX_BYTES {
        return Err(BridgeError::Limit);
    }
    Ok(body)
}

pub(super) fn response_for_request(
    root: &Path,
    path: &Path,
    nonce: &str,
) -> Result<Option<Vec<u8>>, BridgeError> {
    let body = match read_response(root, path) {
        Ok(body) => body,
        Err(BridgeError::Missing | BridgeError::Busy) => return Ok(None),
        Err(error) => return Err(error),
    };
    // A replacement gap, in-progress JSON write or old response is pending,
    // never a complete empty snapshot. The caller retains the overall deadline.
    let Ok(response) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return Ok(None);
    };
    Ok((response
        .get("request_id")
        .and_then(serde_json::Value::as_str)
        == Some(nonce))
    .then_some(body))
}

struct RequestFile(PathBuf);
impl Drop for RequestFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn replace_request(source: &Path, destination: &Path) -> Result<(), BridgeError> {
    let error = match fs::rename(source, destination) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    #[cfg(windows)]
    if error.raw_os_error() == Some(5) {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_FLAG_OPEN_REPARSE_POINT};
        // MoveFileExW reports access denied for a destination opened without
        // delete sharing; Rust's fallback preserves that error. Probe DELETE
        // access without deleting anything to distinguish a reader from an ACL
        // denial. A successful/missing probe means the reader already moved on.
        return match OpenOptions::new()
            .access_mode(DELETE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(destination)
        {
            Ok(_) => Err(BridgeError::Busy),
            Err(error) => Err(match file_error(error) {
                BridgeError::Missing => BridgeError::Busy,
                error => error,
            }),
        };
    }
    Err(file_error(error))
}

pub(super) fn publish_request(
    root: &Path,
    directory: &Path,
    nonce: &str,
    observed_at: u64,
    deadline: Instant,
) -> Result<(), BridgeError> {
    if !valid_nonce(nonce) {
        return Err(BridgeError::Incomplete);
    }
    let request_path = directory.join("request.json");
    match check_path(root, &request_path) {
        Ok(metadata) if metadata.is_file() => {}
        Err(BridgeError::Missing) => {}
        Ok(_) => return Err(BridgeError::Io),
        Err(error) => return Err(error),
    }
    let temporary_path = directory.join(format!("request_{nonce}.json"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .map_err(file_error)?;
    // Ownership starts only after create_new succeeds; a pre-existing path
    // must never be removed by this request's cleanup.
    let temporary = RequestFile(temporary_path);
    let request = serde_json::json!({"request_id": nonce, "requested_at": observed_at / 1000});
    file.write_all(request.to_string().as_bytes())
        .map_err(file_error)?;
    drop(file);
    // Rust rename replaces an existing file on Windows too. Removing the
    // destination first would introduce an unnecessary request-file gap.
    loop {
        match replace_request(&temporary.0, &request_path) {
            Ok(()) => return Ok(()),
            Err(BridgeError::Busy) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(BridgeError::Busy) => return Err(BridgeError::Timeout),
            Err(error) => return Err(error),
        }
    }
}

pub(super) fn exchange(
    game: Game,
    pid: u32,
    identity: &ProcessIdentity,
    nonce: &str,
    observed_at: u64,
) -> Result<Vec<u8>, BridgeError> {
    let deadline = Instant::now() + TIMEOUT;
    verify_process(pid, identity)?;
    let root = fs::canonicalize(game.install_root(Path::new(&identity.image_path))?)
        .map_err(|_| BridgeError::Io)?;
    let directory = root.join("langame_player_query");
    // Extension preparation creates this directory before launch. Its absence
    // never causes a native installation to be changed by a refresh request.
    let _directory = open_directory(&root, &directory)?;
    publish_request(&root, &directory, nonce, observed_at, deadline)?;
    let response_path = directory.join("response.json");
    loop {
        if Instant::now() >= deadline {
            return Err(BridgeError::Timeout);
        }
        verify_process(pid, identity)?;
        if let Some(body) = response_for_request(&root, &response_path, nonce)? {
            verify_process(pid, identity)?;
            if Instant::now() >= deadline {
                return Err(BridgeError::Timeout);
            }
            return Ok(body);
        }
        if Instant::now() >= deadline {
            return Err(BridgeError::Timeout);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
