//! Current-run Palworld readiness when the native server emits no ready log line.
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_core::{
    InstanceDetails, InstanceStatus, ProcessIdentity, RuntimeHealth, RuntimeHealthReason,
};
use app_platform_win::WindowsPlatform;
use app_runtime::RuntimeSupervisor;
use app_storage::{StoragePaths, read_instance_details, read_instance_details_and_runtime_health};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, Semaphore};

use crate::live_players::{http_api, palworld_rest};
use crate::state::{DesktopState, TimedCache, spawn_timed_cache_refresh};

const CACHE_TTL: Duration = Duration::from_secs(5);
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
// Two bounded platform inspections plus the HTTP pair share one refresh owner.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(35);
const REFRESH_WAIT_TIMEOUT: Duration = Duration::from_secs(40);
const MAX_INSTANCES: usize = 64;

// Never serialize or Debug this key: the credential hash is private cache identity.
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

#[derive(Clone, Debug, PartialEq)]
struct WorldSample {
    world: String,
    uptime: f64,
}

#[derive(Clone, Debug, PartialEq)]
enum Observation {
    World {
        sample: WorldSample,
        advancing: bool,
    },
    Unavailable,
}

impl Observation {
    fn with_previous(self, previous: Option<&Self>) -> Self {
        match self {
            Self::World { sample, .. } => {
                let advancing = matches!(previous, Some(Self::World { sample: old, .. })
                    if old.world == sample.world && sample.uptime > old.uptime);
                Self::World { sample, advancing }
            }
            Self::Unavailable => Self::Unavailable,
        }
    }

    fn is_ready(&self) -> bool {
        matches!(
            self,
            Self::World {
                advancing: true,
                ..
            }
        )
    }
}

type ObservationCache = Arc<Mutex<TimedCache<Observation>>>;

struct Entry {
    key: Key,
    cache: ObservationCache,
    completed: Arc<Notify>,
    accessed: Instant,
}

pub(crate) struct PalworldHealthRegistry {
    entries: Mutex<HashMap<String, Entry>>,
    refreshes: Arc<Semaphore>,
}

impl Default for PalworldHealthRegistry {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            refreshes: Arc::new(Semaphore::new(4)),
        }
    }
}

