use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Poll;
use std::time::Duration;

use app_core::RuntimeLivePlayerStatus;

use super::cache::LivePlayerCollectionResult;
use super::cache_test_support::*;

#[test]
fn live_player_cache_future_size_is_independent_of_collector_state() {
    let registry = registry(Arc::new(TestClock::new(10_000)));
    let cache_key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let small = registry.refresh_or_join(cache_key.clone(), 10_000, || async {
        std::future::pending::<LivePlayerCollectionResult>().await
    });
    let large = registry.refresh_or_join(cache_key, 10_000, || async {
        let state = [0_u8; 64 * 1024];
        std::future::pending::<()>().await;
        std::hint::black_box(&state);
        Ok(cached(
            ready_snapshot(INSTANCE_A, "large-collector", 10_000),
            10_000,
        ))
    });

    assert_eq!(std::mem::size_of_val(&small), std::mem::size_of_val(&large));
}

#[tokio::test]
async fn live_player_cache_read_returns_fresh_snapshot_without_collecting() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "snapshot-a", 10_000), 10_000),
    );
    clock.set(20_000);
    let snapshot = registry.read(&key).expect("fresh cached snapshot");

    assert_eq!(snapshot.snapshot_id, "snapshot-a");
}

#[tokio::test]
async fn live_player_cache_explicit_refresh_collects_even_with_fresh_cache() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "snapshot-a", 10_000), 10_000),
    );
    clock.set(20_000);
    let calls = Arc::new(AtomicUsize::new(0));

    let snapshot = registry
        .refresh_or_join(key, 20_000, {
            let calls = calls.clone();
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(cached(
                    ready_snapshot(INSTANCE_A, "snapshot-b", 20_000),
                    20_000,
                ))
            }
        })
        .await;

    assert_eq!(snapshot.snapshot_id, "snapshot-b");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn live_player_cache_coalesces_same_instance_refreshes() {
    let clock = Arc::new(TestClock::new(50_000));
    let registry = registry(clock);
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let calls = Arc::new(AtomicUsize::new(0));
    let collector_started = Arc::new(tokio::sync::Notify::new());
    let release_collector = Arc::new(tokio::sync::Notify::new());

    let first = tokio::spawn({
        let registry = registry.clone();
        let key = key.clone();
        let calls = calls.clone();
        let collector_started = collector_started.clone();
        let release_collector = release_collector.clone();
        async move {
            registry
                .refresh_or_join(key, 50_000, move || async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    collector_started.notify_one();
                    release_collector.notified().await;
                    Ok(cached(ready_snapshot(INSTANCE_A, "shared", 50_000), 50_000))
                })
                .await
        }
    });
    collector_started.notified().await;
    let mut second = Box::pin(registry.refresh_or_join(key, 50_000, {
        let calls = calls.clone();
        move || async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(cached(
                ready_snapshot(INSTANCE_A, "duplicate", 50_000),
                50_000,
            ))
        }
    }));
    std::future::poll_fn(|context| {
        assert!(matches!(second.as_mut().poll(context), Poll::Pending));
        Poll::Ready(())
    })
    .await;
    release_collector.notify_one();

    let (first, second) = tokio::join!(first, second.as_mut());
    assert_eq!(first.expect("first refresh").snapshot_id, "shared");
    assert_eq!(second.snapshot_id, "shared");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn live_player_cache_refreshes_different_instances_concurrently() {
    let clock = Arc::new(TestClock::new(70_000));
    let registry = registry(clock);
    let active_collectors = Arc::new(AtomicUsize::new(0));
    let max_collectors = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let refresh = |instance_id: &'static str| {
        let registry = registry.clone();
        let active_collectors = active_collectors.clone();
        let max_collectors = max_collectors.clone();
        let barrier = barrier.clone();
        tokio::spawn(async move {
            registry
                .refresh_or_join(
                    key(instance_id, RUN_A, FINGERPRINT_A),
                    70_000,
                    move || async move {
                        let active = active_collectors.fetch_add(1, Ordering::SeqCst) + 1;
                        max_collectors.fetch_max(active, Ordering::SeqCst);
                        barrier.wait().await;
                        active_collectors.fetch_sub(1, Ordering::SeqCst);
                        Ok(cached(
                            ready_snapshot(instance_id, instance_id, 70_000),
                            70_000,
                        ))
                    },
                )
                .await
        })
    };

    let (first, second) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(refresh(INSTANCE_A), refresh("instance-b"))
    })
    .await
    .expect("different instances must not share one refresh gate");
    assert_eq!(first.expect("first instance").snapshot_id, INSTANCE_A);
    assert_eq!(second.expect("second instance").snapshot_id, "instance-b");
    assert_eq!(max_collectors.load(Ordering::SeqCst), 2);
}

