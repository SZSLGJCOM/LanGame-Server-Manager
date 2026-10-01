use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime};

use app_core::{AppState, RuntimeStartupSchedule, StorageStatus, SystemSnapshot};
use app_platform_win::{BindAddressCandidate, WindowsHostMonitor};
use app_runtime::RuntimeSupervisor;
use app_storage::{StorageBootstrap, StoragePaths, bootstrap_storage};

use crate::live_players::cache::LivePlayerRegistry;

mod app_shutdown;
mod install_operations;
mod storage_management;
mod storage_shutdown;
pub(crate) use storage_management::StorageManagement;
mod system_snapshot_refresh;
mod timed_cache;
pub(crate) use install_operations::{InstallOperationLease, InstallOperationRegistry};
pub(crate) use system_snapshot_refresh::SystemSnapshotRefresh;
pub(crate) use timed_cache::{TimedCache, spawn_timed_cache_refresh};
mod autostart;
pub(crate) use autostart::{AutostartBatch, AutostartQueue};
mod runtime_reconciliation;
pub(crate) use runtime_reconciliation::RuntimeExitReconciliation;
mod runtime_log_streams;
pub use runtime_log_streams::{RuntimeLogStreamLease, RuntimeLogStreamStatus, RuntimeLogStreams};
mod runtime_restart_scheduler;
pub use runtime_restart_scheduler::{
    RuntimeRestartScheduleEntry, RuntimeRestartScheduleRequest, RuntimeRestartScheduler,
};

const MAX_RUNTIME_STARTUP_STAGGER_MS: u64 = 10_000;
const RUNNING_INSTANCE_STAGGER_BONUS_MS: u64 = 250;
const ACTIVE_PROCESS_STAGGER_BONUS_MS: u64 = 125;
const EXTRA_PROCESS_STAGGER_BONUS_MS: u64 = 500;

#[derive(Debug)]
pub struct LanDirectoryWorker {
    cancel: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl LanDirectoryWorker {
    pub fn new(cancel: Arc<AtomicBool>, thread: std::thread::JoinHandle<()>) -> Self {
        Self { cancel, thread }
    }

    pub fn cancel_and_take_thread(self) -> std::thread::JoinHandle<()> {
        self.cancel.store(true, Ordering::SeqCst);
        self.thread
    }
}

pub struct DesktopState {
    pub(crate) knowledge: Arc<crate::knowledge_runtime::KnowledgeCoordinator>,
    pub(crate) assistant_sessions: crate::assistant_sessions::AssistantSessionStore,
    pub booted_at: SystemTime,
    pub app_state: Arc<RwLock<AppState>>,
    pub host_monitor: Arc<Mutex<WindowsHostMonitor>>,
    pub system_snapshot_cache: Arc<Mutex<TimedCache<SystemSnapshot>>>,
    pub(crate) system_snapshot_refresh: Arc<SystemSnapshotRefresh>,
    pub bind_address_cache: Arc<Mutex<TimedCache<Vec<BindAddressCandidate>>>>,
    pub runtime_supervisor: Arc<Mutex<RuntimeSupervisor>>,
    pub runtime_resource_admission: app_runtime::RuntimeResourceAdmission,
    pub(crate) runtime_reconciliation: Arc<RuntimeExitReconciliation>,
    pub runtime_start_reservations: Arc<Mutex<RuntimeStartReservations>>,
    pub autostart: AutostartQueue,
    pub managed_save_policy_revision: AtomicU64,
    pub runtime_log_streams: Mutex<RuntimeLogStreams>,
    lan_directory_worker: Mutex<Option<LanDirectoryWorker>>,
    lan_host_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    storage_context_coordinator: Arc<Mutex<StorageContextCoordinator>>,
    pub startup_scheduler: Mutex<RuntimeStartupScheduler>,
    pub runtime_restart_scheduler: Mutex<RuntimeRestartScheduler>,
    pub live_player_registry: LivePlayerRegistry,
    pub(crate) astroneer_health: crate::runtime_astroneer_health::AstroneerHealthRegistry,
    module_sync_lock: tokio::sync::Mutex<()>,
    instance_mutation_locks: tokio::sync::Mutex<InstanceMutationLocks>,
    pub shutdown_in_progress: AtomicBool,
    final_exit_requested: AtomicBool,
    app_shutdown: Mutex<()>,
    pub shutdown_completed: AtomicBool,
    pub(crate) steamcmd_preparation: Arc<crate::steamcmd_preparation::SteamCmdPreparationTracker>,
    pub(crate) install_operations: Arc<InstallOperationRegistry>,
    pub(crate) storage_management: Arc<StorageManagement>,
}

impl Default for DesktopState {
    fn default() -> Self {
        let default_paths = StoragePaths::default();
        let app_state = initial_app_state(&default_paths, bootstrap_storage().ok());

        Self {
            knowledge: Arc::new(crate::knowledge_runtime::KnowledgeCoordinator::default()),
            assistant_sessions: crate::assistant_sessions::AssistantSessionStore::default(),
            booted_at: SystemTime::now(),
            install_operations: Arc::new(InstallOperationRegistry::default()),
            storage_management: Arc::new(StorageManagement::default()),
            app_state: Arc::new(RwLock::new(app_state)),
            host_monitor: Arc::new(Mutex::new(WindowsHostMonitor::default())),
            system_snapshot_cache: Arc::new(Mutex::new(TimedCache::default())),
            system_snapshot_refresh: Arc::new(SystemSnapshotRefresh::default()),
            bind_address_cache: Arc::new(Mutex::new(TimedCache::default())),
            runtime_supervisor: Arc::new(Mutex::new(RuntimeSupervisor::default())),
            runtime_resource_admission: app_runtime::RuntimeResourceAdmission::default(),
            runtime_reconciliation: Arc::new(RuntimeExitReconciliation::default()),
            runtime_start_reservations: Arc::new(Mutex::new(RuntimeStartReservations::default())),
            autostart: AutostartQueue::default(),
            managed_save_policy_revision: AtomicU64::new(0),
            runtime_log_streams: Mutex::new(RuntimeLogStreams::default()),
            lan_directory_worker: Mutex::new(None),
            lan_host_thread: Mutex::new(None),
            storage_context_coordinator: Arc::new(Mutex::new(StorageContextCoordinator::default())),
            startup_scheduler: Mutex::new(RuntimeStartupScheduler::default()),
            runtime_restart_scheduler: Mutex::new(RuntimeRestartScheduler::default()),
            live_player_registry: LivePlayerRegistry::default(),
            astroneer_health: crate::runtime_astroneer_health::AstroneerHealthRegistry::default(),
            module_sync_lock: tokio::sync::Mutex::new(()),
            instance_mutation_locks: tokio::sync::Mutex::new(InstanceMutationLocks::default()),
            shutdown_in_progress: AtomicBool::new(false),
            final_exit_requested: AtomicBool::new(false),
            app_shutdown: Mutex::new(()),
            shutdown_completed: AtomicBool::new(false),
            steamcmd_preparation: Arc::new(
                crate::steamcmd_preparation::SteamCmdPreparationTracker::default(),
            ),
        }
    }
}

impl DesktopState {
    pub(crate) fn request_final_exit(&self) -> bool {
        !self.final_exit_requested.swap(true, Ordering::SeqCst)
    }

