//! Current-world evidence belongs to the backend runtime, not a storage-global cache.
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::{
    InstanceDetails, InstanceStatus, ProcessIdentity, RuntimeHealth, RuntimeHealthReason,
};
use app_platform_win::{WindowInspectionTarget, WindowsPlatform};
use app_runtime::RuntimeSupervisor;
use app_storage::{StoragePaths, read_instance_details};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, Semaphore};

use crate::astroneer_console::{self, Query};
use crate::state::{DesktopState, TimedCache, spawn_timed_cache_refresh};

const CACHE_TTL: Duration = Duration::from_secs(5);
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
// Two existing platform inspections each allow 15s; native queries share 3s.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(35);
const REFRESH_WAIT_TIMEOUT: Duration = Duration::from_secs(40);
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_INSTANCES: usize = 64;

// Never serialize or Debug this key: its credential digest is private cache identity.
#[derive(Clone, PartialEq, Eq)]
struct Key {
    database: PathBuf,
    instance_id: String,
    run_id: i64,
    session_id: String,
    process_key: String,
    pid: u32,
    identity: ProcessIdentity,
    port: u16,
    credential: [u8; 32],
}

struct Context {
    key: Key,
    password: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Observation {
    Ready,
    Waiting,
    Unavailable,
    InvalidConfiguration,
    Changed,
}

type ObservationCache = Arc<Mutex<TimedCache<Observation>>>;

struct Entry {
    key: Key,
    cache: ObservationCache,
    completed: Arc<Notify>,
    accessed: Instant,
}

pub(crate) struct AstroneerHealthRegistry {
    entries: Mutex<HashMap<String, Entry>>,
    refreshes: Arc<Semaphore>,
}

impl Default for AstroneerHealthRegistry {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            refreshes: Arc::new(Semaphore::new(4)),
        }
    }
}

impl AstroneerHealthRegistry {
    fn remove(&self, instance_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(instance_id);
        }
    }

    fn cache(&self, key: &Key) -> Option<(ObservationCache, Arc<Notify>)> {
        let mut entries = self.entries.lock().ok()?;
        if let Some(entry) = entries.get_mut(&key.instance_id)
            && entry.key == *key
        {
            entry.accessed = Instant::now();
            return Some((entry.cache.clone(), entry.completed.clone()));
        }
        if !entries.contains_key(&key.instance_id) && entries.len() >= MAX_INSTANCES {
            // Do not evict an active worker. Capacity pressure fails closed, without a queue.
            let oldest = entries
                .iter()
                .filter(|(_, entry)| Arc::strong_count(&entry.cache) == 1)
                .min_by_key(|(_, entry)| entry.accessed)
                .map(|(id, _)| id.clone())?;
            entries.remove(&oldest);
        }
        let cache = Arc::new(Mutex::new(TimedCache::default()));
        let completed = Arc::new(Notify::new());
        entries.insert(
            key.instance_id.clone(),
            Entry {
                key: key.clone(),
                cache: cache.clone(),
                completed: completed.clone(),
                accessed: Instant::now(),
            },
        );
        Some((cache, completed))
    }

    async fn observe<F>(&self, key: &Key, refresh: F) -> Observation
    where
        F: std::future::Future<Output = Observation> + Send + 'static,
    {
        let Some((cache, completed)) = self.cache(key) else {
            return Observation::Unavailable;
        };
        // Register before inspecting the cache, so completion between unlocking
        // and awaiting cannot be lost. All readers wait for this fresh result.
        let notified = completed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let permit = {
            let Ok(mut entry) = cache.lock() else {
                return Observation::Unavailable;
            };
            if let Some(value) = entry.fresh(CACHE_TTL) {
                return value;
            }
            if entry.try_begin_refresh() {
                let Ok(permit) = self.refreshes.clone().try_acquire_owned() else {
                    entry.store(Observation::Unavailable);
                    completed.notify_waiters();
                    return Observation::Unavailable;
                };
                Some(permit)
            } else {
                None
            }
        };
        if let Some(permit) = permit {
            let worker_cache = cache.clone();
            let worker_completed = completed.clone();
            // The completion notifier must outlive the requesting IPC future,
            // just like the existing cache refresh lease. Publish before waking.
            tauri::async_runtime::spawn(async move {
                let result = spawn_timed_cache_refresh(
                    worker_cache.clone(),
                    "ASTRONEER health",
                    async move {
                        let _permit = permit;
                        Ok(tokio::time::timeout(REFRESH_TIMEOUT, refresh)
                            .await
                            .unwrap_or(Observation::Unavailable))
                    },
                )
                .await;
                if !matches!(result, Ok(Ok(_)))
                    && let Ok(mut entry) = worker_cache.lock()
                {
                    entry.store(Observation::Unavailable);
                }
                worker_completed.notify_waiters();
            });
        }
        if tokio::time::timeout(REFRESH_WAIT_TIMEOUT, notified)
            .await
            .is_err()
        {
            return Observation::Unavailable;
        }
        // Never extend stale Ready while a probe runs, or after a timed-out wait.
        cache
            .lock()
            .ok()
            .and_then(|entry| entry.fresh(CACHE_TTL))
            .unwrap_or(Observation::Unavailable)
    }
}

