const ASSISTANT_RUN_SLICE_CALLS: usize = 24;
const ASSISTANT_RUN_SLICE_READS: usize = 24;
const ASSISTANT_RUN_SLICE_OPERATIONS: usize = 8;
const ASSISTANT_RUN_MAX_CALLS: usize = 128;
const ASSISTANT_RUN_MAX_READS: usize = 128;
const ASSISTANT_RUN_MAX_OPERATIONS: usize = 32;
const ASSISTANT_RUN_WORK_TIME: std::time::Duration = std::time::Duration::from_secs(20 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRunPauseReason {
    ModelSlice,
    ReadSlice,
    OperationSlice,
    ModelLimit,
    ReadLimit,
    OperationLimit,
    WorkTime,
    RepeatedOperation,
    InvalidFingerprint,
    WorkInProgress,
    NotPaused,
    StateUnavailable,
    InvestigationFailed,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantRunPause {
    pub reason: AssistantRunPauseReason,
    pub summary: String,
    pub calls: usize,
    pub reads: usize,
    pub operations: usize,
    pub slice_calls: usize,
    pub slice_reads: usize,
    pub slice_operations: usize,
    pub slices_granted: usize,
    pub work_ms: u64,
    pub remaining_work_ms: u64,
    pub can_resume: bool,
}

impl std::fmt::Display for AssistantRunPause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.summary)
    }
}

impl std::error::Error for AssistantRunPause {}

#[derive(Debug, Default)]
pub(super) struct AssistantTaskRun {
    state: std::sync::Mutex<AssistantRunState>,
}

#[derive(Debug)]
struct AssistantRunState {
    calls: usize,
    reads: usize,
    operations: usize,
    slice_calls: usize,
    slice_reads: usize,
    slice_operations: usize,
    slices_granted: usize,
    work_time: std::time::Duration,
    active: std::collections::HashMap<usize, std::time::Instant>,
    next_work_id: usize,
    attempted_operations: std::collections::HashSet<String>,
    pause: Option<AssistantRunPauseReason>,
}

impl Default for AssistantRunState {
    fn default() -> Self {
        Self {
            calls: 0,
            reads: 0,
            operations: 0,
            slice_calls: 0,
            slice_reads: 0,
            slice_operations: 0,
            slices_granted: 1,
            work_time: std::time::Duration::ZERO,
            active: std::collections::HashMap::new(),
            next_work_id: 0,
            attempted_operations: std::collections::HashSet::new(),
            pause: None,
        }
    }
}

/// Keep this guard only around real model, evidence, or confirmed write work.
/// Operator confirmation waits must not own a work guard. The caller also caps
/// cancellable I/O with remaining_time(); the ledger cannot interrupt a commit.
#[derive(Debug)]
pub(super) struct AssistantRunWork {
    run: std::sync::Arc<AssistantTaskRun>,
    id: usize,
}

#[derive(Clone, Copy)]
enum AssistantRunWorkKind {
    Model,
    Read,
    Operation,
}

impl AssistantTaskRun {
    pub(super) fn reserve_model(
        self: &std::sync::Arc<Self>,
    ) -> Result<AssistantRunWork, AssistantRunPause> {
        self.reserve(AssistantRunWorkKind::Model, None)
    }

    pub(super) fn reserve_read(
        self: &std::sync::Arc<Self>,
    ) -> Result<AssistantRunWork, AssistantRunPause> {
        self.reserve(AssistantRunWorkKind::Read, None)
    }

    /// The caller supplies a bounded digest of the normalized operation and its
    /// current precondition. Even failed/unknown writes remain attempted; a new
    /// slice never makes replay safe. A rejection can be returned to the model.
    pub(super) fn reserve_operation(
        self: &std::sync::Arc<Self>,
        fingerprint: &str,
    ) -> Result<AssistantRunWork, AssistantRunPause> {
        self.reserve(AssistantRunWorkKind::Operation, Some(fingerprint))
    }

