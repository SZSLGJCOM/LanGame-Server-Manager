use super::*;
use app_core::{InstallVerificationProgress, InstallVerificationStage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(in crate::commands) struct LibraryBaselineRecorder {
    operation: StorageContextOperationGuard,
    descriptor: ModuleDescriptor,
    installation: app_steamcmd::InstallCancellation,
    fresh_payload_recorded: AtomicBool,
    require_verified_inventory: bool,
}

pub(in crate::commands) enum SteamAcquisition<G> {
    Recorded(G),
    MissingMetadata { guard: G, detail: String },
}

impl LibraryBaselineRecorder {
    pub(in crate::commands) fn new(
        operation: &StorageContextOperationGuard,
        descriptor: &ModuleDescriptor,
        installation: &app_steamcmd::InstallCancellation,
    ) -> Self {
        Self {
            operation: operation.clone(),
            descriptor: descriptor.clone(),
            installation: installation.clone(),
            fresh_payload_recorded: AtomicBool::new(false),
            require_verified_inventory: false,
        }
    }

    pub(in crate::commands) fn require_verified_inventory(mut self) -> Self {
        self.require_verified_inventory = true;
        self
    }

    pub(in crate::commands) async fn prepare_fresh_payload(
        &self,
        root: PathBuf,
        on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<(), app_steamcmd::SteamCmdError> {
        self.prepare_payload(root, None, on_progress).await
    }

    pub(in crate::commands) async fn prepare_archive_replacement(
        &self,
        root: PathBuf,
        source: PathBuf,
        protected: Vec<PathBuf>,
        on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<(), app_steamcmd::SteamCmdError> {
        self.prepare_payload(root, Some((source, protected)), on_progress)
            .await
    }

    async fn prepare_payload(
        &self,
        root: PathBuf,
        replacement: Option<(PathBuf, Vec<PathBuf>)>,
        on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<(), app_steamcmd::SteamCmdError> {
        let descriptor = self.descriptor.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &self.descriptor.summary.id,
            on_progress,
            move |cancellation, progress| {
                app_storage::record_library_program_baseline_with_progress(
                    &root,
                    &descriptor,
                    true,
                    Some(cancellation),
                    progress,
                )
                .map_err(|error| error.to_string())?;
                if let Some((source, protected)) = replacement {
                    app_storage::merge_archive_program_update(
                        &source,
                        &root,
                        &descriptor,
                        &protected,
                        Some(cancellation),
                    )
                    .map_err(|error| error.to_string())?;
                }
                Ok(())
            },
        )
        .await?;
        self.fresh_payload_recorded.store(true, Ordering::Release);
        Ok(())
    }

    pub(in crate::commands) async fn finish<G: Send + 'static>(
        self,
        guard: G,
        root: PathBuf,
        source_was_empty: bool,
        same_version: bool,
        on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<G, app_steamcmd::SteamCmdError> {
        let fresh_payload_recorded = self.fresh_payload_recorded.load(Ordering::Acquire);
        let require_verified_inventory = self.require_verified_inventory;
        let descriptor = self.descriptor;
        let module_id = descriptor.summary.id.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &module_id,
            on_progress,
            move |cancellation, progress| {
                if fresh_payload_recorded {
                    // Retained user data can replace shipped defaults at publication.
                    // Keep only the installer's allowlist that still matches afterwards.
                    let verified =
                        app_storage::retain_published_library_program_baseline_with_progress(
                            &root,
                            &descriptor,
                            Some(cancellation),
                            progress,
                        )
                        .map_err(|error| error.to_string())?;
                    if !verified {
                        return Err(
                            "Published server program does not match its verified inventory."
                                .into(),
                        );
                    }
                } else if !source_was_empty && same_version {
                    let verified = app_storage::retain_existing_program_baseline_with_progress(
                        &root,
                        &descriptor,
                        Some(cancellation),
                        progress,
                    )
                    .map_err(|error| error.to_string())?;
                    if require_verified_inventory && !verified {
                        return Err(
                            "Server program repair did not restore its verified inventory.".into(),
                        );
                    }
                } else {
                    app_storage::record_library_program_baseline_with_progress(
                        &root,
                        &descriptor,
                        source_was_empty,
                        Some(cancellation),
                        progress,
                    )
                    .map_err(|error| error.to_string())?;
                }
                Ok(guard)
            },
        )
        .await
    }

    /// A pending marker proves ownership, not clean contents. Interrupted Steam
    /// downloads can be retried in place, but only exact depot-verified files
    /// become the next copy allowlist; operator additions remain untouched.
    pub(in crate::commands) async fn finish_steam_acquisition<G: Send + 'static>(
        self,
        guard: G,
        root: PathBuf,
        steamcmd: PathBuf,
        app_id: u32,
        version: String,
        on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<G, app_steamcmd::SteamCmdError> {
        let module_id = self.descriptor.summary.id.clone();
        match self
            .finish_steam_acquisition_for_creation(
                guard,
                root,
                steamcmd,
                app_id,
                version,
                on_progress,
            )
            .await?
        {
            SteamAcquisition::Recorded(guard) => Ok(guard),
            SteamAcquisition::MissingMetadata { guard, detail } => {
                drop(guard);
                Err(
                    app_steamcmd::SteamCmdError::InstallationVerificationFailed {
                        module_id,
                        operation: "recording original program inventory".into(),
                        detail,
                    },
                )
            }
        }
    }

    /// Creation alone may continue into its existing fresh-root repair path
    /// when official metadata is absent. No original files are certified then.
    pub(in crate::commands) async fn finish_steam_acquisition_for_creation<G: Send + 'static>(
        self,
        guard: G,
        root: PathBuf,
        steamcmd: PathBuf,
        app_id: u32,
        version: String,
        mut on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<SteamAcquisition<G>, app_steamcmd::SteamCmdError> {
        if self.fresh_payload_recorded.load(Ordering::Acquire) {
            return self
                .finish(guard, root, false, false, on_progress)
                .await
                .map(SteamAcquisition::Recorded);
        }
        // The caller can cancel before any filesystem worker starts. Totals
        // become measurable after the official depot inventory has been read.
        on_progress(InstallVerificationProgress {
            stage: InstallVerificationStage::SteamFiles,
            completed_bytes: 0,
            total_bytes: 0,
            completed_files: 0,
            total_files: 0,
        });
        let descriptor = self.descriptor;
        let module_id = descriptor.summary.id.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &module_id,
            on_progress,
            move |cancellation, progress| {
                let mut evidence = app_storage::VerifiedProgramFiles::default();
                let program_path = app_storage::program_package_path_filter(&descriptor)
                    .map_err(|error| error.to_string())?;
                let optional_path = |path: &str| !program_path(path);
                let verified = match app_steamcmd::verify_installed_steam_package_with_progress(
                    &root,
                    &steamcmd,
                    app_id,
                    &version,
                    app_steamcmd::SteamPackageVerificationOptions {
                        cancellation: Some(cancellation),
                        optional_path: Some(&optional_path),
                        ..Default::default()
                    },
                    &mut *progress,
                    |relative, file, digest| {
                        evidence
                            .capture_large_verified_file(relative, file, digest)
                            .map_err(std::io::Error::other)
                    },
                ) {
                    Ok(verified) => verified,
                    Err(error) if app_steamcmd::steam_depot::steam_metadata_is_missing(&error) => {
                        return Ok(SteamAcquisition::MissingMetadata {
                            guard,
                            detail: error.to_string(),
                        });
                    }
                    Err(error) => return Err(error.to_string()),
                };
                app_storage::record_verified_library_program_baseline_with_progress(
                    &root,
                    &descriptor,
                    app_storage::ProgramBaselineInventory {
                        files: verified.files,
                        directories: verified.directories,
                        verified_files: evidence,
                    },
                    Some(cancellation),
                    progress,
                )
                .map_err(|error| error.to_string())?;
                Ok(SteamAcquisition::Recorded(guard))
            },
        )
        .await
    }

    /// Older Steam inventories omitted shipped mutable defaults before the
    /// original/copy inventories were separated. Recover their evidence locally
    /// under the caller's program lease; no download or game process is started.
    pub(in crate::commands) async fn restore_steam_initial_defaults<G: Send + 'static>(
        self,
        guard: G,
        root: PathBuf,
        steamcmd: PathBuf,
        app_id: u32,
        version: String,
        mut on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<(G, bool), app_steamcmd::SteamCmdError> {
        let descriptor = self.descriptor.clone();
        let inspect_root = root.clone();
        let inspect_steamcmd = steamcmd.clone();
        let inspect_version = version.clone();
        let defaults = run_baseline_worker(
            &self.operation,
            &self.installation,
            &self.descriptor.summary.id,
            &mut on_progress,
            move |cancellation, progress| {
                let files = verify_steam_original_defaults(
                    &inspect_root,
                    &inspect_steamcmd,
                    &descriptor,
                    app_id,
                    &inspect_version,
                    cancellation,
                    progress,
                )
                .map_err(|error| error.to_string())?;
                if !app_storage::library_reuse_preview_with_original_defaults(
                    &inspect_root,
                    &descriptor.summary.id,
                    &files,
                    Some(cancellation),
                )
                .map_err(|error| error.to_string())?
                {
                    return Ok(None);
                }
                let complete = app_storage::library_initial_program_matches_files(
                    &inspect_root,
                    &descriptor.summary.id,
                    &files,
                )
                .map_err(|error| error.to_string())?;
                Ok((!complete).then_some(files))
            },
        )
        .await?;
        let Some(defaults) = defaults else {
            return Ok((guard, false));
        };
        let descriptor = self.descriptor;
        let module_id = descriptor.summary.id.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &module_id,
            on_progress,
            move |cancellation, progress| {
                // Existing certified program hashes remain authoritative even
                // after Steam prunes unrelated depot caches. Verify the whole
                // merged inventory before publishing the recovered defaults.
                app_storage::restore_library_initial_program_defaults_with_progress(
                    &root,
                    &descriptor,
                    defaults,
                    Some(cancellation),
                    progress,
                )
                .map_err(|error| error.to_string())?;
                Ok((guard, true))
            },
        )
        .await
    }
}

