//! Narrow recovery for an installed depot manifest that Steam no longer serves.
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::steam_depot::manifest::{collect_depots, parse_acf_app_state};
use super::steam_depot::{MAX_ACF, bounded, check_plain};
use super::{InstallCancellation, InstallDeadline, SteamCmdError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ManifestDenial {
    pub(super) app: u32,
    pub(super) depot: u64,
    pub(super) manifest: u64,
}

/// Accept the observed native record, not generic network errors or denied URLs.
pub(super) fn manifest_denial(line: &str) -> Option<ManifestDenial> {
    let (_, details) = line.split_once("CDepotDownloadMgr::BYldRequestDepotManifest(App: ")?;
    let (app, details) = details.split_once(", Depot: ")?;
    let (depot, details) = details.split_once(", Manifest: ")?;
    let (manifest, details) = details.split_once(", branch: ")?;
    let (_, failure) = details.split_once("): ")?;
    if failure.trim() != "Failed to get manifest request code, 'Access Denied'" {
        return None;
    }
    let number = |value: &str| {
        (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.parse::<u64>().ok())
            .flatten()
            .filter(|value| *value != 0)
    };
    Some(ManifestDenial {
        app: u32::try_from(number(app)?).ok()?,
        depot: number(depot)?,
        manifest: number(manifest)?,
    })
}

pub(super) struct InstalledManifest {
    path: PathBuf,
    app: u32,
    bytes: Vec<u8>,
    depots: BTreeMap<u64, u64>,
}

impl InstalledManifest {
    pub(super) async fn before_attempt(root: &Path, app: u32) -> Option<Self> {
        let path = root.join(format!("steamapps/appmanifest_{app}.acf"));
        tokio::task::spawn_blocking(move || {
            let bytes = bounded(&path, MAX_ACF).ok()?;
            let state = parse_acf_app_state(&bytes, app).ok()?;
            let mut depots = BTreeMap::new();
            collect_depots(&state, &mut depots).ok()?;
            (!depots.is_empty()).then_some(Self {
                path,
                app,
                bytes,
                depots,
            })
        })
        .await
        .ok()
        .flatten()
    }

    pub(super) fn matching_denial(&self, denials: &[ManifestDenial]) -> Option<ManifestDenial> {
        denials.iter().copied().find(|denial| {
            denial.app == self.app && self.depots.get(&denial.depot) == Some(&denial.manifest)
        })
    }

    /// Keep both the pre-attempt manifest and Steam's failed state. The caller
    /// retains its install/runtime leases and pending revision for the retry.
    /// These backups are deliberately retained even if validation fails.
    pub(super) async fn preserve_and_reset(
        self,
        deadline: InstallDeadline,
    ) -> Result<PathBuf, SteamCmdError> {
        deadline.check_cancelled()?;
        let cancellation = InstallCancellation::current();
        let path = self.path.clone();
        let result = tokio::task::spawn_blocking(move || {
            self.reset(|| {
                if cancellation
                    .as_ref()
                    .is_some_and(|token| token.is_cancelled())
                    || tokio::time::Instant::now() >= deadline.expires_at()
                {
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "manifest recovery cancelled",
                    ))
                } else {
                    Ok(())
                }
            })
        })
        .await
        .map_err(|error| SteamCmdError::SteamCmdCommandFailed {
            output_excerpt: format!("Steam manifest recovery worker failed: {error}"),
        })?;
        deadline.check_cancelled()?;
        result.map_err(|error| SteamCmdError::SteamCmdCommandFailed {
            output_excerpt: format!(
                "Could not preserve/reset installed Steam manifest {}: {error}",
                path.display()
            ),
        })
    }

    fn reset(self, check: impl Fn() -> io::Result<()>) -> io::Result<PathBuf> {
        check()?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("manifest has no parent"))?;
        check_plain(parent, true)?;
        // Recheck the active path immediately before mutation. Native SteamCMD
        // may have changed its state, but not its app identity or entry type.
        match bounded(&self.path, MAX_ACF) {
            Ok(bytes) => {
                parse_acf_app_state(&bytes, self.app)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let backup = parent.join(format!(
            ".langame-manifest-recovery-{}-{}-{stamp}-{}",
            self.app,
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&backup)?;
        let original = backup.join("before-attempt.acf");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&original)?;
        file.write_all(&self.bytes)?;
        file.sync_all()?;
        drop(file);
        if bounded(&original, MAX_ACF)? != self.bytes {
            return Err(io::Error::other("preserved manifest verification failed"));
        }
        check()?;
        match std::fs::rename(&self.path, backup.join("failed-attempt.acf")) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(backup)
    }
}

#[cfg(test)]
#[path = "steamcmd_manifest_recovery_tests.rs"]
mod tests;
