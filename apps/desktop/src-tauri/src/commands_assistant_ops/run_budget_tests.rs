use super::*;

fn run() -> std::sync::Arc<AssistantTaskRun> {
    std::sync::Arc::new(AssistantTaskRun::default())
}

#[test]
fn follow_up_shares_usage_and_each_explicit_continuation_grants_one_slice() {
    let run = run();
    let follow_up = std::sync::Arc::clone(&run);
    for _ in 0..ASSISTANT_RUN_SLICE_CALLS {
        drop(follow_up.reserve_model().unwrap());
    }
    let pause = run.reserve_model().unwrap_err();
    assert_eq!(pause.reason, AssistantRunPauseReason::ModelSlice);
    assert_eq!(pause.calls, ASSISTANT_RUN_SLICE_CALLS);
    assert!(run.is_paused());
    assert!(run.can_resume());
    assert_eq!(run.reserve_read().unwrap_err().reason, pause.reason);

    run.grant_continuation().unwrap();
    assert!(!run.is_paused());
    assert!(!run.can_resume());
    assert_eq!(
        run.grant_continuation().unwrap_err().reason,
        AssistantRunPauseReason::NotPaused
    );
    drop(follow_up.reserve_model().unwrap());
    let state = run.state.lock().unwrap();
    assert_eq!(state.calls, ASSISTANT_RUN_SLICE_CALLS + 1);
    assert_eq!(state.slice_calls, 1);
    assert_eq!(state.slices_granted, 2);
}

#[test]
fn lifetime_model_and_read_limits_survive_every_slice_renewal() {
    for (kind, maximum, hard_reason) in [
        (
            AssistantRunWorkKind::Model,
            ASSISTANT_RUN_MAX_CALLS,
            AssistantRunPauseReason::ModelLimit,
        ),
        (
            AssistantRunWorkKind::Read,
            ASSISTANT_RUN_MAX_READS,
            AssistantRunPauseReason::ReadLimit,
        ),
    ] {
        let run = run();
        for _ in 0..maximum {
            let work = match run.reserve(kind, None) {
                Ok(work) => work,
                Err(pause) => {
                    assert!(pause.can_resume);
                    run.grant_continuation().unwrap();
                    run.reserve(kind, None).unwrap()
                }
            };
            drop(work);
        }
        let pause = run.reserve(kind, None).unwrap_err();
        assert_eq!(pause.reason, hard_reason);
        assert!(!pause.can_resume);
        assert_eq!(run.grant_continuation().unwrap_err().reason, hard_reason);
        let state = run.state.lock().unwrap();
        match kind {
            AssistantRunWorkKind::Model => assert_eq!(state.calls, maximum),
            AssistantRunWorkKind::Read => assert_eq!(state.reads, maximum),
            AssistantRunWorkKind::Operation => unreachable!(),
        }
    }
}

#[test]
fn failed_investigation_retry_does_not_renew_its_allowance_or_replay_writes() {
    let run = run();
    drop(run.reserve_model().unwrap());
    drop(run.reserve_read().unwrap());
    drop(run.reserve_operation("already-confirmed").unwrap());
    run.pause_after_investigation_failure().unwrap();
    let paused = run.pause_receipt().unwrap();
    assert_eq!(paused.reason, AssistantRunPauseReason::InvestigationFailed);
    assert!(paused.can_resume);
    run.grant_continuation().unwrap();
    let state = run.state.lock().unwrap();
    assert_eq!((state.calls, state.reads, state.operations), (1, 1, 1));
    assert_eq!(
        (state.slice_calls, state.slice_reads, state.slice_operations),
        (1, 1, 1)
    );
    assert_eq!(state.slices_granted, 1);
    drop(state);
    assert_eq!(
        run.reserve_operation("already-confirmed")
            .unwrap_err()
            .reason,
        AssistantRunPauseReason::RepeatedOperation
    );
}

