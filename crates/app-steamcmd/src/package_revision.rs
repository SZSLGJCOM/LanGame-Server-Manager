use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::SteamCmdError;

const REVISION_PREFIX: &str = "rev-";

pub(crate) fn validate_program_revision_root(root: &Path) -> Result<(), SteamCmdError> {
    if !root.is_absolute()
        || root.file_name().is_none()
        || root.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(revision_io(
            root,
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "program target must be an absolute directory without traversal components",
            ),
        ));
    }
    Ok(())
}

pub(crate) fn program_revision_key(module_id: &str, root: &Path) -> Result<String, SteamCmdError> {
    validate_program_revision_root(root)?;
    // Resolve the existing ancestor so normal paths, long-path prefixes and
    // short-name aliases cannot give one installation different revision keys.
    // Appending the missing tail also keeps identity stable before installation.
    let mut ancestor = root.to_owned();
    let mut missing = Vec::new();
    let mut normalized = loop {
        match fs::canonicalize(&ancestor) {
            Ok(path) => break path,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| revision_io(root, error))?
                    .to_owned();
                missing.push(name);
                ancestor.pop();
            }
            Err(error) => return Err(revision_io(root, error)),
        }
    };
    for name in missing.into_iter().rev() {
        normalized.push(name);
    }
    let root = normalized.to_str().ok_or_else(|| {
        revision_io(
            root,
            io::Error::new(io::ErrorKind::InvalidInput, "program target is not UTF-8"),
        )
    })?;
    #[cfg(windows)]
    let root = root.replace('\\', "/").to_lowercase();
    Ok(format!("{module_id}\0program-root\0{root}"))
}

/// Revision of one program directory, independent of other instances and the
/// module's library revision. A failed installer leaves only this root pending.
pub fn read_program_install_revision(
    servers_root: &Path,
    module_id: &str,
    install_root: &Path,
) -> Result<u64, SteamCmdError> {
    read_game_install_revision(
        servers_root,
        &program_revision_key(module_id, install_root)?,
    )
}

fn revision_directory(servers_root: &Path, module_id: &str) -> PathBuf {
    let module_key = Sha256::digest(module_id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    servers_root
        .join(".langame")
        .join("install-revisions")
        .join(module_key)
}

fn revision_io(path: &Path, source: io::Error) -> SteamCmdError {
    SteamCmdError::PackageRevisionIo {
        path: path.to_path_buf(),
        source,
    }
}

/// A ready generation describes a verified shared package. A pending latest
/// generation means an installer may have changed the package before failing.
pub fn read_game_install_revision(
    servers_root: &Path,
    module_id: &str,
) -> Result<u64, SteamCmdError> {
    let (revision, pending) = latest_revision(servers_root, module_id)?;
    if pending {
        return Err(SteamCmdError::PackageRevisionPending {
            path: revision_directory(servers_root, module_id),
        });
    }
    Ok(revision)
}

fn latest_revision(servers_root: &Path, module_id: &str) -> Result<(u64, bool), SteamCmdError> {
    let metadata_root = servers_root.join(".langame");
    let revisions_root = metadata_root.join("install-revisions");
    let directory = revision_directory(servers_root, module_id);
    for path in [&metadata_root, &revisions_root, &directory] {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_plain_directory(&metadata) => {}
            Ok(_) => {
                return Err(revision_io(
                    path,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "revision path is not a plain directory",
                    ),
                ));
            }
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok((0, false)),
            Err(source) => return Err(revision_io(path, source)),
        }
    }
    let entries = fs::read_dir(&directory).map_err(|source| revision_io(&directory, source))?;
    let mut latest = (0_u64, false);
    for entry in entries {
        let entry = entry.map_err(|source| revision_io(&directory, source))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let (stem, pending) = if let Some(stem) = name.strip_suffix(".pending") {
            (stem, true)
        } else if let Some(stem) = name.strip_suffix(".ready") {
            (stem, false)
        } else {
            continue;
        };
        let Some(number) = stem.strip_prefix(REVISION_PREFIX) else {
            continue;
        };
        if number.len() != 20 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source| revision_io(&entry.path(), source))?;
        if !file_type.is_file() {
            continue;
        }
        if let Ok(revision) = number.parse::<u64>()
            && (revision > latest.0 || (revision == latest.0 && pending))
        {
            latest = (revision, pending);
        }
    }
    Ok(latest)
}
/// Call only while the matching game installation lifecycle locks are held, before
/// the installer can write package files. A failed install still advances the
/// generation, so an existing private runtime cannot silently accept partial
/// changes. The new file remains valid if the process exits during installation.
pub(crate) fn begin_game_install_revision(
    servers_root: &Path,
    module_id: &str,
) -> Result<u64, SteamCmdError> {
    let next = latest_revision(servers_root, module_id)?
        .0
        .checked_add(1)
        .ok_or_else(|| {
            revision_io(
                &revision_directory(servers_root, module_id),
                io::Error::new(io::ErrorKind::InvalidData, "install revision overflow"),
            )
        })?;
    fs::create_dir_all(servers_root).map_err(|source| revision_io(servers_root, source))?;
    let metadata_root = servers_root.join(".langame");
    let revisions_root = metadata_root.join("install-revisions");
    let directory = revision_directory(servers_root, module_id);
    for path in [&metadata_root, &revisions_root, &directory] {
        ensure_plain_directory(path)?;
    }
    let marker = directory.join(format!("{REVISION_PREFIX}{next:020}.pending"));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|source| revision_io(&marker, source))?;
    file.sync_all()
        .map_err(|source| revision_io(&marker, source))?;
    Ok(next)
}

