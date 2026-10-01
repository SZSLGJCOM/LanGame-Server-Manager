use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::SteamCmdError;

const STEAMCMD_OWNERSHIP_FILE: &str = ".langame-steamcmd-owner.json";
const STEAMCMD_OWNERSHIP_PRODUCT: &str = "langame-server-manager";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SteamCmdOwnership {
    Managed,
    External,
    None,
    Invalid,
}

#[derive(Debug, Serialize, Deserialize)]
struct SteamCmdOwnershipMarker {
    product: String,
    canonical_root: String,
}

#[derive(Debug)]
enum SteamCmdOwnershipMarkerInspection {
    Missing,
    Valid(PathBuf),
    Invalid,
}

pub(super) fn configured_steamcmd_ownership(root: &Path) -> SteamCmdOwnership {
    if !root.exists() {
        return SteamCmdOwnership::None;
    }

    match inspect_steamcmd_ownership_marker(root) {
        SteamCmdOwnershipMarkerInspection::Valid(_) => SteamCmdOwnership::Managed,
        SteamCmdOwnershipMarkerInspection::Invalid => SteamCmdOwnership::Invalid,
        SteamCmdOwnershipMarkerInspection::Missing => {
            if steamcmd_root_has_existing_content(root) {
                SteamCmdOwnership::External
            } else {
                SteamCmdOwnership::None
            }
        }
    }
}

pub(super) fn validate_steamcmd_ownership(root: &Path) -> Result<PathBuf, SteamCmdError> {
    match inspect_steamcmd_ownership_marker(root) {
        SteamCmdOwnershipMarkerInspection::Valid(canonical_root) => Ok(canonical_root),
        SteamCmdOwnershipMarkerInspection::Missing => Err(SteamCmdError::UnmanagedSteamCmdRoot {
            path: root.to_path_buf(),
        }),
        SteamCmdOwnershipMarkerInspection::Invalid => {
            Err(SteamCmdError::InvalidSteamCmdOwnership {
                path: ownership_marker_path(root),
            })
        }
    }
}

pub(super) fn prepare_configured_steamcmd_root(root: &Path) -> Result<PathBuf, SteamCmdError> {
    match configured_steamcmd_ownership(root) {
        SteamCmdOwnership::Managed => return Ok(root.to_path_buf()),
        SteamCmdOwnership::External => {
            return Err(SteamCmdError::UnmanagedSteamCmdRoot {
                path: root.to_path_buf(),
            });
        }
        SteamCmdOwnership::Invalid => {
            return Err(SteamCmdError::InvalidSteamCmdOwnership {
                path: ownership_marker_path(root),
            });
        }
        SteamCmdOwnership::None => {}
    }

    fs::create_dir_all(root).map_err(|source| SteamCmdError::CreatePath {
        path: root.to_path_buf(),
        source,
    })?;

    match configured_steamcmd_ownership(root) {
        SteamCmdOwnership::None => write_steamcmd_ownership_marker(root)?,
        SteamCmdOwnership::Managed => {}
        SteamCmdOwnership::External => {
            return Err(SteamCmdError::UnmanagedSteamCmdRoot {
                path: root.to_path_buf(),
            });
        }
        SteamCmdOwnership::Invalid => {
            return Err(SteamCmdError::InvalidSteamCmdOwnership {
                path: ownership_marker_path(root),
            });
        }
    }

    Ok(root.to_path_buf())
}

fn write_steamcmd_ownership_marker(root: &Path) -> Result<(), SteamCmdError> {
    let canonical_root =
        fs::canonicalize(root).map_err(|source| SteamCmdError::SteamCmdOwnershipIo {
            path: root.to_path_buf(),
            source,
        })?;
    let marker = SteamCmdOwnershipMarker {
        product: String::from(STEAMCMD_OWNERSHIP_PRODUCT),
        canonical_root: canonical_root.to_string_lossy().into_owned(),
    };
    let path = ownership_marker_path(&canonical_root);
    let payload = serde_json::to_vec_pretty(&marker)
        .map_err(|_| SteamCmdError::InvalidSteamCmdOwnership { path: path.clone() })?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| SteamCmdError::SteamCmdOwnershipIo {
            path: path.clone(),
            source,
        })?;
    file.write_all(&payload)
        .and_then(|_| file.sync_all())
        .map_err(|source| SteamCmdError::SteamCmdOwnershipIo { path, source })
}

fn inspect_steamcmd_ownership_marker(root: &Path) -> SteamCmdOwnershipMarkerInspection {
    let Ok(root_metadata) = fs::symlink_metadata(root) else {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    };
    if !root_metadata.is_dir() || root_metadata.file_type().is_symlink() {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    }

    let Ok(canonical_root) = fs::canonicalize(root) else {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    };
    if canonical_root.parent().is_none() {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    }

    let marker_path = ownership_marker_path(&canonical_root);
    let marker_metadata = match fs::symlink_metadata(&marker_path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return SteamCmdOwnershipMarkerInspection::Missing;
        }
        Err(_) => return SteamCmdOwnershipMarkerInspection::Invalid,
    };
    if !marker_metadata.is_file() || marker_metadata.file_type().is_symlink() {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    }
    let Ok(payload) = fs::read(&marker_path) else {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    };
    let Ok(marker) = serde_json::from_slice::<SteamCmdOwnershipMarker>(&payload) else {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    };
    let marker_root = PathBuf::from(&marker.canonical_root);
    if marker.product != STEAMCMD_OWNERSHIP_PRODUCT
        || !marker_root
            .to_string_lossy()
            .eq_ignore_ascii_case(canonical_root.to_string_lossy().as_ref())
    {
        return SteamCmdOwnershipMarkerInspection::Invalid;
    }

    SteamCmdOwnershipMarkerInspection::Valid(canonical_root)
}

