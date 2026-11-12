use super::*;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MAX_PREPARATIONS: usize = 32;
const PREPARATION_LEASE: Duration = Duration::from_secs(15 * 60);

pub(super) struct PreparedTicket {
    pub instance_id: String,
    pub module_id: String,
    pub config_path: String,
    pub storage_root: PathBuf,
    pub consumer_app_id: u32,
    pub ids: Vec<String>,
    pub job_id: String,
    pub prepared: Option<Arc<app_steamcmd::PreparedWorkshopDownload>>,
    pub operation: StorageContextOperationGuard,
    pub workspace: Option<PathBuf>,
    pub finished: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for PreparedTicket {
    fn drop(&mut self) {
        // Publication is awaited before a ticket is released. Keep the source
        // lease and storage context through deletion of our private scratch.
        if let Some(path) = self.workspace.take() {
            let prepared = self.prepared.take();
            let operation = self.operation.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(move || {
                    let _operation = operation;
                    let _prepared = prepared;
                    if let Err(error) = remove_preparation_workspace(&path) {
                        eprintln!(
                            "Workshop scratch cleanup failed at {}: {error}",
                            path.display()
                        );
                    }
                });
            }
        }
    }
}

pub(super) fn remove_preparation_workspace(path: &Path) -> std::io::Result<()> {
    // Only server-generated UUID children of our request directory are removable.
    if path
        .parent()
        .and_then(Path::file_name)
        .and_then(|v| v.to_str())
        != Some("requests")
        || path
            .file_name()
            .and_then(|v| v.to_str())
            .and_then(|v| uuid::Uuid::parse_str(v).ok())
            .is_none()
    {
        return Err(std::io::Error::other("invalid Workshop scratch ownership"));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    app_steamcmd::steam_depot::check_plain(path, true)?;
    fs::remove_dir_all(path)
}

enum Entry {
    Preparing(String),
    Ready(Box<PreparedTicket>, Instant),
}

pub(crate) struct WorkshopPreparations {
    entries: Mutex<HashMap<String, Entry>>,
    workers: Arc<Semaphore>,
    free_workers: Mutex<Vec<usize>>,
    usable_workers: AtomicUsize,
}

impl Default for WorkshopPreparations {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            workers: Arc::new(Semaphore::new(2)),
            free_workers: Mutex::new(vec![1, 0]),
            usable_workers: AtomicUsize::new(2),
        }
    }
}

impl WorkshopPreparations {
    pub(super) fn reserve(
        self: &Arc<Self>,
        id: &str,
        instance_id: &str,
    ) -> Result<PreparationReservation, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Workshop preparation registry poisoned")?;
        if entries.contains_key(id) {
            return Err("Workshop attempt has already been accepted".into());
        }
        if entries.len() >= MAX_PREPARATIONS {
            return Err(
                "Too many uncommitted Workshop preparations; finish or discard an earlier request"
                    .into(),
            );
        }
        entries.insert(id.into(), Entry::Preparing(instance_id.into()));
        Ok(PreparationReservation {
            store: self.clone(),
            id: id.into(),
            published: false,
        })
    }

    pub(super) async fn worker(
        self: &Arc<Self>,
        operation: &StorageContextOperationGuard,
    ) -> Result<WorkshopWorker, String> {
        let permit = tokio::select! {
            _ = operation.cancelled() => return Err("installation_cancelled".into()),
            permit = self.workers.clone().acquire_owned() => permit.map_err(|_|
                "Workshop download workers are unavailable after a process cleanup failure; fully quit and restart the app before retrying")?,
        };
        let id = self
            .free_workers
            .lock()
            .map_err(|_| "Workshop workers poisoned")?
            .pop()
            .expect("a semaphore permit owns one worker");
        Ok(WorkshopWorker {
            id,
            store: self.clone(),
            permit: Some(permit),
        })
    }

    pub(super) fn take(
        &self,
        id: &str,
        instance_id: &str,
    ) -> Result<Option<Box<PreparedTicket>>, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Workshop preparation registry poisoned")?;
        match entries.get(id) {
            Some(Entry::Preparing(owner)) if owner == instance_id => {
                return Err("Workshop preparation is still running".into());
            }
            Some(Entry::Ready(ticket, _)) if ticket.instance_id == instance_id => {}
            Some(_) => return Err("Workshop preparation belongs to another instance".into()),
            None => return Ok(None),
        }
        match entries.remove(id) {
            Some(Entry::Ready(ticket, _)) => Ok(Some(ticket)),
            _ => unreachable!("entry was checked while holding the registry lock"),
        }
    }

    pub(super) fn retain(&self, id: &str, instance_id: &str) -> Result<(), String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Workshop preparation registry poisoned")?;
        match entries.get_mut(id) {
            Some(Entry::Ready(ticket, expires)) if ticket.instance_id == instance_id => {
                *expires = Instant::now() + PREPARATION_LEASE;
                Ok(())
            }
            Some(_) => Err("Workshop preparation is unavailable for this instance".into()),
            None => Err("Workshop preparation is no longer available".into()),
        }
    }

    pub(super) fn take_expired(
        &self,
        id: &str,
        now: Instant,
    ) -> Result<Option<Box<PreparedTicket>>, String> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| "Workshop preparation registry poisoned")?;
        if !matches!(entries.get(id), Some(Entry::Ready(_, expires)) if *expires <= now) {
            return Ok(None);
        }
        match entries.remove(id) {
            Some(Entry::Ready(ticket, _)) => Ok(Some(ticket)),
            _ => unreachable!("expiry was checked under the same registry lock"),
        }
    }
}

