//! Squad has no account-unban command. Its native list must be edited under an
//! exclusive file handle. A new server GameSession reads the persisted change.
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::StorageError;
use crate::instance_file_patch::io::{guard_directories, is_link};

const FILE: &str = "SquadGame/ServerConfig/Bans.cfg";
const MAX_BYTES: usize = 8 * 1024 * 1024;

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "squad".into(),
        path: path.to_owned(),
        message: message.into(),
    }
}

/// The caller retains its instance/lifecycle permit through this write and the
/// completion. No startup/configuration renderer calls this method.
pub fn remove_squad_native_ban(
    runtime_root: &Path,
    instance_root: &Path,
    account_id: &str,
) -> Result<(), StorageError> {
    let path = runtime_root.join(FILE);
    let _directories = guard_directories(
        path.parent()
            .ok_or_else(|| invalid(&path, "Missing ban directory."))?,
    )?;
    let before = fs::symlink_metadata(&path).map_err(|error| invalid(&path, error.to_string()))?;
    if !before.is_file() || is_link(&before) || before.len() > MAX_BYTES as u64 {
        return Err(invalid(
            &path,
            "Squad bans must be a bounded regular file without links.",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        // Refuse an active game/editor writer. Hold the same file identity until
        // read-back; replacing a file after a cooperative CAS would race Squad.
        options
            .share_mode(0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut file = options.open(&path).map_err(|error| {
        invalid(
            &path,
            format!("Cannot exclusively open Squad bans: {error}"),
        )
    })?;
    let opened = file
        .metadata()
        .map_err(|error| invalid(&path, error.to_string()))?;
    if !opened.is_file() || is_link(&opened) || opened.len() > MAX_BYTES as u64 {
        return Err(invalid(
            &path,
            "Opened Squad bans are not a bounded regular file.",
        ));
    }
    let mut original = Vec::new();
    (&mut file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut original)
        .map_err(|error| invalid(&path, error.to_string()))?;
    if original.len() > MAX_BYTES || original.len() as u64 != opened.len() {
        return Err(invalid(
            &path,
            "Squad bans changed or exceeded the read limit.",
        ));
    }
    let replacement =
        crate::native_bans::remove_squad_native_ban_lines(&original, account_id, &path)?;
    let backup = retain_original(instance_root, &original)?;
    let write = |file: &mut fs::File, bytes: &[u8]| -> std::io::Result<()> {
        file.seek(SeekFrom::Start(0))?;
        file.write_all(bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_all()
    };
    if let Err(error) = write(&mut file, &replacement) {
        let restored = write(&mut file, &original).is_ok();
        return Err(invalid(
            &path,
            format!(
                "Squad ban write failed: {error}; original restored={restored}. Original retained at {}.",
                backup.display()
            ),
        ));
    }
    file.seek(SeekFrom::Start(0)).map_err(|error| invalid(&path,
        format!("Squad bans were edited, but seeking for read-back failed: {error}. Original retained at {}.", backup.display())))?;
    let mut actual = Vec::new();
    (&mut file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut actual)
        .map_err(|error| {
            invalid(
                &path,
                format!(
                    "Squad ban read-back failed: {error}. Original retained at {}.",
                    backup.display()
                ),
            )
        })?;
    if actual != replacement {
        return Err(invalid(
            &path,
            format!(
                "Squad ban read-back differs from the intended edit. Original retained at {}.",
                backup.display()
            ),
        ));
    }
    Ok(())
}

fn retain_original(instance_root: &Path, bytes: &[u8]) -> Result<std::path::PathBuf, StorageError> {
    let mut root = instance_root.to_owned();
    let mut guards = guard_directories(&root)?;
    for component in ["config", "native-ban-recovery"] {
        root.push(component);
        match fs::create_dir(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(invalid(&root, error.to_string())),
        }
        guards.extend(guard_directories(&root)?);
    }
    let mut count = 0;
    for entry in fs::read_dir(&root).map_err(|error| invalid(&root, error.to_string()))? {
        entry.map_err(|error| invalid(&root, error.to_string()))?;
        count += 1;
        if count >= 64 {
            return Err(invalid(
                &root,
                "Native ban recovery capacity is full (64 files); preserve reviewed recovery files before another edit.",
            ));
        }
    }
    let path = root.join(format!("squad-{}.cfg", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| invalid(&path, error.to_string()))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| invalid(&path, format!("Cannot retain original Squad bans: {error}")))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("lgsm-squad-unban-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join(FILE).parent().unwrap()).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
            assert!(
                self.0
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("lgsm-squad-unban-")
            );
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn squad_unban_preserves_other_bytes_and_retains_recovery_copy() {
        let fixture = Fixture::new();
        let path = fixture.0.join(FILE);
        let original = b"\xef\xbb\xbf// native comment\r\n76561198000000001:0 // Target\r\n76561198000000002:0 // Keep\r\n";
        fs::write(&path, original).unwrap();
        remove_squad_native_ban(&fixture.0, &fixture.0, "76561198000000001").unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"\xef\xbb\xbf// native comment\r\n76561198000000002:0 // Keep\r\n"
        );
        let backups = fs::read_dir(fixture.0.join("config/native-ban-recovery"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(&backups[0]).unwrap(), original);
        assert!(remove_squad_native_ban(&fixture.0, &fixture.0, "76561198000000001").is_err());
        assert_eq!(
            fs::read_dir(fixture.0.join("config/native-ban-recovery"))
                .unwrap()
                .count(),
            1
        );
    }
    #[cfg(windows)]
    #[test]
    fn squad_unban_refuses_a_concurrent_writer_without_changes_or_backup() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let path = fixture.0.join(FILE);
        let original = b"76561198000000001:0\n";
        fs::write(&path, original).unwrap();
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(3)
            .open(&path)
            .unwrap();
        assert!(remove_squad_native_ban(&fixture.0, &fixture.0, "76561198000000001").is_err());
        drop(writer);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!fixture.0.join("config/native-ban-recovery").exists());
    }
}
