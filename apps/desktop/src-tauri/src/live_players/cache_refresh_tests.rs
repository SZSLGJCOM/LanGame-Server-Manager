use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;

use super::cache_test_support::*;

#[tokio::test]
async fn player_count_cache_refreshes_only_after_monotonic_expiry() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let calls = AtomicUsize::new(0);
    for (now, expected_calls) in [(10_000, 1), (20_000, 1), (40_000, 2)] {
        clock.set(now);
        clock.set_unix_ms(1); // Wall clock rollback cannot extend freshness.
        let snapshot = registry
            .refresh_if_expired(key.clone(), now, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(cached(ready_snapshot(INSTANCE_A, "count", now), now))
            })
            .await;
        assert!(!snapshot.stale);
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
}

#[tokio::test]
async fn player_count_cache_reuses_failures_without_retrying_each_poll() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "old", 10_000), 10_000),
    );
    clock.set(40_000);
    let failure = registry
        .refresh_if_expired(key.clone(), 40_000, || async {
            Err(failed_snapshot(INSTANCE_A, "failed").into())
        })
        .await;
    assert!(failure.stale);
    assert_eq!(failure.status, app_core::RuntimeLivePlayerStatus::Failed);
    clock.set(45_000);
    let repeated = registry
        .refresh_if_expired(key, 45_000, || async {
            panic!("fresh failed attempts must not be retried by count polling")
        })
        .await;
    assert_eq!(repeated.snapshot_id, failure.snapshot_id);
    assert!(repeated.stale);
}

#[tokio::test]
async fn player_count_cache_joins_player_center_refresh_and_later_callers() {
    let registry = registry(Arc::new(TestClock::new(10_000)));
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let started = tokio::sync::Notify::new();
    let release = tokio::sync::Notify::new();
    let mut manual = Box::pin(registry.refresh_or_join(key.clone(), 10_000, || async {
        started.notify_one();
        release.notified().await;
        Ok(cached(ready_snapshot(INSTANCE_A, "shared", 10_000), 10_000))
    }));
    std::future::poll_fn(|context| {
        assert!(matches!(manual.as_mut().poll(context), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    started.notified().await;
    let mut overview = Box::pin(registry.refresh_if_expired(key.clone(), 10_000, || async {
        panic!("overview must share the in-flight player center collection")
    }));
    std::future::poll_fn(|context| {
        assert!(matches!(overview.as_mut().poll(context), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    release.notify_one();
    let (manual, overview) = tokio::join!(manual, overview);
    assert_eq!(manual.snapshot_id, overview.snapshot_id);
    let system = registry
        .refresh_if_expired(key, 10_000, || async {
            panic!("later System collection must also reuse the fresh result")
        })
        .await;
    assert_eq!(system.snapshot_id, "shared");
}
