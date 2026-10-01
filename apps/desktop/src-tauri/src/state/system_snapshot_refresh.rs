//! Completion for the one system-snapshot refresh shared by foreground requests.
//! Other TimedCache users retain their existing stale-while-refreshing behavior.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::watch;
use tokio::time::Instant;

#[derive(Clone)]
struct Completion {
    generation: u64,
    result: Result<(), RefreshFailure>,
}

#[derive(Clone, Debug)]
pub(crate) enum RefreshFailure {
    Collection(String),
    Worker(String),
    Deadline,
    Closed,
}

impl RefreshFailure {
    pub(crate) fn is_worker_failure(&self) -> bool {
        matches!(self, Self::Worker(_) | Self::Closed)
    }
}

impl std::fmt::Display for RefreshFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Collection(message) | Self::Worker(message) => formatter.write_str(message),
            Self::Deadline => {
                formatter.write_str("system snapshot refresh did not finish within 20 seconds")
            }
            Self::Closed => {
                formatter.write_str("system snapshot refresh completion channel closed")
            }
        }
    }
}

pub(crate) struct SystemSnapshotRefresh {
    generation: AtomicU64,
    completed: watch::Sender<Completion>,
}

impl Default for SystemSnapshotRefresh {
    fn default() -> Self {
        let (completed, _) = watch::channel(Completion {
            generation: 0,
            result: Ok(()),
        });
        Self {
            generation: AtomicU64::new(0),
            completed,
        }
    }
}

impl SystemSnapshotRefresh {
    /// Called while holding the system cache lock, after reserving its refresh.
    pub(crate) fn begin(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// The cache lock also protects joining this generation against a new one.
    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub(crate) fn subscribe(&self) -> SystemSnapshotWaiter {
        SystemSnapshotWaiter {
            completed: self.completed.subscribe(),
        }
    }

    pub(crate) fn track<T: Send + 'static>(
        self: &Arc<Self>,
        generation: u64,
        worker: tauri::async_runtime::JoinHandle<Result<T, String>>,
    ) {
        let signal = Arc::clone(self);
        // This detached publisher outlives any UI request. The original worker
        // must store the cache / release its lease before subscribers wake.
        tauri::async_runtime::spawn(async move {
            let result = match worker.await {
                Ok(result) => result.map(|_| ()).map_err(RefreshFailure::Collection),
                Err(error) => Err(RefreshFailure::Worker(format!(
                    "system snapshot worker failed: {error}"
                ))),
            };
            signal.complete(generation, result);
        });
    }

    fn complete(&self, generation: u64, result: Result<(), RefreshFailure>) {
        self.completed.send_if_modified(|completed| {
            if generation < completed.generation {
                return false;
            }
            *completed = Completion { generation, result };
            true
        });
    }
}

pub(crate) struct SystemSnapshotWaiter {
    completed: watch::Receiver<Completion>,
}

impl SystemSnapshotWaiter {
    pub(crate) async fn wait(
        mut self,
        generation: u64,
        deadline: Instant,
    ) -> Result<(), RefreshFailure> {
        let completion = tokio::time::timeout_at(
            deadline,
            self.completed
                .wait_for(|completed| completed.generation >= generation),
        )
        .await
        .map_err(|_| RefreshFailure::Deadline)?
        .map_err(|_| RefreshFailure::Closed)?;
        completion.result.clone()
    }
}

#[cfg(test)]
#[path = "system_snapshot_refresh_tests.rs"]
mod tests;
