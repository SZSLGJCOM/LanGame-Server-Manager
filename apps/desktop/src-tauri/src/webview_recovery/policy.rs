use std::time::{Duration, Instant};

const QUIET_WINDOW: Duration = Duration::from_secs(10 * 60);
const MAX_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FailureKind {
    BrowserExited,
    RendererExited,
    RendererUnresponsive,
    Auxiliary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Reload,
    Recreate,
}

impl Action {
    fn merge(self, other: Self) -> Self {
        if self == Self::Recreate || other == Self::Recreate {
            Self::Recreate
        } else {
            Self::Reload
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Ticket(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Attempt {
    pub ticket: Ticket,
    pub action: Action,
    pub due_at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttemptResult {
    Succeeded,
    Failed,
}

#[derive(Debug)]
struct Flight {
    attempt: Attempt,
    valid: bool,
}

/// Owns at most one queued attempt and one executing attempt. Only `begin`
/// spends the budget; a successful dispatch never resets failure history.
#[derive(Debug, Default)]
pub(super) struct Policy {
    pending: Option<Attempt>,
    flight: Option<Flight>,
    attempts: usize,
    last_failure: Option<Instant>,
    generation: u64,
    paused: bool,
    shutdown: bool,
}

impl Policy {
    /// Returns false for auxiliary events or shutdown. A recorded failure can
    /// still leave recovery paused when its automatic budget is exhausted.
    pub fn record_failure(&mut self, kind: FailureKind, now: Instant) -> bool {
        let action = match kind {
            FailureKind::BrowserExited => Action::Recreate,
            FailureKind::RendererExited | FailureKind::RendererUnresponsive => Action::Reload,
            FailureKind::Auxiliary => return false,
        };
        if self.shutdown {
            return false;
        }
        if self
            .last_failure
            .is_some_and(|last| now.saturating_duration_since(last) >= QUIET_WINDOW)
        {
            self.attempts = 0;
            self.paused = false;
        }
        self.last_failure = Some(now);
        self.queue(action, now);
        true
    }

    pub fn next_attempt(&self) -> Option<Attempt> {
        if self.shutdown || self.paused || self.flight.is_some() {
            None
        } else {
            self.pending
        }
    }

    /// Returns the current action, including upgrades since the ticket was
    /// observed. No second worker may begin until the previous one completes.
    pub fn begin(&mut self, ticket: Ticket, now: Instant) -> Option<Action> {
        let attempt = self.next_attempt()?;
        if attempt.ticket != ticket || now < attempt.due_at || self.attempts >= MAX_ATTEMPTS {
            return None;
        }
        self.pending = None;
        self.attempts += 1;
        self.flight = Some(Flight {
            attempt,
            valid: true,
        });
        Some(attempt.action)
    }

    /// A superseded completion releases its old worker slot but returns false
    /// and cannot alter the replacement ticket. Callers should query the next
    /// attempt after every completion, including a false return value.
    pub fn complete(&mut self, ticket: Ticket, result: AttemptResult, now: Instant) -> bool {
        if self.shutdown
            || !self
                .flight
                .as_ref()
                .is_some_and(|flight| flight.attempt.ticket == ticket)
        {
            return false;
        }
        let flight = self.flight.take().expect("matching recovery flight");
        if !flight.valid {
            return false;
        }
        if result == AttemptResult::Failed {
            self.last_failure = Some(now);
            // A reload that failed or never finished cannot establish that its
            // existing WebView is usable. The next bounded attempt rebuilds it.
            self.queue(Action::Recreate, now);
        }
        true
    }

    /// Explicit user retry opens a new budget and requests an immediate full
    /// recreation. Its execution counts as the first attempt in that budget.
    pub fn manual_retry(&mut self, now: Instant) -> bool {
        if self.shutdown {
            return false;
        }
        self.attempts = 0;
        self.paused = false;
        self.last_failure = Some(now);
        if let Some(flight) = &mut self.flight {
            flight.valid = false;
        }
        self.pending = Some(self.new_attempt(Action::Recreate, now));
        true
    }

    pub fn shutdown(&mut self) {
        self.shutdown = true;
        self.pending = None;
        self.flight = None;
        self.paused = false;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    #[cfg(test)]
    pub fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    #[cfg(test)]
    pub fn attempt_count(&self) -> usize {
        self.attempts
    }

    /// Paused recovery must also keep the host alive for an explicit user retry.
    pub fn keep_alive(&self) -> bool {
        !self.shutdown && (self.paused || self.pending.is_some() || self.flight.is_some())
    }

    fn queue(&mut self, action: Action, now: Instant) {
        if self.attempts >= MAX_ATTEMPTS {
            self.paused = true;
            self.pending = None;
            return;
        }
        if let Some(pending) = &mut self.pending {
            pending.action = pending.action.merge(action);
            return;
        }
        let delay = Duration::from_secs(1 << self.attempts);
        self.pending = Some(self.new_attempt(action, now + delay));
    }

    fn new_attempt(&mut self, action: Action, due_at: Instant) -> Attempt {
        self.generation = self.generation.wrapping_add(1);
        Attempt {
            ticket: Ticket(self.generation),
            action,
            due_at,
        }
    }
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
