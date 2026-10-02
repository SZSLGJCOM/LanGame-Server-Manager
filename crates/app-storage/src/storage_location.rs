use std::fs::{self, OpenOptions, TryLockError};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::atomic_file::create_file_atomically;
use crate::{StorageError, StoragePaths};

const LOCATION_FILE: &str = "storage-location.json";
const MAX_LOCATION_BYTES: u64 = 8192;

#[path = "storage_candidate.rs"]
mod candidate;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageLocation {
    version: u32,
    runtime_root: PathBuf,
    app_data_root: PathBuf,
}

pub(crate) fn resolve_default_paths() -> Result<StoragePaths, StorageError> {
    let user_root = crate::default_app_data_root();
    resolve_paths(&user_root, || {
        #[cfg(windows)]
        {
            crate::storage_volumes::available_volumes().map(|volumes| {
                volumes
                    .into_iter()
                    .map(|volume| volume.root.join("LanGame"))
                    .collect()
            })
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| vec![PathBuf::from(home).join("LanGame")])
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "Home directory unavailable")
                })
        }
    })
}

pub(crate) fn resolve_in_directory(parent: &Path) -> Result<StoragePaths, StorageError> {
    resolve_selected_directory(&crate::default_app_data_root(), parent)
}

fn resolve_selected_directory(
    user_root: &Path,
    parent: &Path,
) -> Result<StoragePaths, StorageError> {
    resolve_paths(user_root, || {
        if !parent.is_absolute()
            || parent
                .components()
                .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "请选择绝对路径表示的本地文件夹。",
            ));
        }
        #[cfg(windows)]
        if !matches!(parent.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "请选择本地磁盘上的文件夹。",
            ));
        }
        let root = if parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("LanGame"))
        {
            parent.with_file_name("LanGame")
        } else {
            parent.join("LanGame")
        };
        Ok(vec![root])
    })
}

fn resolve_paths(
    user_root: &Path,
    candidates: impl FnOnce() -> io::Result<Vec<PathBuf>>,
) -> Result<StoragePaths, StorageError> {
    if let Some(paths) = existing_paths(user_root)? {
        return Ok(paths);
    }
    create_normal_directory(user_root)?;
    let lock_path = user_root.join("storage-location.lock");
    check_plain_file_if_present(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|source| StorageError::WriteConfig {
            path: lock_path.clone(),
            source,
        })?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                return Err(StorageError::WriteConfig {
                    path: lock_path,
                    source: io::Error::other(format!(
                        "Cannot initialize the data location: {error}"
                    )),
                });
            }
        }
    }
    // The desktop and its runtime can arrive together. A complete location is
    // published once, under a per-user OS file lock that is released on exit.
    if let Some(paths) = existing_paths(user_root)? {
        return Ok(paths);
    }
    let roots = candidates().map_err(|source| {
        if source.kind() == io::ErrorKind::InvalidInput {
            StorageError::NoUsableStorageLocation {
                details: source.to_string(),
            }
        } else {
            StorageError::ReadPath {
                path: user_root.to_owned(),
                source,
            }
        }
    })?;
    let mut failures = Vec::new();
    for root in roots {
        match prepare_candidate(&root) {
            Ok((paths, _candidate_guard)) => {
                let location = StorageLocation {
                    version: 1,
                    runtime_root: root,
                    app_data_root: paths.app_data_root.clone(),
                };
                let pointer = user_root.join(LOCATION_FILE);
                create_file_atomically(&pointer, &serde_json::to_vec_pretty(&location)?).map_err(
                    |source| StorageError::WriteConfig {
                        path: pointer,
                        source,
                    },
                )?;
                return Ok(paths);
            }
            Err(error) => failures.push(format!("{}: {error}", root.display())),
        }
    }
    Err(StorageError::NoUsableStorageLocation {
        details: if failures.is_empty() {
            "未找到可用的本地固定磁盘。".to_owned()
        } else {
            failures.join("\n")
        },
    })
}

pub(crate) fn existing_paths(user_root: &Path) -> Result<Option<StoragePaths>, StorageError> {
    let pointer = user_root.join(LOCATION_FILE);
    if check_plain_file_if_present(&pointer)? {
        let mut text = String::new();
        fs::File::open(&pointer)
            .and_then(|file| file.take(MAX_LOCATION_BYTES + 1).read_to_string(&mut text))
            .map_err(|source| StorageError::ReadConfig {
                path: pointer.clone(),
                source,
            })?;
        if text.len() as u64 > MAX_LOCATION_BYTES {
            return Err(invalid_location(
                &pointer,
                "Data location file is too large",
            ));
        }
        let location: StorageLocation = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|source| StorageError::InvalidConfigJson {
            path: pointer.clone(),
            source,
        })?;
        validate_location(&pointer, &location)?;
        // Never recreate an unavailable selected directory or reselect another
        // disk: that would hide the user's database behind a fresh installation.
        require_normal_directory(&location.runtime_root)?;
        require_normal_directory(&location.app_data_root)?;
        let paths = StoragePaths::from_data_roots(location.app_data_root, location.runtime_root);
        if !check_plain_file_if_present(&paths.settings_path)? {
            return Err(invalid_location(
                &paths.settings_path,
                "Saved data settings are missing; restore the selected data directory",
            ));
        }
        if !check_plain_file_if_present(&paths.database_path)? {
            return Err(invalid_location(
                &paths.database_path,
                "Saved database is missing; restore the selected data directory",
            ));
        }
        return Ok(Some(paths));
    }

    if check_plain_file_if_present(&user_root.join("settings.json"))?
        || check_plain_file_if_present(&user_root.join("db/lgs.db"))?
    {
        // Existing installations retain their current-user database and paths.
        // In particular, an old local runtime must not move when a D: disk is added.
        let local_runtime = user_root.join("runtime");
        let runtime_root = if local_runtime.is_dir() {
            local_runtime
        } else {
            crate::default_runtime_root(user_root)
        };
        return Ok(Some(StoragePaths::from_data_roots(
            user_root.to_owned(),
            runtime_root,
        )));
    }
    Ok(None)
}

