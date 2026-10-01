use super::*;

fn begin_next(policy: &mut Policy) -> Attempt {
    let attempt = policy.next_attempt().expect("queued recovery");
    assert_eq!(
        policy.begin(attempt.ticket, attempt.due_at),
        Some(attempt.action)
    );
    attempt
}

#[test]
fn reliability_webview_recovery_waits_for_exact_deadline_and_claims_ticket_once() {
    let now = Instant::now();
    let mut policy = Policy::default();
    assert!(policy.record_failure(FailureKind::RendererExited, now));
    let attempt = policy.next_attempt().unwrap();
    assert_eq!(attempt.due_at, now + Duration::from_secs(1));
    assert_eq!(
        policy.begin(attempt.ticket, attempt.due_at - Duration::from_nanos(1)),
        None
    );
    assert_eq!(policy.attempt_count(), 0);
    assert_eq!(
        policy.begin(attempt.ticket, attempt.due_at),
        Some(Action::Reload)
    );
    assert_eq!(policy.begin(attempt.ticket, attempt.due_at), None);
    assert!(policy.in_flight());
    assert!(policy.keep_alive());
    assert!(policy.complete(attempt.ticket, AttemptResult::Succeeded, attempt.due_at));
    assert!(!policy.complete(attempt.ticket, AttemptResult::Failed, attempt.due_at));
    assert_eq!(policy.attempt_count(), 1);
    assert!(!policy.keep_alive());
}

#[test]
fn reliability_webview_recovery_bounds_failed_attempts_with_one_two_four_second_backoff() {
    let mut now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererUnresponsive, now);
    for (index, delay) in [1, 2, 4].into_iter().enumerate() {
        let attempt = policy.next_attempt().unwrap();
        assert_eq!(attempt.due_at, now + Duration::from_secs(delay));
        begin_next(&mut policy);
        now = attempt.due_at;
        assert!(policy.complete(attempt.ticket, AttemptResult::Failed, now));
        assert_eq!(policy.attempt_count(), index + 1);
    }
    assert!(policy.paused());
    assert!(policy.keep_alive());
    assert!(!policy.in_flight());
    assert!(policy.next_attempt().is_none());
    assert!(policy.pending.is_none());
}

#[test]
fn reliability_webview_recovery_merges_duplicates_and_upgrades_without_moving_deadline() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererExited, now);
    let observed = policy.next_attempt().unwrap();
    for kind in [
        FailureKind::RendererExited,
        FailureKind::RendererUnresponsive,
        FailureKind::BrowserExited,
        FailureKind::RendererExited,
    ] {
        policy.record_failure(kind, now + Duration::from_millis(500));
    }
    let upgraded = policy.next_attempt().unwrap();
    assert_eq!(upgraded.ticket, observed.ticket);
    assert_eq!(upgraded.due_at, observed.due_at);
    assert_eq!(upgraded.action, Action::Recreate);
    assert_eq!(policy.attempt_count(), 0);
    assert_eq!(
        policy.begin(observed.ticket, observed.due_at),
        Some(Action::Recreate)
    );
}

#[test]
fn reliability_webview_recovery_retains_new_browser_failure_after_old_reload_succeeds() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererExited, now);
    let old = begin_next(&mut policy);
    let browser_failed = old.due_at + Duration::from_millis(50);
    policy.record_failure(FailureKind::BrowserExited, browser_failed);
    let queued = policy.pending.unwrap();
    policy.record_failure(
        FailureKind::BrowserExited,
        browser_failed + Duration::from_millis(50),
    );
    assert_eq!(policy.pending, Some(queued));
    assert_eq!(queued.due_at, browser_failed + Duration::from_secs(2));
    assert!(policy.next_attempt().is_none());
    assert_eq!(policy.begin(queued.ticket, queued.due_at), None);
    assert!(policy.complete(old.ticket, AttemptResult::Succeeded, browser_failed));
    assert_eq!(policy.next_attempt(), Some(queued));
    assert_eq!(begin_next(&mut policy).action, Action::Recreate);
    assert!(!policy.complete(old.ticket, AttemptResult::Failed, queued.due_at));
    assert!(policy.in_flight());
}