pub(super) struct WorkshopWorker {
    pub id: usize,
    store: Arc<WorkshopPreparations>,
    permit: Option<OwnedSemaphorePermit>,
}

impl WorkshopWorker {
    pub(super) fn quarantine(&mut self) {
        // An unconfirmed native process can still write this runtime. Remove
        // its slot until the backend restarts; never clear or reuse that path.
        if let Some(permit) = self.permit.take() {
            permit.forget();
            if self.store.usable_workers.fetch_sub(1, Ordering::SeqCst) == 1 {
                self.store.workers.close();
            }
        }
    }
}

impl Drop for WorkshopWorker {
    fn drop(&mut self) {
        if self.permit.is_some()
            && let Ok(mut free) = self.store.free_workers.lock()
        {
            free.push(self.id);
        }
    }
}

pub(super) struct PreparationReservation {
    store: Arc<WorkshopPreparations>,
    id: String,
    published: bool,
}

impl PreparationReservation {
    pub(super) fn publish(
        mut self,
        mut ticket: PreparedTicket,
        app_state: Arc<std::sync::RwLock<AppState>>,
    ) -> Result<(), String> {
        let (finished, mut released) = tokio::sync::oneshot::channel();
        ticket.finished = Some(finished);
        let operation = ticket.operation.clone();
        let instance_id = ticket.instance_id.clone();
        let job_id = ticket.job_id.clone();
        self.store
            .entries
            .lock()
            .map_err(|_| "Workshop preparation registry poisoned")?
            .insert(
                self.id.clone(),
                Entry::Ready(Box::new(ticket), Instant::now() + PREPARATION_LEASE),
            );
        self.published = true;
        let store = Arc::downgrade(&self.store);
        let id = self.id.clone();
        tokio::spawn(async move {
            loop {
                let cancelled = tokio::select! {
                    _ = &mut released => break,
                    _ = operation.cancelled() => true,
                    _ = tokio::time::sleep(Duration::from_secs(30)) => false,
                };
                let Some(store) = store.upgrade() else { break };
                let discarded = if cancelled {
                    store.take(&id, &instance_id)
                } else {
                    store.take_expired(&id, Instant::now())
                };
                if let Ok(Some(ticket)) = discarded {
                    drop(ticket);
                    if let Ok(mut state) = app_state.write()
                        && let Some(job) = state.jobs.iter_mut().find(|job| job.id == job_id)
                    {
                        job.status = JobStatus::Failed;
                        job.detail = Some(if cancelled { "Workshop preparation cancelled before deployment" }
                            else { "Workshop preparation expired after the client stopped retaining it; retry the download" }.into());
                    }
                    break;
                }
                if cancelled {
                    // Commit has already claimed this ticket. Its owned task
                    // must finish before shutdown; do not spin on cancellation.
                    let _ = released.await;
                    break;
                }
            }
        });
        Ok(())
    }
}

impl Drop for PreparationReservation {
    fn drop(&mut self) {
        if !self.published
            && let Ok(mut entries) = self.store.entries.lock()
        {
            entries.remove(&self.id);
        }
    }
}
