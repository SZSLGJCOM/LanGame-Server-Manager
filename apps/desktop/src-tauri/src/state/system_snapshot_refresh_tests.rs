use super::*;
use crate::state::{TimedCache, spawn_timed_cache_refresh};
use std::future::{Future, poll_fn};
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn system_snapshot_refresh_joiners_wait_for_one_worker_and_see_its_stored_value() {
    let cache = Arc::new(Mutex::new(TimedCache::default()));
    cache.lock().unwrap().store(7_u32);
    assert!(cache.lock().unwrap().try_begin_refresh());
    let signal = Arc::new(SystemSnapshotRefresh::default());
    let first = signal.subscribe();
    let generation = signal.begin();
    let second = signal.subscribe();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    signal.track(
        generation,
        spawn_timed_cache_refresh(Arc::clone(&cache), "test", async move {
            release_rx.await.unwrap();
            Ok(42_u32)
        }),
    );
    let mut first = Box::pin(first.wait(generation, deadline()));
    let mut second = Box::pin(second.wait(generation, deadline()));
    assert!(
        poll_fn(|cx| Poll::Ready(first.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(
        poll_fn(|cx| Poll::Ready(second.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    assert!(!cache.lock().unwrap().try_begin_refresh());
    release_tx.send(()).unwrap();
    let (first, second) = tokio::join!(first, second);
    first.unwrap();
    second.unwrap();
    assert_eq!(cache.lock().unwrap().latest(), Some(42));
}

#[tokio::test]
async fn system_snapshot_refresh_completed_before_subscription_is_observed_immediately() {
    let signal = SystemSnapshotRefresh::default();
    let generation = signal.begin();
    signal.complete(generation, Ok(()));
    let mut wait = Box::pin(signal.subscribe().wait(generation, deadline()));
    assert!(matches!(
        poll_fn(|cx| Poll::Ready(wait.as_mut().poll(cx))).await,
        Poll::Ready(Ok(()))
    ));
}

#[tokio::test]
async fn system_snapshot_refresh_late_previous_completion_cannot_finish_the_next_generation() {
    let signal = SystemSnapshotRefresh::default();
    let previous = signal.begin();
    let current = signal.begin();
    let mut wait = Box::pin(signal.subscribe().wait(current, deadline()));
    signal.complete(
        previous,
        Err(RefreshFailure::Collection("previous probe failed".into())),
    );
    assert!(
        poll_fn(|cx| Poll::Ready(wait.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    signal.complete(current, Ok(()));
    wait.await.unwrap();
    // Even a late publisher from the older worker cannot replace newer success.
    signal.complete(
        previous,
        Err(RefreshFailure::Collection("late failure".into())),
    );
    signal.subscribe().wait(current, deadline()).await.unwrap();
}

#[tokio::test]
async fn system_snapshot_refresh_waiter_timeout_does_not_release_or_cancel_the_worker() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    assert!(cache.lock().unwrap().try_begin_refresh());
    let signal = Arc::new(SystemSnapshotRefresh::default());
    let generation = signal.begin();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    signal.track(
        generation,
        spawn_timed_cache_refresh(Arc::clone(&cache), "test", async move {
            release_rx.await.unwrap();
            Ok(42)
        }),
    );
    let error = signal
        .subscribe()
        .wait(generation, Instant::now())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("did not finish"));
    assert!(!cache.lock().unwrap().try_begin_refresh());
    release_tx.send(()).unwrap();
    signal
        .subscribe()
        .wait(generation, deadline())
        .await
        .unwrap();
    assert_eq!(cache.lock().unwrap().latest(), Some(42));
}

#[tokio::test]
async fn system_snapshot_refresh_failure_preserves_cause_and_allows_a_new_generation() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    assert!(cache.lock().unwrap().try_begin_refresh());
    let signal = Arc::new(SystemSnapshotRefresh::default());
    let generation = signal.begin();
    signal.track(
        generation,
        spawn_timed_cache_refresh(Arc::clone(&cache), "test", async {
            Err("storage paths unavailable".into())
        }),
    );
    assert_eq!(
        signal
            .subscribe()
            .wait(generation, deadline())
            .await
            .unwrap_err()
            .to_string(),
        "storage paths unavailable"
    );
    assert!(cache.lock().unwrap().latest().is_none());
    assert!(cache.lock().unwrap().try_begin_refresh());
    assert!(signal.begin() > generation);
}

#[tokio::test]
async fn system_snapshot_refresh_worker_failure_is_not_hidden_by_the_cached_sample() {
    let cache = Arc::new(Mutex::new(TimedCache::<u32>::default()));
    cache.lock().unwrap().store(7);
    assert!(cache.lock().unwrap().try_begin_refresh());
    let signal = Arc::new(SystemSnapshotRefresh::default());
    let generation = signal.begin();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let worker = spawn_timed_cache_refresh(Arc::clone(&cache), "test", async move {
        started_tx.send(()).unwrap();
        std::future::pending::<Result<u32, String>>().await
    });
    started_rx.await.unwrap();
    worker.abort();
    signal.track(generation, worker);
    let failure = signal
        .subscribe()
        .wait(generation, deadline())
        .await
        .unwrap_err();
    assert!(failure.is_worker_failure());
    assert!(
        failure
            .to_string()
            .contains("system snapshot worker failed")
    );
    assert_eq!(cache.lock().unwrap().latest(), Some(7));
    assert!(cache.lock().unwrap().try_begin_refresh());
}
