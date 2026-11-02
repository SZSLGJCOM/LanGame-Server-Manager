//! Cancellation belongs to one startup attempt, including its preparatory workers.
use super::{DesktopState, RuntimeStartReservationLease, RuntimeStartReservations};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_CANCELLED_START_ATTEMPTS: usize = 1024;
const MAX_ATTEMPT_ID_BYTES: usize = 128;

#[derive(Debug)]
pub(crate) struct RuntimeStartControl {
    cancellation: Arc<AtomicBool>,
    requested: AtomicBool,
    ready: AtomicBool,
    settled: AtomicBool,
    cleanup_failure: Mutex<Option<String>>,
}

impl RuntimeStartControl {
    pub(super) fn new(cancellation: Arc<AtomicBool>) -> Self {
        Self {
            cancellation,
            requested: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            settled: AtomicBool::new(false),
            cleanup_failure: Mutex::new(None),
        }
    }

    pub(super) fn mark_settled(&self) {
        self.settled.store(true, Ordering::SeqCst);
    }

    pub(crate) fn check_cleanup_result(&self) -> Result<(), String> {
        let failure = self
            .cleanup_failure
            .lock()
            .map_err(|_| "Startup cleanup result lock poisoned; complete cleanup is unconfirmed")?;
        match failure.as_deref() {
            Some(message) => Err(format!(
                "Startup cleanup was not confirmed; the instance has not been reported stopped: {message}"
            )),
            None => Ok(()),
        }
    }

    fn request_cancellation(&self) {
        self.requested.store(true, Ordering::SeqCst);
        self.cancellation.store(true, Ordering::SeqCst);
    }

    pub(crate) async fn wait_until_settled(&self, timeout: Duration) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + timeout;
        while !self.settled.load(Ordering::SeqCst) {
            if tokio::time::Instant::now() >= deadline {
                return Err("Startup cancellation is still cleaning up owned resources; the instance has not been reported stopped.".into());
            }
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_millis(25)).min(deadline),
            )
            .await;
        }
        self.check_cleanup_result()
    }
}

pub(super) fn validate_attempt_id(attempt_id: &str) -> Result<(), String> {
    if attempt_id.is_empty() || attempt_id.len() > MAX_ATTEMPT_ID_BYTES {
        return Err(format!(
            "Runtime start attempt ID must contain 1 to {MAX_ATTEMPT_ID_BYTES} UTF-8 bytes"
        ));
    }
    Ok(())
}

impl RuntimeStartReservations {
    pub(super) fn ensure_attempt_not_cancelled(
        &self,
        instance_id: &str,
        attempt_id: &str,
    ) -> Result<(), String> {
        if self
            .cancelled_before_acceptance
            .contains(&(instance_id.to_owned(), attempt_id.to_owned()))
            || self.last_accepted.get(instance_id).is_some_and(|accepted| {
                accepted.attempt_id.as_deref() == Some(attempt_id)
                    && accepted.control.requested.load(Ordering::SeqCst)
            })
        {
            return Err(serde_json::json!({
                "code": "runtime_start_cancelled",
                "instance_id": instance_id,
                "message": "Server startup was cancelled.",
            })
            .to_string());
        }
        // A repeated delivery is the same operation, not permission to launch
        // another process after its first execution has already settled.
        if !self.pending.contains_key(instance_id)
            && self
                .last_accepted
                .get(instance_id)
                .is_some_and(|accepted| accepted.attempt_id.as_deref() == Some(attempt_id))
        {
            return Err("This startup attempt was already accepted; inspect its runtime state instead of submitting it again.".into());
        }
        Ok(())
    }
}

impl DesktopState {
    #[cfg(test)]
    pub(crate) fn request_runtime_start_cancellation(
        &self,
        instance_id: &str,
    ) -> Result<Option<Arc<RuntimeStartControl>>, String> {
        self.request_runtime_start_cancellation_for_attempt(instance_id, None)
    }