#[test]
fn failed_investigation_cannot_bypass_running_work_or_exhausted_budget() {
    let run = run();
    let work = run.reserve_model().unwrap();
    assert!(run.pause_after_investigation_failure().is_err());
    drop(work);
    {
        let mut state = run.state.lock().unwrap();
        state.calls = ASSISTANT_RUN_MAX_CALLS;
    }
    run.pause_after_investigation_failure().unwrap();
    assert_eq!(
        run.pause_receipt().unwrap().reason,
        AssistantRunPauseReason::ModelLimit
    );
    assert!(!run.can_resume());
    assert!(run.grant_continuation().is_err());
}

#[test]
fn confirmed_operations_have_a_separate_slice_and_cumulative_limit() {
    let run = run();
    for index in 0..ASSISTANT_RUN_MAX_OPERATIONS {
        let fingerprint = format!("operation-{index}");
        let work = match run.reserve_operation(&fingerprint) {
            Ok(work) => work,
            Err(pause) => {
                assert_eq!(pause.reason, AssistantRunPauseReason::OperationSlice);
                assert!(pause.can_resume);
                run.grant_continuation().unwrap();
                run.reserve_operation(&fingerprint).unwrap()
            }
        };
        drop(work);
    }
    let pause = run.reserve_operation("new-operation").unwrap_err();
    assert_eq!(pause.reason, AssistantRunPauseReason::OperationLimit);
    assert_eq!(pause.operations, ASSISTANT_RUN_MAX_OPERATIONS);
    assert_eq!(pause.calls, 0);
    assert_eq!(pause.reads, 0);
    assert!(!pause.can_resume);
    assert!(run.grant_continuation().is_err());
}

#[test]
fn uncertain_writes_are_not_replayed_but_corrected_plans_can_continue() {
    let run = run();
    // Dropping the guard records consumed work regardless of outcome. An error
    // or cancellation must not make the same precondition safe to write again.
    drop(run.reserve_operation("plan-a:before-a").unwrap());
    let duplicate = run.reserve_operation("plan-a:before-a").unwrap_err();
    assert_eq!(duplicate.reason, AssistantRunPauseReason::RepeatedOperation);
    assert_eq!(duplicate.operations, 1);
    assert!(!duplicate.can_resume);
    assert!(!run.is_paused());
    drop(run.reserve_read().unwrap());
    drop(run.reserve_model().unwrap());
    drop(run.reserve_operation("plan-b:before-a").unwrap());
    drop(run.reserve_operation("plan-a:before-b").unwrap());
    assert_eq!(run.state.lock().unwrap().operations, 3);
}

#[test]
fn continuation_does_not_clear_attempted_operation_fingerprints() {
    let run = run();
    for index in 0..ASSISTANT_RUN_SLICE_OPERATIONS {
        drop(
            run.reserve_operation(&format!("plan-{index}:before"))
                .unwrap(),
        );
    }
    assert_eq!(
        run.reserve_operation("another:before").unwrap_err().reason,
        AssistantRunPauseReason::OperationSlice
    );
    run.grant_continuation().unwrap();
    assert_eq!(
        run.reserve_operation("plan-0:before").unwrap_err().reason,
        AssistantRunPauseReason::RepeatedOperation
    );
    assert!(!run.is_paused());
    drop(run.reserve_operation("another:before").unwrap());
    assert_eq!(
        run.state.lock().unwrap().operations,
        ASSISTANT_RUN_SLICE_OPERATIONS + 1
    );
}

#[test]
fn active_work_is_charged_and_confirmation_waits_have_no_running_clock() {
    let run = run();
    let work = run.reserve_read().unwrap();
    {
        let mut state = run.state.lock().unwrap();
        *state.active.get_mut(&work.id).unwrap() =
            std::time::Instant::now() - std::time::Duration::from_secs(3);
    }
    assert!(work.remaining_time() <= ASSISTANT_RUN_WORK_TIME - std::time::Duration::from_secs(3));
    assert!(
        run.state.lock().unwrap().work_time.is_zero(),
        "active and completed time are not double charged"
    );
    drop(work);
    let before_wait = run.remaining_work_time();
    assert!(run.state.lock().unwrap().active.is_empty());
    assert!(run.state.lock().unwrap().work_time >= std::time::Duration::from_secs(3));
    assert_eq!(
        run.remaining_work_time(),
        before_wait,
        "without a work guard user wait has no elapsed component"
    );
}

