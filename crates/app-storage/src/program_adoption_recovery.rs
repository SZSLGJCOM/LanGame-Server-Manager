use std::fs;
use std::path::{Path, PathBuf};

use sqlx::Row;

use crate::instance_isolation::paths::{contains, normalize_path};
use crate::instance_settings_lock::acquire_module_instance_creation_lock_blocking;
use crate::instances::validate_managed_instance_root;
use crate::program_adoption::{ADOPTION_JOURNAL, ProgramAdoption};
use crate::program_runtime::invalid;
use crate::storage_db::connect_pool;
use crate::{StorageError, StoragePaths};

const MAX_RECOVERY_ENTRIES: usize = 4_096;

/// Recover this module's configured library and instance acquisitions before
/// deciding that a missing download must be recreated. The module creation lease
/// excludes live adoptions, including acquisitions no longer registered as libraries.
/// Uncommitted instance directories are preserved; only their program is restored.
pub async fn recover_interrupted_program_adoptions(
    paths: &StoragePaths,
    module_id: &str,
    library_root: &Path,
) -> Result<(), StorageError> {
    let lock_paths = paths.clone();
    let lock_module = module_id.to_owned();
    let lock = tokio::task::spawn_blocking(move || {
        acquire_module_instance_creation_lock_blocking(&lock_paths, &lock_module)
    })
    .await
    .map_err(|error| worker_error("acquiring program recovery lease", error))??;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let module_id = module_id.to_owned();
    let library_root = library_root.to_owned();
    lock.complete_mutation("recovering program adoption", async move {
        let instances = paths.instances_root.clone();
        let source = library_root.clone();
        let acquisitions = crate::program_instance_acquisition::module_root(&paths, &module_id)?;
        let journals = worker_lock.spawn_blocking(move || find_journals(&instances, &source, &acquisitions))
            .await.map_err(|error| worker_error("reading adoption journals", error))??;
        if journals.is_empty() { return Ok(()); }
        let pool = connect_pool(&paths).await?;
        let result = async {
            // Keep registration, deletion and launch state stable until every
            // filesystem compensation has finished, including caller cancellation.
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let instance_rows = sqlx::query("SELECT id,module_id,runtime_mode,config_path,install_id FROM instances LIMIT 4097")
                .fetch_all(&mut *tx).await?;
            let install_rows = sqlx::query("SELECT id,module_id,install_root,scope,owner_instance_id FROM game_installs LIMIT 4097")
                .fetch_all(&mut *tx).await?;
            if instance_rows.len() > MAX_RECOVERY_ENTRIES || install_rows.len() > MAX_RECOVERY_ENTRIES {
                return Err(invalid(&library_root, "program recovery ownership inventory is too large"));
            }
            let instances = instance_rows.iter().map(|row| RecoveryInstance {
                id: row.get("id"), module_id: row.get("module_id"), mode: row.get("runtime_mode"),
                config_path: PathBuf::from(row.get::<String, _>("config_path")), install_id: row.get("install_id"),
            }).collect::<Vec<_>>();
            let installs = install_rows.iter().map(|row| RecoveryInstall {
                id: row.get("id"), module_id: row.get("module_id"),
                root: PathBuf::from(row.get::<String, _>("install_root")),
                scope: row.get("scope"), owner: row.get("owner_instance_id"),
            }).collect::<Vec<_>>();
            worker_lock.spawn_blocking(move || {
                for journal in journals {
                    recover_one(journal, &module_id, &instances, &installs)?;
                }
                Ok::<_, StorageError>(())
            }).await.map_err(|error| worker_error("restoring adopted programs", error))??;
            tx.commit().await?;
            Ok(())
        }.await;
        pool.close().await;
        result
    }).await
}

struct RecoveryInstance {
    id: String,
    module_id: String,
    mode: String,
    config_path: PathBuf,
    install_id: Option<i64>,
}

struct RecoveryInstall {
    id: i64,
    module_id: String,
    root: PathBuf,
    scope: String,
    owner: Option<String>,
}

