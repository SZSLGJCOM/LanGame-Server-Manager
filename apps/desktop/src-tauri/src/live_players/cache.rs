use super::contract::{
    CacheRecord, SystemLivePlayerCacheClock, lock_unpoisoned, non_authoritative_projection,
    project_snapshot, record_is_fresh, rejected_failure, rejected_key, rejected_record,
    retain_advertised_bindings, snapshot_is_authoritative, strip_actions,
};
pub(crate) use super::contract::{
    CachedLivePlayerSnapshot, LivePlayerCacheClock, LivePlayerCacheKey,
};
use app_core::{RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

pub(crate) type LivePlayerCollectionResult =
    Result<CachedLivePlayerSnapshot, Box<RuntimeLivePlayerSnapshot>>;

#[derive(Clone)]
pub(crate) struct LivePlayerRegistry {
    inner: Arc<LivePlayerRegistryInner>,
}
struct LivePlayerRegistryInner {
    clock: Arc<dyn LivePlayerCacheClock>,
    instances: Mutex<HashMap<String, Arc<InstanceLivePlayerState>>>,
}
struct InstanceLivePlayerState {
    refresh_gate: Arc<tokio::sync::Mutex<()>>,
    data: Mutex<InstanceLivePlayerData>,
}
#[derive(Default)]
struct InstanceLivePlayerData {
    cached: Option<CacheRecord>,
    active_key: Option<LivePlayerCacheKey>,
    refreshing_generation: Option<u64>,
    next_generation: u64,
    completed_refresh_generation: u64,
}
impl Default for LivePlayerRegistry {
    fn default() -> Self {
        Self::with_clock(Arc::new(SystemLivePlayerCacheClock::default()))
    }
}
impl LivePlayerRegistry {
    pub(crate) fn with_clock(clock: Arc<dyn LivePlayerCacheClock>) -> Self {
        Self {
            inner: Arc::new(LivePlayerRegistryInner {
                clock,
                instances: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn now_unix_ms(&self) -> u64 {
        self.inner.clock.now_unix_ms()
    }

    fn now_monotonic_ms(&self) -> u64 {
        self.inner.clock.now_monotonic_ms()
    }

    pub(crate) fn read(&self, key: &LivePlayerCacheKey) -> Option<RuntimeLivePlayerSnapshot> {
        let state = self.existing_state(&key.instance_id)?;
        let now = self.now_monotonic_ms();
        let data = lock_unpoisoned(&state.data);
        let record = matching_record(&data, key)?;

        Some(project_record(
            record,
            now,
            data.refreshing_generation.is_some(),
        ))
    }
    #[cfg(test)]
    pub(crate) fn store_success(
        &self,
        key: &LivePlayerCacheKey,
        cached: CachedLivePlayerSnapshot,
    ) -> RuntimeLivePlayerSnapshot {
        let state = self.state_for(&key.instance_id);
        let now = self.now_monotonic_ms();
        store_success_as_latest(&state, key, cached, now)
    }
    #[cfg(test)]
    pub(crate) fn store_failure(
        &self,
        key: &LivePlayerCacheKey,
        failure: RuntimeLivePlayerSnapshot,
        attempted_at: u64,
    ) -> RuntimeLivePlayerSnapshot {
        let state = self.state_for(&key.instance_id);
        let now = self.now_monotonic_ms();
        store_failure_if_current(&state, key, failure, attempted_at, now)
    }
    pub(crate) async fn refresh_or_join<Collector, CollectorFuture>(
        &self,
        key: LivePlayerCacheKey,
        requested_at: u64,
        collector: Collector,
    ) -> RuntimeLivePlayerSnapshot
    where
        Collector: FnOnce() -> CollectorFuture,
        CollectorFuture: Future<Output = LivePlayerCollectionResult>,
    {
        self.refresh(key, requested_at, collector, RefreshPolicy::Explicit)
            .await
    }

    pub(crate) async fn refresh_if_expired<Collector, CollectorFuture>(
        &self,
        key: LivePlayerCacheKey,
        requested_at: u64,
        collector: Collector,
    ) -> RuntimeLivePlayerSnapshot
    where
        Collector: FnOnce() -> CollectorFuture,
        CollectorFuture: Future<Output = LivePlayerCollectionResult>,
    {
        self.refresh(key, requested_at, collector, RefreshPolicy::IfExpired)
            .await
    }

    async fn refresh<Collector, CollectorFuture>(
        &self,
        key: LivePlayerCacheKey,
        requested_at: u64,
        collector: Collector,
        policy: RefreshPolicy,
    ) -> RuntimeLivePlayerSnapshot
    where
        Collector: FnOnce() -> CollectorFuture,
        CollectorFuture: Future<Output = LivePlayerCollectionResult>,
    {
        let requested_at_monotonic = self.now_monotonic_ms();
        let state = self.state_for(&key.instance_id);
        let observed_generation = lock_unpoisoned(&state.data).completed_refresh_generation;

        let _gate = state.refresh_gate.clone().lock_owned().await;
        if matches!(policy, RefreshPolicy::IfExpired) {
            let data = lock_unpoisoned(&state.data);
            let now = self.now_monotonic_ms();
            if let Some(record) = matching_record(&data, &key)
                && record_is_fresh(record, now)
            {
                // Recheck inside the gate: a prior refresh may have completed
                // before this caller started waiting, including a failed attempt.
                return project_record(record, now, false);
            }
        }
        if let Some(snapshot) = completed_snapshot_after_join(
            &state,
            &key,
            observed_generation,
            self.now_monotonic_ms(),
        ) {
            return snapshot;
        }

        let refresh_lease = match RefreshLease::begin(state.clone(), &key) {
            Ok(lease) => lease,
            Err(rejected) => return rejected.into_snapshot(),
        };
        // Collector state varies by game and must not enlarge the registry's
        // future or every command that awaits it. Dropping it still releases
        // the refresh lease and gate with the caller.
        let result = Box::pin(collector()).await;
        let completed_at = self.now_unix_ms().max(requested_at);
        let completed_at_monotonic = self.now_monotonic_ms();
        #[cfg(test)]
        super::contract::pause_refresh_commit(&key.instance_id).await;
        // Keep registry ownership stable while committing under the instances -> data lock order.
        let instances = lock_unpoisoned(&self.inner.instances);
        if !instances
            .get(&key.instance_id)
            .is_some_and(|current| Arc::ptr_eq(current, &state))
        {
            let snapshot = match result {
                Ok(cached) => cached.public_snapshot,
                Err(failure) => *failure,
            };
            non_authoritative_projection(snapshot, RuntimeLivePlayerStatus::Failed, true)
        } else {
            match result {
                Ok(cached) => store_refresh_success(
                    &state,
                    &key,
                    refresh_lease.generation,
                    cached,
                    requested_at_monotonic,
                    completed_at_monotonic,
                ),
                Err(failure) => store_refresh_failure(
                    &state,
                    &key,
                    refresh_lease.generation,
                    *failure,
                    completed_at,
                    requested_at_monotonic,
                    completed_at_monotonic,
                ),
            }
        }
    }

    pub(crate) fn resolve_action_binding(
        &self,
        key: &LivePlayerCacheKey,
        snapshot_id: &str,
        player_key: &str,
        action_id: &str,
    ) -> Option<String> {
        let state = self.existing_state(&key.instance_id)?;
        let now = self.now_monotonic_ms();
        let data = lock_unpoisoned(&state.data);
        if data.refreshing_generation.is_some() {
            return None;
        }
        let record = matching_record(&data, key)?;
        let snapshot = &record.cached.public_snapshot;
        if !record_is_fresh(record, now)
            || !snapshot_is_authoritative(snapshot)
            || snapshot.snapshot_id != snapshot_id
        {
            return None;
        }

        let advertised = snapshot.entries.iter().any(|entry| {
            entry.player_key == player_key
                && entry
                    .available_action_ids
                    .iter()
                    .any(|candidate| candidate == action_id)
        });
        if !advertised {
            return None;
        }

        record
            .cached
            .private_action_bindings
            .get(&(player_key.to_owned(), action_id.to_owned()))
            .cloned()
    }

    pub(crate) fn invalidate_instance(&self, instance_id: &str) {
        let removed = lock_unpoisoned(&self.inner.instances).remove(instance_id);
        #[cfg(test)]
        super::contract::pause_invalidation_after_remove(instance_id);
        if let Some(state) = removed {
            let mut data = lock_unpoisoned(&state.data);
            data.cached = None;
            data.active_key = None;
            data.refreshing_generation = None;
        }
    }

    fn state_for(&self, instance_id: &str) -> Arc<InstanceLivePlayerState> {
        let mut instances = lock_unpoisoned(&self.inner.instances);
        instances
            .entry(instance_id.to_owned())
            .or_insert_with(|| Arc::new(InstanceLivePlayerState::default()))
            .clone()
    }

    fn existing_state(&self, instance_id: &str) -> Option<Arc<InstanceLivePlayerState>> {
        lock_unpoisoned(&self.inner.instances)
            .get(instance_id)
            .cloned()
    }
}
enum RefreshPolicy {
    Explicit,
    IfExpired,
}
impl Default for InstanceLivePlayerState {
    fn default() -> Self {
        Self {
            refresh_gate: Arc::new(tokio::sync::Mutex::new(())),
            data: Mutex::new(InstanceLivePlayerData::default()),
        }
    }
}
struct RefreshLease {
    state: Arc<InstanceLivePlayerState>,
    generation: u64,
    active: bool,
}

struct RefreshLeaseRejection {
    snapshot: Box<RuntimeLivePlayerSnapshot>,
}

impl RefreshLeaseRejection {
    fn new(snapshot: RuntimeLivePlayerSnapshot) -> Self {
        Self {
            snapshot: Box::new(snapshot),
        }
    }

    fn into_snapshot(self) -> RuntimeLivePlayerSnapshot {
        *self.snapshot
    }
}

impl RefreshLease {
    fn begin(
        state: Arc<InstanceLivePlayerState>,
        key: &LivePlayerCacheKey,
    ) -> Result<Self, RefreshLeaseRejection> {
        let generation = {
            let mut data = lock_unpoisoned(&state.data);
            if data.active_key.as_ref().is_some_and(|active| active != key) {
                return Err(RefreshLeaseRejection::new(
                    data.cached
                        .as_ref()
                        .map(rejected_record)
                        .unwrap_or_else(|| rejected_key(key)),
                ));
            }
            data.next_generation = data.next_generation.wrapping_add(1);
            data.active_key = Some(key.clone());
            data.refreshing_generation = Some(data.next_generation);
            data.next_generation
        };
        Ok(Self {
            state,
            generation,
            active: true,
        })
    }

    fn release(&mut self) {
        if self.active {
            let mut data = lock_unpoisoned(&self.state.data);
            if data.refreshing_generation == Some(self.generation) {
                data.refreshing_generation = None;
            }
            self.active = false;
        }
    }
}
impl Drop for RefreshLease {
    fn drop(&mut self) {
        self.release();
    }
}
fn completed_snapshot_after_join(
    state: &InstanceLivePlayerState,
    key: &LivePlayerCacheKey,
    observed_generation: u64,
    now_monotonic_ms: u64,
) -> Option<RuntimeLivePlayerSnapshot> {
    let data = lock_unpoisoned(&state.data);
    if data.refreshing_generation.is_some() {
        return None;
    }
    let record = matching_record(&data, key)?;
    (data.completed_refresh_generation > observed_generation)
        .then(|| project_record(record, now_monotonic_ms, false))
}

#[cfg(test)]
fn store_success_as_latest(
    state: &InstanceLivePlayerState,
    key: &LivePlayerCacheKey,
    cached: CachedLivePlayerSnapshot,
    now_monotonic_ms: u64,
) -> RuntimeLivePlayerSnapshot {
    let mut data = lock_unpoisoned(&state.data);
    supersede_operations(&mut data, key);
    install_success(&mut data, key, cached, now_monotonic_ms);
    project_record(
        data.cached.as_ref().expect("success was installed"),
        now_monotonic_ms,
        false,
    )
}

fn store_refresh_success(
    state: &InstanceLivePlayerState,
    key: &LivePlayerCacheKey,
    generation: u64,
    cached: CachedLivePlayerSnapshot,
    freshness_started_at: u64,
    now_monotonic_ms: u64,
) -> RuntimeLivePlayerSnapshot {
    let mut data = lock_unpoisoned(&state.data);
    if !refresh_is_current(&data, key, generation) {
        return non_authoritative_projection(
            cached.public_snapshot,
            RuntimeLivePlayerStatus::Failed,
            true,
        );
    }
    install_success(&mut data, key, cached, freshness_started_at);
    data.refreshing_generation = None;
    project_record(
        data.cached.as_ref().expect("refresh success was installed"),
        now_monotonic_ms,
        false,
    )
}

fn install_success(
    data: &mut InstanceLivePlayerData,
    key: &LivePlayerCacheKey,
    mut cached: CachedLivePlayerSnapshot,
    freshness_started_at: u64,
) {
    cached.public_snapshot.instance_id = key.instance_id.clone();
    cached.public_snapshot.observed_at_unix_ms = cached
        .public_snapshot
        .observed_at_unix_ms
        .or(Some(cached.collected_at));
    cached.public_snapshot.expires_at_unix_ms =
        Some(cached.collected_at.saturating_add(key.refresh_interval_ms));
    if !snapshot_is_authoritative(&cached.public_snapshot) {
        strip_actions(&mut cached.public_snapshot);
        cached.private_action_bindings.clear();
    } else {
        retain_advertised_bindings(&mut cached);
    }

    let retains_authoritative_success = snapshot_is_authoritative(&cached.public_snapshot);
    data.completed_refresh_generation = data.completed_refresh_generation.saturating_add(1);
    data.cached = Some(CacheRecord {
        key: key.clone(),
        cached,
        freshness_started_at_monotonic_ms: freshness_started_at,
        retains_authoritative_success,
    });
}

#[cfg(test)]
fn store_failure_if_current(
    state: &InstanceLivePlayerState,
    key: &LivePlayerCacheKey,
    failure: RuntimeLivePlayerSnapshot,
    attempted_at: u64,
    now_monotonic_ms: u64,
) -> RuntimeLivePlayerSnapshot {
    let mut data = lock_unpoisoned(&state.data);
    if data.active_key.as_ref().is_some_and(|active| active != key) {
        return rejected_failure(key, failure);
    }
    supersede_operations(&mut data, key);
    install_failure(&mut data, key, failure, attempted_at, now_monotonic_ms);
    project_record(
        data.cached.as_ref().expect("failure was installed"),
        now_monotonic_ms,
        false,
    )
}

fn store_refresh_failure(
    state: &InstanceLivePlayerState,
    key: &LivePlayerCacheKey,
    generation: u64,
    failure: RuntimeLivePlayerSnapshot,
    attempted_at: u64,
    freshness_started_at: u64,
    now_monotonic_ms: u64,
) -> RuntimeLivePlayerSnapshot {
    let mut data = lock_unpoisoned(&state.data);
    if !refresh_is_current(&data, key, generation) {
        return rejected_failure(key, failure);
    }
    install_failure(&mut data, key, failure, attempted_at, freshness_started_at);
    data.refreshing_generation = None;
    project_record(
        data.cached.as_ref().expect("refresh failure was installed"),
        now_monotonic_ms,
        false,
    )
}

fn install_failure(
    data: &mut InstanceLivePlayerData,
    key: &LivePlayerCacheKey,
    mut failure: RuntimeLivePlayerSnapshot,
    attempted_at: u64,
    freshness_started_at: u64,
) {
    let unsupported = failure.status == RuntimeLivePlayerStatus::Unsupported;
    let previous = data
        .cached
        .take()
        .filter(|record| record.retains_authoritative_success && !unsupported);

    failure.instance_id = key.instance_id.clone();
    failure.status = match failure.status {
        RuntimeLivePlayerStatus::Misconfigured => RuntimeLivePlayerStatus::Misconfigured,
        RuntimeLivePlayerStatus::Unsupported => RuntimeLivePlayerStatus::Unsupported,
        _ => RuntimeLivePlayerStatus::Failed,
    };
    failure.complete = false;
    if unsupported {
        // A disabled query capability invalidates the roster, rather than retaining old players.
        failure.observed_at_unix_ms = None;
        failure.current_players = None;
        failure.max_players = None;
        failure.entries.clear();
        failure.truncated = false;
    }
    if let Some(previous) = previous {
        failure.stale = true;
        let public = previous.cached.public_snapshot;
        failure.source = failure.source.or(public.source);
        failure.observed_at_unix_ms = public.observed_at_unix_ms;
        failure.expires_at_unix_ms = Some(attempted_at.saturating_add(key.refresh_interval_ms));
        failure.current_players = public.current_players;
        failure.max_players = failure.max_players.or(public.max_players);
        failure.entries = public.entries;
        strip_actions(&mut failure);
        data.completed_refresh_generation = data.completed_refresh_generation.saturating_add(1);
        data.cached = Some(CacheRecord {
            key: key.clone(),
            cached: CachedLivePlayerSnapshot {
                public_snapshot: failure,
                private_action_bindings: HashMap::new(),
                collected_at: previous.cached.collected_at,
            },
            freshness_started_at_monotonic_ms: freshness_started_at,
            retains_authoritative_success: true,
        });
        return;
    }

    failure.stale = false;
    failure.expires_at_unix_ms = Some(attempted_at.saturating_add(key.refresh_interval_ms));
    strip_actions(&mut failure);
    data.completed_refresh_generation = data.completed_refresh_generation.saturating_add(1);
    data.cached = Some(CacheRecord {
        key: key.clone(),
        cached: CachedLivePlayerSnapshot {
            public_snapshot: failure,
            private_action_bindings: HashMap::new(),
            collected_at: attempted_at,
        },
        freshness_started_at_monotonic_ms: freshness_started_at,
        retains_authoritative_success: false,
    });
}

fn project_record(
    record: &CacheRecord,
    now_monotonic_ms: u64,
    refreshing: bool,
) -> RuntimeLivePlayerSnapshot {
    project_snapshot(
        record.cached.public_snapshot.clone(),
        record_is_fresh(record, now_monotonic_ms),
        refreshing,
    )
}

fn matching_record<'a>(
    data: &'a InstanceLivePlayerData,
    key: &LivePlayerCacheKey,
) -> Option<&'a CacheRecord> {
    data.cached
        .as_ref()
        .filter(|record| data.active_key.as_ref() == Some(key) && record.key == *key)
}

#[cfg(test)]
fn supersede_operations(data: &mut InstanceLivePlayerData, key: &LivePlayerCacheKey) {
    data.next_generation = data.next_generation.wrapping_add(1);
    data.active_key = Some(key.clone());
    data.refreshing_generation = None;
}

fn refresh_is_current(
    data: &InstanceLivePlayerData,
    key: &LivePlayerCacheKey,
    generation: u64,
) -> bool {
    data.active_key.as_ref() == Some(key) && data.refreshing_generation == Some(generation)
}
