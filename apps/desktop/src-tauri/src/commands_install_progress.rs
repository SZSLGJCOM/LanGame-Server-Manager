use app_core::{BackgroundJob, InstallPhase, InstallProgress, JobStatus};
use app_steamcmd::InstallProgressUpdate;

pub(super) struct InstallationJobLease {
    state: std::sync::Arc<std::sync::RwLock<app_core::AppState>>,
    job_id: String,
    operation: crate::state::InstallOperationLease,
}

impl InstallationJobLease {
    pub(super) fn begin(
        state: &crate::state::DesktopState,
        job_id: String,
    ) -> Result<Self, String> {
        let operation = state.install_operations.begin(job_id.clone())?;
        Ok(Self {
            state: std::sync::Arc::clone(&state.app_state),
            job_id,
            operation,
        })
    }

    pub(super) fn cancellation(&self) -> &app_steamcmd::InstallCancellation {
        self.operation.cancellation()
    }

    pub(super) fn update(&self, update: impl FnOnce(&mut BackgroundJob)) -> Result<(), String> {
        let mut state = self
            .state
            .write()
            .map_err(|_| String::from("desktop state lock poisoned"))?;
        let job = state
            .jobs
            .iter_mut()
            .find(|job| job.id == self.job_id)
            .ok_or_else(|| String::from("Installation job was not found."))?;
        update(job);
        Ok(())
    }
}

impl Drop for InstallationJobLease {
    fn drop(&mut self) {
        // A failed persistence/refresh step or dropped request must not leave an
        // immortal Running row after its provider and cancellation owner are gone.
        let mut state = self
            .state
            .write()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(job) = state.jobs.iter_mut().find(|job| job.id == self.job_id)
            && matches!(job.status, JobStatus::Pending | JobStatus::Running)
        {
            fail_install_progress(job, 1.0);
            job.detail = Some(String::from(
                "Installation was interrupted before its result was saved.",
            ));
        }
    }
}

pub(super) fn queued_install_progress() -> InstallProgress {
    InstallProgress {
        phase: InstallPhase::Queued,
        downloaded_bytes: None,
        total_bytes: None,
        percent: None,
        elapsed_seconds: 0,
    }
}

pub(super) fn apply_install_progress(job: &mut BackgroundJob, update: &InstallProgressUpdate) {
    job.status = JobStatus::Running;
    job.progress_percent = update.progress_percent.clamp(1.0, 100.0);
    job.install_progress = update.install_progress.clone();
    job.detail = Some(update.detail.clone());
    if !update.output_excerpt.trim().is_empty() {
        job.output_excerpt = Some(update.output_excerpt.clone());
    }
}

pub(super) fn complete_install_progress(job: &mut BackgroundJob) {
    job.status = JobStatus::Completed;
    job.cancellable = false;
    job.progress_percent = 100.0;
    if let Some(progress) = &mut job.install_progress {
        progress.phase = InstallPhase::Ready;
        progress.downloaded_bytes = None;
        progress.total_bytes = None;
        progress.percent = Some(100.0);
    }
}

pub(super) fn fail_install_progress(job: &mut BackgroundJob, minimum_progress: f32) {
    job.status = JobStatus::Failed;
    job.cancellable = false;
    job.progress_percent = job.progress_percent.max(minimum_progress);
    if let Some(progress) = &mut job.install_progress {
        progress.downloaded_bytes = None;
        progress.total_bytes = None;
    }
}

pub(super) fn finish_install_error(job: &mut BackgroundJob, error: &app_steamcmd::SteamCmdError) {
    fail_install_progress(job, 1.0);
    if matches!(error, app_steamcmd::SteamCmdError::InstallCancelled { .. }) {
        job.status = JobStatus::Cancelled;
    }
}

#[tauri::command]
pub fn cancel_installation_job(
    state: tauri::State<'_, crate::state::DesktopState>,
    job_id: String,
) -> Result<BackgroundJob, String> {
    cancel_installation_job_inner(&state, &job_id)
}

fn cancel_installation_job_inner(
    state: &crate::state::DesktopState,
    job_id: &str,
) -> Result<BackgroundJob, String> {
    let mut app_state = state
        .app_state
        .write()
        .map_err(|_| String::from("desktop state lock poisoned"))?;
    let job = app_state
        .jobs
        .iter_mut()
        .find(|job| job.id == job_id)
        .ok_or_else(|| String::from("Installation job was not found."))?;
    if !matches!(job.status, JobStatus::Pending | JobStatus::Running) {
        return Ok(job.clone());
    }
    if !job.cancellable || !state.install_operations.request_cancel(job_id)? {
        return Err(String::from("This installation job cannot be stopped."));
    }
    job.cancel_requested = true;
    Ok(job.clone())
}

