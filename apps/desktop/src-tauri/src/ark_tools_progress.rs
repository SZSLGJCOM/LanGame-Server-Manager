use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

// Keep recent outcomes inspectable without growing with every instance or retry.
const MAX_RECORDS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallPhase {
    Queued,
    Checking,
    Connecting,
    Downloading,
    Verifying,
    Installing,
    Ready,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallUpdate {
    pub phase: InstallPhase,
    pub source: Option<String>,
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

pub type InstallReporter = Arc<dyn Fn(InstallUpdate) + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSnapshot {
    pub instance_id: String,
    pub operation_id: String,
    pub active: bool,
    #[serde(flatten)]
    pub update: InstallUpdate,
    pub error: Option<String>,
    pub elapsed_seconds: u64,
}

struct Record {
    snapshot: InstallSnapshot,
    started_at: Instant,
}

#[derive(Default)]
pub struct InstallTracker {
    records: Mutex<VecDeque<Record>>,
}

impl InstallTracker {
    pub fn begin(
        self: &Arc<Self>,
        instance_id: String,
        operation_id: String,
    ) -> Result<InstallLease, String> {
        validate_identity(&instance_id, &operation_id)?;
        let mut records = self.records.lock().map_err(|_| lock_error())?;
        if records
            .iter()
            .any(|record| record.snapshot.instance_id == instance_id && record.snapshot.active)
        {
            return Err("This instance's ARK extension installation is already running.".into());
        }
        if records
            .iter()
            .any(|record| record.snapshot.operation_id == operation_id)
        {
            return Err("ARK extension installation operation ID has already been used.".into());
        }
        if records.len() == MAX_RECORDS {
            let index = records
                .iter()
                .position(|record| !record.snapshot.active)
                .ok_or(
                    "Too many ARK extension installations are running. Wait for one to finish.",
                )?;
            records.remove(index);
        }
        records.push_back(Record {
            snapshot: InstallSnapshot {
                instance_id: instance_id.clone(),
                operation_id: operation_id.clone(),
                active: true,
                update: InstallUpdate {
                    phase: InstallPhase::Queued,
                    source: None,
                    downloaded_bytes: None,
                    total_bytes: None,
                },
                error: None,
                elapsed_seconds: 0,
            },
            started_at: Instant::now(),
        });
        Ok(InstallLease {
            tracker: Arc::clone(self),
            instance_id,
            operation_id,
            finished: false,
        })
    }

    pub fn snapshot(
        &self,
        instance_id: &str,
        operation_id: Option<&str>,
    ) -> Result<Option<InstallSnapshot>, String> {
        validate_identity(instance_id, operation_id.unwrap_or("latest"))?;
        let records = self.records.lock().map_err(|_| lock_error())?;
        Ok(records
            .iter()
            .rev()
            .find(|record| {
                record.snapshot.instance_id == instance_id
                    && operation_id.is_none_or(|id| record.snapshot.operation_id == id)
            })
            .map(|record| {
                let mut snapshot = record.snapshot.clone();
                if snapshot.active {
                    snapshot.elapsed_seconds = record.started_at.elapsed().as_secs();
                }
                snapshot
            }))
    }

    pub fn forget_completed(&self, instance_id: &str) {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // A new installation can queue while removal holds the instance lock.
        // Its lease must survive cleanup of the installation we just removed.
        records
            .retain(|record| record.snapshot.instance_id != instance_id || record.snapshot.active);
    }
}

pub struct InstallLease {
    tracker: Arc<InstallTracker>,
    instance_id: String,
    operation_id: String,
    finished: bool,
}

impl InstallLease {
    pub fn reporter(&self) -> InstallReporter {
        let tracker = Arc::clone(&self.tracker);
        let instance_id = self.instance_id.clone();
        let operation_id = self.operation_id.clone();
        Arc::new(move |mut update| {
            // Only the command's verified result can publish a terminal outcome.
            if matches!(update.phase, InstallPhase::Ready | InstallPhase::Failed) {
                return;
            }
            let mut records = tracker
                .records
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(record) = records.iter_mut().find(|record| {
                record.snapshot.instance_id == instance_id
                    && record.snapshot.operation_id == operation_id
                    && record.snapshot.active
            }) {
                if update.source.is_none() {
                    update.source = record.snapshot.update.source.clone();
                }
                record.snapshot.update = update;
            }
        })
    }

    pub fn snapshot(&self) -> Option<InstallSnapshot> {
        let records = self
            .tracker
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        records
            .iter()
            .find(|record| {
                record.snapshot.instance_id == self.instance_id
                    && record.snapshot.operation_id == self.operation_id
            })
            .map(|record| record.snapshot.clone())
    }

    pub fn finish(&mut self, error: Option<String>) -> Option<InstallSnapshot> {
        if self.finished {
            return self.snapshot();
        }
        let mut records = self
            .tracker
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let terminal = records
            .iter_mut()
            .find(|record| {
                record.snapshot.instance_id == self.instance_id
                    && record.snapshot.operation_id == self.operation_id
                    && record.snapshot.active
            })
            .map(|record| {
                record.snapshot.active = false;
                record.snapshot.elapsed_seconds = record.started_at.elapsed().as_secs();
                record.snapshot.update.phase = if error.is_some() {
                    InstallPhase::Failed
                } else {
                    InstallPhase::Ready
                };
                record.snapshot.error = error;
                // Return the final clock while this record is still locked; a new
                // operation may evict completed history immediately after release.
                record.snapshot.clone()
            });
        self.finished = true;
        terminal
    }
}

impl Drop for InstallLease {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish(Some("ARK extension installation was interrupted; inspect the installation before retrying.".into()));
        }
    }
}

