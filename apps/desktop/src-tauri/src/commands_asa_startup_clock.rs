//! Process-owned ASA initialization budgets; observations cannot renew a phase.
use super::COLD_CACHE_TIMEOUT;
use super::evidence::Observation;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct PhaseClock {
    pub(super) cold_deadline: Duration,
    pub(super) hard_deadline: Duration,
    pub(super) binding_deadline: Duration,
    pub(super) cold_seen: bool,
    pub(super) initialized: bool,
    pub(super) mods_required: bool,
    pub(super) mods_validated: bool,
}

impl PhaseClock {
    pub(super) fn new(normal: Duration, process_age: Duration, now: Duration) -> Self {
        Self {
            cold_deadline: now + COLD_CACHE_TIMEOUT.saturating_sub(process_age),
            hard_deadline: now + (COLD_CACHE_TIMEOUT + normal).saturating_sub(process_age),
            binding_deadline: now + normal,
            cold_seen: false,
            initialized: false,
            mods_required: false,
            mods_validated: false,
        }
    }

    pub(super) fn require_mods(&mut self) {
        self.mods_required = true;
    }

    /// Listener observations are admissible only after required Mod validation.
    /// Offset-cache initialization is a separate operation and cannot satisfy it.
    pub(super) fn mods_ready(&self) -> bool {
        !self.mods_required || self.mods_validated
    }

    pub(super) fn check(&self, now: Duration) -> Result<(), String> {
        if now < self.deadline() {
            return Ok(());
        }
        Err(self.timeout_message())
    }

    pub(super) fn observe(
        &mut self,
        observation: &Observation,
        now: Duration,
        normal: Duration,
    ) -> Result<(), String> {
        // Check before a completion record can change the active deadline.
        self.check(now)?;
        if self.initialized {
            return Ok(());
        }
        if observation.cold_download && now >= self.cold_deadline {
            return Err(format!(
                "ASA API offset initialization exceeded its {} ms phase budget",
                COLD_CACHE_TIMEOUT.as_millis()
            ));
        }
        self.cold_seen |= observation.cold_download;
        if observation.initialized {
            self.initialized = true;
            if self.cold_seen {
                self.binding_deadline = (now + normal).min(self.hard_deadline);
            }
        }
        Ok(())
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
        if !self.mods_ready() || (self.cold_seen && !self.initialized) {
            self.cold_deadline
        } else {
            self.binding_deadline
        }
    }

    pub(super) fn timeout_message(&self) -> String {
        if !self.mods_ready() {
            format!(
                "ASA CurseForge Mod validation did not complete within {} ms; inspect this startup's native log for API, network or file-download failures.",
                COLD_CACHE_TIMEOUT.as_millis()
            )
        } else if self.cold_seen && !self.initialized {
            format!(
                "ASA API offset cache initialization did not complete within {} ms",
                COLD_CACHE_TIMEOUT.as_millis()
            )
        } else {
            "owned game listener was not verified before the normal startup deadline".into()
        }
    }
}

#[cfg(test)]
#[path = "commands_asa_startup_clock_tests.rs"]
mod tests;