#[test]
fn reliability_webview_recovery_failed_recreation_upgrades_new_pending_renderer_retry() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::BrowserExited, now);
    let old = begin_next(&mut policy);
    policy.record_failure(FailureKind::RendererExited, old.due_at);
    let queued = policy.pending.unwrap();
    assert!(policy.complete(old.ticket, AttemptResult::Failed, old.due_at));
    let next = policy.next_attempt().unwrap();
    assert_eq!(next.ticket, queued.ticket);
    assert_eq!(next.due_at, queued.due_at);
    assert_eq!(next.action, Action::Recreate);
}

#[test]
fn reliability_webview_recovery_failed_reload_escalates_without_resetting_backoff_or_budget() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererUnresponsive, now);
    let reload = begin_next(&mut policy);
    assert_eq!(reload.action, Action::Reload);
    assert!(policy.complete(reload.ticket, AttemptResult::Failed, reload.due_at));
    let recreate = policy.next_attempt().unwrap();
    assert_eq!(recreate.action, Action::Recreate);
    assert_eq!(recreate.due_at, reload.due_at + Duration::from_secs(2));
    assert_eq!(policy.attempt_count(), 1);
    begin_next(&mut policy);
    policy.complete(recreate.ticket, AttemptResult::Failed, recreate.due_at);
    let final_attempt = policy.next_attempt().unwrap();
    assert_eq!(final_attempt.action, Action::Recreate);
    assert_eq!(
        final_attempt.due_at,
        recreate.due_at + Duration::from_secs(4)
    );
    begin_next(&mut policy);
    policy.complete(
        final_attempt.ticket,
        AttemptResult::Failed,
        final_attempt.due_at,
    );
    assert_eq!(policy.attempt_count(), 3);
    assert!(policy.paused());
    assert!(policy.next_attempt().is_none());
}

#[test]
fn reliability_webview_recovery_failed_reload_upgrades_a_reentrant_pending_retry_in_place() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererExited, now);
    let reload = begin_next(&mut policy);
    policy.record_failure(FailureKind::RendererUnresponsive, reload.due_at);
    let queued = policy.pending.unwrap();
    assert_eq!(queued.action, Action::Reload);
    policy.complete(
        reload.ticket,
        AttemptResult::Failed,
        reload.due_at + Duration::from_millis(50),
    );
    let next = policy.next_attempt().unwrap();
    assert_eq!(next.action, Action::Recreate);
    assert_eq!(next.ticket, queued.ticket);
    assert_eq!(next.due_at, queued.due_at);
    assert_eq!(policy.attempt_count(), 1);
}

#[test]
fn reliability_webview_recovery_success_does_not_reset_budget_or_clear_new_paused_failure() {
    let mut now = Instant::now();
    let mut policy = Policy::default();
    for count in 1..=3 {
        policy.record_failure(FailureKind::RendererExited, now);
        let attempt = begin_next(&mut policy);
        now = attempt.due_at;
        if count == 3 {
            policy.record_failure(FailureKind::BrowserExited, now);
        }
        policy.complete(attempt.ticket, AttemptResult::Succeeded, now);
        assert_eq!(policy.attempt_count(), count);
    }
    assert!(policy.paused());
    assert!(policy.next_attempt().is_none());
    assert!(policy.keep_alive());
}

#[test]
fn reliability_webview_recovery_requires_a_full_quiet_window_before_reopening_budget() {
    let mut now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::BrowserExited, now);
    for _ in 0..3 {
        let attempt = begin_next(&mut policy);
        now = attempt.due_at;
        policy.complete(attempt.ticket, AttemptResult::Failed, now);
    }
    for _ in 0..12 {
        now += QUIET_WINDOW - Duration::from_nanos(1);
        policy.record_failure(FailureKind::RendererExited, now);
        assert!(policy.paused());
        assert_eq!(policy.attempt_count(), 3);
        assert!(policy.next_attempt().is_none());
    }
    now += QUIET_WINDOW;
    policy.record_failure(FailureKind::BrowserExited, now);
    assert!(!policy.paused());
    assert_eq!(policy.attempt_count(), 0);
    assert_eq!(
        policy.next_attempt().unwrap().due_at,
        now + Duration::from_secs(1)
    );
}

