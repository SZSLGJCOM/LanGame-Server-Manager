//! Rust's Fleck listener rejects a sixth new connection from one IP within
//! five seconds, before the HTTP upgrade. Successful authentication and closing
//! a connection do not reset that counter (Rust build 26498655).
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const ATTEMPTS_PER_WINDOW: usize = 5;
const ATTEMPT_WINDOW: Duration = Duration::from_secs(5);
// Native expiry is strictly `UtcNow - WindowStart > 5 seconds`. Leave room for
// that strict boundary without allowing an unbounded queue behind other calls.
// This is additional to the unchanged eight-second wire deadline (three-second
// idle I/O limit): queue plus wire work is bounded to 13.1 seconds.
const ADMISSION_BUDGET: Duration = Duration::from_millis(5100);
const BOUNDARY_RECHECK: Duration = Duration::from_millis(1);

static ADMISSION: OnceLock<ConnectionAdmission> = OnceLock::new();

pub(super) fn admit(endpoint: &str) -> Result<ConnectionAttempt<'static>, String> {
    let endpoint = endpoint
        .parse::<SocketAddr>()
        .map_err(|_| String::from("WebSocket RCON endpoint must be an IP address and port"))?;
    ADMISSION
        .get_or_init(ConnectionAdmission::default)
        .acquire(endpoint, ADMISSION_BUDGET)
}

#[derive(Default)]
struct ConnectionAdmission {
    endpoints: Mutex<HashMap<SocketAddr, EndpointAttempts>>,
    changed: Condvar,
}

impl ConnectionAdmission {
    fn acquire(
        &self,
        endpoint: SocketAddr,
        budget: Duration,
    ) -> Result<ConnectionAttempt<'_>, String> {
        let deadline = Instant::now() + budget;
        let mut endpoints = self.endpoints.lock().map_err(|_| admission_poisoned())?;
        loop {
            let now = AttemptTime::now();
            let remaining = deadline
                .checked_duration_since(now.monotonic)
                .filter(|remaining| !remaining.is_zero())
                .ok_or_else(|| {
                    String::from(
                        "WebSocket RCON connection admission timed out before connecting; no command was sent",
                    )
                })?;
            endpoints.retain(|_, attempts| {
                attempts.release_expired(now);
                attempts
                    .slots
                    .iter()
                    .any(|slot| !matches!(slot, Slot::Vacant))
            });
            let attempts = endpoints.entry(endpoint).or_default();
            if let Some(slot) = attempts
                .slots
                .iter()
                .position(|slot| matches!(slot, Slot::Vacant))
            {
                attempts.slots[slot] = Slot::InFlight;
                return Ok(ConnectionAttempt {
                    admission: self,
                    endpoint,
                    slot,
                });
            }
            let wait = attempts
                .slots
                .iter()
                .filter_map(|slot| match slot {
                    Slot::Finished(at) => at.remaining(now),
                    _ => None,
                })
                .min()
                .unwrap_or(remaining)
                .min(remaining);
            // Never hold the registry lock during a wait or network operation;
            // another endpoint has its own native listener and allowance.
            (endpoints, _) = self
                .changed
                .wait_timeout(endpoints, wait)
                .map_err(|_| admission_poisoned())?;
        }
    }
}

pub(super) struct ConnectionAttempt<'a> {
    admission: &'a ConnectionAdmission,
    endpoint: SocketAddr,
    slot: usize,
}

impl Drop for ConnectionAttempt<'_> {
    fn drop(&mut self) {
        let mut endpoints = self
            .admission
            .endpoints
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let attempts = endpoints
            .get_mut(&self.endpoint)
            .expect("an in-flight WebSocket connection keeps its endpoint entry");
        // Count failures too: a failed upgrade may already have consumed the
        // server's allowance. Completion is later than native TCP acceptance,
        // so slow connections cannot release their slots prematurely. Keeping
        // the guard through stream closure also bounds active connections.
        attempts.slots[self.slot] = Slot::Finished(AttemptTime::now());
        self.admission.changed.notify_all();
    }
}

struct EndpointAttempts {
    slots: [Slot; ATTEMPTS_PER_WINDOW],
}

impl Default for EndpointAttempts {
    fn default() -> Self {
        Self {
            slots: [Slot::Vacant; ATTEMPTS_PER_WINDOW],
        }
    }
}

impl EndpointAttempts {
    fn release_expired(&mut self, now: AttemptTime) {
        for slot in &mut self.slots {
            if matches!(slot, Slot::Finished(at) if at.remaining(now).is_none()) {
                *slot = Slot::Vacant;
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Slot {
    Vacant,
    InFlight,
    Finished(AttemptTime),
}

#[derive(Clone, Copy)]
struct AttemptTime {
    monotonic: Instant,
    utc: SystemTime,
}

impl AttemptTime {
    fn now() -> Self {
        Self {
            monotonic: Instant::now(),
            utc: SystemTime::now(),
        }
    }

    fn remaining(self, now: Self) -> Option<Duration> {
        let elapsed = now
            .monotonic
            .saturating_duration_since(self.monotonic)
            .min(now.utc.duration_since(self.utc).unwrap_or_default());
        // Native uses DateTime.UtcNow; a backward wall-clock adjustment must not
        // free slots early. The monotonic clock bounds the queue and prevents a
        // forward adjustment from shortening the five-second interval.
        (elapsed <= ATTEMPT_WINDOW)
            .then(|| ATTEMPT_WINDOW.saturating_sub(elapsed) + BOUNDARY_RECHECK)
    }
}

fn admission_poisoned() -> String {
    String::from("WebSocket RCON connection admission is unavailable; no command was sent")
}

#[cfg(test)]
#[path = "runtime_transport_websocket_admission_tests.rs"]
mod tests;