pub(crate) fn complete_game_install_revision(
    servers_root: &Path,
    module_id: &str,
    revision: u64,
) -> Result<(), SteamCmdError> {
    let directory = revision_directory(servers_root, module_id);
    let pending = directory.join(format!("{REVISION_PREFIX}{revision:020}.pending"));
    let ready = directory.join(format!("{REVISION_PREFIX}{revision:020}.ready"));
    if ready.exists() {
        return Err(revision_io(
            &ready,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "install revision was already completed",
            ),
        ));
    }
    fs::rename(&pending, &ready).map_err(|source| revision_io(&ready, source))
}

fn is_plain_directory(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    metadata.is_dir() && !metadata.file_type().is_symlink() && !reparse
}
fn ensure_plain_directory(path: &Path) -> Result<(), SteamCmdError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if is_plain_directory(&metadata) => Ok(()),
        Ok(_) => Err(revision_io(
            path,
            io::Error::new(
                io::ErrorKind::InvalidData,
                "revision path is not a plain directory",
            ),
        )),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|source| revision_io(path, source))
        }
        Err(source) => Err(revision_io(path, source)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn failed_program_revision_does_not_invalidate_library_or_another_instance() {
        let root = crate::tests::unique_test_root();
        let first = root.join("instances/first/runtime");
        let second = root.join("instances/second/runtime");
        let first_key = program_revision_key("minecraft", &first).unwrap();
        let second_key = program_revision_key("minecraft", &second).unwrap();
        let library = begin_game_install_revision(&root, "minecraft").unwrap();
        complete_game_install_revision(&root, "minecraft", library).unwrap();
        let ready = begin_game_install_revision(&root, &second_key).unwrap();
        complete_game_install_revision(&root, &second_key, ready).unwrap();
        begin_game_install_revision(&root, &first_key).unwrap();
        assert!(matches!(
            read_program_install_revision(&root, "minecraft", &first),
            Err(SteamCmdError::PackageRevisionPending { path })
                if path == revision_directory(&root, &first_key)
        ));
        assert_eq!(
            read_program_install_revision(&root, "minecraft", &second).unwrap(),
            1
        );
        assert_eq!(read_game_install_revision(&root, "minecraft").unwrap(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn explicit_program_revision_rejects_relative_and_traversing_paths() {
        assert!(program_revision_key("minecraft", Path::new("runtime")).is_err());
        let root = std::env::temp_dir();
        assert!(program_revision_key("minecraft", &root.join("instance/../runtime")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn program_revision_normalizes_windows_case_and_separators() {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let target = root.join("Game/Runtime");
        let alternate = target.to_string_lossy().replace('\\', "/").to_uppercase();
        assert_eq!(
            program_revision_key("minecraft", &target).unwrap(),
            program_revision_key("minecraft", Path::new(&alternate)).unwrap(),
        );
        fs::create_dir_all(&target).unwrap();
        assert_eq!(
            program_revision_key("minecraft", &target).unwrap(),
            program_revision_key("minecraft", &fs::canonicalize(&target).unwrap()).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn latest_pending_revision_blocks_read_until_install_is_verified() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lgsm-install-revision-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        assert_eq!(read_game_install_revision(&root, "dontstarve").unwrap(), 0);
        let first = begin_game_install_revision(&root, "dontstarve").unwrap();
        assert_eq!(first, 1);
        let pending = read_game_install_revision(&root, "dontstarve").unwrap_err();
        assert!(matches!(
            &pending,
            SteamCmdError::PackageRevisionPending { path }
                if *path == revision_directory(&root, "dontstarve")
        ));
        assert_eq!(
            pending.to_string(),
            format!(
                "failed to read or write game install revision at {}: the last game install did not finish; validate or update this program installation before starting it",
                revision_directory(&root, "dontstarve").display()
            )
        );
        complete_game_install_revision(&root, "dontstarve", first).unwrap();
        assert_eq!(read_game_install_revision(&root, "dontstarve").unwrap(), 1);

        let failed = begin_game_install_revision(&root, "dontstarve").unwrap();
        assert_eq!(failed, 2);
        assert!(matches!(
            read_game_install_revision(&root, "dontstarve"),
            Err(SteamCmdError::PackageRevisionPending { .. })
        ));
        let recovered = begin_game_install_revision(&root, "dontstarve").unwrap();
        assert_eq!(recovered, 3);
        complete_game_install_revision(&root, "dontstarve", recovered).unwrap();
        assert_eq!(read_game_install_revision(&root, "dontstarve").unwrap(), 3);
        assert_eq!(read_game_install_revision(&root, "valheim").unwrap(), 0);
        fs::write(
            revision_directory(&root, "dontstarve").join("rev-invalid.ready"),
            b"ignored",
        )
        .unwrap();
        assert_eq!(read_game_install_revision(&root, "dontstarve").unwrap(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_revision_directories_remain_io_errors() {
        let root = crate::tests::unique_test_root();
        for depth in 0..3 {
            let servers = root.join(depth.to_string());
            let metadata = servers.join(".langame");
            let catalog = metadata.join("install-revisions");
            let directory = revision_directory(&servers, "dontstarve");
            let blocked = [&metadata, &catalog, &directory][depth];
            fs::create_dir_all(blocked.parent().unwrap()).unwrap();
            fs::write(blocked, b"not a revision directory").unwrap();

            let error = read_game_install_revision(&servers, "dontstarve").unwrap_err();
            assert!(matches!(
                error,
                SteamCmdError::PackageRevisionIo { path, source }
                    if path == *blocked
                        && source.kind() == io::ErrorKind::InvalidData
                        && source.to_string() == "revision path is not a plain directory"
            ));
            assert_eq!(fs::read(blocked).unwrap(), b"not a revision directory");
        }
        fs::remove_dir_all(root).unwrap();
    }
}