#[test]
fn reliability_webview_recovery_auxiliary_events_neither_schedule_nor_extend_quiet_window() {
    let now = Instant::now();
    let mut policy = Policy::default();
    assert!(!policy.record_failure(FailureKind::Auxiliary, now));
    assert!(!policy.keep_alive());
    policy.record_failure(FailureKind::RendererExited, now);
    let attempt = begin_next(&mut policy);
    policy.complete(attempt.ticket, AttemptResult::Succeeded, attempt.due_at);
    assert!(!policy.record_failure(
        FailureKind::Auxiliary,
        now + QUIET_WINDOW - Duration::from_secs(1)
    ));
    policy.record_failure(FailureKind::RendererExited, now + QUIET_WINDOW);
    assert_eq!(policy.attempt_count(), 0);
    assert_eq!(
        policy.next_attempt().unwrap().due_at,
        now + QUIET_WINDOW + Duration::from_secs(1)
    );
}

#[test]
fn reliability_webview_manual_retry_invalidates_pending_and_resets_exhausted_budget() {
    let mut now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::BrowserExited, now);
    for _ in 0..3 {
        let attempt = begin_next(&mut policy);
        now = attempt.due_at;
        policy.complete(attempt.ticket, AttemptResult::Failed, now);
    }
    assert!(policy.manual_retry(now));
    let first_manual = policy.next_attempt().unwrap();
    assert_eq!(first_manual.action, Action::Recreate);
    assert_eq!(first_manual.due_at, now);
    assert_eq!(policy.attempt_count(), 0);
    assert!(!policy.paused());
    policy.manual_retry(now);
    let replacement = policy.next_attempt().unwrap();
    assert_ne!(replacement.ticket, first_manual.ticket);
    assert_eq!(policy.begin(first_manual.ticket, now), None);
    assert_eq!(
        policy.begin(replacement.ticket, now),
        Some(Action::Recreate)
    );
    assert_eq!(policy.attempt_count(), 1);
}

#[test]
fn reliability_webview_manual_retry_waits_for_old_worker_and_ignores_its_completion_result() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererExited, now);
    let old = begin_next(&mut policy);
    policy.manual_retry(old.due_at);
    let replacement = policy.pending.unwrap();
    assert!(policy.in_flight());
    assert!(policy.next_attempt().is_none());
    assert_eq!(policy.begin(replacement.ticket, old.due_at), None);
    assert!(!policy.complete(old.ticket, AttemptResult::Failed, old.due_at));
    assert_eq!(policy.attempt_count(), 0);
    assert_eq!(policy.next_attempt(), Some(replacement));
    begin_next(&mut policy);
    assert!(!policy.complete(old.ticket, AttemptResult::Succeeded, old.due_at));
    assert!(policy.in_flight());
    assert!(policy.complete(
        replacement.ticket,
        AttemptResult::Succeeded,
        replacement.due_at
    ));
    assert!(!policy.keep_alive());
}

#[test]
fn reliability_webview_shutdown_cancels_pending_and_inflight_and_cannot_be_reopened() {
    let now = Instant::now();
    let mut policy = Policy::default();
    policy.record_failure(FailureKind::RendererExited, now);
    let old = begin_next(&mut policy);
    policy.record_failure(FailureKind::BrowserExited, old.due_at);
    let pending = policy.pending.unwrap();
    policy.shutdown();
    assert!(!policy.keep_alive());
    assert!(!policy.paused());
    assert!(!policy.in_flight());
    assert!(policy.next_attempt().is_none());
    assert_eq!(policy.begin(pending.ticket, pending.due_at), None);
    assert!(!policy.complete(old.ticket, AttemptResult::Succeeded, pending.due_at));
    assert!(!policy.manual_retry(now + QUIET_WINDOW));
    assert!(!policy.record_failure(FailureKind::BrowserExited, now + QUIET_WINDOW));
    assert!(policy.pending.is_none());
}