fn find_journals(
    instances: &Path,
    source: &Path,
    acquisitions: &Path,
) -> Result<Vec<ProgramAdoption>, StorageError> {
    let instances = normalize_path(instances)?;
    let entries = match fs::read_dir(&instances) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(StorageError::ReadDirectory {
                path: instances,
                source,
            });
        }
    };
    let mut journals = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RECOVERY_ENTRIES {
            return Err(invalid(
                &instances,
                "too many instance entries to recover safely",
            ));
        }
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: instances.clone(),
            source,
        })?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if !kind.is_dir()
            || kind.is_symlink()
            || crate::private_runtime::is_reparse_point(&path)?
            || entry.file_name() == ".trash"
        {
            continue;
        }
        validate_managed_instance_root(&path, &instances)?;
        let journal_path = path.join(ADOPTION_JOURNAL);
        match fs::symlink_metadata(&journal_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: journal_path,
                    source,
                });
            }
            Ok(_) => {}
        }
        if let Some(journal) = ProgramAdoption::load_for_recovery(&journal_path, &path, source)? {
            journals.push(journal);
        } else if let Some(journal) =
            ProgramAdoption::load_for_instance_program_recovery(&journal_path, &path, acquisitions)?
        {
            journals.push(journal);
        }
    }
    Ok(journals)
}

fn recover_one(
    journal: ProgramAdoption,
    module_id: &str,
    instances: &[RecoveryInstance],
    installs: &[RecoveryInstall],
) -> Result<(), StorageError> {
    let runtime = normalize_path(&journal.runtime_root())?;
    let instance_root = runtime
        .parent()
        .ok_or_else(|| invalid(&runtime, "instance directory is missing"))?;
    let mut matching = Vec::new();
    for instance in instances {
        let config = normalize_path(&instance.config_path)?;
        if instance_root
            .file_name()
            .is_some_and(|name| name == instance.id.as_str())
            && !config
                .parent()
                .is_some_and(|root| same_path(root, instance_root))
        {
            return Err(invalid(
                instance_root,
                "instance identifier is registered at another directory",
            ));
        }
        if config
            .parent()
            .is_some_and(|root| same_path(root, instance_root))
        {
            matching.push(instance);
        }
    }
    if matching.len() > 1 {
        return Err(invalid(
            instance_root,
            "multiple database instances claim this adoption directory",
        ));
    }
    if let Some(instance) = matching.first() {
        let install = installs
            .iter()
            .find(|install| Some(install.id) == instance.install_id)
            .ok_or_else(|| {
                invalid(
                    instance_root,
                    "committed instance installation registration is missing",
                )
            })?;
        let valid = instance.module_id == module_id
            && instance.mode == "independent"
            && instance
                .config_path
                .file_name()
                .is_some_and(|name| name == "config")
            && install.module_id == module_id
            && install.scope == "instance"
            && install.owner.as_deref() == Some(instance.id.as_str());
        if !valid || !same_path(&normalize_path(&install.root)?, &runtime) {
            return Err(invalid(
                instance_root,
                "committed instance ownership does not match its adoption journal",
            ));
        }
        crate::resolve_instance_private_runtime_root(instance_root)?;
        return journal.commit();
    }
    let source = normalize_path(journal.source_root())?;
    for install in installs {
        let root = normalize_path(&install.root)?;
        if (contains(&source, &root) || contains(&root, &source))
            && !(same_path(&source, &root)
                && install.module_id == module_id
                && install.scope == "library"
                && install.owner.is_none())
        {
            return Err(invalid(
                &source,
                "another installation owns the interrupted adoption source",
            ));
        }
        if same_path(&source, &root)
            && instances
                .iter()
                .any(|instance| instance.install_id == Some(install.id))
        {
            return Err(invalid(
                &source,
                "an existing instance still references the interrupted adoption source",
            ));
        }
        if contains(&runtime, &root) || contains(&root, &runtime) {
            return Err(invalid(
                &runtime,
                "an installation registration still owns the uncommitted runtime",
            ));
        }
    }
    journal.rollback()
}

fn same_path(left: &Path, right: &Path) -> bool {
    contains(left, right) && contains(right, left)
}

fn worker_error(operation: &'static str, error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
#[path = "program_adoption_recovery_tests.rs"]
mod tests;
