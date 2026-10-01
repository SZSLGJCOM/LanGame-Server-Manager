use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::{Arc, Barrier, OnceLock};

use app_core::{
    ModulePlayerActionSpec, ModulePlayerListSpec, RuntimeLivePlayerSnapshot,
    RuntimeLivePlayerStatus,
};
use serde::Serialize;

type PrivateLivePlayerActionBindings = HashMap<(String, String), String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LivePlayerCacheKey {
    pub instance_id: String,
    pub run_id: String,
    pub security_contract_fingerprint: String,
    pub refresh_interval_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedLivePlayerSnapshot {
    pub public_snapshot: RuntimeLivePlayerSnapshot,
    pub private_action_bindings: PrivateLivePlayerActionBindings,
    pub collected_at: u64,
}

pub(super) struct CacheRecord {
    pub key: LivePlayerCacheKey,
    pub cached: CachedLivePlayerSnapshot,
    pub freshness_started_at_monotonic_ms: u64,
    pub retains_authoritative_success: bool,
}

pub(crate) trait LivePlayerCacheClock: std::fmt::Debug + Send + Sync {
    fn now_unix_ms(&self) -> u64;
    fn now_monotonic_ms(&self) -> u64;
}

#[derive(Debug)]
pub(super) struct SystemLivePlayerCacheClock {
    monotonic_origin: Instant,
}

impl Default for SystemLivePlayerCacheClock {
    fn default() -> Self {
        Self {
            monotonic_origin: Instant::now(),
        }
    }
}

impl LivePlayerCacheClock for SystemLivePlayerCacheClock {
    fn now_unix_ms(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }

    fn now_monotonic_ms(&self) -> u64 {
        u64::try_from(self.monotonic_origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

#[derive(Serialize)]
struct LivePlayerSecurityContract<'a> {
    module_id: &'a str,
    player_list: &'a ModulePlayerListSpec,
    referenced_actions: Vec<ReferencedPlayerActionContract<'a>>,
}

#[derive(Serialize)]
struct ReferencedPlayerActionContract<'a> {
    action_id: &'a str,
    action: Option<PlayerActionSecurityFields<'a>>,
}

#[derive(Serialize)]
struct PlayerActionSecurityFields<'a> {
    command_template: &'a str,
    transport: &'a str,
    process_key: &'a Option<String>,
    port_name: &'a Option<String>,
    password_setting_key: &'a Option<String>,
    enabled_setting_key: &'a Option<String>,
    target_required: bool,
    target_encoding: &'a Option<String>,
    role_values: &'a [String],
    destructive: bool,
}

pub(crate) fn build_live_player_security_contract_fingerprint(
    module_id: &str,
    player_list: &ModulePlayerListSpec,
    actions: &[ModulePlayerActionSpec],
) -> String {
    let referenced_actions = player_list
        .action_id
        .iter()
        .chain(player_list.player_action_ids.iter())
        .map(|action_id| {
            let action = actions
                .iter()
                .find(|candidate| candidate.id == *action_id)
                .map(|action| PlayerActionSecurityFields {
                    command_template: &action.command_template,
                    transport: &action.transport,
                    process_key: &action.process_key,
                    port_name: &action.port_name,
                    password_setting_key: &action.password_setting_key,
                    enabled_setting_key: &action.enabled_setting_key,
                    target_required: action.target_required,
                    target_encoding: &action.target_encoding,
                    role_values: &action.role_values,
                    destructive: action.destructive,
                });
            ReferencedPlayerActionContract { action_id, action }
        })
        .collect();
    serde_json::to_string(&LivePlayerSecurityContract {
        module_id,
        player_list,
        referenced_actions,
    })
    .expect("live-player security contracts only contain serializable values")
}

pub(super) fn snapshot_is_authoritative(snapshot: &RuntimeLivePlayerSnapshot) -> bool {
    snapshot.status == RuntimeLivePlayerStatus::Ready
        && snapshot.complete
        && !snapshot.truncated
        && !snapshot.stale
}

pub(super) fn non_authoritative_projection(
    mut snapshot: RuntimeLivePlayerSnapshot,
    status: RuntimeLivePlayerStatus,
    stale: bool,
) -> RuntimeLivePlayerSnapshot {
    snapshot.status = status;
    snapshot.complete = false;
    snapshot.stale = stale;
    strip_actions(&mut snapshot);
    snapshot
}

pub(super) fn project_snapshot(
    mut snapshot: RuntimeLivePlayerSnapshot,
    fresh: bool,
    refreshing: bool,
) -> RuntimeLivePlayerSnapshot {
    if refreshing {
        snapshot.status = RuntimeLivePlayerStatus::Refreshing;
        strip_actions(&mut snapshot);
    } else if !fresh {
        snapshot.stale = true;
        strip_actions(&mut snapshot);
    } else if !snapshot_is_authoritative(&snapshot) {
        strip_actions(&mut snapshot);
    }
    snapshot
}

pub(super) fn record_is_fresh(record: &CacheRecord, now_monotonic_ms: u64) -> bool {
    now_monotonic_ms
        < record
            .freshness_started_at_monotonic_ms
            .saturating_add(record.key.refresh_interval_ms)
}

pub(super) fn rejected_failure(
    key: &LivePlayerCacheKey,
    mut failure: RuntimeLivePlayerSnapshot,
) -> RuntimeLivePlayerSnapshot {
    failure.instance_id = key.instance_id.clone();
    let status = if failure.status == RuntimeLivePlayerStatus::Misconfigured {
        RuntimeLivePlayerStatus::Misconfigured
    } else {
        RuntimeLivePlayerStatus::Failed
    };
    non_authoritative_projection(failure, status, true)
}

pub(super) fn rejected_record(record: &CacheRecord) -> RuntimeLivePlayerSnapshot {
    non_authoritative_projection(
        record.cached.public_snapshot.clone(),
        RuntimeLivePlayerStatus::Failed,
        true,
    )
}

pub(super) fn rejected_key(key: &LivePlayerCacheKey) -> RuntimeLivePlayerSnapshot {
    RuntimeLivePlayerSnapshot {
        snapshot_id: uuid::Uuid::new_v4().simple().to_string(),
        instance_id: key.instance_id.clone(),
        status: RuntimeLivePlayerStatus::Failed,
        source: None,
        observed_at_unix_ms: None,
        expires_at_unix_ms: None,
        complete: false,
        truncated: false,
        stale: true,
        current_players: None,
        max_players: None,
        entries: Vec::new(),
        issue: None,
    }
}

pub(super) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
pub(super) struct RefreshCommitRaceHook {
    pub before_commit: Arc<tokio::sync::Notify>,
    pub continue_commit: Arc<tokio::sync::Notify>,
    pub map_removed: Arc<Barrier>,
    pub continue_invalidation: Arc<Barrier>,
}

#[cfg(test)]
type RefreshCommitRaceHookSlot = Option<(String, Arc<RefreshCommitRaceHook>)>;

#[cfg(test)]
static REFRESH_COMMIT_RACE_HOOK: OnceLock<Mutex<RefreshCommitRaceHookSlot>> = OnceLock::new();

#[cfg(test)]
pub(super) fn install_refresh_commit_race_hook(instance_id: &str) -> Arc<RefreshCommitRaceHook> {
    let hook = Arc::new(RefreshCommitRaceHook {
        before_commit: Arc::new(tokio::sync::Notify::new()),
        continue_commit: Arc::new(tokio::sync::Notify::new()),
        map_removed: Arc::new(Barrier::new(2)),
        continue_invalidation: Arc::new(Barrier::new(2)),
    });
    *lock_unpoisoned(REFRESH_COMMIT_RACE_HOOK.get_or_init(|| Mutex::new(None))) =
        Some((instance_id.to_owned(), hook.clone()));
    hook
}

#[cfg(test)]
fn refresh_commit_race_hook(instance_id: &str) -> Option<Arc<RefreshCommitRaceHook>> {
    lock_unpoisoned(REFRESH_COMMIT_RACE_HOOK.get_or_init(|| Mutex::new(None)))
        .as_ref()
        .filter(|(candidate, _)| candidate == instance_id)
        .map(|(_, hook)| hook.clone())
}

#[cfg(test)]
pub(super) async fn pause_refresh_commit(instance_id: &str) {
    if let Some(hook) = refresh_commit_race_hook(instance_id) {
        hook.before_commit.notify_one();
        hook.continue_commit.notified().await;
    }
}

#[cfg(test)]
pub(super) fn pause_invalidation_after_remove(instance_id: &str) {
    if let Some(hook) = refresh_commit_race_hook(instance_id) {
        hook.map_removed.wait();
        hook.continue_invalidation.wait();
    }
}

#[cfg(test)]
pub(super) fn clear_refresh_commit_race_hook() {
    *lock_unpoisoned(REFRESH_COMMIT_RACE_HOOK.get_or_init(|| Mutex::new(None))) = None;
}

pub(super) fn strip_actions(snapshot: &mut RuntimeLivePlayerSnapshot) {
    for entry in &mut snapshot.entries {
        entry.available_action_ids.clear();
    }
}

pub(super) fn retain_advertised_bindings(cached: &mut CachedLivePlayerSnapshot) {
    let entries = &cached.public_snapshot.entries;
    cached
        .private_action_bindings
        .retain(|(player_key, action_id), _| {
            entries.iter().any(|entry| {
                entry.player_key == *player_key
                    && entry
                        .available_action_ids
                        .iter()
                        .any(|candidate| candidate == action_id)
            })
        });
}

#[cfg(test)]
mod tests {
    use app_core::{
        ModulePlayerActionSpec, ModulePlayerListCodec, ModulePlayerListScope,
        ModulePlayerListSource, ModulePlayerListSpec, RuntimePlayerIdentityKind,
    };

    use super::build_live_player_security_contract_fingerprint;

    const ACTION_ID: &str = "kick_userid";
    type PlayerActionMutation = fn(&mut ModulePlayerActionSpec);

    #[test]
    fn fingerprint_covers_every_authorization_field() {
        let player_list = ModulePlayerListSpec {
            scope: ModulePlayerListScope::Online,
            source: ModulePlayerListSource::StructuredLog,
            action_id: Some(String::from("list_online_players")),
            player_action_ids: vec![ACTION_ID.to_owned()],
            response_codec: ModulePlayerListCodec::DstClientTableV1,
            identity_kind: RuntimePlayerIdentityKind::KleiUserId,
            refresh_interval_ms: 30_000,
        };
        let actions = vec![
            runtime_action("list_online_players", "list {{request_id}}", false),
            runtime_action(ACTION_ID, "kick {{target}}", true),
        ];
        let baseline =
            build_live_player_security_contract_fingerprint("dontstarve", &player_list, &actions);
        let mutations: &[PlayerActionMutation] = &[
            |action| action.command_template.push_str(" changed"),
            |action| action.transport = String::from("rcon"),
            |action| action.process_key = Some(String::from("caves")),
            |action| action.port_name = Some(String::from("rcon")),
            |action| action.password_setting_key = Some(String::from("rcon.password")),
            |action| action.enabled_setting_key = Some(String::from("rcon.enabled")),
            |action| action.target_encoding = Some(String::from("quoted")),
            |action| action.role_values.push(String::from("moderator")),
            |action| action.destructive = !action.destructive,
        ];
        for action_index in 0..actions.len() {
            for mutate in mutations {
                let mut changed = actions.clone();
                mutate(&mut changed[action_index]);
                assert_ne!(
                    baseline,
                    build_live_player_security_contract_fingerprint(
                        "dontstarve",
                        &player_list,
                        &changed
                    )
                );
            }
        }
        assert_ne!(
            baseline,
            build_live_player_security_contract_fingerprint(
                "dontstarve-together",
                &player_list,
                &actions
            )
        );
        let mut changed_list = player_list;
        changed_list.refresh_interval_ms += 1;
        assert_ne!(
            baseline,
            build_live_player_security_contract_fingerprint("dontstarve", &changed_list, &actions)
        );
    }

    fn runtime_action(
        id: &str,
        command_template: &str,
        destructive: bool,
    ) -> ModulePlayerActionSpec {
        ModulePlayerActionSpec {
            id: id.to_owned(),
            kind: None,
            label: id.to_owned(),
            label_zh_cn: None,
            transport: String::from("stdin"),
            command_template: command_template.to_owned(),
            target_label: None,
            target_label_zh_cn: None,
            target_placeholder: None,
            target_placeholder_zh_cn: None,
            target_required: id == ACTION_ID,
            target_encoding: (id == ACTION_ID).then(|| String::from("raw")),
            role_values: Vec::new(),
            process_key: Some(String::from("master")),
            port_name: None,
            password_setting_key: None,
            enabled_setting_key: None,
            destructive,
        }
    }
}