pub(in crate::commands) fn verify_steam_original_defaults(
    root: &Path,
    steamcmd: &Path,
    descriptor: &ModuleDescriptor,
    app_id: u32,
    version: &str,
    cancellation: &AtomicBool,
    progress: &mut dyn FnMut(InstallVerificationProgress),
) -> std::io::Result<std::collections::BTreeMap<String, String>> {
    let program_path =
        app_storage::program_package_path_filter(descriptor).map_err(std::io::Error::other)?;
    let defaults = |path: &str| !program_path(path);
    let mut verified = match app_steamcmd::verify_installed_steam_package_with_progress(
        root,
        steamcmd,
        app_id,
        version,
        app_steamcmd::SteamPackageVerificationOptions {
            cancellation: Some(cancellation),
            include_path: Some(&defaults),
            optional_path: Some(&defaults),
            allow_missing_depots: true,
        },
        progress,
        |_, _, _| Ok(()),
    ) {
        Ok(verified) => verified,
        // This probe only improves a conservative copy plan. Missing, stale or
        // invalid cached evidence cannot authorize direct reuse and must not
        // block the existing clean-package copy checks. Cancellation and I/O
        // failures still propagate; acquisition certification remains strict.
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::InvalidData
            ) =>
        {
            return Ok(Default::default());
        }
        Err(error) => return Err(error),
    };
    // The verifier always checks ACFs; they are generation evidence, not defaults.
    verified.files.retain(|path, _| defaults(path));
    Ok(verified.files)
}

