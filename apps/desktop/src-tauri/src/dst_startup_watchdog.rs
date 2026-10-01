use std::time::{Duration, Instant};

pub(super) const IDLE_LIMIT: Duration = Duration::from_secs(300);
pub(super) const TOTAL_LIMIT: Duration = Duration::from_secs(900);

pub(super) struct StartupWatchdog {
    started: Instant,
    last_progress: Instant,
}

impl StartupWatchdog {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            started: now,
            last_progress: now,
        }
    }

    pub(super) fn observe_progress(&mut self, now: Instant) {
        self.last_progress = self.last_progress.max(now);
    }

    pub(super) fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_progress) >= IDLE_LIMIT
            || now.saturating_duration_since(self.started) >= TOTAL_LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_generation_progress_extends_idle_deadline_but_never_total_budget() {
        let start = Instant::now();
        let mut watchdog = StartupWatchdog::new(start);
        assert!(!watchdog.expired(start + Duration::from_secs(299)));
        watchdog.observe_progress(start + Duration::from_secs(290));
        assert!(!watchdog.expired(start + Duration::from_secs(301)));
        assert!(watchdog.expired(start + Duration::from_secs(590)));
        watchdog.observe_progress(start + Duration::from_secs(800));
        assert!(watchdog.expired(start + TOTAL_LIMIT));
    }

    #[test]
    fn no_progress_still_times_out_after_five_minutes() {
        assert!(StartupWatchdog::new(Instant::now()).expired(Instant::now() + IDLE_LIMIT));
    }
}
