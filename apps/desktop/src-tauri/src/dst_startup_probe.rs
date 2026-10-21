use std::io::{self, ErrorKind};
use std::time::{Duration, Instant};

use super::protocol;

const PROBE_INTERVAL: Duration = Duration::from_secs(2);

/// Readiness probing for one shard. At most one probe waits in the shard's
/// stdin: the next is written only after the Lua VM acknowledges the previous
/// one, so a shard still loading mods never accumulates unread commands.
pub(super) struct ProbeSchedule {
    ready: bool,
    awaiting_ack: bool,
    next_write: Instant,
}

impl ProbeSchedule {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            ready: false,
            awaiting_ack: false,
            next_write: now,
        }
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    pub(super) fn may_write(&self, now: Instant) -> bool {
        !self.ready && !self.awaiting_ack && now >= self.next_write
    }

    pub(super) fn write_started(&mut self, now: Instant) {
        self.next_write = now + PROBE_INTERVAL;
    }

    /// A full pipe that accepted nothing leaves the stream intact: the shard has
    /// not started reading its console yet, so the probe is retried later. A
    /// broken or partially written stream is a real failure.
    pub(super) fn write_finished(
        &mut self,
        result: io::Result<()>,
        writer_healthy: bool,
    ) -> Result<(), String> {
        match result {
            Ok(()) => {
                self.awaiting_ack = true;
                Ok(())
            }
            Err(error)
                if writer_healthy
                    && matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                Ok(())
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(super) fn observe(&mut self, line: &str, nonce: &str) {
        if protocol::is_ack(line, "PROBE", nonce) {
            self.awaiting_ack = false;
        }
        self.ready |= protocol::is_ack(line, "READY", nonce);
    }
}