#[test]
fn cumulative_active_time_is_a_hard_limit_and_cannot_be_renewed() {
    let run = run();
    run.state.lock().unwrap().work_time = ASSISTANT_RUN_WORK_TIME;
    let pause = run.reserve_model().unwrap_err();
    assert_eq!(pause.reason, AssistantRunPauseReason::WorkTime);
    assert_eq!(pause.calls, 0);
    assert_eq!(pause.remaining_work_ms, 0);
    assert!(!pause.can_resume);
    assert!(run.is_paused());
    assert_eq!(
        run.grant_continuation().unwrap_err().reason,
        AssistantRunPauseReason::WorkTime
    );
    assert_eq!(run.remaining_work_time(), std::time::Duration::ZERO);
}

#[test]
fn finishing_work_can_exhaust_time_without_another_reservation() {
    let run = run();
    let work = run.reserve_operation("confirmed-plan:before").unwrap();
    {
        let mut state = run.state.lock().unwrap();
        *state.active.get_mut(&work.id).unwrap() =
            std::time::Instant::now() - ASSISTANT_RUN_WORK_TIME;
    }
    drop(work);
    let pause = run.pause_receipt().unwrap();
    assert_eq!(pause.reason, AssistantRunPauseReason::WorkTime);
    assert_eq!(pause.operations, 1);
    assert!(run.state.lock().unwrap().active.is_empty());
    assert!(!run.can_resume());
}

#[test]
fn continuation_waits_for_running_work_and_does_not_grant_early() {
    let run = run();
    let work = run.reserve_model().unwrap();
    for _ in 1..ASSISTANT_RUN_SLICE_CALLS {
        drop(run.reserve_model().unwrap());
    }
    let pause = run.reserve_model().unwrap_err();
    assert_eq!(pause.reason, AssistantRunPauseReason::ModelSlice);
    assert!(!pause.can_resume);
    assert_eq!(
        run.grant_continuation().unwrap_err().reason,
        AssistantRunPauseReason::WorkInProgress
    );
    assert_eq!(run.state.lock().unwrap().slices_granted, 1);
    drop(work);
    assert!(run.can_resume());
    run.grant_continuation().unwrap();
    assert_eq!(run.state.lock().unwrap().slices_granted, 2);
}

#[test]
fn invalid_operation_fingerprints_consume_no_budget_and_do_not_pause_reads() {
    let run = run();
    for fingerprint in [String::new(), "x".repeat(257)] {
        let error = run.reserve_operation(&fingerprint).unwrap_err();
        assert_eq!(error.reason, AssistantRunPauseReason::InvalidFingerprint);
        assert_eq!(error.operations, 0);
        assert!(!run.is_paused());
    }
    drop(run.reserve_read().unwrap());
    assert_eq!(run.state.lock().unwrap().reads, 1);
}

#[test]
fn pause_receipts_expose_stable_serialized_budget_fields() {
    let run = run();
    for _ in 0..ASSISTANT_RUN_SLICE_READS {
        drop(run.reserve_read().unwrap());
    }
    let pause = run.reserve_read().unwrap_err();
    let value = serde_json::to_value(&pause).unwrap();
    assert_eq!(value["reason"], "read_slice");
    assert_eq!(value["reads"], ASSISTANT_RUN_SLICE_READS);
    assert_eq!(value["sliceReads"], ASSISTANT_RUN_SLICE_READS);
    assert_eq!(value["slicesGranted"], 1);
    assert_eq!(value["canResume"], true);
    assert!(value["remainingWorkMs"].as_u64().is_some());
    assert!(value["workMs"].as_u64().is_some());
    assert!(
        value["summary"]
            .as_str()
            .is_some_and(|summary| !summary.is_empty())
    );
}