pub(crate) async fn apply(
    state: &DesktopState,
    paths: &StoragePaths,
    instance: &InstanceDetails,
    health: &mut RuntimeHealth,
) {
    if instance.summary.module_id != "astroneer" {
        return;
    }
    if !eligible(instance, health) {
        state.astroneer_health.remove(&instance.summary.id);
        return;
    }
    let context = match context(instance, paths.database_path.clone()) {
        Ok(context) => context,
        Err(observation) => {
            *health = observation.health();
            return;
        }
    };
    let key = context.key.clone();
    let supervisor = state.runtime_supervisor.clone();
    let query_paths = paths.clone();
    let observed = state
        .astroneer_health
        .observe(&key, async move {
            observe_world(supervisor, query_paths, context).await
        })
        .await;
    // Even a fresh cached value cannot outlive stop, restart, configuration or ownership.
    let current = read_instance_details(paths, &instance.summary.id)
        .await
        .ok();
    let same = current
        .as_ref()
        .and_then(|details| self::context(details, paths.database_path.clone()).ok())
        .is_some_and(|context| context.key == key);
    *health = if same && owned(state.runtime_supervisor.clone(), key).await {
        observed.health()
    } else {
        Observation::Changed.health()
    };
}

fn eligible(instance: &InstanceDetails, health: &RuntimeHealth) -> bool {
    matches!(
        instance.summary.status,
        InstanceStatus::Starting | InstanceStatus::Running
    ) && matches!(health.status.as_str(), "starting" | "ready")
}

fn context(instance: &InstanceDetails, database: PathBuf) -> Result<Context, Observation> {
    if instance.summary.module_id != "astroneer"
        || !matches!(
            instance.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running
        )
    {
        return Err(Observation::Changed);
    }
    let run = instance.active_run.as_ref().ok_or(Observation::Changed)?;
    let session = run
        .session_id
        .as_ref()
        .filter(|session| !session.is_empty())
        .ok_or(Observation::Changed)?;
    if run.run_id <= 0
        || run.process_count != 1
        || run.processes.len() != 1
        || instance.summary.active_process_count != 1
    {
        return Err(Observation::Changed);
    }
    let primary = &run.processes[0];
    if !primary.is_primary
        || primary.run_id != run.run_id
        || primary.session_id != run.session_id
        || primary.pid != run.pid
        || primary.status != "running"
        || primary.exit_code.is_some()
        || primary.crash_flag
    {
        return Err(Observation::Changed);
    }
    let pid = primary
        .pid
        .filter(|pid| *pid > 0)
        .ok_or(Observation::Changed)?;
    let identity = primary
        .process_identity
        .clone()
        .filter(|identity| identity.creation_time > 0 && !identity.image_path.is_empty())
        .ok_or(Observation::Changed)?;
    let settings: Value = serde_json::from_str(&instance.settings_json)
        .map_err(|_| Observation::InvalidConfiguration)?;
    let password = settings
        .get("console_password")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.trim().is_empty()
                && value.chars().count() <= 128
                && !value.chars().any(char::is_control)
        })
        .ok_or(Observation::InvalidConfiguration)?
        .to_owned();
    let mut ports = instance.ports.iter().filter(|port| port.name == "console");
    let port = ports.next().ok_or(Observation::InvalidConfiguration)?;
    if port.port == 0 || !port.protocol.eq_ignore_ascii_case("tcp") || ports.next().is_some() {
        return Err(Observation::InvalidConfiguration);
    }
    Ok(Context {
        key: Key {
            database,
            instance_id: instance.summary.id.clone(),
            run_id: run.run_id,
            session_id: session.clone(),
            process_key: primary.process_key.clone(),
            pid,
            identity,
            port: port.port,
            credential: Sha256::digest(password.as_bytes()).into(),
        },
        password,
    })
}

