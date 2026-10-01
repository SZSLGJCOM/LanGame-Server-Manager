use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex};

use app_runtime::{ExitedManagedProcess, RuntimePerformanceRefresh};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use super::{StorageContextOperationGuard, spawn_storage_context_task};

pub(crate) type ReconciliationAdmission = Arc<OwnedMutexGuard<()>>;

#[derive(Clone)]
pub(crate) struct PendingRuntimeExit {
    pub exited: ExitedManagedProcess,
    pub persisted: bool,
    pub mutation: Arc<OwnedMutexGuard<()>>,
}

struct ExitBatch {
    entries: VecDeque<PendingRuntimeExit>,
    // Pending events still refer to this storage context after their caller leaves.
    _storage: StorageContextOperationGuard,
}

/// One batch per desktop state. Reaping pauses until every event is acknowledged.
#[derive(Default)]
pub(crate) struct RuntimeExitReconciliation {
    admission: Arc<AsyncMutex<()>>,
    pending: Mutex<Option<ExitBatch>>,
}

impl RuntimeExitReconciliation {
    pub(crate) async fn acquire(&self) -> ReconciliationAdmission {
        Arc::new(Arc::clone(&self.admission).lock_owned().await)
    }

    pub(crate) async fn collect<F>(
        self: &Arc<Self>,
        admission: ReconciliationAdmission,
        storage: StorageContextOperationGuard,
        mutations: HashMap<String, Arc<OwnedMutexGuard<()>>>,
        collect: F,
    ) -> Result<Vec<RuntimePerformanceRefresh>, String>
    where
        F: FnOnce() -> Result<(Vec<ExitedManagedProcess>, Vec<RuntimePerformanceRefresh>), String>
            + Send
            + 'static,
    {
        let owner = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let _admission = admission;
            // Lock before the collector removes any supervisor entries. No fallible
            // lock acquisition or await may separate removal from publication here.
            let mut pending = owner.pending.lock().map_err(|_| pending_poisoned())?;
            if pending.is_some() {
                return Ok(Vec::new());
            }
            let (exits, refreshed) = collect()?;
            if !exits.is_empty() {
                *pending = Some(ExitBatch {
                    entries: exits
                        .into_iter()
                        .map(|exited| PendingRuntimeExit {
                            // The collector only reaps the prelocked instance set.
                            // Sibling exits share that lock until their last acknowledgement.
                            mutation: Arc::clone(mutations.get(&exited.summary.id).expect(
                                "exit collector returned an instance without its mutation lock",
                            )),
                            exited,
                            persisted: false,
                        })
                        .collect(),
                    _storage: storage,
                });
            }
            Ok(refreshed)
        })
        .await
        .map_err(|error| format!("Runtime reconciliation task failed: {error}"))?
    }

    pub(crate) fn next(&self) -> Result<Option<PendingRuntimeExit>, String> {
        Ok(self
            .pending
            .lock()
            .map_err(|_| pending_poisoned())?
            .as_ref()
            .and_then(|batch| batch.entries.front().cloned()))
    }

    /// The single admitted persistence task keeps both lifecycle locks when its
    /// caller is cancelled, and records completion before releasing those locks.
    pub(crate) fn persist<F>(
        self: &Arc<Self>,
        admission: ReconciliationAdmission,
        storage: StorageContextOperationGuard,
        mutation: Arc<OwnedMutexGuard<()>>,
        exited: ExitedManagedProcess,
        operation: F,
    ) -> impl Future<Output = Result<bool, String>> + Send + 'static
    where
        F: Future<Output = Result<bool, String>> + Send + 'static,
    {
        let owner = Arc::clone(self);
        async move {
            spawn_storage_context_task(&storage, async move {
                let _admission = admission;
                let _mutation = mutation;
                let current = owner
                    .next()?
                    .ok_or_else(|| String::from("Runtime exit is no longer pending"))?;
                if !same_exit(&current.exited, &exited) {
                    return Err(String::from(
                        "Runtime exit ownership changed before persistence",
                    ));
                }
                if current.persisted {
                    return Ok(true);
                }
                if !operation.await? {
                    owner.acknowledge(&exited)?;
                    return Ok(false);
                }
                let mut pending = owner.pending.lock().map_err(|_| pending_poisoned())?;
                let entry = pending
                    .as_mut()
                    .and_then(|batch| batch.entries.front_mut())
                    .filter(|entry| same_exit(&entry.exited, &exited))
                    .ok_or_else(|| {
                        String::from("Runtime exit ownership changed after persistence")
                    })?;
                entry.persisted = true;
                Ok(true)
            })
            .await
            .map_err(|error| format!("Runtime exit persistence task failed: {error}"))?
        }
    }

    pub(crate) fn acknowledge(&self, exited: &ExitedManagedProcess) -> Result<(), String> {
        let mut pending = self.pending.lock().map_err(|_| pending_poisoned())?;
        let batch = pending
            .as_mut()
            .filter(|batch| {
                batch
                    .entries
                    .front()
                    .is_some_and(|entry| same_exit(&entry.exited, exited))
            })
            .ok_or_else(|| String::from("Runtime exit acknowledgement does not match its owner"))?;
        batch.entries.pop_front();
        if batch.entries.is_empty() {
            *pending = None;
        }
        Ok(())
    }
}

fn same_exit(left: &ExitedManagedProcess, right: &ExitedManagedProcess) -> bool {
    left.summary.id == right.summary.id && left.run_id == right.run_id
}

fn pending_poisoned() -> String {
    String::from("Runtime exit queue lock poisoned")
}

#[cfg(test)]
#[path = "runtime_reconciliation_tests.rs"]
mod tests;
