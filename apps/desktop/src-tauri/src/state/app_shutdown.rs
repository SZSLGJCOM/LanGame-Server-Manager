use super::{DesktopState, Ordering};

impl DesktopState {
    /// Final intent upgrades the current shutdown owner without starting a
    /// second save/stop operation. Admission and retry share this same lock.
    pub(crate) fn begin_app_shutdown(&self, final_exit: bool) -> Result<bool, String> {
        let _phase = self
            .app_shutdown
            .lock()
            .map_err(|_| String::from("application shutdown phase lock poisoned"))?;
        if final_exit {
            self.request_final_exit();
        }
        Ok(self
            .shutdown_in_progress
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok())
    }

    /// Ordinary failures may reopen admission; accepted final intent stays sealed.
    pub(crate) fn release_app_shutdown_for_retry(&self) -> Result<bool, String> {
        let _phase = self
            .app_shutdown
            .lock()
            .map_err(|_| String::from("application shutdown phase lock poisoned"))?;
        if self.is_final_exit_requested() {
            return Ok(false);
        }
        self.shutdown_in_progress.store(false, Ordering::SeqCst);
        Ok(true)
    }
}

#[cfg(test)]
#[path = "app_shutdown_tests.rs"]
mod tests;
