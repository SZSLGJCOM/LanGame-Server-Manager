use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static MINECRAFT_FILE_PUBLISH_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn write_minecraft_server_file_atomically(
    path: &Path,
    contents: &[u8],
) -> Result<(), SteamCmdError> {
    write_minecraft_server_file_atomically_with(path, contents, replace_file_with_backup)
}

pub(super) fn write_minecraft_server_file_atomically_with<F>(
    path: &Path,
    contents: &[u8],
    replace_existing: F,
) -> Result<(), SteamCmdError>
where
    F: FnOnce(&Path, &Path, &Path) -> std::io::Result<()>,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| SteamCmdError::CreatePath {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let (staging_path, mut staging_file) = create_minecraft_staging_file(path)?;
    if let Err(source) = staging_file
        .write_all(contents)
        .and_then(|_| staging_file.sync_all())
    {
        drop(staging_file);
        let _ = fs::remove_file(&staging_path);
        return Err(SteamCmdError::WriteMinecraftServerFile {
            path: staging_path,
            source,
        });
    }
    drop(staging_file);

    publish_minecraft_staging_file_with(path, &staging_path, replace_existing)
}

pub(super) fn publish_minecraft_staging_file(
    path: &Path,
    staging_path: &Path,
) -> Result<(), SteamCmdError> {
    publish_minecraft_staging_file_with(path, staging_path, replace_file_with_backup)
}

fn publish_minecraft_staging_file_with<F>(
    path: &Path,
    staging_path: &Path,
    replace_existing: F,
) -> Result<(), SteamCmdError>
where
    F: FnOnce(&Path, &Path, &Path) -> std::io::Result<()>,
{
    if !path.exists() {
        return fs::rename(staging_path, path).map_err(|source| {
            let _ = fs::remove_file(staging_path);
            SteamCmdError::WriteMinecraftServerFile {
                path: path.to_path_buf(),
                source,
            }
        });
    }

    let backup_path = minecraft_publish_sibling_path(path, "rollback");
    match replace_existing(path, staging_path, &backup_path) {
        Ok(()) => {
            // Replacement is already committed. A locked backup is safer to
            // leave recoverable than to report a false failed installation.
            let _ = fs::remove_file(&backup_path);
            Ok(())
        }
        Err(source) => {
            let rollback_result = if !path.exists() && backup_path.exists() {
                fs::rename(&backup_path, path)
            } else {
                Ok(())
            };
            let _ = fs::remove_file(staging_path);
            match rollback_result {
                Ok(()) => Err(SteamCmdError::WriteMinecraftServerFile {
                    path: path.to_path_buf(),
                    source,
                }),
                Err(rollback_source) => Err(SteamCmdError::MinecraftServerFileRollbackFailed {
                    path: path.to_path_buf(),
                    backup_path,
                    source,
                    rollback_source,
                }),
            }
        }
    }
}

pub(super) fn create_minecraft_staging_file(
    path: &Path,
) -> Result<(PathBuf, fs::File), SteamCmdError> {
    const MAX_ATTEMPTS: usize = 32;

    let mut last_path = path.to_path_buf();
    for _ in 0..MAX_ATTEMPTS {
        let staging_path = minecraft_publish_sibling_path(path, "download");
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging_path)
        {
            Ok(file) => return Ok((staging_path, file)),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                last_path = staging_path;
            }
            Err(source) => {
                return Err(SteamCmdError::WriteMinecraftServerFile {
                    path: staging_path,
                    source,
                });
            }
        }
    }

    Err(SteamCmdError::WriteMinecraftServerFile {
        path: last_path,
        source: std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not allocate a unique Minecraft staging file",
        ),
    })
}

fn minecraft_publish_sibling_path(path: &Path, phase: &str) -> PathBuf {
    let sequence = MINECRAFT_FILE_PUBLISH_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    // Expand-Archive requires a .zip suffix even when -LiteralPath is used.
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| format!(".{value}"))
        .unwrap_or_default();
    path.with_file_name(format!(
        ".lg-{phase}-{:x}-{stamp:x}-{sequence:x}{extension}",
        std::process::id()
    ))
}

#[cfg(windows)]
fn replace_file_with_backup(
    destination: &Path,
    replacement: &Path,
    backup: &Path,
) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replacement = replacement
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let backup = backup
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let replaced = unsafe {
        ReplaceFileW(
            destination.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_with_backup(
    destination: &Path,
    replacement: &Path,
    _backup: &Path,
) -> std::io::Result<()> {
    fs::rename(replacement, destination)
}

pub(super) fn sha1_file_hex(
    path: &Path,
    deadline: super::InstallDeadline,
    cancellation: Option<super::InstallCancellation>,
) -> Result<String, SteamCmdError> {
    use std::io::Read;
    let io_error = |source| SteamCmdError::WriteMinecraftServerFile {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::open(path).map_err(io_error)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation
            .as_ref()
            .is_some_and(|token| token.is_cancelled())
        {
            return Err(SteamCmdError::InstallCancelled {
                operation: deadline.operation().to_owned(),
            });
        }
        if tokio::time::Instant::now() >= deadline.expires_at() {
            return Err(super::operation_timeout(deadline));
        }
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