    fn reserve(
        self: &std::sync::Arc<Self>,
        kind: AssistantRunWorkKind,
        fingerprint: Option<&str>,
    ) -> Result<AssistantRunWork, AssistantRunPause> {
        let mut state = self.state.lock().map_err(|_| unavailable_run())?;
        state.pause_if_time_exhausted();
        if let Some(reason) = state.pause {
            return Err(state.receipt(reason));
        }
        if let Some(fingerprint) = fingerprint {
            if fingerprint.is_empty() || fingerprint.len() > 256 {
                return Err(state.receipt(AssistantRunPauseReason::InvalidFingerprint));
            }
            if state.attempted_operations.contains(fingerprint) {
                // This rejects only the duplicate invocation. Different evidence
                // or a corrected plan may proceed in the same live task.
                return Err(state.receipt(AssistantRunPauseReason::RepeatedOperation));
            }
        }
        if let Some(reason) = state.limit_for(kind) {
            state.pause = Some(reason);
            return Err(state.receipt(reason));
        }
        match kind {
            AssistantRunWorkKind::Model => {
                state.calls += 1;
                state.slice_calls += 1;
            }
            AssistantRunWorkKind::Read => {
                state.reads += 1;
                state.slice_reads += 1;
            }
            AssistantRunWorkKind::Operation => {
                state.operations += 1;
                state.slice_operations += 1;
            }
        }
        if let Some(fingerprint) = fingerprint {
            state.attempted_operations.insert(fingerprint.into());
        }
        let id = state.next_work_id;
        state.next_work_id += 1;
        state.active.insert(id, std::time::Instant::now());
        Ok(AssistantRunWork {
            run: std::sync::Arc::clone(self),
            id,
        })
    }

    pub(super) fn is_paused(&self) -> bool {
        self.pause_receipt().is_some()
    }

    pub(super) fn pause_receipt(&self) -> Option<AssistantRunPause> {
        let Ok(mut state) = self.state.lock() else {
            return Some(unavailable_run());
        };
        state.pause_if_time_exhausted();
        state.pause.map(|reason| state.receipt(reason))
    }

    pub(super) fn pause_after_investigation_failure(&self) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| unavailable_run().summary)?;
        if !state.active.is_empty() {
            return Err(state
                .receipt(AssistantRunPauseReason::WorkInProgress)
                .summary);
        }
        state.pause_if_time_exhausted();
        if state.pause.is_none() {
            state.pause = [
                AssistantRunWorkKind::Model,
                AssistantRunWorkKind::Read,
                AssistantRunWorkKind::Operation,
            ]
            .into_iter()
            .find_map(|kind| state.limit_for(kind))
            .or(Some(AssistantRunPauseReason::InvestigationFailed));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn can_resume(&self) -> bool {
        self.pause_receipt().is_some_and(|pause| pause.can_resume)
    }

    /// Only a currently paused slice can be renewed. Repeated clicks cannot
    /// accumulate grants, reset lifetime usage, or erase attempted writes.
    pub(super) fn grant_continuation(&self) -> Result<(), AssistantRunPause> {
        let mut state = self.state.lock().map_err(|_| unavailable_run())?;
        state.pause_if_time_exhausted();
        if !state.active.is_empty() {
            return Err(state.receipt(AssistantRunPauseReason::WorkInProgress));
        }
        let reason = state
            .pause
            .ok_or_else(|| state.receipt(AssistantRunPauseReason::NotPaused))?;
        let receipt = state.receipt(reason);
        if !receipt.can_resume {
            return Err(receipt);
        }
        // Retrying a failed read/model request retains its current allowance;
        // failures must not become a way to obtain extra slices.
        if reason != AssistantRunPauseReason::InvestigationFailed {
            state.slice_calls = 0;
            state.slice_reads = 0;
            state.slice_operations = 0;
            state.slices_granted += 1;
        }
        state.pause = None;
        Ok(())
    }

    pub(super) fn remaining_work_time(&self) -> std::time::Duration {
        self.state
            .lock()
            .map(|state| ASSISTANT_RUN_WORK_TIME.saturating_sub(state.elapsed()))
            .unwrap_or_default()
    }
}

impl AssistantRunWork {
    pub(super) fn remaining_time(&self) -> std::time::Duration {
        self.run.remaining_work_time()
    }
}

impl Drop for AssistantRunWork {
    fn drop(&mut self) {
        if let Ok(mut state) = self.run.state.lock()
            && let Some(started) = state.active.remove(&self.id)
        {
            state.work_time = state.work_time.saturating_add(started.elapsed());
            state.pause_if_time_exhausted();
        }
    }
}

impl AssistantRunState {
    fn elapsed(&self) -> std::time::Duration {
        self.active
            .values()
            .fold(self.work_time, |elapsed, started| {
                elapsed.saturating_add(started.elapsed())
            })
    }

    fn pause_if_time_exhausted(&mut self) {
        if self.elapsed() >= ASSISTANT_RUN_WORK_TIME {
            self.pause = Some(AssistantRunPauseReason::WorkTime);
        }
    }

