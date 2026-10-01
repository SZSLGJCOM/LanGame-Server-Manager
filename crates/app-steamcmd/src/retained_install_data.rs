use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

pub const RETAINED_INSTALL_DATA_MARKER: &str = ".langame-uninstalled";
const CONTENT: &[u8] = b"LanGame: server files removed; retained user data\n";

pub fn has_retained_install_data(root: &Path) -> bool {
    let marker = root.join(RETAINED_INSTALL_DATA_MARKER);
    let Ok(metadata) = std::fs::symlink_metadata(&marker) else {
        return false;
    };
    if !metadata.file_type().is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    let Ok(file) = File::open(marker) else {
        return false;
    };
    let mut bytes = Vec::new();
    file.take(CONTENT.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes == CONTENT
}

pub fn mark_retained_install_data(root: &Path) -> std::io::Result<()> {
    let path = root.join(RETAINED_INSTALL_DATA_MARKER);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    let result = file.write_all(CONTENT);
    drop(file);
    if let Err(error) = result {
        std::fs::remove_file(&path).map_err(|cleanup| {
            std::io::Error::other(format!(
                "write uninstall marker: {error}; remove incomplete marker: {cleanup}"
            ))
        })?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_is_explicit_bounded_and_never_overwrites_existing_data() {
        let root = std::env::temp_dir().join(format!(
            "lg-retained-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        assert_eq!(
            crate::derive_install_state(&root, false, Some(false), None),
            app_core::InstallState::Corrupted
        );
        assert!(!has_retained_install_data(&root));
        mark_retained_install_data(&root).unwrap();
        assert!(has_retained_install_data(&root));
        assert_eq!(
            crate::derive_install_state(&root, false, Some(false), None),
            app_core::InstallState::NotInstalled
        );
        assert_eq!(
            crate::derive_install_state(&root, true, Some(true), None),
            app_core::InstallState::Installed
        );
        assert!(mark_retained_install_data(&root).is_err());
        std::fs::write(root.join(RETAINED_INSTALL_DATA_MARKER), b"user data").unwrap();
        assert!(!has_retained_install_data(&root));
        std::fs::remove_dir_all(root).unwrap();
    }
}