    pub(crate) fn request_runtime_start_cancellation_for_attempt(
        &self,
        instance_id: &str,
        attempt_id: Option<&str>,
    ) -> Result<Option<Arc<RuntimeStartControl>>, String> {
        if let Some(attempt_id) = attempt_id {
            validate_attempt_id(attempt_id)?;
        }
        let mut reservations = self
            .runtime_start_reservations
            .lock()
            .map_err(|_| "runtime start reservation lock poisoned")?;
        let control = if let Some(attempt_id) = attempt_id {
            match reservations.last_accepted.get(instance_id) {
                Some(accepted) if accepted.attempt_id.as_deref() == Some(attempt_id) => {
                    Arc::clone(&accepted.control)
                }
                _ => {
                    let key = (instance_id.to_owned(), attempt_id.to_owned());
                    if !reservations.cancelled_before_acceptance.contains(&key) {
                        if reservations.cancelled_before_acceptance.len()
                            == MAX_CANCELLED_START_ATTEMPTS
                        {
                            return Err("Startup cancellation could not be recorded because the pending-attempt cancellation limit was reached; no startup was reported cancelled.".into());
                        }
                        reservations.cancelled_before_acceptance.insert(key);
                    }
                    // The start may still be connecting to IPC. Retain its ID
                    // without touching a different attempt for this instance.
                    let control =
                        Arc::new(RuntimeStartControl::new(Arc::new(AtomicBool::new(false))));
                    control.request_cancellation();
                    control.mark_settled();
                    return Ok(Some(control));
                }
            }
        } else if let Some(entry) = reservations.pending.get(instance_id) {
            Arc::clone(&entry.control)
        } else {
            return Ok(None);
        };
        // The same lock arbitrates readiness and cancellation. A ready attempt
        // must take the ordinary save/stop path even if the UI is still Starting.
        if control.ready.load(Ordering::SeqCst) {
            return Ok(None);
        }
        control.request_cancellation();
        Ok(Some(control))
    }

    /// Call under the instance mutation lock before stopping a ready attempt.
    /// An untagged start also replaces this identity and invalidates old stops.
    pub(crate) fn validate_runtime_start_attempt(
        &self,
        instance_id: &str,
        attempt_id: &str,
    ) -> Result<(), String> {
        validate_attempt_id(attempt_id)?;
        let reservations = self
            .runtime_start_reservations
            .lock()
            .map_err(|_| "runtime start reservation lock poisoned")?;
        if reservations
            .last_accepted
            .get(instance_id)
            .is_some_and(|accepted| accepted.attempt_id.as_deref() == Some(attempt_id))
        {
            Ok(())
        } else {
            Err("This stop belongs to an earlier startup attempt; the replacement instance was not stopped.".into())
        }
    }
}

impl RuntimeStartReservationLease {
    pub(crate) fn record_cleanup_failure(&self, message: &str) {
        let mut failure = self
            .lease
            .control
            .cleanup_failure
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Preserve the first failure across later best-effort persistence and
        // owner teardown. Dropping the final lease cannot erase this evidence.
        failure.get_or_insert_with(|| message.to_owned());
    }

    pub(crate) fn cancelled_by_user(&self) -> bool {
        self.lease.control.requested.load(Ordering::SeqCst)
    }

    /// Authorizes startup work before a durable run exists. The instance ID
    /// alone cannot authorize a worker from an earlier or already-ready start.
    pub(crate) fn ensure_current_for(&self, instance_id: &str) -> Result<(), String> {
        if self.lease.instance_id != instance_id {
            return Err("Startup reservation belongs to another instance".into());
        }
        let reservations = self
            .lease
            .reservations
            .lock()
            .map_err(|_| "runtime start reservation lock poisoned")?;
        let entry = reservations
            .pending
            .get(instance_id)
            .ok_or("Startup reservation was released")?;
        if !Arc::ptr_eq(&entry.control, &self.lease.control) {
            return Err("Startup reservation identity changed".into());
        }
        if self.is_cancelled() || entry.control.ready.load(Ordering::SeqCst) {
            return Err("Startup reservation is cancelled or already ready".into());
        }
        Ok(())
    }

    pub(crate) fn confirm_ready(&self) -> Result<bool, String> {
        let reservations = self
            .lease
            .reservations
            .lock()
            .map_err(|_| "runtime start reservation lock poisoned")?;
        let entry = reservations
            .pending
            .get(&self.lease.instance_id)
            .ok_or("Startup reservation was released before readiness was committed")?;
        if !Arc::ptr_eq(&entry.control, &self.lease.control) {
            return Err("Startup reservation identity changed".into());
        }
        if self.is_cancelled() {
            return Ok(false);
        }
        entry.control.ready.store(true, Ordering::SeqCst);
        Ok(true)
    }
}