    pub(crate) fn is_final_exit_requested(&self) -> bool {
        self.final_exit_requested.load(Ordering::SeqCst)
    }

    pub fn register_lan_directory_worker(
        &self,
        worker: LanDirectoryWorker,
    ) -> Result<(), (String, LanDirectoryWorker)> {
        let Ok(mut slot) = self.lan_directory_worker.lock() else {
            return Err((
                String::from("LanGame LAN directory worker lock poisoned"),
                worker,
            ));
        };
        if slot.is_some() {
            return Err((
                String::from("LanGame LAN directory worker is already registered"),
                worker,
            ));
        }
        *slot = Some(worker);
        Ok(())
    }

    pub fn take_lan_directory_worker(&self) -> Result<Option<LanDirectoryWorker>, String> {
        self.lan_directory_worker
            .lock()
            .map_err(|_| String::from("LanGame LAN directory worker lock poisoned"))
            .map(|mut slot| slot.take())
    }

    pub fn begin_storage_context_transition(
        &self,
    ) -> Result<StorageContextTransitionGuard, String> {
        let mut coordinator = self
            .storage_context_coordinator
            .lock()
            .map_err(|_| String::from("storage context coordinator lock poisoned"))?;
        if coordinator.transition_in_progress {
            return Err(String::from(
                "application paths are already being updated; retry after the current update finishes",
            ));
        }
        if self.shutdown_in_progress.load(Ordering::SeqCst)
            || self.is_final_exit_requested()
            || coordinator.shutdown_exclusive_in_progress
        {
            return Err(String::from(
                "application paths cannot be updated while application shutdown is in progress",
            ));
        }
        if !coordinator.active_operations.is_empty() {
            return Err(String::from(
                "a storage operation is in progress; retry the path update after it finishes",
            ));
        }
        coordinator.transition_in_progress = true;
        drop(coordinator);

        Ok(StorageContextTransitionGuard {
            coordinator: Arc::clone(&self.storage_context_coordinator),
        })
    }

    pub fn begin_storage_context_operation(
        &self,
        operation: &str,
    ) -> Result<StorageContextOperationGuard, String> {
        let mut coordinator = self
            .storage_context_coordinator
            .lock()
            .map_err(|_| String::from("storage context coordinator lock poisoned"))?;
        if coordinator.transition_in_progress {
            return Err(format!(
                "{operation} cannot begin while application paths are being updated; retry after the update finishes"
            ));
        }
        if self.shutdown_in_progress.load(Ordering::SeqCst)
            || self.is_final_exit_requested()
            || coordinator.shutdown_exclusive_in_progress
        {
            return Err(format!(
                "{operation} cannot begin while application shutdown is in progress"
            ));
        }
        let operation_id = coordinator
            .next_operation_id
            .checked_add(1)
            .ok_or_else(|| String::from("storage operation counter overflow"))?;
        coordinator.next_operation_id = operation_id;
        let cancellation = Arc::new(AtomicBool::new(false));
        coordinator.active_operations.insert(
            operation_id,
            (operation.to_owned(), Arc::clone(&cancellation)),
        );
        drop(coordinator);

        Ok(StorageContextOperationGuard {
            _lease: Arc::new(StorageContextOperationLease {
                coordinator: Arc::clone(&self.storage_context_coordinator),
                operation_id,
                cancellation,
            }),
        })
    }

