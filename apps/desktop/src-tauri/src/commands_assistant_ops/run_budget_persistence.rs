#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct AssistantRunCheckpoint {
    calls: usize,
    reads: usize,
    operations: usize,
    slice_calls: usize,
    slice_reads: usize,
    slice_operations: usize,
    slices_granted: usize,
    work_ms: u64,
    active: bool,
    saved_unix_ms: u64,
    attempted_operations: std::collections::HashSet<String>,
    pause: Option<AssistantRunPauseReason>,
}

impl AssistantTaskRun {
    fn checkpoint(&self) -> Result<AssistantRunCheckpoint, String> {
        let state = self.state.lock().map_err(|_| unavailable_run().summary)?;
        Ok(AssistantRunCheckpoint {
            calls: state.calls,
            reads: state.reads,
            operations: state.operations,
            slice_calls: state.slice_calls,
            slice_reads: state.slice_reads,
            slice_operations: state.slice_operations,
            slices_granted: state.slices_granted,
            work_ms: state.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            active: !state.active.is_empty(),
            saved_unix_ms: unix_timestamp_ms(),
            attempted_operations: state.attempted_operations.clone(),
            pause: state.pause,
        })
    }

    fn from_checkpoint(
        saved: AssistantRunCheckpoint,
        requires_restatement: bool,
    ) -> Result<Self, String> {
        if saved.calls > ASSISTANT_RUN_MAX_CALLS
            || saved.reads > ASSISTANT_RUN_MAX_READS
            || saved.operations > ASSISTANT_RUN_MAX_OPERATIONS
            || saved.slice_calls > ASSISTANT_RUN_SLICE_CALLS
            || saved.slice_reads > ASSISTANT_RUN_SLICE_READS
            || saved.slice_operations > ASSISTANT_RUN_SLICE_OPERATIONS
            || saved.slice_calls > saved.calls
            || saved.slice_reads > saved.reads
            || saved.slice_operations > saved.operations
            || saved.slices_granted == 0
            || saved.slices_granted
                > ASSISTANT_RUN_MAX_CALLS + ASSISTANT_RUN_MAX_READS + ASSISTANT_RUN_MAX_OPERATIONS
            || saved.attempted_operations.len() > ASSISTANT_RUN_MAX_OPERATIONS
            || saved.attempted_operations.len() != saved.operations
            || saved
                .attempted_operations
                .iter()
                .any(|digest| digest.is_empty() || digest.len() > 256)
        {
            return Err(
                "Assistant task budget checkpoint is invalid; no allowance was granted.".into(),
            );
        }
        // A missing completion receipt cannot prove when work stopped. Charge
        // elapsed wall time conservatively rather than letting restarts renew
        // active-work allowance. An idle, saved checkpoint incurs no downtime.
        let interrupted_ms = if saved.active {
            unix_timestamp_ms().saturating_sub(saved.saved_unix_ms)
        } else {
            0
        };
        let mut state = AssistantRunState {
            calls: saved.calls,
            reads: saved.reads,
            operations: saved.operations,
            slice_calls: saved.slice_calls,
            slice_reads: saved.slice_reads,
            slice_operations: saved.slice_operations,
            slices_granted: saved.slices_granted,
            work_time: Duration::from_millis(saved.work_ms.saturating_add(interrupted_ms))
                .min(ASSISTANT_RUN_WORK_TIME),
            attempted_operations: saved.attempted_operations,
            pause: Some(if requires_restatement {
                AssistantRunPauseReason::StateUnavailable
            } else {
                saved
                    .pause
                    .unwrap_or(AssistantRunPauseReason::InvestigationFailed)
            }),
            ..AssistantRunState::default()
        };
        state.pause_if_time_exhausted();
        Ok(Self {
            state: StdMutex::new(state),
        })
    }
}
