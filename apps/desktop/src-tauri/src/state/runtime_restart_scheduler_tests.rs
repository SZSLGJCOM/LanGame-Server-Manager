use super::*;

fn request(instance_id: &str, backoff: Duration) -> RuntimeRestartScheduleRequest {
    RuntimeRestartScheduleRequest {
        instance_id: instance_id.to_owned(),
        instance_name: format!("Server {instance_id}"),
        backoff,
        recent_crash_count: 1,
        exit_code: Some(7),
    }
}

#[test]
fn reliability_restart_due_boundary_and_countdown_use_the_same_injected_time() {
    let now = Instant::now();
    let backoff = Duration::from_secs(300);
    let mut scheduler = RuntimeRestartScheduler::default();
    let scheduled = scheduler
        .schedule_at(request("world", backoff), now)
        .unwrap();
    assert_eq!(scheduled.scheduled_at, now);
    assert_eq!(scheduled.due_at, now + backoff);
    assert_eq!(scheduler.next_restart_delay_at(now), backoff);
    assert_eq!(scheduler.next_restart_at(now).unwrap().delay_ms, 300_000);
    let before_due = scheduled.due_at - Duration::from_nanos(1);
    assert!(scheduler.take_due_at(before_due).is_empty());
    assert_eq!(
        scheduler.next_restart_delay_at(before_due),
        Duration::from_nanos(1)
    );
    // Display milliseconds may round down, but the launch still waits for due_at.
    assert_eq!(
        scheduler
            .pending_restart_for_at("world", before_due)
            .unwrap()
            .delay_ms,
        0
    );
    let due = scheduler.take_due_at(scheduled.due_at);
    assert_eq!(due.len(), 1);
    assert!(scheduler.entry_is_current(&due[0]));
    assert!(scheduler.take_due_at(scheduled.due_at).is_empty());
    assert!(scheduler.next_restart_at(scheduled.due_at).is_none());
}

#[test]
fn reliability_restart_years_of_elapsed_time_do_not_replay_or_multiply_tickets() {
    let now = Instant::now();
    let later = now + Duration::from_secs(5 * 365 * 24 * 60 * 60);
    let mut scheduler = RuntimeRestartScheduler::default();
    for (id, seconds) in [("first", 1), ("second", 300), ("cancelled", 60)] {
        scheduler
            .schedule_at(request(id, Duration::from_secs(seconds)), now)
            .unwrap();
    }
    scheduler.cancel("cancelled").unwrap();
    assert_eq!(scheduler.next_restart_delay_at(later), Duration::ZERO);
    let due = scheduler.take_due_at(later);
    let mut ids: Vec<_> = due.iter().map(|entry| entry.instance_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["first", "second"]);
    assert_eq!(scheduler.pending_count(), 0);
    for entry in &due {
        assert!(scheduler.entry_is_current(entry));
        scheduler.finish_restart(entry);
    }
    assert!(scheduler.take_due_at(later).is_empty());
    assert!(
        scheduler
            .take_due_at(later + Duration::from_secs(365 * 24 * 60 * 60))
            .is_empty()
    );
    assert!(!scheduler.restart_allowed("first"));
}

#[test]
fn reliability_restart_cancelled_pending_ticket_cannot_shorten_replacement_backoff() {
    let now = Instant::now();
    let mut scheduler = RuntimeRestartScheduler::default();
    scheduler
        .schedule_at(request("world", Duration::from_secs(30)), now)
        .unwrap();
    let cancelled = scheduler.cancel("world").unwrap();
    assert!(scheduler.cancel("world").is_none());
    let replacement = scheduler
        .schedule_at(
            request("world", Duration::from_secs(300)),
            now + Duration::from_secs(10),
        )
        .unwrap();
    assert!(scheduler.take_due_at(cancelled.due_at).is_empty());
    assert_eq!(scheduler.pending_count(), 1);
    assert_eq!(
        scheduler.next_restart_delay_at(cancelled.due_at),
        Duration::from_secs(280)
    );
    let due = scheduler.take_due_at(replacement.due_at);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].scheduled_at, replacement.scheduled_at);
}

#[test]
fn reliability_restart_stopped_flight_stays_invalid_after_time_jump_and_manual_start() {
    let now = Instant::now();
    let later = now + Duration::from_secs(365 * 24 * 60 * 60);
    let mut scheduler = RuntimeRestartScheduler::default();
    scheduler
        .schedule_at(request("world", Duration::ZERO), now)
        .unwrap();
    let old = scheduler.take_due_at(now).pop().unwrap();
    scheduler.request_stop("world");
    assert!(!scheduler.entry_is_current(&old));
    assert!(
        scheduler
            .schedule_at(request("world", Duration::ZERO), later)
            .is_none()
    );
    scheduler.reset_for_manual_start("world");
    scheduler
        .schedule_at(request("world", Duration::ZERO), later)
        .unwrap();
    assert!(
        scheduler.take_due_at(later).is_empty(),
        "the old owner has not left yet"
    );
    assert!(!scheduler.entry_is_current(&old));
    scheduler.finish_restart(&old);
    let replacement = scheduler.take_due_at(later).pop().unwrap();
    scheduler.finish_restart(&old);
    assert!(
        scheduler.entry_is_current(&replacement),
        "stale cleanup must preserve the new flight"
    );
    assert!(!scheduler.entry_is_current(&old));
    scheduler.finish_restart(&replacement);
    assert!(scheduler.take_due_at(later).is_empty());
}

#[test]
fn reliability_restart_duplicate_exit_cannot_postpone_an_existing_deadline() {
    let now = Instant::now();
    let mut scheduler = RuntimeRestartScheduler::default();
    let first = scheduler
        .schedule_at(request("world", Duration::from_secs(30)), now)
        .unwrap();
    assert!(
        scheduler
            .schedule_at(
                request("world", Duration::from_secs(300)),
                now + Duration::from_secs(29)
            )
            .is_none()
    );
    assert_eq!(scheduler.pending_count(), 1);
    let due = scheduler.take_due_at(first.due_at);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].scheduled_at, now);
    assert_eq!(due[0].due_at, first.due_at);
}