fn lock_error() -> String {
    "ARK extension installation progress lock was poisoned.".into()
}

fn validate_identity(instance_id: &str, operation_id: &str) -> Result<(), String> {
    if instance_id.is_empty()
        || instance_id.len() > 256
        || instance_id.chars().any(char::is_control)
        || operation_id.is_empty()
        || operation_id.len() > 128
        || !operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("Invalid ARK extension installation identity.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn download(bytes: u64) -> InstallUpdate {
        InstallUpdate {
            phase: InstallPhase::Downloading,
            source: Some("gitcode.com".into()),
            downloaded_bytes: Some(bytes),
            total_bytes: Some(1024),
        }
    }

    #[test]
    fn identity_isolation_and_terminal_outcomes_reject_late_updates() -> Result<(), String> {
        let tracker = Arc::new(InstallTracker::default());
        let mut first = tracker.begin("ark-a".into(), "first".into())?;
        let report = first.reporter();
        report(download(512));
        assert!(tracker.begin("ark-a".into(), "second".into()).is_err());
        assert!(tracker.snapshot("ark-b", Some("first"))?.is_none());
        let _ = first.finish(Some("connection reset by peer".into()));
        report(download(1024));
        let failed = tracker
            .snapshot("ark-a", Some("first"))?
            .ok_or("missing failed result")?;
        assert!(!failed.active);
        assert_eq!(failed.update.phase, InstallPhase::Failed);
        assert_eq!(failed.update.downloaded_bytes, Some(512));
        assert_eq!(failed.error.as_deref(), Some("connection reset by peer"));
        assert!(tracker.begin("ark-a".into(), "first".into()).is_err());
        let mut next = tracker.begin("ark-a".into(), "second".into())?;
        report(download(1024));
        let _ = next.finish(None);
        let ready = tracker
            .snapshot("ark-a", None)?
            .ok_or("missing latest result")?;
        assert_eq!(ready.operation_id, "second");
        assert_eq!(ready.update.phase, InstallPhase::Ready);
        assert!(ready.error.is_none());
        Ok(())
    }

    #[test]
    fn dropped_operation_becomes_failed_and_completed_history_is_bounded() -> Result<(), String> {
        let tracker = Arc::new(InstallTracker::default());
        drop(tracker.begin("ark".into(), "interrupted".into())?);
        let failed = tracker
            .snapshot("ark", None)?
            .ok_or("missing interrupted result")?;
        assert!(!failed.active && failed.error.is_some());
        for index in 0..MAX_RECORDS {
            let _ = tracker
                .begin("ark".into(), format!("attempt-{index}"))?
                .finish(None);
        }
        assert!(tracker.snapshot("ark", Some("interrupted"))?.is_none());
        assert_eq!(
            tracker.records.lock().map_err(|_| lock_error())?.len(),
            MAX_RECORDS
        );
        Ok(())
    }

    #[test]
    fn tracker_does_not_evict_active_installations() -> Result<(), String> {
        let tracker = Arc::new(InstallTracker::default());
        let leases: Vec<_> = (0..MAX_RECORDS)
            .map(|index| tracker.begin(format!("ark-{index}"), format!("run-{index}")))
            .collect::<Result<_, _>>()?;
        assert!(tracker.begin("overflow".into(), "overflow".into()).is_err());
        assert!(
            tracker
                .snapshot("ark-0", Some("run-0"))?
                .is_some_and(|snapshot| snapshot.active)
        );
        drop(leases);
        Ok(())
    }

    #[test]
    fn removal_forgets_completed_history_without_erasing_a_queued_install() -> Result<(), String> {
        let tracker = Arc::new(InstallTracker::default());
        let _ = tracker
            .begin("ark".into(), "installed".into())?
            .finish(None);
        drop(tracker.begin("ark".into(), "failed".into())?);
        let _ = tracker
            .begin("other".into(), "other-installed".into())?
            .finish(None);
        let mut queued = tracker.begin("ark".into(), "queued".into())?;

        tracker.forget_completed("ark");

        assert!(tracker.snapshot("ark", Some("installed"))?.is_none());
        assert!(tracker.snapshot("ark", Some("failed"))?.is_none());
        assert!(tracker.snapshot("other", None)?.is_some());
        assert!(
            tracker
                .snapshot("ark", None)?
                .is_some_and(|snapshot| { snapshot.active && snapshot.operation_id == "queued" })
        );
        assert!(queued.finish(None).is_some());
        tracker.forget_completed("ark");
        assert!(tracker.snapshot("ark", None)?.is_none());
        Ok(())
    }

    #[test]
    fn completion_receipt_keeps_the_native_final_elapsed_time() -> Result<(), String> {
        let tracker = Arc::new(InstallTracker::default());
        let mut lease = tracker.begin("ark".into(), "timed-install".into())?;
        tracker.records.lock().map_err(|_| lock_error())?[0].started_at =
            Instant::now() - std::time::Duration::from_secs(8);
        let terminal = lease.finish(None).ok_or("missing final receipt")?;
        assert!(!terminal.active);
        assert_eq!(terminal.elapsed_seconds, 8);
        assert_eq!(terminal.update.phase, InstallPhase::Ready);
        tracker.records.lock().map_err(|_| lock_error())?[0].started_at =
            Instant::now() - std::time::Duration::from_secs(90);
        assert_eq!(
            tracker
                .snapshot("ark", None)?
                .ok_or("missing record")?
                .elapsed_seconds,
            8
        );
        assert_eq!(
            lease
                .finish(None)
                .ok_or("missing repeated receipt")?
                .elapsed_seconds,
            8
        );
        Ok(())
    }
}