pub(super) fn begin_install_deployment(job: &mut BackgroundJob) {
    job.status = JobStatus::Running;
    job.progress_percent = 96.0;
    if let Some(progress) = &mut job.install_progress {
        progress.phase = InstallPhase::Installing;
        progress.downloaded_bytes = None;
        progress.total_bytes = None;
        progress.percent = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::JobKind;

    fn downloading_job() -> BackgroundJob {
        BackgroundJob {
            id: String::from("install-fixture"),
            cancellable: true,
            cancel_requested: false,
            kind: JobKind::DownloadGame,
            label: String::from("Install fixture game"),
            status: JobStatus::Running,
            progress_percent: 25.0,
            install_progress: Some(InstallProgress {
                phase: InstallPhase::Downloading,
                downloaded_bytes: Some(256),
                total_bytes: Some(1024),
                percent: Some(25.0),
                elapsed_seconds: 12,
            }),
            target_id: Some(String::from("fixture")),
            detail: None,
            output_excerpt: Some(String::from("previous output")),
        }
    }

    #[test]
    fn cancel_request_only_signals_provider_and_cannot_cancel_another_job() {
        let state = crate::state::DesktopState::default();
        let lease = InstallationJobLease::begin(&state, String::from("install-fixture")).unwrap();
        state.app_state.write().unwrap().jobs = vec![downloading_job()];
        let pending = cancel_installation_job_inner(&state, "install-fixture").unwrap();
        assert!(matches!(pending.status, JobStatus::Running));
        assert!(pending.cancel_requested && pending.cancellable);
        assert!(lease.cancellation().is_cancelled());
        assert!(cancel_installation_job_inner(&state, "other").is_err());
        assert!(cancel_installation_job_inner(&state, "install-fixture").is_ok());
        {
            let mut app = state.app_state.write().unwrap();
            finish_install_error(
                &mut app.jobs[0],
                &app_steamcmd::SteamCmdError::InstallCancelled {
                    operation: String::from("fixture"),
                },
            );
        }
        let terminal = cancel_installation_job_inner(&state, "install-fixture").unwrap();
        assert!(matches!(terminal.status, JobStatus::Cancelled));
        assert!(!terminal.cancellable);
        drop(lease);
        assert!(matches!(
            state.app_state.read().unwrap().jobs[0].status,
            JobStatus::Cancelled
        ));
    }

    #[test]
    fn lost_request_does_not_leave_a_running_job_without_a_cancel_owner() {
        let state = crate::state::DesktopState::default();
        let lease = InstallationJobLease::begin(&state, String::from("install-fixture")).unwrap();
        state.app_state.write().unwrap().jobs = vec![downloading_job()];
        drop(lease);
        let app = state.app_state.read().unwrap();
        assert!(matches!(app.jobs[0].status, JobStatus::Failed));
        assert!(!app.jobs[0].cancellable);
        assert!(
            app.jobs[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("interrupted")
        );
    }

    #[test]
    fn failed_cleanup_must_not_be_reported_as_cancelled() {
        let mut job = downloading_job();
        job.cancel_requested = true;
        finish_install_error(
            &mut job,
            &app_steamcmd::SteamCmdError::InstallProcessCleanupFailed {
                operation: String::from("fixture"),
                detail: String::from("child still active"),
            },
        );
        assert!(matches!(job.status, JobStatus::Failed));
        assert!(!job.cancellable);
    }

    #[test]
    fn phase_transition_replaces_download_measurements_and_preserves_output() {
        let mut job = downloading_job();
        let progress = InstallProgress {
            phase: InstallPhase::Verifying,
            downloaded_bytes: None,
            total_bytes: None,
            percent: None,
            elapsed_seconds: 18,
        };
        apply_install_progress(
            &mut job,
            &InstallProgressUpdate {
                progress_percent: 90.0,
                install_progress: Some(progress.clone()),
                detail: String::from("Verifying files"),
                output_excerpt: String::new(),
            },
        );

        assert_eq!(job.install_progress, Some(progress));
        assert_eq!(job.progress_percent, 90.0);
        assert_eq!(job.output_excerpt.as_deref(), Some("previous output"));
        assert_eq!(job.detail.as_deref(), Some("Verifying files"));
    }

    #[test]
    fn unmeasured_workshop_update_clears_prior_structured_progress() {
        let mut job = downloading_job();
        apply_install_progress(
            &mut job,
            &InstallProgressUpdate {
                progress_percent: 40.0,
                install_progress: None,
                detail: String::from("Preparing Workshop content"),
                output_excerpt: String::from("current output"),
            },
        );

        assert!(job.install_progress.is_none());
        assert_eq!(job.output_excerpt.as_deref(), Some("current output"));
        complete_install_progress(&mut job);
        assert!(job.install_progress.is_none());
    }

    #[test]
    fn completion_keeps_elapsed_time_and_clears_transfer_measurements() {
        let mut job = downloading_job();
        complete_install_progress(&mut job);

        assert!(matches!(job.status, JobStatus::Completed));
        assert_eq!(job.progress_percent, 100.0);
        assert_eq!(
            job.install_progress,
            Some(InstallProgress {
                phase: InstallPhase::Ready,
                downloaded_bytes: None,
                total_bytes: None,
                percent: Some(100.0),
                elapsed_seconds: 12,
            })
        );
    }

    #[test]
    fn failed_download_retains_progress_without_stale_transfer_measurements() {
        let mut job = downloading_job();
        fail_install_progress(&mut job, 1.0);

        assert!(matches!(job.status, JobStatus::Failed));
        assert_eq!(job.progress_percent, 25.0);
        let progress = job.install_progress.unwrap();
        assert_eq!(progress.phase, InstallPhase::Downloading);
        assert_eq!(progress.percent, Some(25.0));
        assert_eq!(progress.elapsed_seconds, 12);
        assert_eq!(progress.downloaded_bytes, None);
        assert_eq!(progress.total_bytes, None);
    }

    #[test]
    fn deployment_does_not_reuse_download_percentage_or_bytes() {
        let mut job = downloading_job();
        begin_install_deployment(&mut job);

        let progress = job.install_progress.unwrap();
        assert_eq!(progress.phase, InstallPhase::Installing);
        assert_eq!(progress.percent, None);
        assert_eq!(progress.downloaded_bytes, None);
        assert_eq!(progress.total_bytes, None);
        assert_eq!(progress.elapsed_seconds, 12);
    }
}
