use std::sync::{Arc, Mutex};
use std::time::Instant;

use app_steamcmd::{InstallCancellation, SteamCmdPreparePhase, SteamCmdPrepareProgress};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct SteamCmdPrepareSnapshot {
    operation_id: String,
    active: bool,
    cancellable: bool,
    cancel_requested: bool,
    cancelled: bool,
    #[serde(flatten)]
    progress: SteamCmdPrepareProgress,
    elapsed_seconds: u64,
    idle_seconds: u64,
    error: Option<String>,
}

struct PreparationRecord {
    operation_id: String,
    started_at: Instant,
    updated_at: Instant,
    finished_at: Option<Instant>,
    progress: SteamCmdPrepareProgress,
    error: Option<String>,
    cancellation: InstallCancellation,
    cancelled: bool,
}

/// Retain one bounded preparation result. The operation ID prevents late polls
/// from a previous attempt from observing or replacing the next attempt.
#[derive(Default)]
pub(crate) struct SteamCmdPreparationTracker {
    record: Mutex<Option<PreparationRecord>>,
}

impl SteamCmdPreparationTracker {
    pub(crate) fn begin(
        self: &Arc<Self>,
        operation_id: String,
    ) -> Result<SteamCmdPreparationLease, String> {
        validate_operation_id(&operation_id)?;
        let mut record = self.record.lock().map_err(|_| tracker_lock_error())?;
        if record
            .as_ref()
            .is_some_and(|current| current.finished_at.is_none())
        {
            return Err(String::from("SteamCMD preparation is already running."));
        }
        let now = Instant::now();
        *record = Some(PreparationRecord {
            operation_id: operation_id.clone(),
            started_at: now,
            updated_at: now,
            finished_at: None,
            progress: SteamCmdPrepareProgress {
                phase: SteamCmdPreparePhase::Queued,
                detail: String::from("Waiting for the SteamCMD installation slot..."),
                downloaded_bytes: None,
                total_bytes: None,
                output_excerpt: String::new(),
            },
            error: None,
            cancellation: InstallCancellation::new(),
            cancelled: false,
        });
        Ok(SteamCmdPreparationLease {
            tracker: Arc::clone(self),
            operation_id,
            finished: false,
        })
    }

    pub(crate) fn snapshot(
        &self,
        operation_id: Option<&str>,
    ) -> Result<Option<SteamCmdPrepareSnapshot>, String> {
        if let Some(operation_id) = operation_id {
            validate_operation_id(operation_id)?;
        }
        let record = self.record.lock().map_err(|_| tracker_lock_error())?;
        Ok(record
            .as_ref()
            .filter(|current| operation_id.is_none_or(|id| current.operation_id == id))
            .map(|current| {
                let now = current.finished_at.unwrap_or_else(Instant::now);
                SteamCmdPrepareSnapshot {
                    operation_id: current.operation_id.clone(),
                    active: current.finished_at.is_none(),
                    cancellable: current.finished_at.is_none(),
                    cancel_requested: current.cancellation.is_cancelled(),
                    cancelled: current.cancelled,
                    progress: current.progress.clone(),
                    elapsed_seconds: now.saturating_duration_since(current.started_at).as_secs(),
                    idle_seconds: now.saturating_duration_since(current.updated_at).as_secs(),
                    error: current.error.clone(),
                }
            }))
    }

    pub(crate) fn request_cancel(
        &self,
        operation_id: &str,
    ) -> Result<SteamCmdPrepareSnapshot, String> {
        validate_operation_id(operation_id)?;
        {
            let record = self.record.lock().map_err(|_| tracker_lock_error())?;
            let current = record
                .as_ref()
                .filter(|current| current.operation_id == operation_id)
                .ok_or_else(|| String::from("SteamCMD preparation operation was not found."))?;
            if current.finished_at.is_none() {
                current.cancellation.cancel();
            }
        }
        self.snapshot(Some(operation_id))?
            .ok_or_else(|| String::from("SteamCMD preparation operation changed."))
    }

    pub(crate) fn request_cancel_all(&self) -> Result<(), String> {
        let record = self.record.lock().map_err(|_| tracker_lock_error())?;
        if let Some(current) = record.as_ref().filter(|item| item.finished_at.is_none()) {
            current.cancellation.cancel();
        }
        Ok(())
    }
}

pub(crate) struct SteamCmdPreparationLease {
    tracker: Arc<SteamCmdPreparationTracker>,
    operation_id: String,
    finished: bool,
}

impl SteamCmdPreparationLease {
    pub(crate) fn cancellation(&self) -> Result<InstallCancellation, String> {
        let record = self
            .tracker
            .record
            .lock()
            .map_err(|_| tracker_lock_error())?;
        record
            .as_ref()
            .filter(|current| current.operation_id == self.operation_id)
            .map(|current| current.cancellation.clone())
            .ok_or_else(|| String::from("SteamCMD preparation operation changed."))
    }

    pub(crate) fn update(&self, progress: SteamCmdPrepareProgress) {
        if self.finished {
            return;
        }
        let mut record = self
            .tracker
            .record
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(current) = record.as_mut().filter(|current| {
            current.operation_id == self.operation_id && current.finished_at.is_none()
        }) {
            current.updated_at = Instant::now();
            current.progress = progress;
        }
    }

    pub(crate) fn finish(&mut self, error: Option<String>) {
        self.finish_outcome(error, false);
    }

    pub(crate) fn finish_cancelled(&mut self) {
        self.finish_outcome(None, true);
    }

