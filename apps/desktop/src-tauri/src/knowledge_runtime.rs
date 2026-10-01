//! The existing runtime service owns one knowledge update, independent of UI connections.
use app_knowledge::{
    KnowledgeLibrary, KnowledgeSettings, KnowledgeStatus, SyncProgress, SyncReport,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;

use crate::state::{DesktopState, spawn_storage_context_task};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum KnowledgeJobState {
    Running,
    Cancelling,
    Completed,
    Partial,
    Failed,
    Cancelled,
    Interrupted,
}

impl KnowledgeJobState {
    fn active(&self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSyncJob {
    pub id: String,
    pub module_id: Option<String>,
    pub state: KnowledgeJobState,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub progress: SyncProgress,
    pub report: Option<SyncReport>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeRuntimeStatus {
    pub library: KnowledgeStatus,
    pub job: Option<KnowledgeSyncJob>,
    pub scheduler_error: Option<String>,
}

#[derive(Default)]
struct RuntimeState {
    job: Option<KnowledgeSyncJob>,
    cancellation: Option<Arc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
    scheduler_error: Option<String>,
}

pub(crate) struct KnowledgeCoordinator {
    root: PathBuf,
    modules_root: PathBuf,
    library: AsyncMutex<Option<Arc<KnowledgeLibrary>>>,
    state: Mutex<RuntimeState>,
    admission: AsyncMutex<()>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
    stopping: AtomicBool,
    wake: Notify,
}

impl Default for KnowledgeCoordinator {
    fn default() -> Self {
        let paths = app_storage::StoragePaths::default();
        Self::new(paths.app_data_root.join("knowledge"), paths.modules_root)
    }
}

impl KnowledgeCoordinator {
    fn new(root: PathBuf, modules_root: PathBuf) -> Self {
        Self {
            root,
            modules_root,
            library: AsyncMutex::new(None),
            state: Mutex::new(RuntimeState::default()),
            admission: AsyncMutex::new(()),
            scheduler: Mutex::new(None),
            stopping: AtomicBool::new(false),
            wake: Notify::new(),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, RuntimeState>, String> {
        self.state
            .lock()
            .map_err(|_| "Knowledge runtime lock poisoned".into())
    }

    pub(crate) async fn library(&self) -> Result<Arc<KnowledgeLibrary>, String> {
        if self.stopping.load(Ordering::Acquire) {
            return Err("Knowledge runtime is shutting down".into());
        }
        let mut slot = self.library.lock().await;
        // A reader may have passed the first check before shutdown acquired the
        // library lock. Never reopen the pool after the runtime has been sealed.
        if self.stopping.load(Ordering::Acquire) {
            return Err("Knowledge runtime is shutting down".into());
        }
        if let Some(library) = slot.as_ref() {
            return Ok(library.clone());
        }
        let library = Arc::new(
            KnowledgeLibrary::open(&self.root, &self.modules_root)
                .await
                .map_err(|e| e.to_string())?,
        );
        match self.load_job().await {
            Ok(job) => self.lock()?.job = job,
            Err(error) => self.lock()?.scheduler_error = Some(error),
        }
        *slot = Some(library.clone());
        Ok(library)
    }

    pub(crate) async fn status(&self) -> Result<KnowledgeRuntimeStatus, String> {
        let library = self
            .library()
            .await?
            .status()
            .await
            .map_err(|e| e.to_string())?;
        let state = self.lock()?;
        Ok(KnowledgeRuntimeStatus {
            library,
            job: state.job.clone(),
            scheduler_error: state.scheduler_error.clone(),
        })
    }

    pub(crate) async fn update_settings(&self, settings: KnowledgeSettings) -> Result<(), String> {
        self.library()
            .await?
            .update_settings(settings)
            .await
            .map_err(|e| e.to_string())?;
        self.wake.notify_one();
        Ok(())
    }

    pub(crate) async fn start(
        self: &Arc<Self>,
        state: &DesktopState,
        module_id: Option<String>,
        force: bool,
    ) -> Result<KnowledgeSyncJob, String> {
        let _admission = self.admission.lock().await;
        if self.stopping.load(Ordering::Acquire) {
            return Err("Knowledge runtime is shutting down".into());
        }
        self.reap_worker().await?;
        if let Some(job) = self.lock()?.job.as_ref().filter(|job| job.state.active()) {
            if job.module_id != module_id {
                return Err(
                    "A knowledge update for a different game scope is already running".into(),
                );
            }
            return Ok(job.clone());
        }
        // A terminal result is published just before the task releases its lease.
        // Join that final tail before replacing its handle with the next update.
        let previous = self.lock()?.worker.take();
        if let Some(previous) = previous {
            previous
                .await
                .map_err(|error| format!("Knowledge worker cleanup failed: {error}"))?;
        }
        let operation = state.begin_storage_context_operation("official game knowledge sync")?;
        let library = self.library().await?;
        if let Some(module) = module_id.as_ref() {
            let status = library.status().await.map_err(|e| e.to_string())?;
            if !status.games.iter().any(|game| &game.module_id == module) {
                return Err("Unknown knowledge game module".into());
            }
        }
        let job = KnowledgeSyncJob {
            id: uuid::Uuid::new_v4().to_string(),
            module_id,
            state: KnowledgeJobState::Running,
            started_at: app_knowledge::unix_seconds(),
            finished_at: None,
            progress: SyncProgress::default(),
            report: None,
            error: None,
        };
        self.persist_job(&job).await?;
        let cancel = operation.cancellation_token();
        {
            let mut runtime = self.lock()?;
            runtime.job = Some(job.clone());
            runtime.cancellation = Some(cancel.clone());
            runtime.scheduler_error = None;
        }
        let owner = self.clone();
        let worker_job = job.clone();
        let task = spawn_storage_context_task(&operation, async move {
            let progress_owner = owner.clone();
            let id = worker_job.id.clone();
            let progress = Arc::new(move |progress: SyncProgress| {
                if let Ok(mut state) = progress_owner.lock()
                    && let Some(job) = state.job.as_mut().filter(|job| job.id == id)
                {
                    job.progress = progress;
                }
            });
            // Cancellation is cooperative: retain the storage lease until the library
            // has settled its downloads/index transaction, including blocking inference.
            let deadline_cancel = cancel.clone();
            let result = {
                let mut sync = Box::pin(library.sync(
                    worker_job.module_id.as_deref(),
                    force,
                    cancel,
                    progress,
                ));
                tokio::select! {
                    result = &mut sync => result,
                    _ = tokio::time::sleep(Duration::from_secs(2 * 3600)) => {
                        deadline_cancel.store(true, Ordering::Release);
                        let _ = sync.await;
                        Err(app_knowledge::KnowledgeError::Unavailable("Knowledge update exceeded its two-hour limit and was cancelled; the last published index remains available.".into()))
                    }
                }
            };
            owner.finish(worker_job, result).await;
        });
        self.lock()?.worker = Some(task);
        Ok(job)
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<bool, String> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid knowledge job identifier")?;
        let mut state = self.lock()?;
        if !state
            .job
            .as_ref()
            .is_some_and(|job| job.id == id && job.state.active())
        {
            return Ok(false);
        }
        if let Some(cancel) = state.cancellation.as_ref() {
            cancel.store(true, Ordering::Release);
        }
        if let Some(job) = state.job.as_mut() {
            job.state = KnowledgeJobState::Cancelling;
        }
        Ok(true)
    }

    async fn finish(&self, mut job: KnowledgeSyncJob, result: app_knowledge::Result<SyncReport>) {
        match result {
            Ok(report) => {
                job.state = if report.cancelled {
                    KnowledgeJobState::Cancelled
                } else if report.sources_failed > 0 || !report.errors.is_empty() {
                    KnowledgeJobState::Partial
                } else {
                    KnowledgeJobState::Completed
                };
                job.report = Some(report);
            }
            Err(error) => {
                job.state = if matches!(error, app_knowledge::KnowledgeError::Cancelled) {
                    KnowledgeJobState::Cancelled
                } else {
                    KnowledgeJobState::Failed
                };
                job.error = Some(error.to_string());
            }
        }
        job.finished_at = Some(app_knowledge::unix_seconds());
        if let Ok(state) = self.lock()
            && let Some(active) = state.job.as_ref().filter(|active| active.id == job.id)
        {
            job.progress = active.progress.clone();
        }
        let persistence = self.persist_job(&job).await;
        if let Ok(mut state) = self.lock() {
            state.job = Some(job);
            state.cancellation = None;
            if let Err(error) = persistence {
                state.scheduler_error = Some(error);
            }
        }
        self.wake.notify_one();
    }

    async fn reap_worker(&self) -> Result<(), String> {
        let finished = {
            let mut state = self.lock()?;
            if state.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                state.worker.take()
            } else {
                None
            }
        };
        if let Some(worker) = finished
            && let Err(error) = worker.await
        {
            let job = self.lock()?.job.clone();
            if let Some(job) = job {
                self.finish(
                    job,
                    Err(app_knowledge::KnowledgeError::Unavailable(format!(
                        "Knowledge worker failed: {error}"
                    ))),
                )
                .await;
            }
        }
        Ok(())
    }

    async fn persist_job(&self, job: &KnowledgeSyncJob) -> Result<(), String> {
        let bytes = serde_json::to_vec(job).map_err(|e| e.to_string())?;
        let staging = self
            .root
            .join(format!("runtime-job-{}.json", uuid::Uuid::new_v4()));
        let write = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staging)
                .await?;
            file.write_all(&bytes).await?;
            file.sync_all().await
        }
        .await;
        if let Err(error) = write {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(format!("Cannot save knowledge job: {error}"));
        }
        if let Err(error) = tokio::fs::rename(&staging, self.root.join("runtime-job.json")).await {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(format!("Cannot publish knowledge job: {error}"));
        }
        Ok(())
    }

    async fn load_job(&self) -> Result<Option<KnowledgeSyncJob>, String> {
        let path = self.root.join("runtime-job.json");
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Cannot read knowledge job: {error}")),
        };
        if metadata.len() > 256 * 1024 {
            return Err("Knowledge job journal exceeds its size limit".into());
        }
        let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
        let mut job: KnowledgeSyncJob = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid knowledge job journal: {e}"))?;
        if job.state.active() {
            job.state = KnowledgeJobState::Interrupted;
            job.finished_at = Some(app_knowledge::unix_seconds());
            job.error = Some("The previous runtime stopped during an update; the last published index remains available.".into());
            self.persist_job(&job).await?;
        }
        Ok(Some(job))
    }

    pub(crate) async fn shutdown(&self) {
        {
            // Share admission with start(): every accepted job is registered
            // before sealing, and its cancellation remains owned until join.
            let _admission = self.admission.lock().await;
            self.stopping.store(true, Ordering::Release);
            if let Ok(state) = self.lock()
                && let Some(cancel) = state.cancellation.as_ref()
            {
                cancel.store(true, Ordering::Release);
            }
            self.wake.notify_one();
        }
        let scheduler = self.scheduler.lock().ok().and_then(|mut slot| slot.take());
        if let Some(scheduler) = scheduler
            && let Err(error) = scheduler.await
        {
            eprintln!("Knowledge scheduler stopped unexpectedly: {error}");
        }
        if let Err(error) = self.reap_worker().await {
            eprintln!("Knowledge worker cleanup failed: {error}");
        }
        let worker = self.lock().ok().and_then(|mut state| state.worker.take());
        if let Some(worker) = worker
            && let Err(error) = worker.await
        {
            eprintln!("Knowledge worker stopped unexpectedly: {error}");
        }
        if let Some(library) = self.library.lock().await.take() {
            library.close().await;
        }
    }
}

fn due(settings: &KnowledgeSettings, last_finished: Option<u64>, now: u64) -> bool {
    settings.auto_update
        && last_finished.is_none_or(|last| {
            now.saturating_sub(last) >= u64::from(settings.interval_hours) * 3600
        })
}

pub(crate) fn spawn_knowledge_scheduler<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    let owner = app.state::<DesktopState>().knowledge.clone();
    let Ok(mut slot) = owner.scheduler.lock() else {
        return;
    };
    if slot.is_some() {
        return;
    }
    let coordinator = owner.clone();
    // Tauri setup runs on the native UI thread, outside an entered Tokio runtime.
    *slot = Some(tauri::async_runtime::handle().inner().spawn(async move {
        // An error is retried with a bounded delay, never on every UI status poll.
        let mut retry_after = 0;
        loop {
            if coordinator.stopping.load(Ordering::Acquire) {
                break;
            }
            let state = app.state::<DesktopState>();
            if state.shutdown_completed.load(Ordering::SeqCst) {
                break;
            }
            let now = app_knowledge::unix_seconds();
            if !state.shutdown_in_progress.load(Ordering::SeqCst)
                && state.is_storage_ready()
                && now >= retry_after
            {
                let result = async {
                    let _operation =
                        state.begin_storage_context_operation("knowledge update schedule check")?;
                    coordinator.reap_worker().await?;
                    let status = coordinator.status().await?;
                    let active = status.job.as_ref().is_some_and(|job| job.state.active());
                    let last = status
                        .job
                        .as_ref()
                        .and_then(|job| job.finished_at)
                        .or_else(|| status.library.last_run.as_ref().map(|run| run.finished_at));
                    if !active && due(&status.library.settings, last, now) {
                        coordinator.start(&state, None, false).await?;
                    }
                    Ok::<_, String>(())
                }
                .await;
                if let Err(error) = result {
                    if let Ok(mut state) = coordinator.lock() {
                        state.scheduler_error = Some(error);
                    }
                    retry_after = now.saturating_add(300);
                }
            }
            tokio::select! {
                _ = coordinator.wake.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(30)) => {},
            }
        }
    }));
}

#[cfg(test)]
#[path = "knowledge_runtime_tests.rs"]
mod tests;