fn steamcmd_root_has_existing_content(root: &Path) -> bool {
    if !root.is_dir() {
        return true;
    }

    match fs::read_dir(root) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => true,
    }
}

fn ownership_marker_path(root: &Path) -> PathBuf {
    root.join(STEAMCMD_OWNERSHIP_FILE)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    static TEST_ROOT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn unique_test_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let sequence = TEST_ROOT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "langame-steamcmd-ownership-test-{}-{stamp}-{sequence}",
            std::process::id()
        ))
    }

    fn write_unmarked_steamcmd_fixture(root: &Path) {
        fs::create_dir_all(root.join("logs")).expect("create SteamCMD logs");
        fs::create_dir_all(root.join("package")).expect("create SteamCMD package");
        fs::write(root.join("steamcmd.exe"), b"fixture").expect("write SteamCMD executable");
        fs::write(root.join("steam.dll"), b"fixture").expect("write SteamCMD DLL");
        fs::write(root.join("steamclient.dll"), b"fixture").expect("write SteamCMD client DLL");
    }

    #[test]
    fn prepare_claims_only_empty_custom_root() {
        let root = unique_test_root();
        fs::create_dir_all(&root).expect("create empty custom root");

        let prepared = prepare_configured_steamcmd_root(&root).expect("claim empty root");

        assert_eq!(prepared, root);
        assert!(ownership_marker_path(&root).is_file());
        assert_eq!(
            configured_steamcmd_ownership(&root),
            SteamCmdOwnership::Managed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepare_rejects_nonempty_custom_root_without_marker_or_deletion() {
        let root = unique_test_root();
        fs::create_dir_all(&root).expect("create custom root");
        let personal_file = root.join("personal-file.txt");
        fs::write(&personal_file, b"keep").expect("write personal file");

        assert!(matches!(
            prepare_configured_steamcmd_root(&root),
            Err(SteamCmdError::UnmanagedSteamCmdRoot { .. })
        ));
        assert!(!ownership_marker_path(&root).exists());
        assert_eq!(fs::read(&personal_file).unwrap(), b"keep");
        assert!(root.is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prepare_never_overwrites_invalid_ownership_marker() {
        let root = unique_test_root();
        fs::create_dir_all(&root).expect("create custom root");
        let marker_path = ownership_marker_path(&root);
        fs::write(&marker_path, b"not valid json").expect("write invalid marker");

        assert!(matches!(
            prepare_configured_steamcmd_root(&root),
            Err(SteamCmdError::InvalidSteamCmdOwnership { .. })
        ));
        assert_eq!(fs::read(&marker_path).unwrap(), b"not valid json");
        assert_eq!(
            configured_steamcmd_ownership(&root),
            SteamCmdOwnership::Invalid
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn complete_unmarked_installation_remains_external() {
        let root = unique_test_root();
        write_unmarked_steamcmd_fixture(&root);

        assert_eq!(
            configured_steamcmd_ownership(&root),
            SteamCmdOwnership::External
        );
        assert!(matches!(
            prepare_configured_steamcmd_root(&root),
            Err(SteamCmdError::UnmanagedSteamCmdRoot { .. })
        ));
        assert!(matches!(
            validate_steamcmd_ownership(&root),
            Err(SteamCmdError::UnmanagedSteamCmdRoot { .. })
        ));
        assert!(!ownership_marker_path(&root).exists());
        for file in ["steamcmd.exe", "steam.dll", "steamclient.dll"] {
            assert_eq!(fs::read(root.join(file)).unwrap(), b"fixture");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marker_for_another_directory_is_rejected_without_modification() {
        let root = unique_test_root();
        fs::create_dir_all(&root).unwrap();
        let marker_path = ownership_marker_path(&root);
        let payload = serde_json::to_vec(&SteamCmdOwnershipMarker {
            product: String::from(STEAMCMD_OWNERSHIP_PRODUCT),
            canonical_root: root
                .join("another-directory")
                .to_string_lossy()
                .into_owned(),
        })
        .unwrap();
        fs::write(&marker_path, &payload).unwrap();

        assert_eq!(
            configured_steamcmd_ownership(&root),
            SteamCmdOwnership::Invalid
        );
        assert!(matches!(
            prepare_configured_steamcmd_root(&root),
            Err(SteamCmdError::InvalidSteamCmdOwnership { .. })
        ));
        assert!(matches!(
            validate_steamcmd_ownership(&root),
            Err(SteamCmdError::InvalidSteamCmdOwnership { .. })
        ));
        assert_eq!(fs::read(&marker_path).unwrap(), payload);
        fs::remove_dir_all(root).unwrap();
    }
}
