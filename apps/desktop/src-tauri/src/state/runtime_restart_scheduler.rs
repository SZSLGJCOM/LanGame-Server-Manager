use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use app_core::RuntimePendingRestartSnapshot;

#[derive(Debug, Clone)]
pub struct RuntimeRestartScheduleRequest {
    pub instance_id: String,
    pub instance_name: String,
    pub backoff: Duration,
    pub recent_crash_count: usize,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct RuntimeRestartScheduleEntry {
    pub instance_id: String,
    pub instance_name: String,
    pub scheduled_at: Instant,
    pub due_at: Instant,
    pub recent_crash_count: usize,
    pub exit_code: Option<i32>,
    generation: u64,
}

#[derive(Debug)]
struct RuntimeRestartFlight {
    generation: u64,
    valid: bool,
    shutdown_run_ids: HashSet<i64>,
}

#[derive(Debug, Default)]
pub struct RuntimeRestartScheduler {
    pending: HashMap<String, RuntimeRestartScheduleEntry>,
    in_flight: HashMap<String, RuntimeRestartFlight>,
    stop_requested: HashSet<String>,
    generation: u64,
}

impl RuntimeRestartScheduler {
    pub fn schedule(
        &mut self,
        request: RuntimeRestartScheduleRequest,
    ) -> Option<RuntimeRestartScheduleEntry> {
        self.schedule_at(request, Instant::now())
    }

    pub(crate) fn schedule_at(
        &mut self,
        request: RuntimeRestartScheduleRequest,
        now: Instant,
    ) -> Option<RuntimeRestartScheduleEntry> {
        if !self.accepts_exit(&request.instance_id)
            || self.pending.contains_key(&request.instance_id)
        {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        let scheduled_at = now;
        let due_at = scheduled_at + request.backoff;
        let entry = RuntimeRestartScheduleEntry {
            instance_id: request.instance_id,
            instance_name: request.instance_name,
            scheduled_at,
            due_at,
            recent_crash_count: request.recent_crash_count,
            exit_code: request.exit_code,
            generation: self.generation,
        };
        self.pending
            .insert(entry.instance_id.clone(), entry.clone());
        Some(entry)
    }

    pub fn request_stop(&mut self, instance_id: &str) {
        self.stop_requested.insert(instance_id.to_owned());
        self.invalidate(instance_id);
    }

    pub fn reset_for_manual_start(&mut self, instance_id: &str) {
        self.stop_requested.remove(instance_id);
        self.invalidate(instance_id);
    }

    fn invalidate(&mut self, instance_id: &str) {
        self.pending.remove(instance_id);
        // Retain the old flight until its owner leaves. A replacement ticket must
        // not authorize the old task between releasing and reacquiring the lock.
        if let Some(flight) = self.in_flight.get_mut(instance_id) {
            flight.valid = false;
        }
    }

    pub fn accepts_exit(&self, instance_id: &str) -> bool {
        !self.stop_requested.contains(instance_id)
    }

    pub fn exit_is_expected(&self, instance_id: &str, run_id: i64) -> bool {
        self.stop_requested.contains(instance_id)
            || self
                .in_flight
                .get(instance_id)
                .is_some_and(|flight| flight.shutdown_run_ids.contains(&run_id))
    }

    pub fn expect_survivor_shutdown(
        &mut self,
        entry: &RuntimeRestartScheduleEntry,
        run_ids: impl IntoIterator<Item = i64>,
    ) -> bool {
        if !self.entry_is_current(entry) {
            return false;
        }
        if let Some(flight) = self.in_flight.get_mut(&entry.instance_id) {
            flight.shutdown_run_ids.extend(run_ids);
        }
        true
    }

    pub fn restart_allowed(&self, instance_id: &str) -> bool {
        !self.stop_requested.contains(instance_id)
            && self
                .in_flight
                .get(instance_id)
                .is_some_and(|flight| flight.valid)
    }

    pub fn entry_is_current(&self, entry: &RuntimeRestartScheduleEntry) -> bool {
        self.restart_allowed(&entry.instance_id)
            && self
                .in_flight
                .get(&entry.instance_id)
                .is_some_and(|flight| flight.generation == entry.generation)
    }

    pub fn finish_restart(&mut self, entry: &RuntimeRestartScheduleEntry) {
        if self
            .in_flight
            .get(&entry.instance_id)
            .is_some_and(|flight| flight.generation == entry.generation)
        {
            self.in_flight.remove(&entry.instance_id);
        }
    }

    pub fn cancel(&mut self, instance_id: &str) -> Option<RuntimeRestartScheduleEntry> {
        self.pending.remove(instance_id)
    }

    pub fn take_due(&mut self) -> Vec<RuntimeRestartScheduleEntry> {
        self.take_due_at(Instant::now())
    }

    pub(crate) fn take_due_at(&mut self, now: Instant) -> Vec<RuntimeRestartScheduleEntry> {
        let due_instance_ids = self
            .pending
            .iter()
            .filter_map(|(instance_id, entry)| {
                if entry.due_at <= now && !self.in_flight.contains_key(instance_id) {
                    Some(instance_id.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        let mut due = Vec::new();
        for instance_id in due_instance_ids {
            if let Some(entry) = self.pending.remove(&instance_id) {
                self.in_flight.insert(
                    instance_id,
                    RuntimeRestartFlight {
                        generation: entry.generation,
                        valid: true,
                        shutdown_run_ids: HashSet::new(),
                    },
                );
                due.push(entry);
            }
        }
        due
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn next_restart_delay(&self) -> Duration {
        self.next_restart_delay_at(Instant::now())
    }

    fn next_restart_delay_at(&self, now: Instant) -> Duration {
        self.pending
            .values()
            .map(|entry| entry.due_at.saturating_duration_since(now))
            .min()
            .unwrap_or(Duration::ZERO)
    }

    pub fn next_restart(&self) -> Option<RuntimePendingRestartSnapshot> {
        self.next_restart_at(Instant::now())
    }

    fn next_restart_at(&self, now: Instant) -> Option<RuntimePendingRestartSnapshot> {
        self.pending
            .values()
            .min_by_key(|entry| entry.due_at)
            .map(|entry| pending_restart_snapshot(entry, now))
    }

    pub fn pending_restart_for(&self, instance_id: &str) -> Option<RuntimePendingRestartSnapshot> {
        self.pending_restart_for_at(instance_id, Instant::now())
    }

    fn pending_restart_for_at(
        &self,
        instance_id: &str,
        now: Instant,
    ) -> Option<RuntimePendingRestartSnapshot> {
        self.pending
            .get(instance_id)
            .map(|entry| pending_restart_snapshot(entry, now))
    }
}

fn pending_restart_snapshot(
    entry: &RuntimeRestartScheduleEntry,
    now: Instant,
) -> RuntimePendingRestartSnapshot {
    RuntimePendingRestartSnapshot {
        instance_id: entry.instance_id.clone(),
        instance_name: entry.instance_name.clone(),
        delay_ms: u64::try_from(entry.due_at.saturating_duration_since(now).as_millis())
            .unwrap_or(u64::MAX),
        recent_crash_count: entry.recent_crash_count,
        exit_code: entry.exit_code,
    }
}

#[cfg(test)]
#[path = "runtime_restart_scheduler_tests.rs"]
mod tests;
