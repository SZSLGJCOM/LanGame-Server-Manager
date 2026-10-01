use super::*;
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
    ) -> Result<(), app_steamcmd::SteamCmdError> {
        let descriptor = self.descriptor.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &self.descriptor.summary.id,
            move |cancellation| {
                app_storage::record_library_program_baseline(
                    &root,
                    &descriptor,
                    true,
                    Some(cancellation),
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
    ) -> Result<G, app_steamcmd::SteamCmdError> {
        let fresh_payload_recorded = self.fresh_payload_recorded.load(Ordering::Acquire);
        let descriptor = self.descriptor;
        let module_id = descriptor.summary.id.clone();
        run_baseline_worker(
            &self.operation,
            &self.installation,
            &module_id,
            move |cancellation| {
                if fresh_payload_recorded {
                    // Retained user data can replace shipped defaults at publication.
                    // Keep only the installer's allowlist that still matches afterwards.
                    app_storage::retain_published_library_program_baseline(
                        &root,
                        &descriptor,
                        Some(cancellation),
                    )
                    .map_err(|error| error.to_string())?;
                } else if !source_was_empty && same_version {
                    app_storage::retain_verified_library_program_baseline(
                        &root,
                        &descriptor,
                        Some(cancellation),
                    )
                    .map_err(|error| error.to_string())?;
                } else {
                    app_storage::record_library_program_baseline(
                        &root,
                        &descriptor,
                        source_was_empty,
                        Some(cancellation),
                    )
                    .map_err(|error| error.to_string())?;
                }
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
    work: F,
) -> Result<T, app_steamcmd::SteamCmdError>
where
    T: Send + 'static,
    F: FnOnce(&AtomicBool) -> Result<T, String> + Send + 'static,
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
    let mut worker = spawn_blocking_storage_context_task(operation, move || work(&worker_signal));
    let completed = tokio::select! {
        biased;
        _ = installation.cancelled() => None,
        _ = operation.cancelled() => None,
        result = &mut worker => Some(result),
    };
    let result = match completed {
        Some(result) => result,
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