async fn owned(supervisor: Arc<Mutex<RuntimeSupervisor>>, key: Key) -> bool {
    tokio::task::spawn_blocking(move || {
        if app_runtime::inspect_process_identity(key.pid)
            .ok()
            .flatten()
            .as_ref()
            != Some(&key.identity)
        {
            return false;
        }
        let Ok(mut supervisor) = supervisor.lock() else {
            return false;
        };
        supervisor.tracked_instances().iter().any(|tracked| {
            tracked.summary.id == key.instance_id
                && tracked.run_id == key.run_id
                && tracked.session_id.as_deref() == Some(&key.session_id)
                && tracked.pid == Some(key.pid)
        }) && supervisor
            .matches_running_process(&key.instance_id, key.run_id, &key.process_key, key.pid)
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

async fn endpoint(key: Key) -> Option<(u32, ProcessIdentity)> {
    tokio::task::spawn_blocking(move || {
        let target = WindowInspectionTarget {
            pid: key.pid,
            process_key: key.process_key,
            display_name: String::from("ASTRONEER"),
            process_identity: key.identity,
        };
        let result =
            WindowsPlatform::inspect_process_network_endpoints(&[target], &[key.port]).ok()?;
        let pids = result
            .endpoints
            .iter()
            .filter(|endpoint| {
                endpoint.protocol == "tcp"
                    && endpoint.local_port == key.port
                    && matches!(endpoint.local_address.as_str(), "127.0.0.1" | "0.0.0.0")
            })
            .map(|endpoint| endpoint.owning_pid)
            .collect::<BTreeSet<_>>();
        if pids.len() != 1 {
            return None;
        }
        let pid = *pids.first()?;
        Some((pid, app_runtime::inspect_process_identity(pid).ok()??))
    })
    .await
    .ok()
    .flatten()
}

async fn observe_world(
    supervisor: Arc<Mutex<RuntimeSupervisor>>,
    paths: StoragePaths,
    context: Context,
) -> Observation {
    let key = &context.key;
    if !owned(supervisor.clone(), key.clone()).await {
        return Observation::Changed;
    }
    let Some(before) = endpoint(key.clone()).await else {
        return Observation::Unavailable;
    };
    let result = query_world(key.port, &context.password).await;
    if endpoint(key.clone()).await.as_ref() != Some(&before)
        || !owned(supervisor, key.clone()).await
    {
        return Observation::Changed;
    }
    let current = read_instance_details(&paths, &key.instance_id).await.ok();
    if current
        .as_ref()
        .and_then(|details| self::context(details, paths.database_path).ok())
        .is_none_or(|current| current.key != *key)
    {
        return Observation::Changed;
    }
    result
}

async fn query_world(port: u16, password: &str) -> Observation {
    tokio::time::timeout(QUERY_TIMEOUT, async {
        let games = astroneer_console::fetch(
            port,
            password,
            Query::Games,
            QUERY_TIMEOUT,
            MAX_RESPONSE_BYTES,
        )
        .await
        .ok()?;
        let statistics = astroneer_console::fetch(
            port,
            password,
            Query::Statistics,
            QUERY_TIMEOUT,
            MAX_RESPONSE_BYTES,
        )
        .await
        .ok()?;
        let games = serde_json::from_slice(&games).ok()?;
        let statistics = serde_json::from_slice(&statistics).ok()?;
        Some(if selected_world_matches(&games, &statistics) {
            Observation::Ready
        } else {
            Observation::Waiting
        })
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(Observation::Unavailable)
}

fn selected_world_matches(games: &Value, statistics: &Value) -> bool {
    let Some(active) = games.get("activeSaveName").and_then(Value::as_str) else {
        return false;
    };
    games.get("gameList").is_some_and(Value::is_array)
        && !active.trim().is_empty()
        && active.chars().count() <= 256
        && !active.chars().any(char::is_control)
        && statistics.get("saveGameName").and_then(Value::as_str) == Some(active)
}

impl Observation {
    fn health(self) -> RuntimeHealth {
        let (status, code, summary) = match self {
            Self::Ready => (
                "ready",
                "ready_signal",
                "ASTRONEER reports the same active world in its game list and server statistics.",
            ),
            Self::Waiting => (
                "starting",
                "starting_tasks",
                "Waiting for ASTRONEER to select its current world.",
            ),
            Self::Changed => (
                "starting",
                "starting_tasks",
                "ASTRONEER runtime ownership changed; waiting for current-world evidence.",
            ),
            Self::Unavailable => (
                "warning",
                "astroneer_world_query_unavailable",
                "The current ASTRONEER process could not provide complete native world status.",
            ),
            Self::InvalidConfiguration => (
                "warning",
                "astroneer_console_configuration",
                "ASTRONEER world status requires one console TCP port and a valid console password.",
            ),
        };
        RuntimeHealth {
            status: status.into(),
            summary: summary.into(),
            reason: RuntimeHealthReason {
                code: code.into(),
                params: Default::default(),
            },
            matched_line: None,
        }
    }
}

#[cfg(test)]
#[path = "runtime_astroneer_health_tests.rs"]
mod tests;
