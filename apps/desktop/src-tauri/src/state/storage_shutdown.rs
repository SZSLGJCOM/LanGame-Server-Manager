use super::{
    DesktopState, StorageContextOperationGuard, StorageContextOperationLease,
    StorageContextShutdownGuard,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(25);

impl StorageContextOperationGuard {
    pub(crate) fn cancellation_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self._lease.cancellation)
    }

    pub(crate) async fn cancelled(&self) {
        while !self._lease.cancellation.load(Ordering::SeqCst) {
            tokio::time::sleep(DRAIN_POLL_INTERVAL).await;
        }
    }
}

impl DesktopState {
    /// Reserve the shutdown owner's work under stable paths before starting
    /// drain. Per-instance locks serialize it with already admitted mutations;
    /// unrelated copies and backups must not delay every server's save request.
    pub(crate) async fn reserve_storage_shutdown_operation(
        &self,
        timeout: Duration,
    ) -> Result<StorageContextOperationGuard, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut coordinator = self
                    .storage_context_coordinator
                    .lock()
                    .map_err(|_| "storage context coordinator lock poisoned")?;
                if !self.shutdown_in_progress.load(Ordering::SeqCst)
                    || coordinator.shutdown_exclusive_in_progress
                {
                    return Err("Shutdown work requires closed admission before exclusivity".into());
                }
                if !coordinator.transition_in_progress {
                    let operation_id = coordinator
                        .next_operation_id
                        .checked_add(1)
                        .ok_or("storage operation counter overflow")?;
                    coordinator.next_operation_id = operation_id;
                    let cancellation = Arc::new(AtomicBool::new(false));
                    coordinator.active_operations.insert(
                        operation_id,
                        (
                            "application server shutdown".into(),
                            Arc::clone(&cancellation),
                        ),
                    );
                    return Ok(StorageContextOperationGuard {
                        _lease: Arc::new(StorageContextOperationLease {
                            coordinator: Arc::clone(&self.storage_context_coordinator),
                            operation_id,
                            cancellation,
                        }),
                    });
                }
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err("Shutdown could not acquire stable application paths".into());
            }
            tokio::time::sleep_until((now + DRAIN_POLL_INTERVAL).min(deadline)).await;
        }
    }

    /// Admission is already closed by shutdown_in_progress. Cancel preparatory
    /// work, but retain every lease until its owner commits or rolls back.
    pub(crate) async fn drain_storage_operations_for_shutdown(
        &self,
        timeout: Duration,
    ) -> Result<StorageContextShutdownGuard, String> {
        let deadline = tokio::time::Instant::now() + timeout;
        self.autostart.cancel_all()?;
        loop {
            if !self.shutdown_in_progress.load(Ordering::SeqCst) {
                return Err(String::from(
                    "storage drain requires an active shutdown request",
                ));
            }
            let pending = {
                let coordinator = self
                    .storage_context_coordinator
                    .lock()
                    .map_err(|_| String::from("storage context coordinator lock poisoned"))?;
                let mut names = Vec::new();
                for (name, cancellation) in coordinator.active_operations.values() {
                    cancellation.store(true, Ordering::SeqCst);
                    names.push(name.clone());
                }
                if coordinator.transition_in_progress {
                    names.push(String::from("application path update"));
                }
                names.sort();
                names
            };
            // An operation admitted just before shutdown may register its
            // provider handle later. Recheck until all storage leases settle.
            self.install_operations.request_cancel_all()?;
            self.steamcmd_preparation.request_cancel_all()?;
            if pending.is_empty() {
                return self.begin_storage_shutdown_exclusive();
            }
            let now = tokio::time::Instant::now();
            if now >= deadline {
                let count = pending.len();
                let mut names = pending;
                names.dedup();
                names.truncate(8);
                return Err(format!(
                    "storage cleanup timed out after {} seconds; {count} operation(s) still settling: {}",
                    timeout.as_secs(),
                    names.join(", ")
                ));
            }
            tokio::time::sleep_until((now + DRAIN_POLL_INTERVAL).min(deadline)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_work_runs_while_unrelated_writes_drain_and_keeps_exclusivity_blocked() {
        let state = DesktopState::default();
        assert!(
            state
                .reserve_storage_shutdown_operation(Duration::ZERO)
                .await
                .is_err()
        );
        let unrelated_write = state
            .begin_storage_context_operation("existing backup")
            .unwrap();
        assert!(state.begin_app_shutdown(true).unwrap());
        let stop = state
            .reserve_storage_shutdown_operation(Duration::ZERO)
            .await
            .unwrap();
        assert!(state.begin_storage_context_operation("new work").is_err());
        assert!(state.begin_storage_context_transition().is_err());
        assert!(
            state
                .drain_storage_operations_for_shutdown(Duration::ZERO)
                .await
                .is_err()
        );
        assert!(unrelated_write.cancellation_token().load(Ordering::SeqCst));
        drop(unrelated_write);
        assert!(state.begin_storage_shutdown_exclusive().is_err());
        drop(stop);
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn shutdown_work_never_uses_paths_during_an_admitted_transition() {
        let state = DesktopState::default();
        let transition = state.begin_storage_context_transition().unwrap();
        assert!(state.begin_app_shutdown(true).unwrap());
        assert!(
            state
                .reserve_storage_shutdown_operation(Duration::ZERO)
                .await
                .is_err()
        );
        drop(transition);
        let stop = state
            .reserve_storage_shutdown_operation(Duration::ZERO)
            .await
            .unwrap();
        drop(stop);
        let exclusive = state.begin_storage_shutdown_exclusive().unwrap();
        assert!(
            state
                .reserve_storage_shutdown_operation(Duration::ZERO)
                .await
                .is_err()
        );
        drop(exclusive);
    }

    #[tokio::test]
    async fn shutdown_cancels_creation_and_waits_for_the_worker_lease() {
        let state = Arc::new(DesktopState::default());
        let caller = state
            .begin_storage_context_operation("instance creation")
            .unwrap();
        let worker = caller.clone();
        drop(caller);
        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
        let (cleanup_tx, cleanup_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            worker.cancelled().await;
            cancelled_tx.send(()).unwrap();
            cleanup_rx.await.unwrap();
            drop(worker);
        });
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        let shutdown_state = Arc::clone(&state);
        let shutdown = tokio::spawn(async move {
            shutdown_state
                .drain_storage_operations_for_shutdown(Duration::from_secs(2))
                .await
        });
        tokio::time::timeout(Duration::from_secs(1), cancelled_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !shutdown.is_finished(),
            "cancellation acknowledgement is not cleanup"
        );
        assert!(
            state
                .begin_storage_context_operation("late create")
                .is_err()
        );
        cleanup_tx.send(()).unwrap();
        task.await.unwrap();
        let exclusive = shutdown.await.unwrap().unwrap();
        assert!(state.begin_storage_context_transition().is_err());
        drop(exclusive);
    }

    #[tokio::test]
    async fn shutdown_timeout_keeps_pending_writes_owned_and_names_the_blocker() {
        let state = DesktopState::default();
        let operation = state
            .begin_storage_context_operation("instance backup")
            .unwrap();
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        let error = state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .err()
            .expect("active writer must block exit");
        assert!(error.contains("instance backup"));
        assert!(state.begin_storage_shutdown_exclusive().is_err());
        state.shutdown_in_progress.store(false, Ordering::SeqCst);
        let retry = state
            .begin_storage_context_operation("retry operation")
            .unwrap();
        assert!(!retry.cancellation_token().load(Ordering::SeqCst));
        drop(retry);
        drop(operation);
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        state
            .drain_storage_operations_for_shutdown(Duration::ZERO)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn shutdown_cancels_installation_and_steamcmd_preparation() {
        let state = DesktopState::default();
        let install = state
            .install_operations
            .begin(String::from("installation"))
            .unwrap();
        let prepare = state
            .steamcmd_preparation
            .begin(String::from("preparation"))
            .unwrap();
        let operation = state
            .begin_storage_context_operation("game installation")
            .unwrap();
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        assert!(
            state
                .drain_storage_operations_for_shutdown(Duration::ZERO)
                .await
                .is_err()
        );
        assert!(install.cancellation().is_cancelled());
        assert!(prepare.cancellation().unwrap().is_cancelled());
        drop(operation);
        drop(install);
        drop(prepare);
    }
}
