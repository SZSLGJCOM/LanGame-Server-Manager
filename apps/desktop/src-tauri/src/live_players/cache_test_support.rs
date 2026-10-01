use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use app_core::{
    ModulePlayerListSource, RuntimeLivePlayerEntry, RuntimeLivePlayerIdentifier,
    RuntimeLivePlayerIssue, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
    RuntimeLivePlayerStatus, RuntimePlayerIdentityKind,
};

use super::cache::{
    CachedLivePlayerSnapshot, LivePlayerCacheClock, LivePlayerCacheKey, LivePlayerRegistry,
};
use super::contract::{clear_refresh_commit_race_hook, install_refresh_commit_race_hook};

pub(super) const INSTANCE_A: &str = "instance-a";
pub(super) const RUN_A: &str = "run-a";
pub(super) const FINGERPRINT_A: &str = "fingerprint-a";
pub(super) const PLAYER_KEY: &str = "player-1";
pub(super) const ACTION_ID: &str = "kick_userid";
pub(super) const CANONICAL_TARGET: &str = "KU_1234567890";

#[derive(Debug)]
pub(super) struct TestClock {
    unix_ms: AtomicU64,
    monotonic_ms: AtomicU64,
}

impl TestClock {
    pub(super) fn new(now_unix_ms: u64) -> Self {
        Self {
            unix_ms: AtomicU64::new(now_unix_ms),
            monotonic_ms: AtomicU64::new(now_unix_ms),
        }
    }

    pub(super) fn set(&self, now_unix_ms: u64) {
        self.unix_ms.store(now_unix_ms, Ordering::SeqCst);
        self.monotonic_ms.fetch_max(now_unix_ms, Ordering::SeqCst);
    }

    pub(super) fn set_unix_ms(&self, now_unix_ms: u64) {
        self.unix_ms.store(now_unix_ms, Ordering::SeqCst);
    }
}

impl LivePlayerCacheClock for TestClock {
    fn now_unix_ms(&self) -> u64 {
        self.unix_ms.load(Ordering::SeqCst)
    }

    fn now_monotonic_ms(&self) -> u64 {
        self.monotonic_ms.load(Ordering::SeqCst)
    }
}

#[test]
fn live_player_cache_wall_clock_rollback_cannot_revive_authorization() {
    let clock = Arc::new(TestClock::new(10_000));
    let registry = registry(clock.clone());
    let key = key(INSTANCE_A, RUN_A, FINGERPRINT_A);
    registry.store_success(
        &key,
        cached(ready_snapshot(INSTANCE_A, "snapshot", 10_000), 10_000),
    );
    clock.set(40_000);
    assert!(registry.read(&key).expect("expired").stale);
    clock.set_unix_ms(5_000);

    assert!(registry.read(&key).expect("still expired").stale);
    assert!(
        registry
            .resolve_action_binding(&key, "snapshot", PLAYER_KEY, ACTION_ID)
            .is_none()
    );
}

#[tokio::test]
async fn invalidation_between_current_check_and_commit_revokes_old_response() {
    const RACE_INSTANCE: &str = "cache-commit-race-instance";
    let registry = registry(Arc::new(TestClock::new(100_000)));
    let old_key = key(RACE_INSTANCE, RUN_A, FINGERPRINT_A);
    let new_key = key(RACE_INSTANCE, "run-b", FINGERPRINT_A);
    let hook = install_refresh_commit_race_hook(RACE_INSTANCE);
    let refresh = tokio::spawn({
        let registry = registry.clone();
        async move {
            registry
                .refresh_or_join(old_key, 100_000, || async {
                    Ok(cached(
                        ready_snapshot(RACE_INSTANCE, "old-response", 100_000),
                        100_000,
                    ))
                })
                .await
        }
    });
    hook.before_commit.notified().await;
    let invalidation = std::thread::spawn({
        let registry = registry.clone();
        move || registry.invalidate_instance(RACE_INSTANCE)
    });
    let map_removed = hook.map_removed.clone();
    tokio::task::spawn_blocking(move || map_removed.wait())
        .await
        .expect("map-removal barrier");
    registry.store_success(
        &new_key,
        cached(
            ready_snapshot(RACE_INSTANCE, "new-current", 100_001),
            100_001,
        ),
    );
    hook.continue_commit.notify_one();
    let old_response = refresh.await.expect("old refresh task");
    let continue_invalidation = hook.continue_invalidation.clone();
    tokio::task::spawn_blocking(move || continue_invalidation.wait())
        .await
        .expect("invalidation release barrier");
    invalidation.join().expect("invalidation thread");
    clear_refresh_commit_race_hook();

    assert_eq!(old_response.status, RuntimeLivePlayerStatus::Failed);
    assert!(!old_response.complete);
    assert!(old_response.stale);
    assert!(old_response.entries[0].available_action_ids.is_empty());
    assert_eq!(
        registry.read(&new_key).expect("new cache").snapshot_id,
        "new-current"
    );
}