    fn limit_for(&self, kind: AssistantRunWorkKind) -> Option<AssistantRunPauseReason> {
        use AssistantRunPauseReason::*;
        let (used, hard, slice, soft, hard_reason, soft_reason) = match kind {
            AssistantRunWorkKind::Model => (
                self.calls,
                ASSISTANT_RUN_MAX_CALLS,
                self.slice_calls,
                ASSISTANT_RUN_SLICE_CALLS,
                ModelLimit,
                ModelSlice,
            ),
            AssistantRunWorkKind::Read => (
                self.reads,
                ASSISTANT_RUN_MAX_READS,
                self.slice_reads,
                ASSISTANT_RUN_SLICE_READS,
                ReadLimit,
                ReadSlice,
            ),
            AssistantRunWorkKind::Operation => (
                self.operations,
                ASSISTANT_RUN_MAX_OPERATIONS,
                self.slice_operations,
                ASSISTANT_RUN_SLICE_OPERATIONS,
                OperationLimit,
                OperationSlice,
            ),
        };
        if used >= hard {
            Some(hard_reason)
        } else if slice >= soft {
            Some(soft_reason)
        } else {
            None
        }
    }

    fn receipt(&self, reason: AssistantRunPauseReason) -> AssistantRunPause {
        use AssistantRunPauseReason::*;
        let work = self.elapsed();
        let remaining = ASSISTANT_RUN_WORK_TIME.saturating_sub(work);
        let can_resume = self.active.is_empty()
            && !remaining.is_zero()
            && match reason {
                ModelSlice => self.calls < ASSISTANT_RUN_MAX_CALLS,
                ReadSlice => self.reads < ASSISTANT_RUN_MAX_READS,
                OperationSlice => self.operations < ASSISTANT_RUN_MAX_OPERATIONS,
                InvestigationFailed => {
                    self.calls < ASSISTANT_RUN_MAX_CALLS
                        && self.reads < ASSISTANT_RUN_MAX_READS
                        && self.operations < ASSISTANT_RUN_MAX_OPERATIONS
                }
                _ => false,
            };
        let summary = match reason {
            ModelSlice => {
                "The current model-call allowance is exhausted. Continue this task to grant the next bounded slice."
            }
            ReadSlice => {
                "The current evidence-read allowance is exhausted. Continue this task to grant the next bounded slice."
            }
            OperationSlice => {
                "The current confirmed-operation allowance is exhausted. Continue this task to grant the next bounded slice."
            }
            ModelLimit => {
                "The task reached its cumulative model-call limit; it cannot continue within this run."
            }
            ReadLimit => {
                "The task reached its cumulative evidence-read limit; it cannot continue within this run."
            }
            OperationLimit => {
                "The task reached its cumulative confirmed-operation limit; it cannot continue within this run."
            }
            WorkTime => {
                "The task exhausted its 20-minute active-work budget. Completed changes remain recorded; no further step may start in this run."
            }
            RepeatedOperation => {
                "This operation was already attempted against the same state. Inspect its result and obtain new evidence or propose a different correction; do not replay a failed or uncertain write."
            }
            InvalidFingerprint => {
                "The operation has no valid bounded fingerprint; no write was started."
            }
            WorkInProgress => {
                "A task step is still finishing. Its budget cannot be renewed until that work completes."
            }
            NotPaused => "This task has no exhausted slice to renew; no extra budget was granted.",
            StateUnavailable => {
                "The task budget state is unavailable; no further work was started."
            }
            InvestigationFailed => {
                "The investigation failed. Its task, evidence and consumed allowance are retained; continuing retries the investigation without replaying a confirmed operation."
            }
        };
        AssistantRunPause {
            reason,
            summary: summary.into(),
            calls: self.calls,
            reads: self.reads,
            operations: self.operations,
            slice_calls: self.slice_calls,
            slice_reads: self.slice_reads,
            slice_operations: self.slice_operations,
            slices_granted: self.slices_granted,
            work_ms: work.as_millis().min(u128::from(u64::MAX)) as u64,
            remaining_work_ms: remaining.as_millis().min(u128::from(u64::MAX)) as u64,
            can_resume,
        }
    }
}

fn unavailable_run() -> AssistantRunPause {
    AssistantRunState::default().receipt(AssistantRunPauseReason::StateUnavailable)
}

#[cfg(test)]
#[path = "run_budget_tests.rs"]
mod run_budget_tests;

include!("run_budget_persistence.rs");