#[test]
fn live_player_cache_old_key_reads_and_failures_do_not_mutate_current_key() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock);
    let current = key(INSTANCE_A, "run-b", "fingerprint-b");
    let old = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &current,
        cached(ready_snapshot(INSTANCE_A, "current", 10_000), 10_000),
    );

    assert!(registry.read(&old).is_none());
    let rejected = registry.store_failure(
        &old,
        failed_snapshot(INSTANCE_A, "expired-old-failure"),
        10_001,
    );

    assert_eq!(rejected.status, RuntimeLivePlayerStatus::Failed);
    assert_eq!(
        registry.read(&current).expect("current key").snapshot_id,
        "current"
    );
}

#[tokio::test]
async fn live_player_cache_expired_refresh_generation_cannot_overwrite_new_key() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock);
    let old = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let current = key(INSTANCE_A, "run-b", "fingerprint-b");
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let refresh = tokio::spawn({
        let registry = registry.clone();
        let old = old.clone();
        let started = started.clone();
        let release = release.clone();
        async move {
            registry
                .refresh_or_join(old, 10_000, move || async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(cached(
                        ready_snapshot(INSTANCE_A, "expired", 10_000),
                        10_000,
                    ))
                })
                .await
        }
    });
    started.notified().await;
    registry.store_success(
        &current,
        cached(ready_snapshot(INSTANCE_A, "current", 10_001), 10_001),
    );
    release.notify_one();
    assert!(refresh.await.expect("expired refresh").stale);
    assert_eq!(
        registry.read(&current).expect("current key").snapshot_id,
        "current"
    );

    let calls = Arc::new(AtomicUsize::new(0));
    let rejected = registry
        .refresh_or_join(old, 10_002, {
            let calls = calls.clone();
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(cached(
                    ready_snapshot(INSTANCE_A, "late-old", 10_002),
                    10_002,
                ))
            }
        })
        .await;
    assert!(rejected.stale);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        registry.read(&current).expect("current key").snapshot_id,
        "current"
    );
}

#[tokio::test]
async fn live_player_cache_long_collection_returns_the_same_stale_projection_as_read() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let refreshed = registry
        .refresh_or_join(key.clone(), 10_000, || async {
            clock.set(40_001);
            Ok(cached(ready_snapshot(INSTANCE_A, "late", 10_000), 10_000))
        })
        .await;
    let read = registry.read(&key).expect("late snapshot");

    assert!(refreshed.stale && read.stale);
    assert!(refreshed.entries[0].available_action_ids.is_empty());
    assert_eq!(refreshed.snapshot_id, read.snapshot_id);
    assert_eq!(refreshed.status, read.status);
    assert_eq!(
        refreshed.entries[0].available_action_ids,
        read.entries[0].available_action_ids
    );
}

#[test]
fn live_player_cache_invalidation_removes_public_and_private_state() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock);
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "snapshot-a", 10_000), 10_000),
    );

    assert_eq!(
        registry.resolve_action_binding(&key, "snapshot-a", PLAYER_KEY, ACTION_ID),
        Some(CANONICAL_TARGET.to_owned())
    );
    registry.invalidate_instance(INSTANCE_A);
    assert!(registry.read(&key).is_none());
    assert!(
        registry
            .resolve_action_binding(&key, "snapshot-a", PLAYER_KEY, ACTION_ID)
            .is_none()
    );
}

