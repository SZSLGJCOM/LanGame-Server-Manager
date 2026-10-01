use std::future::Future;
use std::sync::{Arc, Barrier};
use std::task::Poll;
use std::time::Duration;

use super::*;

#[tokio::test]
async fn final_exit_upgrades_existing_owner_without_starting_another_stop() {
    let state = DesktopState::default();
    let active_write = state
        .begin_storage_context_operation("controlled backup")
        .expect("admit existing write");
    assert!(state.begin_app_shutdown(false).unwrap());
    let drain = state.drain_storage_operations_for_shutdown(Duration::from_secs(5));
    tokio::pin!(drain);
    std::future::poll_fn(|context| {
        assert!(drain.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;

    assert!(!state.begin_app_shutdown(true).unwrap());
    assert!(!state.begin_app_shutdown(true).unwrap());
    assert!(state.is_final_exit_requested());
    assert!(state.begin_storage_context_operation("late write").is_err());
    drop(active_write);
    let shutdown = drain.await.expect("existing owner drains admitted storage");
    assert!(!state.release_app_shutdown_for_retry().unwrap());
    drop(shutdown);
    assert!(
        state
            .begin_storage_context_operation("failed final exit")
            .is_err()
    );
}

#[test]
fn ordinary_failure_reopens_admission_but_final_exit_does_not() {
    let state = DesktopState::default();
    assert!(state.begin_app_shutdown(false).unwrap());
    assert!(state.release_app_shutdown_for_retry().unwrap());
    let operation = state
        .begin_storage_context_operation("ordinary retry")
        .unwrap();
    drop(operation);

    assert!(state.begin_app_shutdown(true).unwrap());
    assert!(!state.release_app_shutdown_for_retry().unwrap());
    assert!(state.shutdown_in_progress.load(Ordering::SeqCst));
    assert!(state.begin_storage_context_operation("late write").is_err());
}

#[test]
fn concurrent_requests_have_one_stop_owner_and_retain_final_intent() {
    let state = Arc::new(DesktopState::default());
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                state.begin_app_shutdown(index == 0).unwrap()
            })
        })
        .collect();
    let owner_count = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|owns_shutdown| *owns_shutdown)
        .count();
    assert_eq!(owner_count, 1);
    assert!(state.is_final_exit_requested());
    assert!(!state.release_app_shutdown_for_retry().unwrap());
}
