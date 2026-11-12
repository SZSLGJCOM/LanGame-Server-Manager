use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::SteamCmdError;

static NEXT_SCRIPT: AtomicU64 = AtomicU64::new(1);

/// Scripts belong to the leased runtime and are never shared between attempts.
pub(super) struct WorkshopScript(PathBuf);

impl WorkshopScript {
    pub(super) fn create(root: &Path, app_id: u32, contents: &str) -> Result<Self, SteamCmdError> {
        let root = root.join(".langame-workshop-scripts");
        fs::create_dir_all(&root).map_err(|source| SteamCmdError::CreatePath {
            path: root.clone(),
            source,
        })?;
        crate::dst_workshop_cache_fs::canonical_plain_directory(&root).map_err(|error| {
            SteamCmdError::WriteScript {
                path: root.clone(),
                source: std::io::Error::other(error),
            }
        })?;
        for _ in 0..32 {
            let path = root.join(format!(
                "{app_id}-{}-{}.txt",
                std::process::id(),
                NEXT_SCRIPT.fetch_add(1, Ordering::Relaxed),
            ));
            let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => return Err(SteamCmdError::WriteScript { path, source }),
            };
            let script = Self(path);
            file.write_all(contents.as_bytes())
                .map_err(|source| SteamCmdError::WriteScript {
                    path: script.0.clone(),
                    source,
                })?;
            return Ok(script);
        }
        Err(SteamCmdError::WriteScript {
            path: root,
            source: std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "Workshop script names are already in use",
            ),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for WorkshopScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