#[test]
fn live_player_cache_failure_keeps_stale_rows_but_revokes_actions() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "success", 10_000), 10_000),
    );
    clock.set(45_000);

    let failed = registry.store_failure(&key, failed_snapshot(INSTANCE_A, "failure"), 45_000);

    assert_eq!(failed.snapshot_id, "failure");
    assert_eq!(failed.status, RuntimeLivePlayerStatus::Failed);
    assert!(failed.stale);
    assert!(!failed.complete);
    assert_eq!(failed.observed_at_unix_ms, Some(10_000));
    assert_eq!(failed.expires_at_unix_ms, Some(75_000));
    assert_eq!(failed.entries.len(), 1);
    assert!(failed.entries[0].available_action_ids.is_empty());
    assert!(
        registry
            .resolve_action_binding(&key, "failure", PLAYER_KEY, ACTION_ID)
            .is_none()
    );
}

#[test]
fn live_player_cache_failure_without_previous_rows_is_current_and_preserves_misconfiguration() {
    let clock = Arc::new(TestClock::new(45_000));
    let registry = registry(clock);
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let mut failure = failed_snapshot(INSTANCE_A, "misconfigured");
    failure.status = RuntimeLivePlayerStatus::Misconfigured;

    let stored = registry.store_failure(&key, failure, 45_000);

    assert_eq!(stored.status, RuntimeLivePlayerStatus::Misconfigured);
    assert!(!stored.stale);
    assert!(!stored.complete);
    assert!(stored.entries.is_empty());
    assert_eq!(stored.expires_at_unix_ms, Some(75_000));
    let repeated = registry.store_failure(
        &key,
        failed_snapshot(INSTANCE_A, "repeated-failure"),
        45_001,
    );
    assert!(!repeated.stale, "a failed attempt is not stale player data");
    assert!(repeated.observed_at_unix_ms.is_none());
    registry.invalidate_instance(INSTANCE_A);
    let stored = registry.store_failure(&key, failed_snapshot(INSTANCE_A, "fresh-failure"), 45_001);
    assert_eq!(stored.status, RuntimeLivePlayerStatus::Failed);
    assert!(!stored.stale);
}

#[tokio::test]
async fn live_player_cache_preserves_unsupported_capability_and_discards_previous_roster() {
    for had_previous_roster in [false, true] {
        let clock = Arc::new(TestClock::new(10_000));
        let registry = registry(clock.clone());
        let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
        if had_previous_roster {
            registry.store_success(
                &key,
                cached(ready_snapshot(INSTANCE_A, "success", 10_000), 10_000),
            );
        }
        clock.set(20_000);
        let snapshot = registry
            .refresh_or_join(key.clone(), 20_000, || async {
                Err(super::service::unsupported_snapshot(INSTANCE_A, "unsupported".into()).into())
            })
            .await;

        assert_eq!(snapshot.status, RuntimeLivePlayerStatus::Unsupported);
        assert!(!snapshot.stale);
        assert!(!snapshot.complete);
        assert!(snapshot.entries.is_empty());
        assert_eq!(snapshot.current_players, None);
        assert_eq!(snapshot.max_players, None);
        assert_eq!(snapshot.observed_at_unix_ms, None);
        assert!(
            registry
                .resolve_action_binding(&key, "success", PLAYER_KEY, ACTION_ID)
                .is_none()
        );
        assert_eq!(
            registry.read(&key).expect("cached capability").status,
            RuntimeLivePlayerStatus::Unsupported
        );

        let next =
            registry.store_failure(&key, failed_snapshot(INSTANCE_A, "later-failure"), 20_001);
        assert!(!next.stale);
        assert!(next.entries.is_empty());
        assert_eq!(next.current_players, None);
    }
}

#[test]
fn live_player_cache_does_not_promote_truncated_rows_to_stale_success() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock);
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let mut truncated = ready_snapshot(INSTANCE_A, "truncated", 10_000);
    truncated.truncated = true;
    registry.store_success(&key, cached(truncated, 10_000));

    let failure =
        registry.store_failure(&key, failed_snapshot(INSTANCE_A, "after-truncated"), 40_001);

    assert!(!failure.stale);
    assert!(failure.entries.is_empty());
    assert!(failure.observed_at_unix_ms.is_none());
}

