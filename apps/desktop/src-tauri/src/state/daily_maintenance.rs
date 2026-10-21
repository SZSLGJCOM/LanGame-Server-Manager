use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct CoordinatorState {
    active: bool,
    next_scan: Instant,
    last_tick: Option<Instant>,
    date: String,
    pending_minutes: BTreeSet<String>,
    generation: u64,
}

/// One worker serializes due backups and restarts without blocking the heartbeat.
pub(crate) struct DailyMaintenanceCoordinator {
    state: Arc<Mutex<CoordinatorState>>,
    owner_session: String,
}

impl Default for DailyMaintenanceCoordinator {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinatorState {
                active: false,
                next_scan: Instant::now(),
                last_tick: None,
                date: String::new(),
                pending_minutes: BTreeSet::new(),
                generation: 0,
            })),
            owner_session: uuid::Uuid::new_v4().to_string(),
        }
    }
}

pub(crate) struct DailyMaintenanceWorker {
    state: Arc<Mutex<CoordinatorState>>,
    pub(crate) owner_session: String,
    pub(crate) local_date: String,
    pub(crate) due_minutes: Vec<String>,
    generation: u64,
}

impl DailyMaintenanceCoordinator {
    pub(crate) fn try_start(
        &self,
        date: &str,
        time: &str,
    ) -> Result<Option<DailyMaintenanceWorker>, String> {
        self.try_start_at(date, time, Instant::now())
    }

    fn try_start_at(
        &self,
        date: &str,
        time: &str,
        now: Instant,
    ) -> Result<Option<DailyMaintenanceWorker>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "daily maintenance coordinator lock poisoned")?;
        // Tick observation continues while the worker is busy. A suspension or
        // date change discards queued windows instead of replaying an offline day.
        if state.date != date
            || state
                .last_tick
                .is_some_and(|tick| now.saturating_duration_since(tick) > Duration::from_secs(90))
        {
            state.pending_minutes.clear();
            state.generation = state.generation.wrapping_add(1);
        }
        state.date = date.to_owned();
        state.last_tick = Some(now);
        state.pending_minutes.insert(time.to_owned());
        if state.active || now < state.next_scan {
            return Ok(None);
        }
        state.active = true;
        state.next_scan = now + Duration::from_secs(15);
        let due_minutes = std::mem::take(&mut state.pending_minutes)
            .into_iter()
            .collect();
        Ok(Some(DailyMaintenanceWorker {
            state: Arc::clone(&self.state),
            owner_session: self.owner_session.clone(),
            local_date: date.to_owned(),
            due_minutes,
            generation: state.generation,
        }))
    }
}

impl DailyMaintenanceWorker {
    pub(crate) fn window_is_current(&self) -> Result<bool, String> {
        self.window_is_current_at(Instant::now())
    }

    fn window_is_current_at(&self, now: Instant) -> Result<bool, String> {
        let state = self
            .state
            .lock()
            .map_err(|_| "daily maintenance coordinator lock poisoned")?;
        Ok(state.generation == self.generation
            && state
                .last_tick
                .is_some_and(|tick| now.saturating_duration_since(tick) <= Duration::from_secs(90)))
    }
}

impl Drop for DailyMaintenanceWorker {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_worker_is_single_flight_and_released_on_drop() {
        let coordinator = DailyMaintenanceCoordinator::default();
        let first = coordinator
            .try_start("2026-10-18", "04:00")
            .unwrap()
            .unwrap();
        assert!(
            coordinator
                .try_start("2026-10-18", "04:00")
                .unwrap()
                .is_none()
        );
        drop(first);
        coordinator.state.lock().unwrap().next_scan = Instant::now();
        assert!(
            coordinator
                .try_start("2026-10-18", "04:00")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn busy_work_retains_observed_minutes_but_sleep_and_new_day_do_not_replay_them() {
        let coordinator = DailyMaintenanceCoordinator::default();
        let now = Instant::now();
        let first = coordinator
            .try_start_at("2026-10-18", "04:00", now)
            .unwrap()
            .unwrap();
        assert!(
            coordinator
                .try_start_at("2026-10-18", "04:01", now + Duration::from_secs(60))
                .unwrap()
                .is_none()
        );
        assert!(
            coordinator
                .try_start_at("2026-10-18", "04:02", now + Duration::from_secs(120))
                .unwrap()
                .is_none()
        );
        drop(first);
        let next = coordinator
            .try_start_at("2026-10-18", "04:02", now + Duration::from_secs(121))
            .unwrap()
            .unwrap();
        assert_eq!(next.due_minutes, ["04:01", "04:02"]);
        assert!(
            coordinator
                .try_start_at("2026-10-19", "10:00", now + Duration::from_secs(86_400))
                .unwrap()
                .is_none()
        );
        assert!(!next.window_is_current().unwrap());
        drop(next);
        let awake = coordinator
            .try_start_at("2026-10-19", "10:00", now + Duration::from_secs(86_401))
            .unwrap()
            .unwrap();
        assert_eq!(awake.due_minutes, ["10:00"]);
    }

    #[test]
    fn same_day_sleep_discards_unexecuted_minutes() {
        let coordinator = DailyMaintenanceCoordinator::default();
        let now = Instant::now();
        let first = coordinator
            .try_start_at("2026-10-18", "04:00", now)
            .unwrap()
            .unwrap();
        coordinator
            .try_start_at("2026-10-18", "04:01", now + Duration::from_secs(60))
            .unwrap();
        assert!(
            !first
                .window_is_current_at(now + Duration::from_secs(20_000))
                .unwrap(),
            "A resumed worker must not outrun the heartbeat's suspension detection"
        );
        coordinator
            .try_start_at("2026-10-18", "10:00", now + Duration::from_secs(20_000))
            .unwrap();
        assert!(!first.window_is_current().unwrap());
        drop(first);
        let next = coordinator
            .try_start_at("2026-10-18", "10:00", now + Duration::from_secs(20_001))
            .unwrap()
            .unwrap();
        assert_eq!(next.due_minutes, ["10:00"]);
    }
}