fn validate_location(path: &Path, location: &StorageLocation) -> Result<(), StorageError> {
    let private_parent = location.runtime_root.join("app-data/ServerManager");
    let valid_path = |value: &Path| {
        value.is_absolute()
            && !value
                .components()
                .any(|part| matches!(part, Component::ParentDir))
    };
    if location.version != 1
        || !valid_path(&location.runtime_root)
        || !valid_path(&location.app_data_root)
        || location
            .runtime_root
            .file_name()
            .and_then(|name| name.to_str())
            != Some("LanGame")
        || location.app_data_root.parent() != Some(private_parent.as_path())
        || location
            .app_data_root
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| Uuid::parse_str(name).ok())
            .is_none()
    {
        return Err(invalid_location(
            path,
            "Invalid saved LanGame data location",
        ));
    }
    Ok(())
}

fn prepare_candidate(
    root: &Path,
) -> Result<(StoragePaths, candidate::CandidateGuard), StorageError> {
    let guard = candidate::prepare(root)?;
    probe_writable_directory(root)?;
    for directory in [
        root.join("cmd/steamcmd"),
        root.join("server-files"),
        root.join("instances"),
        root.join("instances/.trash"),
    ] {
        probe_writable_directory(&directory)?;
    }
    let private_parent = root.join("app-data/ServerManager");
    // Each account has its own pointer and private metadata. Do not adopt an
    // existing account's database simply because it shares the selected disk.
    let app_data_root = private_parent.join(Uuid::new_v4().simple().to_string());
    crate::storage_private_directory::create_private_directory(&app_data_root).map_err(
        |source| StorageError::CreatePath {
            path: app_data_root.clone(),
            source,
        },
    )?;
    let paths = StoragePaths::from_data_roots(app_data_root, root.to_owned());
    // A candidate is usable only when all required roots can be prepared.
    // Publish the pointer last so readers never observe partial initialization.
    crate::bootstrap_storage_with_paths(paths.clone())?;
    create_file_atomically(
        &paths.settings_path,
        &serde_json::to_vec_pretty(&paths.settings())?,
    )
    .map_err(|source| StorageError::WriteConfig {
        path: paths.settings_path.clone(),
        source,
    })?;
    initialize_new_database(&paths)?;
    Ok((paths, guard))
}

fn probe_writable_directory(root: &Path) -> Result<(), StorageError> {
    let probe = root.join(format!(".lgsm-write-probe-{}", Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|source| StorageError::CreatePath {
            path: probe.clone(),
            source,
        })?;
    let writable = file
        .write_all(b"LanGame data location\n")
        .and_then(|()| file.sync_all());
    drop(file);
    fs::remove_file(&probe).map_err(|source| StorageError::DeletePath {
        path: probe.clone(),
        source,
    })?;
    writable.map_err(|source| StorageError::WriteConfig {
        path: probe,
        source,
    })?;
    Ok(())
}

fn initialize_new_database(paths: &StoragePaths) -> Result<(), StorageError> {
    let owned_paths = paths.clone();
    // This synchronous startup API is also called from Tokio. A short-lived
    // thread avoids nesting runtimes; no location is published until the
    // database has a complete schema and its connections have closed.
    std::thread::Builder::new()
        .name("lgsm-storage-init".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|source| StorageError::CreatePath {
                    path: owned_paths.database_path.clone(),
                    source,
                })?;
            runtime.block_on(crate::initialize_database(&owned_paths))?;
            Ok(())
        })
        .map_err(|source| StorageError::CreatePath {
            path: paths.database_path.clone(),
            source,
        })?
        .join()
        .map_err(|_| {
            invalid_location(
                &paths.database_path,
                "Database initialization thread failed",
            )
        })?
}

fn create_normal_directory(path: &Path) -> Result<(), StorageError> {
    check_normal_ancestors(path)?;
    fs::create_dir_all(path).map_err(|source| StorageError::CreatePath {
        path: path.to_owned(),
        source,
    })?;
    require_normal_directory(path)
}

fn require_normal_directory(path: &Path) -> Result<(), StorageError> {
    check_normal_ancestors(path)?;
    if !crate::instance_archive_files::plain_directory(path)? {
        return Err(invalid_location(
            path,
            "Selected data directory is unavailable; reconnect its disk or restore the directory",
        ));
    }
    Ok(())
}

fn check_normal_ancestors(path: &Path) -> Result<(), StorageError> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        crate::instance_archive_files::plain_directory(ancestor)?;
    }
    Ok(())
}

fn check_plain_file_if_present(path: &Path) -> Result<bool, StorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || crate::private_runtime::is_reparse_point(path)?
    {
        return Err(invalid_location(
            path,
            "Expected a regular data configuration file",
        ));
    }
    if let Some(parent) = path.parent() {
        check_normal_ancestors(parent)?;
    }
    Ok(true)
}

fn invalid_location(path: &Path, message: &str) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source: io::Error::new(io::ErrorKind::InvalidData, message),
    }
}

#[cfg(test)]
#[path = "storage_location_tests.rs"]
mod tests;
