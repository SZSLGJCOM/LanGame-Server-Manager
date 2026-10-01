use std::time::{Duration, Instant};

const COLLECTION_TIMEOUT: Duration = Duration::from_secs(5);
const COLLECTION_DISPATCH_PHASE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DstStructuredLogCollectionBudget {
    pub(super) total: Duration,
    pub(super) dispatch_phase: Duration,
}

impl DstStructuredLogCollectionBudget {
    pub(super) fn standard() -> Self {
        Self {
            total: COLLECTION_TIMEOUT,
            dispatch_phase: COLLECTION_DISPATCH_PHASE_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(total: Duration, dispatch_phase: Duration) -> Self {
        Self {
            total,
            dispatch_phase,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeActionDispatchDeadlineOutcome {
    Completed,
    SubmittedPending,
}

#[derive(Debug)]
pub(crate) enum RuntimeActionDispatchDeadlineError {
    DeadlineBeforeSubmission,
    DispatchFailed,
}

pub(crate) async fn await_runtime_action_dispatch_until<Dispatch, DispatchFuture>(
    deadline: Instant,
    dispatch: Dispatch,
) -> Result<RuntimeActionDispatchDeadlineOutcome, RuntimeActionDispatchDeadlineError>
where
    Dispatch: FnOnce(app_runtime::RuntimeCommandSubmissionTracker, Instant) -> DispatchFuture,
    DispatchFuture: std::future::Future<Output = Result<(), String>>,
{
    if Instant::now() >= deadline {
        return Err(RuntimeActionDispatchDeadlineError::DeadlineBeforeSubmission);
    }
    let submission = app_runtime::RuntimeCommandSubmissionTracker::default();
    match tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        dispatch(submission.clone(), deadline),
    )
    .await
    {
        Ok(Ok(())) => Ok(RuntimeActionDispatchDeadlineOutcome::Completed),
        Ok(Err(_)) => Err(RuntimeActionDispatchDeadlineError::DispatchFailed),
        Err(_) if submission.is_submitted() => {
            Ok(RuntimeActionDispatchDeadlineOutcome::SubmittedPending)
        }
        Err(_) => Err(RuntimeActionDispatchDeadlineError::DeadlineBeforeSubmission),
    }
}