pub(super) fn key(instance_id: &str, run_id: &str, fingerprint: &str) -> LivePlayerCacheKey {
    LivePlayerCacheKey {
        instance_id: instance_id.to_owned(),
        run_id: run_id.to_owned(),
        security_contract_fingerprint: fingerprint.to_owned(),
        refresh_interval_ms: 30_000,
    }
}

pub(super) fn ready_snapshot(
    instance_id: &str,
    snapshot_id: &str,
    observed_at: u64,
) -> RuntimeLivePlayerSnapshot {
    RuntimeLivePlayerSnapshot {
        snapshot_id: snapshot_id.to_owned(),
        instance_id: instance_id.to_owned(),
        status: RuntimeLivePlayerStatus::Ready,
        source: Some(ModulePlayerListSource::StructuredLog),
        observed_at_unix_ms: Some(observed_at),
        expires_at_unix_ms: None,
        complete: true,
        truncated: false,
        stale: false,
        current_players: Some(1),
        max_players: Some(8),
        entries: vec![RuntimeLivePlayerEntry {
            player_key: PLAYER_KEY.to_owned(),
            display_name: String::from("Host Alice"),
            identifiers: vec![RuntimeLivePlayerIdentifier {
                kind: RuntimePlayerIdentityKind::KleiUserId,
                value: CANONICAL_TARGET.to_owned(),
                stable: true,
            }],
            available_action_ids: vec![ACTION_ID.to_owned()],
            ping_ms: None,
            session_started_at_unix_ms: None,
            role: Some(String::from("admin")),
            attributes: Vec::new(),
        }],
        issue: None,
    }
}

pub(super) fn failed_snapshot(instance_id: &str, snapshot_id: &str) -> RuntimeLivePlayerSnapshot {
    RuntimeLivePlayerSnapshot {
        snapshot_id: snapshot_id.to_owned(),
        instance_id: instance_id.to_owned(),
        status: RuntimeLivePlayerStatus::Failed,
        source: Some(ModulePlayerListSource::StructuredLog),
        observed_at_unix_ms: None,
        expires_at_unix_ms: None,
        complete: false,
        truncated: false,
        stale: false,
        current_players: None,
        max_players: None,
        entries: Vec::new(),
        issue: Some(RuntimeLivePlayerIssue {
            code: RuntimeLivePlayerIssueCode::CollectionTimeout,
            setting_keys: Vec::new(),
            summary: String::from("Player collection timed out."),
        }),
    }
}

pub(super) fn cached(
    snapshot: RuntimeLivePlayerSnapshot,
    collected_at: u64,
) -> CachedLivePlayerSnapshot {
    CachedLivePlayerSnapshot {
        public_snapshot: snapshot,
        private_action_bindings: HashMap::from([(
            (PLAYER_KEY.to_owned(), ACTION_ID.to_owned()),
            CANONICAL_TARGET.to_owned(),
        )]),
        collected_at,
    }
}

pub(super) fn registry(clock: Arc<TestClock>) -> LivePlayerRegistry {
    LivePlayerRegistry::with_clock(clock)
}

#[test]
fn live_player_cache_public_serialization_cannot_expose_private_bindings() {
    let cached = cached(ready_snapshot(INSTANCE_A, "snapshot-a", 10_000), 10_000);
    let serialized = serde_json::to_string(&cached.public_snapshot).expect("public snapshot JSON");

    assert!(serialized.contains(CANONICAL_TARGET));
    for forbidden in [
        "private_action_bindings",
        "canonical_target",
        "rendered_target",
        "encoded_target",
    ] {
        assert!(!serialized.contains(forbidden), "leaked field: {forbidden}");
    }
}