impl PalworldHealthRegistry {
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
        let (permit, previous) = {
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
                (Some(permit), entry.latest())
            } else {
                (None, None)
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
                    "Palworld health",
                    async move {
                        let _permit = permit;
                        let observed = tokio::time::timeout(REFRESH_TIMEOUT, refresh)
                            .await
                            .unwrap_or(Observation::Unavailable);
                        Ok(observed.with_previous(previous.as_ref()))
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
    if instance.summary.module_id != "palworld" {
        return;
    }
    if !eligible(instance, health) {
        state.palworld_health.remove(&instance.summary.id);
        return;
    }
    let Some(context) = self::context(instance, paths.database_path.clone()) else {
        // REST is optional: preserve the existing log interpretation when disabled.
        state.palworld_health.remove(&instance.summary.id);
        return;
    };
    let key = context.key.clone();
    let supervisor = state.runtime_supervisor.clone();
    let query_paths = paths.clone();
    let observed = state
        .palworld_health
        .observe(&key, async move {
            observe_world(supervisor, query_paths, context).await
        })
        .await;
    if !observed.is_ready() {
        return;
    }
    let ownership_verified = owned(state.runtime_supervisor.clone(), key.clone()).await
        && endpoint_owned(key.clone()).await
        && owned(state.runtime_supervisor.clone(), key.clone()).await;
    // Nothing that can wait for a process/network inspection belongs after this
    // read: a fatal log, stop or configuration change during probing takes priority.
    let Ok((current, mut latest)) =
        read_instance_details_and_runtime_health(paths, &instance.summary.id).await
    else {
        return;
    };
    apply_ready_observation(&key, ownership_verified, &current, &mut latest);
    *health = latest;
}

fn apply_ready_observation(
    key: &Key,
    ownership_verified: bool,
    current: &InstanceDetails,
    health: &mut RuntimeHealth,
) {
    if ownership_verified
        && eligible(current, health)
        && context(current, key.database.clone()).is_some_and(|current| current.key == *key)
    {
        *health = RuntimeHealth {
            status: "ready".into(),
            summary: "Palworld reports an active world and advancing server uptime through its authenticated REST API.".into(),
            reason: RuntimeHealthReason { code: "ready_signal".into(), params: Default::default() },
            matched_line: None,
        };
    }
}

fn eligible(instance: &InstanceDetails, health: &RuntimeHealth) -> bool {
    instance.summary.module_id == "palworld"
        && matches!(
            instance.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running
        )
        && health.status == "starting"
        && matches!(
            health.reason.code.as_str(),
            "starting_tasks" | "starting_waiting_logs"
        )
}

fn context(instance: &InstanceDetails, database: PathBuf) -> Option<Context> {
    if instance.summary.module_id != "palworld"
        || !matches!(
            instance.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running
        )
    {
        return None;
    }
    let endpoint = palworld_rest::resolve_endpoint(instance).ok()?;
    let run = instance.active_run.as_ref()?;
    let session = run
        .session_id
        .as_ref()
        .filter(|session| !session.is_empty())?;
    if run.run_id <= 0
        || run.process_count != 1
        || run.processes.len() != 1
        || instance.summary.active_process_count != 1
    {
        return None;
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
        return None;
    }
    let pid = primary.pid.filter(|pid| *pid > 0)?;
    let identity = primary
        .process_identity
        .clone()
        .filter(|identity| identity.creation_time > 0 && !identity.image_path.is_empty())?;
    Some(Context {
        key: Key {
            database,
            instance_id: instance.summary.id.clone(),
            run_id: run.run_id,
            session_id: session.clone(),
            process_key: primary.process_key.clone(),
            pid,
            identity,
            port: endpoint.address.port(),
            credential: Sha256::digest(endpoint.password.as_bytes()).into(),
        },
        password: endpoint.password,
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

fn endpoint_matches(pid: u32, owners: &BTreeSet<u32>) -> bool {
    owners == &BTreeSet::from([pid])
}

async fn endpoint_owned(key: Key) -> bool {
    tokio::task::spawn_blocking(move || {
        let same_process = || {
            app_runtime::inspect_process_identity(key.pid)
                .ok()
                .flatten()
                .as_ref()
                == Some(&key.identity)
        };
        same_process()
            && WindowsPlatform::loopback_tcp_listener_owners(key.port)
                .is_ok_and(|owners| endpoint_matches(key.pid, &owners))
            && same_process()
    })
    .await
    .unwrap_or(false)
}

async fn observe_world(
    supervisor: Arc<Mutex<RuntimeSupervisor>>,
    paths: StoragePaths,
    context: Context,
) -> Observation {
    let key = &context.key;
    // Check the global listener snapshot before sending credentials and again
    // after the request; target-filtered inspection cannot expose competing PIDs.
    if !owned(supervisor.clone(), key.clone()).await || !endpoint_owned(key.clone()).await {
        return Observation::Unavailable;
    }
    let observed = query_world(key.port, &context.password).await;
    if !endpoint_owned(key.clone()).await || !owned(supervisor, key.clone()).await {
        return Observation::Unavailable;
    }
    let current = read_instance_details(&paths, &key.instance_id).await.ok();
    if current
        .as_ref()
        .and_then(|details| self::context(details, paths.database_path))
        .is_none_or(|current| current.key != *key)
    {
        return Observation::Unavailable;
    }
    observed
}

#[derive(Deserialize)]
struct Info {
    worldguid: String,
    version: String,
}
#[derive(Deserialize)]
struct Metrics {
    serverfps: f64,
    uptime: f64,
}

fn world_sample(info: &[u8], metrics: &[u8]) -> Option<WorldSample> {
    let info: Info = serde_json::from_slice(info).ok()?;
    let metrics: Metrics = serde_json::from_slice(metrics).ok()?;
    // Palworld also uses DedicatedServerName (the managed instance ID) here;
    // this is an opaque native world identity, not necessarily a UUID.
    let world = info.worldguid;
    if world.trim().is_empty() || world.chars().count() > 256 || world.chars().any(char::is_control)
    {
        return None;
    }
    if info.version.trim().is_empty()
        || info.version.len() > 128
        || info.version.chars().any(char::is_control)
        || !metrics.serverfps.is_finite()
        || metrics.serverfps <= 0.0
        || !metrics.uptime.is_finite()
        || metrics.uptime < 0.0
    {
        return None;
    }
    Some(WorldSample {
        world,
        uptime: metrics.uptime,
    })
}

async fn query_world(port: u16, password: &str) -> Observation {
    let address = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
    let result = tokio::time::timeout(QUERY_TIMEOUT, async {
        let info = http_api::fetch_json(address, "/v1/api/info", Some(password), QUERY_TIMEOUT)
            .await
            .ok()?;
        let metrics =
            http_api::fetch_json(address, "/v1/api/metrics", Some(password), QUERY_TIMEOUT)
                .await
                .ok()?;
        world_sample(&info, &metrics)
    })
    .await
    .ok()
    .flatten();
    result.map_or(Observation::Unavailable, |sample| Observation::World {
        sample,
        advancing: false,
    })
}

#[cfg(test)]
#[path = "runtime_palworld_health_tests.rs"]
mod tests;