    fn finish_outcome(&mut self, error: Option<String>, cancelled: bool) {
        if self.finished {
            return;
        }
        let mut record = self
            .tracker
            .record
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(current) = record
            .as_mut()
            .filter(|current| current.operation_id == self.operation_id)
        {
            current.finished_at = Some(Instant::now());
            current.error = error;
            current.cancelled = cancelled;
            if cancelled || current.error.is_some() {
                current.progress.downloaded_bytes = None;
                current.progress.total_bytes = None;
            }
        }
        self.finished = true;
    }
}

impl Drop for SteamCmdPreparationLease {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(Some(String::from("SteamCMD preparation was interrupted.")));
        }
    }
}

fn tracker_lock_error() -> String {
    String::from("SteamCMD preparation state lock was poisoned.")
}

fn validate_operation_id(operation_id: &str) -> Result<(), String> {
    if operation_id.is_empty()
        || operation_id.len() > 128
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(String::from("Invalid SteamCMD preparation operation ID."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_request_keeps_busy_until_provider_cleanup_finishes() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        let mut lease = tracker.begin(String::from("cancel-me")).unwrap();
        let cancellation = lease.cancellation().unwrap();
        let pending = tracker.request_cancel("cancel-me").unwrap();
        assert!(pending.active && pending.cancellable && pending.cancel_requested);
        assert!(!pending.cancelled);
        assert!(cancellation.is_cancelled());
        assert!(tracker.request_cancel("cancel-me").unwrap().active);
        assert!(tracker.request_cancel("another-operation").is_err());
        assert!(tracker.begin(String::from("next")).is_err());
        lease.finish_cancelled();
        let terminal = tracker.snapshot(None).unwrap().unwrap();
        assert!(!terminal.active && !terminal.cancellable && terminal.cancelled);
        assert!(terminal.error.is_none());
        assert!(tracker.request_cancel("cancel-me").unwrap().cancelled);
        assert!(tracker.begin(String::from("next")).is_ok());
    }

    #[test]
    fn new_frontend_can_discover_an_existing_preparation_without_restarting_it() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        assert!(tracker.snapshot(None).unwrap().is_none());
        let mut lease = tracker.begin(String::from("running-operation")).unwrap();
        let active = tracker.snapshot(None).unwrap().unwrap();
        assert_eq!(active.operation_id, "running-operation");
        assert!(active.active);
        lease.finish(Some(String::from("network unavailable")));
        let completed = tracker.snapshot(None).unwrap().unwrap();
        assert!(!completed.active);
        assert_eq!(completed.error.as_deref(), Some("network unavailable"));
    }

    #[test]
    fn preparation_excludes_duplicates_and_keeps_attempts_separate() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        let mut first = tracker.begin(String::from("first")).unwrap();
        assert!(tracker.begin(String::from("second")).is_err());
        assert!(tracker.snapshot(Some("second")).unwrap().is_none());
        first.finish(Some(String::from("network unavailable")));
        let completed = tracker.snapshot(Some("first")).unwrap().unwrap();
        assert!(!completed.active);
        assert_eq!(completed.error.as_deref(), Some("network unavailable"));
        let _second = tracker.begin(String::from("second")).unwrap();
        first.finish(None);
        assert!(tracker.snapshot(Some("second")).unwrap().unwrap().active);
        assert!(tracker.snapshot(Some("first")).unwrap().is_none());
    }

    #[test]
    fn interrupted_operation_releases_busy_state_and_retains_diagnostics() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        let lease = tracker.begin(String::from("first")).unwrap();
        lease.update(SteamCmdPrepareProgress {
            phase: SteamCmdPreparePhase::Updating,
            detail: String::from("Downloading update"),
            downloaded_bytes: Some(10),
            total_bytes: Some(20),
            output_excerpt: String::from("Downloading update (10 of 20 KB)..."),
        });
        drop(lease);
        let snapshot = tracker.snapshot(Some("first")).unwrap().unwrap();
        assert!(!snapshot.active);
        assert!(snapshot.error.is_some());
        assert_eq!(snapshot.progress.downloaded_bytes, None);
        let json = serde_json::to_value(snapshot).unwrap();
        assert_eq!(json["phase"], "updating");
        assert!(json["total_bytes"].is_null());
        assert!(tracker.begin(String::from("next")).is_ok());
    }

    #[test]
    fn preparation_rejects_unbounded_or_malformed_identifiers() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        for value in [String::new(), "x".repeat(129), String::from("not an id")] {
            assert!(tracker.begin(value.clone()).is_err());
            assert!(tracker.snapshot(Some(&value)).is_err());
        }
    }

    #[test]
    fn finished_lease_cannot_mutate_a_reused_operation_id() {
        let tracker = Arc::new(SteamCmdPreparationTracker::default());
        let mut first = tracker.begin(String::from("same-id")).unwrap();
        first.finish(None);
        let _second = tracker.begin(String::from("same-id")).unwrap();
        first.finish(Some(String::from("late error")));
        first.update(SteamCmdPrepareProgress {
            phase: SteamCmdPreparePhase::Ready,
            detail: String::from("late success"),
            downloaded_bytes: None,
            total_bytes: None,
            output_excerpt: String::new(),
        });
        let snapshot = tracker.snapshot(Some("same-id")).unwrap().unwrap();
        assert!(snapshot.active);
        assert!(snapshot.error.is_none());
        assert_eq!(snapshot.progress.phase, SteamCmdPreparePhase::Queued);
    }
}