    pub fn begin_storage_shutdown_exclusive(&self) -> Result<StorageContextShutdownGuard, String> {
        let mut coordinator = self
            .storage_context_coordinator
            .lock()
            .map_err(|_| String::from("storage context coordinator lock poisoned"))?;
        if !self.shutdown_in_progress.load(Ordering::SeqCst) {
            return Err(String::from(
                "storage shutdown exclusivity requires an active application shutdown request",
            ));
        }
        // A failed shutdown can restore shutdown_in_progress while a launch is
        // still draining. Keep its queued autostarts cancelled across that retry.
        self.autostart.cancel_all()?;
        if coordinator.transition_in_progress {
            return Err(String::from(
                "application paths are being updated; retry application shutdown after the update finishes",
            ));
        }
        if coordinator.shutdown_exclusive_in_progress {
            return Err(String::from(
                "storage shutdown exclusivity is already active",
            ));
        }
        if !coordinator.active_operations.is_empty() {
            return Err(String::from(
                "storage operations are still running; retry application shutdown after they finish",
            ));
        }
        coordinator.shutdown_exclusive_in_progress = true;
        drop(coordinator);

        Ok(StorageContextShutdownGuard {
            lease: Arc::new(StorageContextShutdownLease {
                coordinator: Arc::clone(&self.storage_context_coordinator),
                committed: AtomicBool::new(false),
            }),
        })
    }

    pub fn register_lan_host_thread(
        &self,
        handle: std::thread::JoinHandle<()>,
    ) -> Result<(), (String, std::thread::JoinHandle<()>)> {
        let Ok(mut slot) = self.lan_host_thread.lock() else {
            return Err((String::from("LAN host thread lock poisoned"), handle));
        };
        if slot.is_some() {
            return Err((
                String::from("LAN host thread is already registered"),
                handle,
            ));
        }
        *slot = Some(handle);
        Ok(())
    }

    pub fn take_lan_host_thread(&self) -> Result<Option<std::thread::JoinHandle<()>>, String> {
        self.lan_host_thread
            .lock()
            .map_err(|_| String::from("LAN host thread lock poisoned"))
            .map(|mut slot| slot.take())
    }

    pub async fn acquire_module_sync(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.module_sync_lock.lock().await
    }

    pub fn is_storage_ready(&self) -> bool {
        self.app_state
            .read()
            .map(|state| storage_status_ready(&state.storage))
            .unwrap_or(false)
    }

    pub async fn acquire_instance_mutation(
        &self,
        instance_id: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self.instance_mutation_locks.lock().await;
            locks.lock_for(instance_id)
        };
        lock.lock_owned().await
    }

    pub(crate) async fn try_acquire_instance_mutation(
        &self,
        instance_id: &str,
    ) -> Option<tokio::sync::OwnedMutexGuard<()>> {
        let lock = {
            let mut locks = self.instance_mutation_locks.lock().await;
            locks.lock_for(instance_id)
        };
        lock.try_lock_owned().ok()
    }

    #[cfg(test)]
    async fn tracked_instance_mutation_lock_count(&self) -> usize {
        self.instance_mutation_locks.lock().await.locks.len()
    }

    pub fn try_reserve_runtime_start(
        &self,
        instance_id: &str,
        source: &str,
    ) -> Result<RuntimeStartReservationAttempt, String> {
        if self.shutdown_in_progress.load(Ordering::SeqCst) {
            return Ok(RuntimeStartReservationAttempt::ShutdownInProgress);
        }
        let storage_context_operation = match self.begin_storage_context_operation("runtime start")
        {
            Ok(operation) => operation,
            Err(_) if self.shutdown_in_progress.load(Ordering::SeqCst) => {
                return Ok(RuntimeStartReservationAttempt::ShutdownInProgress);
            }
            Err(error) => return Err(error),
        };
        let mut reservations = self
            .runtime_start_reservations
            .lock()
            .map_err(|_| String::from("runtime start reservation lock poisoned"))?;
        if self.shutdown_in_progress.load(Ordering::SeqCst) {
            return Ok(RuntimeStartReservationAttempt::ShutdownInProgress);
        }

        let attempt = match reservations.reserve(instance_id, source) {
            Ok(_) => RuntimeStartReservationAttempt::Reserved(RuntimeStartReservationLease {
                lease: Arc::new(RuntimeStartReservationInner {
                    reservations: Arc::clone(&self.runtime_start_reservations),
                    instance_id: instance_id.to_string(),
                    _storage_context_operation: storage_context_operation,
                }),
            }),
            Err(conflict) => RuntimeStartReservationAttempt::Conflict(conflict),
        };
        drop(reservations);
        Ok(attempt)
    }

    pub fn pending_runtime_start_instance_ids(&self) -> Result<Vec<String>, String> {
        self.runtime_start_reservations
            .lock()
            .map_err(|_| String::from("runtime start reservation lock poisoned"))
            .map(|reservations| reservations.pending_instance_ids())
    }
}

