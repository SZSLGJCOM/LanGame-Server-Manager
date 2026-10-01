use std::fs;
use std::path::{Path, PathBuf};

use app_core::{InstanceDetails, InstanceStatus, UpdateInstanceInput};
use serde::{Deserialize, Serialize};

use crate::atomic_file::{compare_and_swap_optional_file_atomically, create_file_atomically};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
};
use crate::{InstanceProgramMode, StorageError, StoragePaths};

#[path = "windrose_bootstrap_state.rs"]
mod native;
use native::{MAX_DOCUMENT, SERVER, checked_path, inspect, read_bytes};

const JOURNAL: &str = ".langame-windrose-bootstrap.json";
const MAX_JOURNAL: u64 = MAX_DOCUMENT * 4 + 4096;

#[derive(Debug, Clone)]
pub struct WindroseBootstrapObservation {
    pub world_ready: bool,
    pub use_direct_connection: Option<bool>,
    pub direct_connection_server_port: Option<u16>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    instance_id: String,
    install_root: PathBuf,
    original: Option<Vec<u8>>,
}

/// The caller owns the native process and must confirm its complete shutdown
/// before finishing or aborting. Dropping the session preserves recovery data.
pub struct WindroseBootstrap {
    paths: StoragePaths,
    instance_id: String,
    lock: InstanceSettingsLock,
    root: PathBuf,
    config: PathBuf,
    journal: Journal,
    journal_bytes: Vec<u8>,
}

pub async fn prepare_windrose_bootstrap(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<Option<WindroseBootstrap>, StorageError> {
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let paths = paths.clone();
    let instance_id = instance_id.to_owned();
    let worker_lock = lock.clone();
    lock.complete_mutation("preparing Windrose native bootstrap", async move {
        let details = stopped(&paths, &instance_id).await?;
        let worker = worker_lock.clone();
        worker_lock
            .spawn_blocking(move || prepare(paths, details, worker))
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "preparing Windrose native bootstrap files",
                message: error.to_string(),
            })?
    })
    .await
}

fn prepare(
    paths: StoragePaths,
    details: InstanceDetails,
    lock: InstanceSettingsLock,
) -> Result<Option<WindroseBootstrap>, StorageError> {
    let config = Path::new(&details.config_file_path)
        .parent()
        .ok_or_else(|| {
            invalid(
                Path::new(&details.config_file_path),
                "missing instance config directory",
            )
        })?
        .to_owned();
    let instance = config
        .parent()
        .ok_or_else(|| invalid(&config, "missing managed instance root"))?;
    if crate::instance_program_mode(instance)? != InstanceProgramMode::Independent {
        return Err(invalid(
            instance,
            "native bootstrap requires an independent runtime",
        ));
    }
    let root = crate::resolve_instance_runtime_root(instance)?;
    let canonical =
        fs::canonicalize(&root).map_err(|_| invalid(&root, "cannot resolve native runtime"))?;
    let state = inspect(&root)?;
    let journal_bytes = read_bytes(&config, Path::new(JOURNAL), MAX_JOURNAL)?;
    if journal_bytes.is_none() && state.observation.world_ready {
        return Ok(None);
    }
    let settings: serde_json::Value = serde_json::from_str(&details.settings_json)?;
    if let Some(selected) = settings
        .get("world_island_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    {
        // The settings transaction may have committed the verified native ID
        // before the process ended or the journal was cleared. Resume only it.
        if journal_bytes.is_none() || state.world_id.as_deref() != Some(selected) {
            return Err(invalid(
                &root,
                "an explicitly selected world must not be bootstrapped",
            ));
        }
    }
    let original = read_bytes(&root, Path::new(SERVER), MAX_DOCUMENT)?;
    let (journal, journal_bytes) = if let Some(bytes) = journal_bytes {
        let journal: Journal = serde_json::from_slice(&bytes)
            .map_err(|_| invalid(&root, "bootstrap journal is malformed"))?;
        if journal.version != 1
            || journal.instance_id != details.summary.id
            || journal.install_root != canonical
            || journal
                .original
                .as_ref()
                .is_some_and(|bytes| bytes.len() as u64 > MAX_DOCUMENT)
        {
            return Err(invalid(
                &root,
                "bootstrap journal does not match this managed instance",
            ));
        }
        (journal, bytes)
    } else {
        if state.has_worlds || !state.uninitialized {
            return Err(invalid(
                &root,
                "existing native identity or world data requires recovery, not a fresh bootstrap",
            ));
        }
        let journal = Journal {
            version: 1,
            instance_id: details.summary.id.clone(),
            install_root: canonical,
            original,
        };
        let bytes = serde_json::to_vec(&journal)?;
        if bytes.len() as u64 > MAX_JOURNAL {
            return Err(invalid(&root, "bootstrap journal exceeds its size limit"));
        }
        let path = config.join(JOURNAL);
        create_file_atomically(&path, &bytes).map_err(|error| {
            invalid(
                &root,
                &format!("cannot publish bootstrap recovery journal: {error}"),
            )
        })?;
        (journal, bytes)
    };
    let session = WindroseBootstrap {
        paths,
        instance_id: details.summary.id,
        lock,
        root,
        config,
        journal,
        journal_bytes,
    };
    // A crash may have persisted the journal before removing the unchanged
    // uninitialized file. Never remove a native partial result on retry.
    if !state.has_worlds && !state.observation.world_ready {
        let current = read_bytes(&session.root, Path::new(SERVER), MAX_DOCUMENT)?;
        if current == session.journal.original {
            session.swap_server(current.as_deref(), None)?;
        }
    }
    Ok(Some(session))
}

impl WindroseBootstrap {
    pub fn install_root(&self) -> &Path {
        &self.root
    }

