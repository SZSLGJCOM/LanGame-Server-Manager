//! A final exit deadline independent of the async executor and application locks.
use crate::ProcessExitTarget;
use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const PROCESS_SETTLE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub struct ExitWatchdogOutcome {
    pub forced: bool,
    pub error: Option<io::Error>,
}

/// The callback must finish the owning UI process without taking business locks.
/// There is deliberately no cancellation: an accepted final exit cannot be undone
/// by a failed save, a dropped future, or another click. The captured process object
/// cannot become a different process when Windows reuses its PID.
pub fn spawn_exit_watchdog(
    target: Arc<ProcessExitTarget>,
    deadline: Instant,
    finish: impl FnOnce(ExitWatchdogOutcome) + Send + 'static,
) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
        .name(String::from("final-exit-watchdog"))
        .spawn(move || {
            let outcome = await_exit(&target, deadline);
            finish(outcome);
        })
}

fn await_exit(target: &ProcessExitTarget, deadline: Instant) -> ExitWatchdogOutcome {
    let mut error = None;
    loop {
        match target.is_running() {
            Ok(false) => {
                // Give normal UI exit a bounded chance to release resources.
                // Even a frozen UI is then terminated by the callback.
                thread::sleep(
                    PROCESS_SETTLE_TIMEOUT.min(deadline.saturating_duration_since(Instant::now())),
                );
                return ExitWatchdogOutcome {
                    forced: false,
                    error,
                };
            }
            Err(cause) => {
                error.get_or_insert(cause);
            }
            Ok(true) => {}
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(POLL_INTERVAL.min(remaining));
    }
    if let Err(cause) = target.terminate() {
        error.get_or_insert(cause);
    }
    let settle_deadline = Instant::now() + PROCESS_SETTLE_TIMEOUT;
    loop {
        match target.is_running() {
            Ok(false) => break,
            Err(cause) => {
                error.get_or_insert(cause);
                break;
            }
            Ok(true) => {
                let remaining = settle_deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    error.get_or_insert_with(|| {
                        io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Runtime process did not finish after forced termination",
                        )
                    });
                    break;
                }
                thread::sleep(POLL_INTERVAL.min(remaining));
            }
        }
    }
    ExitWatchdogOutcome {
        forced: true,
        error,
    }
}

#[cfg(test)]
#[path = "windows_exit_watchdog_tests.rs"]
mod tests;