#[test]
fn live_player_cache_never_authorizes_non_authoritative_snapshots() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);

    for (index, (status, complete, truncated, stale)) in [
        (RuntimeLivePlayerStatus::Ready, false, false, false),
        (RuntimeLivePlayerStatus::Ready, true, true, false),
        (RuntimeLivePlayerStatus::Ready, true, false, true),
        (RuntimeLivePlayerStatus::Stopped, true, false, false),
        (RuntimeLivePlayerStatus::Unsupported, true, false, false),
        (RuntimeLivePlayerStatus::Misconfigured, true, false, false),
        (RuntimeLivePlayerStatus::Failed, true, false, false),
        (RuntimeLivePlayerStatus::Refreshing, true, false, false),
    ]
    .into_iter()
    .enumerate()
    {
        let snapshot_id = format!("unsafe-{index}");
        let mut snapshot = ready_snapshot(INSTANCE_A, &snapshot_id, 10_000);
        snapshot.status = status;
        snapshot.complete = complete;
        snapshot.truncated = truncated;
        snapshot.stale = stale;
        let stored = registry.store_success(&key, cached(snapshot, 10_000));
        assert!(stored.entries[0].available_action_ids.is_empty());
        assert!(
            registry
                .resolve_action_binding(&key, &snapshot_id, PLAYER_KEY, ACTION_ID)
                .is_none()
        );
    }

    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "expired", 10_000), 10_000),
    );
    clock.set(40_000);
    let expired = registry.read(&key).expect("expired display projection");
    assert!(expired.stale);
    assert!(expired.entries[0].available_action_ids.is_empty());
    assert!(
        registry
            .resolve_action_binding(&key, "expired", PLAYER_KEY, ACTION_ID)
            .is_none()
    );
}

#[tokio::test]
async fn live_player_cache_cancellation_releases_instance_gate() {
    let clock = Arc::new(TestClock::new(90_000));
    let registry = registry(clock);
    let current_key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    let started = Arc::new(tokio::sync::Notify::new());

    let hanging = tokio::spawn({
        let registry = registry.clone();
        let current_key = current_key.clone();
        let started = started.clone();
        async move {
            registry
                .refresh_or_join(current_key, 90_000, move || async move {
                    started.notify_one();
                    std::future::pending::<LivePlayerCollectionResult>().await
                })
                .await
        }
    });
    started.notified().await;
    hanging.abort();
    let _ = hanging.await;

    let calls = Arc::new(AtomicUsize::new(0));
    let rejected = registry
        .refresh_or_join(key(INSTANCE_A, "run-b", FINGERPRINT_A), 90_001, {
            let calls = calls.clone();
            move || async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(cached(ready_snapshot(INSTANCE_A, "old", 90_001), 90_001))
            }
        })
        .await;
    assert_eq!(rejected.status, RuntimeLivePlayerStatus::Failed);
    assert!(rejected.stale);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let retry = tokio::time::timeout(
        Duration::from_secs(1),
        registry.refresh_or_join(current_key, 90_002, || async {
            Ok(cached(ready_snapshot(INSTANCE_A, "retry", 90_002), 90_002))
        }),
    )
    .await
    .expect("cancelled collector must release its per-instance gate");
    assert_eq!(retry.snapshot_id, "retry");
}

#[tokio::test]
async fn live_player_cache_refreshing_projection_revokes_actions() {
    let clock = Arc::new(TestClock::new(40_001));
    let registry = registry(clock);
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "old", 10_000), 10_000),
    );
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let refresh = tokio::spawn({
        let registry = registry.clone();
        let key = key.clone();
        let started = started.clone();
        let release = release.clone();
        async move {
            registry
                .refresh_or_join(key, 40_001, move || async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(cached(ready_snapshot(INSTANCE_A, "new", 40_001), 40_001))
                })
                .await
        }
    });
    started.notified().await;

    let refreshing = registry.read(&key).expect("refreshing projection");
    assert_eq!(refreshing.status, RuntimeLivePlayerStatus::Refreshing);
    assert!(refreshing.entries[0].available_action_ids.is_empty());
    assert!(
        registry
            .resolve_action_binding(&key, "old", PLAYER_KEY, ACTION_ID)
            .is_none()
    );

    release.notify_one();
    assert_eq!(refresh.await.expect("refresh task").snapshot_id, "new");
}
