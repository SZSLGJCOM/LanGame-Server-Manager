const ASSISTANT_EVIDENCE_READ_TIMEOUT: Duration = Duration::from_secs(30);
static ASSISTANT_EVIDENCE_SLOTS: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = OnceLock::new();

async fn run_assistant_evidence_read<T, F>(state: &DesktopState, read: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    run_assistant_evidence_read_with(
        state,
        ASSISTANT_EVIDENCE_SLOTS
            .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)))
            .clone(),
        ASSISTANT_EVIDENCE_READ_TIMEOUT,
        read,
    )
    .await
}

async fn run_assistant_evidence_read_with<T, F>(
    state: &DesktopState,
    slots: std::sync::Arc<tokio::sync::Semaphore>,
    timeout: Duration,
    read: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let lease = state.begin_storage_context_operation("assistant evidence read")?;
    run_assistant_read_worker_with(slots, timeout, move || {
        let _lease = lease;
        read()
    })
    .await
}

async fn run_assistant_read_worker_with<T, F>(
    slots: std::sync::Arc<tokio::sync::Semaphore>,
    timeout: Duration,
    read: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let permit = slots.try_acquire_owned().map_err(|_| {
        String::from(
            "Assistant evidence workers are still busy. Retry after the pending reads finish.",
        )
    })?;
    let worker = tokio::task::spawn_blocking(move || {
        // Dropping an async waiter cannot cancel blocking filesystem I/O. Keep
        // both the worker slot and storage lease until the actual read exits.
        let _permit = permit;
        read()
    });
    tokio::time::timeout(timeout, worker)
        .await
        .map_err(|_| {
            String::from("Assistant evidence read timed out; no complete evidence was supplied.")
        })?
        .map_err(|error| format!("Assistant evidence worker failed: {error}"))?
}

#[cfg(test)]
#[path = "evidence_worker_tests.rs"]
mod evidence_worker_tests;
