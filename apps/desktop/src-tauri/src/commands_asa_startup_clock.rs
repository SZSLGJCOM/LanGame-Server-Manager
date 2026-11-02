//! Process-owned ASA initialization budgets; observations cannot renew a phase.
use super::MOD_VALIDATION_TIMEOUT;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct PhaseClock {
    pub(super) mod_deadline: Duration,
    pub(super) hard_deadline: Duration,
    pub(super) binding_deadline: Duration,
    pub(super) mods_required: bool,
    pub(super) mods_validated: bool,
}

impl PhaseClock {
    pub(super) fn new(normal: Duration, process_age: Duration, now: Duration) -> Self {
        Self {
            mod_deadline: now + MOD_VALIDATION_TIMEOUT.saturating_sub(process_age),
            hard_deadline: now + (MOD_VALIDATION_TIMEOUT + normal).saturating_sub(process_age),
            binding_deadline: now + normal,
            mods_required: false,
            mods_validated: false,
        }
    }

    pub(super) fn require_mods(&mut self) {
        self.mods_required = true;
    }

    /// Listener observations are admissible only after required Mod validation.
    pub(super) fn mods_ready(&self) -> bool {
        !self.mods_required || self.mods_validated
    }

    pub(super) fn check(&self, now: Duration) -> Result<(), String> {
        if now < self.deadline() {
            return Ok(());
        }
        Err(self.timeout_message())
    }

    pub(super) fn observe_mods(
        &mut self,
        complete: bool,
        now: Duration,
        normal: Duration,
    ) -> Result<(), String> {
        // A completion arriving at or after expiry must never reopen startup.
        self.check(now)?;
        if self.mods_required && complete && !self.mods_validated {
            self.mods_validated = true;
            self.binding_deadline = (now + normal).min(self.hard_deadline);
        }
        Ok(())
    }

    pub(super) fn deadline(&self) -> Duration {
        if !self.mods_ready() {
            self.mod_deadline
        } else {
            self.binding_deadline
        }
    }

    pub(super) fn timeout_message(&self) -> String {
        if !self.mods_ready() {
            format!(
                "ASA CurseForge Mod validation did not complete within {} ms; inspect this startup's native log for API, network or file-download failures.",
                MOD_VALIDATION_TIMEOUT.as_millis()
            )
        } else {
            "owned game listener was not verified before the normal startup deadline".into()
        }
    }
}

#[cfg(test)]
#[path = "commands_asa_startup_clock_tests.rs"]
mod tests;