struct CancelBaselineOnDrop(Arc<AtomicBool>);

impl Drop for CancelBaselineOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(in crate::commands) async fn run_baseline_worker<T, F>(
    operation: &StorageContextOperationGuard,
    installation: &app_steamcmd::InstallCancellation,
    module_id: &str,
    mut on_progress: impl FnMut(InstallVerificationProgress),
    work: F,
) -> Result<T, app_steamcmd::SteamCmdError>
where
    T: Send + 'static,
    F: FnOnce(&AtomicBool, &mut dyn FnMut(InstallVerificationProgress)) -> Result<T, String>
        + Send
        + 'static,
{
    let cancelled = || app_steamcmd::SteamCmdError::InstallCancelled {
        operation: "recording original program inventory".into(),
    };
    let failed = |detail| app_steamcmd::SteamCmdError::InstallationVerificationFailed {
        module_id: module_id.to_owned(),
        operation: "recording original program inventory".into(),
        detail,
    };
    let context_cancelled = operation.cancellation_token();
    if installation.is_cancelled() || context_cancelled.load(Ordering::Acquire) {
        return Err(cancelled());
    }
    let signal = CancelBaselineOnDrop(Arc::new(AtomicBool::new(false)));
    let worker_signal = Arc::clone(&signal.0);
    // Progress is a bounded ordered stream: phase starts and final measurements
    // must survive a fast transition into recording the manifest. Only this
    // blocking filesystem worker waits for channel capacity.
    let (progress_sender, mut progress_receiver) = tokio::sync::mpsc::channel(16);
    let mut worker = spawn_blocking_storage_context_task(operation, move || {
        work(&worker_signal, &mut |progress| {
            // A closed receiver means cancellation; the worker observes its
            // cancellation flag at the next filesystem boundary.
            let _ = progress_sender.blocking_send(progress);
        })
    });
    let mut progress_open = true;
    let completed = loop {
        tokio::select! {
            biased;
            _ = installation.cancelled() => break None,
            _ = operation.cancelled() => break None,
            result = &mut worker => break Some(result),
            progress = progress_receiver.recv(), if progress_open => {
                match progress {
                    Some(progress) => on_progress(progress),
                    None => progress_open = false,
                }
            }
        }
    };
    let result = match completed {
        Some(result) => {
            // Drain every queued boundary, not merely the last stage snapshot.
            while let Ok(progress) = progress_receiver.try_recv() {
                on_progress(progress);
            }
            result
        }
        None => {
            signal.0.store(true, Ordering::Release);
            progress_receiver.close();
            // The installer may recover/remove staging only after all baseline
            // reads and atomic writes have stopped and worker leases are released.
            drop(
                worker
                    .await
                    .map_err(|error| failed(format!("program baseline worker failed: {error}")))?,
            );
            return Err(cancelled());
        }
    }
    .map_err(|error| failed(format!("program baseline worker failed: {error}")))?;
    if installation.is_cancelled() || context_cancelled.load(Ordering::Acquire) {
        return Err(cancelled());
    }
    result.map_err(failed)
}

#[cfg(test)]
#[path = "commands_library_baseline_tests.rs"]
mod tests;
