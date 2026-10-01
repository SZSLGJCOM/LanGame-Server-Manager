use super::*;
use crate::process_exit_target::tests::Fixture;
use std::sync::mpsc;

#[test]
fn std_watchdog_forces_exit_at_deadline_and_preserves_unrelated_process() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let other_target = other.capture();
    let target = Arc::new(fixture.capture());
    let callback_target = Arc::clone(&target);
    let deadline = Instant::now() + Duration::from_millis(80);
    let (sender, receiver) = mpsc::channel();
    let watchdog = spawn_exit_watchdog(Arc::clone(&target), deadline, move |outcome| {
        sender
            .send((outcome, callback_target.is_running(), Instant::now()))
            .unwrap();
    })
    .unwrap();

    // This is an ordinary test thread with no Tokio runtime to drive the deadline.
    let (outcome, running_at_callback, completed) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("native watchdog must honor its deadline independently");
    watchdog.join().unwrap();
    assert!(outcome.forced);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(completed >= deadline, "grace period must not be shortened");
    assert!(!running_at_callback.unwrap());
    assert!(!target.is_running().unwrap());
    assert!(other_target.is_running().unwrap());
}

#[test]
fn std_watchdog_finishes_after_normal_exit_without_forcing() {
    let mut fixture = Fixture::new();
    let target = Arc::new(fixture.capture());
    let callback_target = Arc::clone(&target);
    let deadline = Instant::now() + Duration::from_secs(15);
    let (sender, receiver) = mpsc::channel();
    let watchdog = spawn_exit_watchdog(Arc::clone(&target), deadline, move |outcome| {
        sender
            .send((outcome, callback_target.is_running(), Instant::now()))
            .unwrap();
    })
    .unwrap();
    fixture.exit_normally();

    let (outcome, running_at_callback, completed) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("normal exit must finish without waiting for the force deadline");
    watchdog.join().unwrap();
    assert!(!outcome.forced);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(!running_at_callback.unwrap());
    assert!(!target.is_running().unwrap());
    assert!(completed < deadline);
}

#[test]
fn std_watchdog_enforces_an_already_expired_deadline_immediately() {
    let fixture = Fixture::new();
    let target = Arc::new(fixture.capture());
    let callback_target = Arc::clone(&target);
    let deadline = Instant::now() - Duration::from_secs(1);
    let (sender, receiver) = mpsc::channel();
    let watchdog = spawn_exit_watchdog(Arc::clone(&target), deadline, move |outcome| {
        sender
            .send((outcome, callback_target.is_running()))
            .unwrap();
    })
    .unwrap();

    let (outcome, running_at_callback) = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("an expired deadline must start native termination immediately");
    watchdog.join().unwrap();
    assert!(outcome.forced);
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(!running_at_callback.unwrap());
    assert!(!target.is_running().unwrap());
}