#[derive(Debug, Default)]
struct StorageContextCoordinator {
    transition_in_progress: bool,
    shutdown_exclusive_in_progress: bool,
    next_operation_id: u64,
    active_operations: HashMap<u64, (String, Arc<AtomicBool>)>,
}

pub struct StorageContextTransitionGuard {
    coordinator: Arc<Mutex<StorageContextCoordinator>>,
}

impl Drop for StorageContextTransitionGuard {
    fn drop(&mut self) {
        if let Ok(mut coordinator) = self.coordinator.lock() {
            coordinator.transition_in_progress = false;
        }
    }
}

#[derive(Clone)]
pub struct StorageContextOperationGuard {
    _lease: Arc<StorageContextOperationLease>,
}

struct StorageContextOperationLease {
    coordinator: Arc<Mutex<StorageContextCoordinator>>,
    operation_id: u64,
    cancellation: Arc<AtomicBool>,
}

impl Drop for StorageContextOperationLease {
    fn drop(&mut self) {
        if let Ok(mut coordinator) = self.coordinator.lock() {
            coordinator.active_operations.remove(&self.operation_id);
        }
    }
}

#[derive(Clone)]
pub struct StorageContextShutdownGuard {
    lease: Arc<StorageContextShutdownLease>,
}

struct StorageContextShutdownLease {
    coordinator: Arc<Mutex<StorageContextCoordinator>>,
    committed: AtomicBool,
}

impl StorageContextShutdownGuard {
    pub fn commit_for_process_exit(self) {
        self.lease.committed.store(true, Ordering::SeqCst);
    }
}

impl Drop for StorageContextShutdownLease {
    fn drop(&mut self) {
        if self.committed.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut coordinator) = self.coordinator.lock() {
            coordinator.shutdown_exclusive_in_progress = false;
        }
    }
}

pub(crate) trait StorageContextTaskLease: Clone + Send + Sync + 'static {}

impl StorageContextTaskLease for StorageContextOperationGuard {}
impl StorageContextTaskLease for StorageContextShutdownGuard {}

pub(crate) fn spawn_storage_context_task<L, F>(
    lease: &L,
    future: F,
) -> tokio::task::JoinHandle<F::Output>
where
    L: StorageContextTaskLease,
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let lease = lease.clone();
    tokio::spawn(async move {
        let _lease = lease;
        future.await
    })
}

pub(crate) fn spawn_blocking_storage_context_task<L, F, T>(
    lease: &L,
    operation: F,
) -> tokio::task::JoinHandle<T>
where
    L: StorageContextTaskLease,
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let lease = lease.clone();
    tokio::task::spawn_blocking(move || {
        let _lease = lease;
        operation()
    })
}

#[derive(Default)]
struct InstanceMutationLocks {
    locks: HashMap<String, Weak<tokio::sync::Mutex<()>>>,
}

impl InstanceMutationLocks {
    fn lock_for(&mut self, instance_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        // Completed or cancelled mutations leave only weak entries. Pruning before
        // every acquisition bounds storage by live instance activity instead of every
        // instance ID observed during the process lifetime.
        self.locks.retain(|_, lock| lock.strong_count() > 0);

        if let Some(lock) = self.locks.get(instance_id).and_then(Weak::upgrade) {
            return lock;
        }

        let lock = Arc::new(tokio::sync::Mutex::new(()));
        self.locks
            .insert(instance_id.to_owned(), Arc::downgrade(&lock));
        lock
    }
}

fn initial_app_state(default_paths: &StoragePaths, storage: Option<StorageBootstrap>) -> AppState {
    let mut app_state = AppState::bootstrap_default();
    app_state.storage = default_paths.probe_status();

    if let Some(storage) = storage {
        app_state.settings = storage.settings;
        app_state.storage = storage.storage_status;
    }

    app_state
}

fn storage_status_ready(status: &StorageStatus) -> bool {
    status.database_exists && status.migrations_applied
}

#[derive(Debug, Clone)]
pub struct RuntimeStartReservationEntry {
    pub instance_id: String,
    pub source: String,
    pub reserved_at: Instant,
    pub console_log_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeStartReservationConflict {
    pub instance_id: String,
    pub source: String,
    pub pending_ms: u128,
}

#[derive(Debug)]
pub enum RuntimeStartReservationAttempt {
    Reserved(RuntimeStartReservationLease),
    ShutdownInProgress,
    Conflict(RuntimeStartReservationConflict),
}

#[derive(Clone)]
pub struct RuntimeStartReservationLease {
    lease: Arc<RuntimeStartReservationInner>,
}

struct RuntimeStartReservationInner {
    reservations: Arc<Mutex<RuntimeStartReservations>>,
    instance_id: String,
    _storage_context_operation: StorageContextOperationGuard,
}

impl RuntimeStartReservationLease {
    pub(crate) fn storage_operation(&self) -> &StorageContextOperationGuard {
        &self.lease._storage_context_operation
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.lease
            ._storage_context_operation
            ._lease
            .cancellation
            .load(Ordering::SeqCst)
    }

