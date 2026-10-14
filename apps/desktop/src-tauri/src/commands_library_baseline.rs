use super::*;
use app_core::{InstallVerificationProgress, InstallVerificationStage};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(in crate::commands) struct LibraryBaselineRecorder {
    operation: StorageContextOperationGuard,
    descriptor: ModuleDescriptor,
    installation: app_steamcmd::InstallCancellation,
    fresh_payload_recorded: AtomicBool,
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
        }
    }

    pub(in crate::commands) async fn prepare_fresh_payload(
        &self,
        root: PathBuf,
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
                .map_err(|error| error.to_string())
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
                    app_storage::retain_published_library_program_baseline_with_progress(
                        &root,
                        &descriptor,
                        Some(cancellation),
                        progress,
                    )
                    .map_err(|error| error.to_string())?;
                } else if !source_was_empty && same_version {
                    app_storage::retain_verified_library_program_baseline_with_progress(
                        &root,
                        &descriptor,
                        Some(cancellation),
                        progress,
                    )
                    .map_err(|error| error.to_string())?;
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
        mut on_progress: impl FnMut(InstallVerificationProgress),
    ) -> Result<G, app_steamcmd::SteamCmdError> {
        if self.fresh_payload_recorded.load(Ordering::Acquire) {
            return self.finish(guard, root, false, false, on_progress).await;
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
                let verified = app_steamcmd::verify_installed_steam_package_with_progress(
                    &root,
                    &steamcmd,
                    app_id,
                    &version,
                    Some(cancellation),
                    &mut *progress,
                    |relative, file, digest| {
                        evidence
                            .capture_large_verified_file(relative, file, digest)
                            .map_err(std::io::Error::other)
                    },
                )
                .map_err(|error| error.to_string())?;
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
                Ok(guard)
            },
        )
        .await
    }
}

struct CancelBaselineOnDrop(Arc<AtomicBool>);

impl Drop for CancelBaselineOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

async fn run_baseline_worker<T, F>(
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
    // One latest snapshot bounds memory when the worker outpaces UI persistence.
    // User progress callbacks run here, never on the filesystem worker.
    let (progress_sender, mut progress_receiver) = tokio::sync::watch::channel(None);
    let mut worker = spawn_blocking_storage_context_task(operation, move || {
        work(&worker_signal, &mut |progress| {
            progress_sender.send_replace(Some(progress));
        })
    });
    let mut progress_open = true;
    let mut last_progress = None;
    let completed = loop {
        tokio::select! {
            biased;
            _ = installation.cancelled() => break None,
            _ = operation.cancelled() => break None,
            result = &mut worker => break Some(result),
            changed = progress_receiver.changed(), if progress_open => {
                if changed.is_err() {
                    progress_open = false;
                    continue;
                }
                let current = progress_receiver.borrow_and_update().clone();
                if let Some(progress) = current {
                    last_progress = Some(progress.clone());
                    on_progress(progress);
                }
            }
        }
    };
    let result = match completed {
        Some(result) => {
            // The worker can complete before the final channel change is polled.
            let current = progress_receiver.borrow_and_update().clone();
            if let Some(progress) = current
                && last_progress.as_ref() != Some(&progress)
            {
                on_progress(progress);
            }
            result
        }
        None => {
            signal.0.store(true, Ordering::Release);
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
