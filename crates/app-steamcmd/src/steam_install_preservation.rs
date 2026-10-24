//! Protect only declared operator data while Steam updates the original program
//! tree incrementally. Recovery precedes the install revision's ready transition.
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::steam_depot::{bounded, check_plain, invalid, relative};
use crate::{GameInstallLifecycleGuard, InstallCancellation, InstallDeadline, SteamCmdError};

#[path = "steam_install_preservation_files.rs"]
mod files;
use files::Entry;

pub(super) fn journal_directory(module_id: &str, root: &Path) -> Result<PathBuf, SteamCmdError> {
    let key = crate::package_revision::program_revision_key(module_id, root)?;
    let digest: String = Sha256::digest(key.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let parent = root
        .parent()
        .ok_or_else(|| failure(root, "program root has no parent"))?;
    // ReplaceFile's target, staging file and displaced-file backup must share a
    // volume. Instance metadata may live elsewhere; the data transaction cannot.
    Ok(parent.join(format!(".langame-steam-retained-{digest}")))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    version: u32,
    module_id: String,
    root: PathBuf,
    root_identity: String,
}

pub(super) struct PreservedSteamData {
    directory: PathBuf,
    owner: Owner,
    entries: Vec<Entry>,
    _root_pin: File,
}

impl PreservedSteamData {
    pub(super) async fn prepare(
        directory: PathBuf,
        root: &Path,
        module_id: &str,
        retained_paths: &[PathBuf],
        guard: &GameInstallLifecycleGuard,
        deadline: InstallDeadline,
    ) -> Result<Option<Self>, SteamCmdError> {
        let root = root.to_owned();
        let module_id = module_id.to_owned();
        let mut retained_paths = retained_paths.to_vec();
        let error_path = directory.clone();
        let cancellation = InstallCancellation::current();
        let abandoned = CancelCopyOnDrop::default();
        let worker_abandoned = std::sync::Arc::clone(&abandoned.0);
        let lease = std::sync::Arc::clone(&guard.0);
        let result = tokio::task::spawn_blocking(move || {
            // A dropped waiter cannot release the lifecycle lease while a copy
            // or recovery still owns filesystem writes.
            let _lease = lease;
            let parent = directory
                .parent()
                .ok_or_else(|| invalid("journal has no parent"))?;
            let directory = fs::canonicalize(parent)?.join(
                directory
                    .file_name()
                    .ok_or_else(|| invalid("journal has no name"))?,
            );
            let (pin, root_identity) = files::pin_root(&root)?;
            let root = fs::canonicalize(root)?;
            let owner = Owner {
                version: 1,
                module_id,
                root,
                root_identity,
            };
            // Recovery can recreate a path that was absent when the caller
            // selected existing data. Protect those restored paths again too.
            retained_paths.extend(Self::recover_previous(&directory, &owner)?);
            // Reinstallation publishes an isolated Steam payload through
            // RetainedSteamInstall. It must be able to rename the retained root.
            if crate::has_retained_install_data(&owner.root) {
                return Ok(None);
            }
            let mut selected = Vec::new();
            for path in retained_paths {
                match fs::symlink_metadata(&path) {
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                    Ok(metadata) => {
                        check_plain(&path, metadata.is_dir())?;
                    }
                }
                let path = fs::canonicalize(path)?;
                let key = path
                    .strip_prefix(&owner.root)
                    .map_err(|_| invalid("retained data is outside its program root"))?;
                let key = relative(&key.to_string_lossy())?;
                selected.push(key);
            }
            selected.sort_by_key(|key| key.split('/').count());
            let mut roots: Vec<String> = Vec::new();
            for key in selected {
                let folded = key.to_ascii_lowercase();
                if !roots.iter().any(|parent| {
                    let parent = parent.to_ascii_lowercase();
                    folded == parent || folded.starts_with(&(parent + "/"))
                }) {
                    roots.push(key);
                }
            }
            if roots.is_empty() {
                return Ok(None);
            }
            if directory.starts_with(&owner.root) || owner.root.starts_with(&directory) {
                return Err(invalid(
                    "retained data journal overlaps the program directory",
                ));
            }
            let mut check = || {
                if cancellation
                    .as_ref()
                    .is_some_and(InstallCancellation::is_cancelled)
                    || worker_abandoned.load(std::sync::atomic::Ordering::Acquire)
                    || tokio::time::Instant::now() >= deadline.expires_at()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "retained data preparation cancelled",
                    ));
                }
                Ok(())
            };
            let entries = files::scan(&owner.root, &roots, &mut check)?;
            files::ensure_directory(
                directory
                    .parent()
                    .ok_or_else(|| invalid("journal has no parent"))?,
            )?;
            fs::create_dir(&directory)?;
            files::write_new(
                &directory.join("owner.json"),
                &serde_json::to_vec(&owner).map_err(invalid)?,
            )?;
            let originals = directory.join("originals");
            files::ensure_directory(&originals)?;
            for entry in &entries {
                check()?;
                let target = originals.join(&entry.path);
                if entry.directory {
                    files::ensure_directory(&target)?;
                } else {
                    files::copy_verified(
                        &owner.root.join(&entry.path),
                        &target,
                        entry,
                        &mut check,
                    )?;
                }
            }
            // Recheck both membership and bytes, including newly added children
            // inside selected directories, before Steam is allowed to write.
            if files::scan(&owner.root, &roots, &mut check)? != entries {
                return Err(invalid(
                    "operator data changed while its recovery copy was prepared",
                ));
            }
            files::write_new(
                &directory.join("inventory.json"),
                &serde_json::to_vec(&entries).map_err(invalid)?,
            )?;
            Ok(Some(Self {
                directory,
                owner,
                entries,
                _root_pin: pin,
            }))
        })
        .await
        .map_err(|error| failure(&error_path, error))?;
        if let Err(error) = &result
            && error.kind() == io::ErrorKind::Interrupted
        {
            deadline.check_cancelled()?;
        }
        result.map_err(|error| failure(&error_path, error))
    }

    pub(super) fn native_started(&self) -> Result<(), SteamCmdError> {
        self.check_root()
            .and_then(|()| files::write_new(&self.directory.join("native-started"), b"1\n"))
            .map_err(|error| failure(&self.directory, error))
    }

    /// Call only after the owned native tree is confirmed stopped. Cancellation
    /// does not cancel restoration; an error leaves the journal and revision pending.
    pub(super) async fn finish(
        self,
        succeeded: bool,
        guard: &GameInstallLifecycleGuard,
    ) -> Result<Option<PathBuf>, SteamCmdError> {
        let path = self.directory.clone();
        let lease = std::sync::Arc::clone(&guard.0);
        tokio::task::spawn_blocking(move || {
            let _lease = lease;
            self.check_root()?;
            files::write_new(
                &self.directory.join("native-stopped"),
                if succeeded { b"success\n" } else { b"failed\n" },
            )?;
            self.restore()?;
            let changed = self.directory.join("changed").try_exists()?;
            if succeeded {
                // Restoration is complete. Retire its journal atomically before
                // pruning copies, so interrupted cleanup cannot leave a partial
                // active journal that blocks the next legitimate update.
                let history = self.retire()?;
                let cleanup = (|| {
                    files::remove_originals(&history, &self.entries)?;
                    if changed {
                        return Ok(());
                    }
                    for name in [
                        "inventory.json",
                        "native-started",
                        "native-stopped",
                        "restored",
                        "owner.json",
                    ] {
                        fs::remove_file(history.join(name))?;
                    }
                    fs::remove_dir(&history)
                })();
                cleanup.map_err(|error: io::Error| {
                    invalid(format!(
                        "operator data was restored; recovery-copy cleanup at {} failed: {error}",
                        history.display()
                    ))
                })?;
                return Ok(changed.then_some(history));
            }
            Ok(changed.then_some(self.directory))
        })
        .await
        .map_err(|error| failure(&path, error))?
        .map_err(|error: io::Error| failure(&path, error))
    }

    fn check_root(&self) -> io::Result<()> {
        if files::identity(&self.owner.root, true)? != self.owner.root_identity {
            return Err(invalid(
                "program directory identity changed; all recovery copies were retained",
            ));
        }
        Ok(())
    }

    fn restore(&self) -> io::Result<()> {
        self.check_root()?;
        files::restore(&self.owner.root, &self.directory, &self.entries)?;
        files::write_new(&self.directory.join("restored"), b"1\n")
    }

    fn retire(&self) -> io::Result<PathBuf> {
        let history = files::unique_sibling(&self.directory, "steam-retained-history");
        fs::rename(&self.directory, &history)?;
        Ok(history)
    }

    fn recover_previous(directory: &Path, expected: &Owner) -> io::Result<Vec<PathBuf>> {
        match fs::symlink_metadata(directory) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
            Ok(_) => check_plain(directory, true)?,
        }
        let owner: Owner = files::read_json(&directory.join("owner.json"))?;
        if owner.version != 1
            || owner.module_id != expected.module_id
            || owner.root != expected.root
            || owner.root_identity != expected.root_identity
        {
            return Err(invalid(
                "retained data journal belongs to another program directory",
            ));
        }
        let started = marker(directory, "native-started")?;
        let stopped = marker(directory, "native-stopped")?;
        let restored = marker(directory, "restored")?;
        if started && !stopped {
            return Err(invalid(format!(
                "Steam process termination was not confirmed. Update and startup remain blocked. Recovery copies and inventory are at {}; confirm the original native process tree has stopped before operator-assisted recovery. No new Steam update was started.",
                directory.display()
            )));
        }
        if (stopped || restored) && !started {
            return Err(invalid("retained data journal has invalid phases"));
        }
        let entries: Vec<Entry> = if started {
            files::read_json(&directory.join("inventory.json"))?
        } else {
            Vec::new()
        };
        let recovered_paths = entries
            .iter()
            .map(|entry| relative(&entry.path).map(|key| owner.root.join(key)))
            .collect::<io::Result<Vec<_>>>()?;
        if started && !restored {
            let (pin, root_identity) = files::pin_root(&owner.root)?;
            if root_identity != owner.root_identity {
                return Err(invalid("retained recovery root changed"));
            }
            Self {
                directory: directory.to_owned(),
                owner,
                entries,
                _root_pin: pin,
            }
            .restore()?;
        }
        // A restored failure belongs to the previous operation. Do not overwrite
        // later operator edits on retry. Keep the prior evidence and capture the
        // current data for the new operation instead.
        fs::rename(
            directory,
            files::unique_sibling(directory, "steam-retained-history"),
        )?;
        Ok(recovered_paths)
    }
}

fn marker(directory: &Path, name: &str) -> io::Result<bool> {
    match bounded(&directory.join(name), 32) {
        Ok(bytes) if matches!(bytes.as_slice(), b"1\n" | b"success\n" | b"failed\n") => Ok(true),
        Ok(_) => Err(invalid("retained data phase marker is invalid")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn failure(path: &Path, error: impl std::fmt::Display) -> SteamCmdError {
    SteamCmdError::InstallRollbackFailed {
        path: path.to_owned(),
        detail: format!(
            "Steam operator data preservation failed: {error}. Recovery evidence was retained; program readiness was not published."
        ),
    }
}

#[derive(Default)]
struct CancelCopyOnDrop(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for CancelCopyOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