    pub(crate) async fn cancelled(&self) {
        self.lease._storage_context_operation.cancelled().await;
    }
}

impl std::fmt::Debug for RuntimeStartReservationLease {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RuntimeStartReservationLease")
            .field("instance_id", &self.lease.instance_id)
            .finish_non_exhaustive()
    }
}

impl StorageContextTaskLease for RuntimeStartReservationLease {}

impl Drop for RuntimeStartReservationInner {
    fn drop(&mut self) {
        if let Ok(mut reservations) = self.reservations.lock() {
            reservations.release(&self.instance_id);
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeStartReservations {
    pending: HashMap<String, RuntimeStartReservationEntry>,
}

impl RuntimeStartReservations {
    pub fn reserve(
        &mut self,
        instance_id: &str,
        source: &str,
    ) -> Result<RuntimeStartReservationEntry, RuntimeStartReservationConflict> {
        if let Some(existing) = self.pending.get(instance_id) {
            return Err(RuntimeStartReservationConflict {
                instance_id: existing.instance_id.clone(),
                source: existing.source.clone(),
                pending_ms: existing.reserved_at.elapsed().as_millis(),
            });
        }

        let entry = RuntimeStartReservationEntry {
            instance_id: instance_id.to_string(),
            source: source.to_string(),
            reserved_at: Instant::now(),
            console_log_path: None,
        };
        self.pending.insert(instance_id.to_string(), entry.clone());
        Ok(entry)
    }

    pub fn set_console_log_path(&mut self, instance_id: &str, console_log_path: &str) -> bool {
        let Some(entry) = self.pending.get_mut(instance_id) else {
            return false;
        };
        entry.console_log_path = Some(console_log_path.to_string());
        true
    }

    pub fn pending_console_log_path(&self, instance_id: &str) -> Option<String> {
        self.pending
            .get(instance_id)
            .and_then(|entry| entry.console_log_path.clone())
    }

    pub fn pending_instance_ids(&self) -> Vec<String> {
        let mut instance_ids = self.pending.keys().cloned().collect::<Vec<_>>();
        instance_ids.sort();
        instance_ids
    }

    pub fn release(&mut self, instance_id: &str) -> bool {
        self.pending.remove(instance_id).is_some()
    }

    #[cfg(test)]
    pub fn is_reserved(&self, instance_id: &str) -> bool {
        self.pending.contains_key(instance_id)
    }
}

#[derive(Debug)]
pub struct RuntimeStartupScheduler {
    next_start_after: Instant,
    last_startup_schedule: Option<RuntimeStartupSchedule>,
}

impl Default for RuntimeStartupScheduler {
    fn default() -> Self {
        Self {
            next_start_after: Instant::now(),
            last_startup_schedule: None,
        }
    }
}

impl RuntimeStartupScheduler {
    pub fn reserve_start(&mut self, request: RuntimeStartupReservation) -> RuntimeStartupSlot {
        let now = Instant::now();
        let delay = self.next_start_after.saturating_duration_since(now);
        let effective_stagger_ms = effective_runtime_startup_stagger_ms(&request);
        let next_base = if self.next_start_after > now {
            self.next_start_after
        } else {
            now
        };
        self.next_start_after = next_base + Duration::from_millis(effective_stagger_ms);
        let queued_start_count = queued_start_count(delay, effective_stagger_ms);
        RuntimeStartupSlot {
            delay,
            effective_stagger_ms,
            queued_start_count,
        }
    }

    pub fn preview_start(&self, request: RuntimeStartupReservation) -> RuntimeStartupSlot {
        let delay = self.next_start_delay();
        let effective_stagger_ms = effective_runtime_startup_stagger_ms(&request);
        let queued_start_count = queued_start_count(delay, effective_stagger_ms);
        RuntimeStartupSlot {
            delay,
            effective_stagger_ms,
            queued_start_count,
        }
    }

    pub fn next_start_delay(&self) -> Duration {
        self.next_start_after
            .saturating_duration_since(Instant::now())
    }

    pub fn record_startup_schedule(&mut self, schedule: RuntimeStartupSchedule) {
        self.last_startup_schedule = Some(schedule);
    }

    pub fn last_startup_schedule(&self) -> Option<RuntimeStartupSchedule> {
        self.last_startup_schedule.clone()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeStartupReservation {
    pub base_stagger_ms: u64,
    pub running_instance_count: usize,
    pub active_process_count: usize,
    pub process_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeStartupSlot {
    pub delay: Duration,
    pub effective_stagger_ms: u64,
    pub queued_start_count: usize,
}

fn effective_runtime_startup_stagger_ms(request: &RuntimeStartupReservation) -> u64 {
    let running_bonus =
        request.running_instance_count.min(8) as u64 * RUNNING_INSTANCE_STAGGER_BONUS_MS;
    let active_process_bonus =
        request.active_process_count.min(16) as u64 * ACTIVE_PROCESS_STAGGER_BONUS_MS;
    let process_bonus =
        request.process_count.saturating_sub(1).min(4) as u64 * EXTRA_PROCESS_STAGGER_BONUS_MS;
    request
        .base_stagger_ms
        .saturating_add(running_bonus)
        .saturating_add(active_process_bonus)
        .saturating_add(process_bonus)
        .min(MAX_RUNTIME_STARTUP_STAGGER_MS)
}

fn queued_start_count(delay: Duration, stagger_ms: u64) -> usize {
    if delay.is_zero() || stagger_ms == 0 {
        return 0;
    }

    let delay_ms = delay.as_millis();
    let stagger_ms = u128::from(stagger_ms);
    delay_ms.div_ceil(stagger_ms) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shard_exit_preserves_the_surviving_shards_log_lease() {
        let mut streams = RuntimeLogStreams::default();
        let master = streams.reserve("world", "master.log").unwrap();
        let caves = streams.reserve("world", "caves.log").unwrap();
        streams.release_path("world", "caves.log");
        assert!(streams.is_active("world", "master.log", master));
        assert!(!streams.is_active("world", "caves.log", caves));
        assert!(streams.reserve("world", "master.log").is_none());
    }

    #[test]
    fn runtime_log_stream_lease_survives_stop_and_same_path_restart() {
        let mut streams = RuntimeLogStreams::default();
        let instance_id = "restartable-server";
        let log_path = r"D:\LanGame\instances\restartable-server\server.log";

        let old_lease = streams
            .reserve(instance_id, log_path)
            .expect("first stream should reserve the log path");
        assert!(streams.reserve(instance_id, log_path).is_none());

        streams.release_instance(instance_id);
        let new_lease = streams
            .reserve(instance_id, log_path)
            .expect("restart should reserve the same log path with a new lease");

        assert_ne!(old_lease, new_lease);
        assert!(!streams.is_active(instance_id, log_path, old_lease));
        assert!(streams.is_active(instance_id, log_path, new_lease));
        assert!(
            !streams.release(instance_id, log_path, old_lease),
            "the old task must not release the restarted stream"
        );
        assert!(streams.is_active(instance_id, log_path, new_lease));
        assert!(streams.release(instance_id, log_path, new_lease));
    }

    #[tokio::test]
    async fn player_access_instance_mutations_serialize_without_blocking_other_instances() {
        let state = DesktopState::default();
        let first = state.acquire_instance_mutation("server-a").await;

        assert!(
            tokio::time::timeout(
                Duration::from_millis(20),
                state.acquire_instance_mutation("server-a"),
            )
            .await
            .is_err(),
            "the same instance must remain serialized while its mutation guard is held",
        );
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                state.acquire_instance_mutation("server-b"),
            )
            .await
            .is_ok(),
            "unrelated instances must not share one global mutation lock",
        );

        drop(first);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(1),
                state.acquire_instance_mutation("server-a"),
            )
            .await
            .is_ok(),
        );
    }

    #[tokio::test]
    async fn instance_mutation_lock_registry_stays_bounded_during_id_churn() {
        let state = DesktopState::default();

        for index in 0..2_048 {
            let guard = state
                .acquire_instance_mutation(&format!("removed-instance-{index}"))
                .await;
            drop(guard);
        }

        assert_eq!(state.tracked_instance_mutation_lock_count().await, 1);

        let current = state.acquire_instance_mutation("current-instance").await;
        assert_eq!(state.tracked_instance_mutation_lock_count().await, 1);
        drop(current);
    }

    #[tokio::test]
    async fn invalid_instance_ids_do_not_accumulate_mutation_locks() {
        let state = DesktopState::default();

        for instance_id in ["", " ", "../server", "server\0id", "\t\r\n"] {
            let guard = state.acquire_instance_mutation(instance_id).await;
            drop(guard);
        }

        assert_eq!(state.tracked_instance_mutation_lock_count().await, 1);

        let valid = state.acquire_instance_mutation("valid-instance").await;
        assert_eq!(state.tracked_instance_mutation_lock_count().await, 1);
        drop(valid);
    }

    #[tokio::test]
    async fn concurrent_instance_mutations_reuse_one_lock_per_instance() {
        use std::sync::atomic::AtomicUsize;

        let state = Arc::new(DesktopState::default());
        let barrier = Arc::new(tokio::sync::Barrier::new(32));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum_active = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();

        for _ in 0..32 {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            let active = Arc::clone(&active);
            let maximum_active = Arc::clone(&maximum_active);
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                let guard = state.acquire_instance_mutation("shared-instance").await;
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum_active.fetch_max(current, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                drop(guard);
            }));
        }

        for task in tasks {
            task.await.expect("mutation task should complete");
        }

        assert_eq!(maximum_active.load(Ordering::SeqCst), 1);
        assert_eq!(state.tracked_instance_mutation_lock_count().await, 1);
    }