    pub fn inspect_native_state(&self) -> Result<WindroseBootstrapObservation, StorageError> {
        inspect(&self.root).map(|state| state.observation)
    }

    pub async fn finish_after_stopped(self) -> Result<(), StorageError> {
        let lease = self.lock.clone();
        lease
            .complete_mutation("finishing Windrose native bootstrap", async move {
                let details = stopped(&self.paths, &self.instance_id).await?;
                let (session, world_id) = self
                    .in_worker(|session| {
                        session.verify_journal()?;
                        inspect(&session.root)?.world_id.ok_or_else(|| {
                            invalid(
                                &session.root,
                                "native world is not ready; recovery data was preserved",
                            )
                        })
                    })
                    .await?;
                let mut settings: serde_json::Value = serde_json::from_str(&details.settings_json)?;
                let settings_object = settings
                    .as_object_mut()
                    .ok_or_else(|| invalid(&session.root, "instance settings are not an object"))?;
                if settings_object
                    .get("world_island_id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|value| !value.is_empty() && value != world_id)
                {
                    return Err(invalid(
                        &session.root,
                        "selected world changed during bootstrap",
                    ));
                }
                settings_object.insert("world_island_id".into(), world_id.clone().into());
                // Use the existing settings transaction to persist the real
                // identity and stage world parameters. Before a normal launch,
                // materialize_for_start applies its official-updater plan.
                crate::instances::update_instance_locked(
                    &session.paths,
                    UpdateInstanceInput {
                        id: session.instance_id.clone(),
                        bind_ip: details.summary.bind_ip,
                        auto_backup_on_stop: details.auto_backup_on_stop,
                        backup_retention_count: details.backup_retention_count,
                        settings_json: serde_json::to_string(&settings)?,
                        ports: details.ports,
                    },
                    &session.lock,
                )
                .await?;
                session
                    .in_worker(move |session| {
                        if inspect(&session.root)?.world_id.as_deref() != Some(world_id.as_str()) {
                            return Err(invalid(
                                &session.root,
                                "native identity changed during configuration merge",
                            ));
                        }
                        session.clear_journal()
                    })
                    .await
                    .map(|_| ())
            })
            .await
    }

    pub async fn abort_after_stopped(self) -> Result<(), StorageError> {
        let lease = self.lock.clone();
        lease
            .complete_mutation("recovering interrupted Windrose bootstrap", async move {
                stopped(&self.paths, &self.instance_id).await?;
                self.in_worker(|session| {
                    session.verify_journal()?;
                    let state = inspect(&session.root)?;
                    let current = read_bytes(&session.root, Path::new(SERVER), MAX_DOCUMENT)?;
                    if state.has_worlds
                        || current
                            .as_ref()
                            .is_some_and(|bytes| Some(bytes) != session.journal.original.as_ref())
                    {
                        // Native partial/complete output remains untouched and the next
                        // session resumes it using this same recovery record.
                        return Ok(());
                    }
                    session.swap_server(current.as_deref(), session.journal.original.as_deref())?;
                    session.clear_journal()
                })
                .await
                .map(|_| ())
            })
            .await
    }

    async fn in_worker<T: Send + 'static>(
        self,
        operation: impl FnOnce(&Self) -> Result<T, StorageError> + Send + 'static,
    ) -> Result<(Self, T), StorageError> {
        let lease = self.lock.clone();
        lease
            .spawn_blocking(move || {
                let result = operation(&self)?;
                Ok((self, result))
            })
            .await
            .map_err(|error| StorageError::BlockingTaskFailed {
                operation: "checking Windrose bootstrap recovery files",
                message: error.to_string(),
            })?
    }

    fn verify_journal(&self) -> Result<(), StorageError> {
        if read_bytes(&self.config, Path::new(JOURNAL), MAX_JOURNAL)?.as_deref()
            != Some(self.journal_bytes.as_slice())
        {
            return Err(invalid(&self.root, "bootstrap recovery journal changed"));
        }
        Ok(())
    }

    fn swap_server(
        &self,
        expected: Option<&[u8]>,
        replacement: Option<&[u8]>,
    ) -> Result<(), StorageError> {
        checked_path(&self.root, Path::new(SERVER), false)?;
        if !compare_and_swap_optional_file_atomically(
            &self.root.join(SERVER),
            expected,
            replacement,
        )
        .map_err(|_| {
            invalid(
                &self.root,
                "cannot atomically restore or stage native configuration",
            )
        })? {
            return Err(invalid(
                &self.root,
                "native configuration changed during bootstrap preparation",
            ));
        }
        Ok(())
    }

    fn clear_journal(&self) -> Result<(), StorageError> {
        self.verify_journal()?;
        if !compare_and_swap_optional_file_atomically(
            &self.config.join(JOURNAL),
            Some(&self.journal_bytes),
            None,
        )
        .map_err(|_| invalid(&self.root, "cannot clear completed bootstrap journal"))?
        {
            return Err(invalid(
                &self.root,
                "bootstrap recovery journal changed during completion",
            ));
        }
        Ok(())
    }
}

async fn stopped(paths: &StoragePaths, instance_id: &str) -> Result<InstanceDetails, StorageError> {
    let details = crate::read_instance_details(paths, instance_id).await?;
    if details.summary.module_id != "windrose"
        || details.active_run.is_some()
        || !matches!(
            details.summary.status,
            InstanceStatus::Stopped | InstanceStatus::Error
        )
    {
        return Err(invalid(
            Path::new(&details.config_file_path),
            "native bootstrap requires an inactive Windrose instance without an active run",
        ));
    }
    Ok(details)
}

fn invalid(root: &Path, message: &str) -> StorageError {
    StorageError::WindroseBootstrap {
        path: root.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "windrose_bootstrap_tests.rs"]
mod tests;