    #[test]
    fn early_storage_bootstrap_failure_keeps_structured_diagnostic_paths() {
        let root = std::env::temp_dir().join(format!(
            "langame-desktop-state-probe-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let paths = StoragePaths {
            app_data_root: root.clone(),
            database_path: root.join("db").join("lgs.db"),
            logs_root: root.join("logs"),
            ..StoragePaths::default()
        };

        let app_state = initial_app_state(&paths, None);

        assert_eq!(
            app_state.storage.database_path,
            paths.database_path.to_string_lossy()
        );
        assert_eq!(
            app_state.storage.app_log_path,
            paths.app_log_path().to_string_lossy()
        );
        assert!(!app_state.storage.migrations_applied);
    }

    #[test]
    fn heartbeat_storage_readiness_requires_a_successful_migration_status() {
        let pending = StorageStatus {
            database_exists: true,
            migrations_applied: false,
            ..StorageStatus::default()
        };
        let ready = StorageStatus {
            migrations_applied: true,
            ..pending.clone()
        };

        assert!(!storage_status_ready(&pending));
        assert!(storage_status_ready(&ready));
    }

    #[test]
    fn startup_scheduler_scales_with_running_instances_and_process_count() {
        let request = RuntimeStartupReservation {
            base_stagger_ms: 1500,
            running_instance_count: 3,
            active_process_count: 0,
            process_count: 2,
        };

        assert_eq!(effective_runtime_startup_stagger_ms(&request), 2750);
    }

    #[test]
    fn startup_scheduler_scales_with_active_process_pressure() {
        let request = RuntimeStartupReservation {
            base_stagger_ms: 1500,
            running_instance_count: 2,
            active_process_count: 6,
            process_count: 1,
        };

        assert_eq!(effective_runtime_startup_stagger_ms(&request), 2750);
    }

    #[test]
    fn startup_scheduler_reports_queued_start_pressure() {
        assert_eq!(queued_start_count(Duration::from_millis(2750), 1500), 2);
        assert_eq!(queued_start_count(Duration::ZERO, 1500), 0);
    }

    #[test]
    fn startup_scheduler_keeps_last_schedule_diagnostics() {
        let mut scheduler = RuntimeStartupScheduler::default();
        let schedule = RuntimeStartupSchedule {
            delay_ms: 250,
            instance_stagger_ms: 1500,
            effective_stagger_ms: 2250,
            child_process_stagger_ms: 500,
            running_instance_count: 2,
            active_process_count: 5,
            process_count: 3,
            queued_start_count: 1,
            reason: String::from("test schedule"),
        };

        scheduler.record_startup_schedule(schedule.clone());

        assert_eq!(
            scheduler
                .last_startup_schedule()
                .map(|snapshot| snapshot.process_count),
            Some(schedule.process_count)
        );
    }

    #[test]
    fn startup_scheduler_preview_does_not_reserve_a_slot() {
        let scheduler = RuntimeStartupScheduler::default();
        let request = RuntimeStartupReservation {
            base_stagger_ms: 1500,
            running_instance_count: 2,
            active_process_count: 4,
            process_count: 2,
        };

        let preview = scheduler.preview_start(request);

        assert_eq!(preview.effective_stagger_ms, 3000);
        assert_eq!(preview.queued_start_count, 0);
        assert_eq!(scheduler.next_start_delay(), Duration::ZERO);
    }

    #[test]
    fn runtime_start_reservations_block_duplicate_instance_starts_until_released() {
        let mut reservations = RuntimeStartReservations::default();

        reservations
            .reserve("abiotic-factor", "manual")
            .expect("first start should reserve the instance");
        let duplicate = reservations
            .reserve("abiotic-factor", "manual")
            .expect_err("second start for the same instance should be blocked");

        assert_eq!(duplicate.instance_id, "abiotic-factor");
        assert_eq!(duplicate.source, "manual");
        assert!(reservations.is_reserved("abiotic-factor"));
        assert!(reservations.release("abiotic-factor"));
        assert!(!reservations.is_reserved("abiotic-factor"));
        reservations
            .reserve("abiotic-factor", "manual")
            .expect("released instance should be startable again");
    }

    #[test]
    fn shutdown_rejects_new_runtime_start_reservations_and_keeps_pending_ids_visible() {
        let state = DesktopState::default();

        let reservation = match state
            .try_reserve_runtime_start("already-pending", "manual")
            .expect("initial reservation")
        {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            attempt => panic!("unexpected reservation attempt: {attempt:?}"),
        };
        state.shutdown_in_progress.store(true, Ordering::SeqCst);

        assert!(matches!(
            state
                .try_reserve_runtime_start("too-late", "auto_restart")
                .expect("shutdown rejection"),
            RuntimeStartReservationAttempt::ShutdownInProgress
        ));
        assert_eq!(
            state
                .pending_runtime_start_instance_ids()
                .expect("pending start snapshot"),
            vec![String::from("already-pending")]
        );
        drop(reservation);
    }

    #[test]
    fn cloned_storage_operation_blocks_transition_until_last_lease_drops() {
        let state = DesktopState::default();
        let operation = state
            .begin_storage_context_operation("clone lease test")
            .expect("storage operation");
        let worker_operation = operation.clone();
        drop(operation);

        assert!(state.begin_storage_context_transition().is_err());
        drop(worker_operation);
        state
            .begin_storage_context_transition()
            .expect("last clone should release the storage operation");
    }

    #[test]
    fn shutdown_exclusive_is_atomic_and_rolls_back_when_not_committed() {
        let state = DesktopState::default();
        let operation = state
            .begin_storage_context_operation("active writer")
            .expect("storage operation");
        state.shutdown_in_progress.store(true, Ordering::SeqCst);

        assert!(state.begin_storage_shutdown_exclusive().is_err());
        drop(operation);
        let shutdown = state
            .begin_storage_shutdown_exclusive()
            .expect("released writer should allow shutdown exclusivity");
        assert!(
            state
                .begin_storage_context_operation("late writer")
                .is_err()
        );

        drop(shutdown);
        state.shutdown_in_progress.store(false, Ordering::SeqCst);
        state
            .begin_storage_context_operation("retry writer")
            .expect("uncommitted shutdown exclusivity should roll back");
    }

    #[test]
    fn committed_shutdown_exclusive_keeps_new_storage_operations_sealed() {
        let state = DesktopState::default();
        state.shutdown_in_progress.store(true, Ordering::SeqCst);
        state
            .begin_storage_shutdown_exclusive()
            .expect("shutdown exclusivity")
            .commit_for_process_exit();

        state.shutdown_in_progress.store(false, Ordering::SeqCst);
        assert!(
            state
                .begin_storage_context_operation("post-exit-request writer")
                .is_err(),
            "committed shutdown exclusivity must remain sealed until process exit"
        );
    }

    #[tokio::test]
    async fn aborted_runtime_start_releases_reservation_and_storage_operation() {
        let state = Arc::new(DesktopState::default());
        let worker_state = Arc::clone(&state);
        let (reserved_tx, reserved_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn(async move {
            let _reservation = match worker_state
                .try_reserve_runtime_start("abortable-start", "manual")
                .expect("runtime start reservation")
            {
                RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
                attempt => panic!("unexpected reservation attempt: {attempt:?}"),
            };
            reserved_tx.send(()).expect("reservation signal");
            std::future::pending::<()>().await;
        });
        reserved_rx.await.expect("reservation signal");
        assert_eq!(
            state
                .pending_runtime_start_instance_ids()
                .expect("pending starts"),
            vec![String::from("abortable-start")]
        );
        assert!(state.begin_storage_context_transition().is_err());

        worker.abort();
        let _ = worker.await;
        assert!(
            state
                .pending_runtime_start_instance_ids()
                .expect("pending starts after abort")
                .is_empty()
        );
        state
            .begin_storage_context_transition()
            .expect("aborted caller should release the reservation lease");
    }

    #[test]
    fn runtime_start_reservations_track_pending_console_log_path() {
        let mut reservations = RuntimeStartReservations::default();

        reservations
            .reserve("abiotic-factor", "manual")
            .expect("start should reserve the instance");
        assert_eq!(
            reservations.pending_console_log_path("abiotic-factor"),
            None
        );

        reservations.set_console_log_path(
            "abiotic-factor",
            "D:/LanGame/instances/abiotic/logs/run-1-main.log",
        );

        assert_eq!(
            reservations.pending_console_log_path("abiotic-factor"),
            Some(String::from(
                "D:/LanGame/instances/abiotic/logs/run-1-main.log"
            ))
        );
        assert_eq!(reservations.pending_console_log_path("other"), None);

        reservations.release("abiotic-factor");
        assert_eq!(
            reservations.pending_console_log_path("abiotic-factor"),
            None
        );
    }

    #[test]
    fn runtime_restart_scheduler_tracks_due_entries_without_blocking() {
        let mut scheduler = RuntimeRestartScheduler::default();

        let _ = scheduler.schedule(RuntimeRestartScheduleRequest {
            instance_id: String::from("late"),
            instance_name: String::from("Late"),
            backoff: Duration::from_secs(60),
            recent_crash_count: 1,
            exit_code: Some(1),
        });
        let _ = scheduler.schedule(RuntimeRestartScheduleRequest {
            instance_id: String::from("now"),
            instance_name: String::from("Now"),
            backoff: Duration::ZERO,
            recent_crash_count: 1,
            exit_code: None,
        });

        let due = scheduler.take_due();

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].instance_id, "now");
        assert_eq!(scheduler.pending_count(), 1);
        assert!(scheduler.next_restart_delay() > Duration::ZERO);
    }

    #[test]
    fn runtime_restart_scheduler_exposes_next_restart_snapshot() {
        let mut scheduler = RuntimeRestartScheduler::default();

        let _ = scheduler.schedule(RuntimeRestartScheduleRequest {
            instance_id: String::from("late"),
            instance_name: String::from("Late Server"),
            backoff: Duration::from_secs(60),
            recent_crash_count: 2,
            exit_code: Some(3),
        });
        let _ = scheduler.schedule(RuntimeRestartScheduleRequest {
            instance_id: String::from("soon"),
            instance_name: String::from("Soon Server"),
            backoff: Duration::from_millis(50),
            recent_crash_count: 1,
            exit_code: Some(1),
        });

        let next = scheduler.next_restart().expect("next restart");
        let selected = scheduler
            .pending_restart_for("late")
            .expect("selected restart");

        assert_eq!(next.instance_id, "soon");
        assert_eq!(next.instance_name, "Soon Server");
        assert_eq!(next.recent_crash_count, 1);
        assert_eq!(next.exit_code, Some(1));
        assert_eq!(selected.instance_id, "late");
        assert_eq!(selected.recent_crash_count, 2);
    }
}
