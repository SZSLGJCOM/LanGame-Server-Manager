use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};

use app_core::{InstallState, InstanceStatus};
use app_modules::ModuleDescriptor;

use crate::instance_creation_io::{check_creation_cancelled, publish_creation_directory};
use crate::instance_isolation::paths::normalize_path;
use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::{prepare_private_runtime_root, validate_managed_instance_root};
use crate::program_runtime::{
    InstanceProgramMode, SHARED_RUNTIME_BINDING, instance_program_mode, invalid,
};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

const STAGING: &str = "program-detach-staging";
pub(crate) const SHARED_ROLLBACK: &str = "runtime.shared-rollback";
const STAGING_MARKER: &str = ".langame-program-detach";

/// The desktop retains its instance mutation and installation lifecycle leases.
/// Storage additionally serializes cross-process settings writes and admission.
pub async fn detach_instance_program(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    instance_id: &str,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<PathBuf, StorageError> {
    let lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    let descriptor = descriptor.clone();
    let instance_id = instance_id.to_owned();
    lock.complete_mutation("detaching an instance program", async move {
        let pool = connect_pool(&paths).await?;
        let mut recovery_root = None;
        let result = async {
            let record = fetch_instance_record(&pool, &instance_id).await?;
            if record.summary.module_id != descriptor.summary.id {
                return Err(invalid(
                    &record.config_dir,
                    "instance and module do not match",
                ));
            }
            ensure_stopped(&record.summary)?;
            let instance_root = record
                .config_dir
                .parent()
                .ok_or_else(|| invalid(&record.config_dir, "instance root is missing"))?
                .to_owned();
            validate_managed_instance_root(&instance_root, &paths.instances_root)?;
            recovery_root = Some(instance_root.clone());
            let previous_root = instance_root.clone();
            let previous_mode = record.runtime_mode.clone();
            worker_lock
                .spawn_blocking(move || recover_detach(&previous_root, &previous_mode))
                .await
                .map_err(|error| StorageError::BlockingTaskFailed {
                    operation: "recovering program detachment",
                    message: error.to_string(),
                })??;
            let registered_root = crate::instances::effective_instance_install_root(&record)?;
            if instance_program_mode(&instance_root)? == InstanceProgramMode::Independent {
                return Ok(registered_root);
            }
            let install = crate::read_instance_program_install(&paths, &instance_id)
                .await?
                .ok_or_else(|| {
                    invalid(
                        &instance_root,
                        "shared installation registration is missing",
                    )
                })?;
            let source = registered_root;
            let root_for_copy = instance_root.clone();
            let token = cancellation.clone();
            let prepared = worker_lock
                .spawn_blocking(move || prepare_detach(&source, &root_for_copy, token.as_deref()))
                .await
                .map_err(|error| StorageError::BlockingTaskFailed {
                    operation: "copying an independent program",
                    message: error.to_string(),
                })?;
            prepared?;
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let current = fetch_instance_record(&mut *tx, &instance_id).await?;
            ensure_stopped(&current.summary)?;
            check_creation_cancelled(cancellation.as_deref())?;
            let runtime = instance_root.join("runtime");
            let publish_root = instance_root.clone();
            worker_lock
                .spawn_blocking(move || publish_detach(&publish_root))
                .await
                .map_err(|error| StorageError::BlockingTaskFailed {
                    operation: "publishing an independent program",
                    message: error.to_string(),
                })??;
            let update = crate::register_instance_install(
                &mut tx,
                &descriptor.summary.id,
                &instance_id,
                &runtime,
                InstallState::Installed,
                install.install.current_version.as_deref(),
            )
            .await;
            if let Err(error) = update {
                tx.rollback().await?;
                return Err(error);
            }
            // A failed/uncertain commit retains both layouts. Recovery consults
            // the committed ownership row before selecting either layout.
            tx.commit().await?;
            if let Err(error) = recover_detach(&instance_root, "independent") {
                eprintln!("independent program committed with pending cleanup: {error}");
            }
            Ok(runtime)
        }
        .await;
        let result = match (result, recovery_root) {
            (Err(error), Some(root)) => {
                let recovery = async {
                    let mode: String =
                        sqlx::query_scalar("SELECT runtime_mode FROM instances WHERE id = ?1")
                            .bind(&instance_id)
                            .fetch_one(&pool)
                            .await?;
                    let recovery_path = root.clone();
                    worker_lock
                        .spawn_blocking(move || recover_detach(&recovery_path, &mode))
                        .await
                        .map_err(|failure| StorageError::BlockingTaskFailed {
                            operation: "compensating program detachment",
                            message: failure.to_string(),
                        })?
                }
                .await;
                match recovery {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(invalid(
                        &root,
                        format!("{error}; program recovery remains pending: {cleanup}"),
                    )),
                }
            }
            (result, _) => result,
        };
        pool.close().await;
        result
    })
    .await
}

fn ensure_stopped(summary: &app_core::InstanceSummary) -> Result<(), StorageError> {
    if summary.active_process_count != 0
        || matches!(
            summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        )
    {
        return Err(invalid(
            Path::new(&summary.id),
            "stop the instance before changing its program installation",
        ));
    }
    Ok(())
}

fn prepare_detach(
    source: &Path,
    instance: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    ensure_reference_only(&instance.join("runtime"))?;
    let staging = instance.join(STAGING);
    normalize_path(&staging)?;
    fs::create_dir(&staging).map_err(|source| StorageError::CreatePath {
        path: staging.clone(),
        source,
    })?;
    fs::write(staging.join(STAGING_MARKER), b"managed\n").map_err(|source| {
        StorageError::WriteConfig {
            path: staging.join(STAGING_MARKER),
            source,
        }
    })?;
    prepare_private_runtime_root(
        source,
        &staging,
        &[],
        None,
        false,
        crate::program_runtime::ProgramFileSelection::Automatic,
        cancellation,
    )?;
    Ok(())
}

fn publish_detach(instance: &Path) -> Result<(), StorageError> {
    let runtime = instance.join("runtime");
    let rollback = instance.join(SHARED_ROLLBACK);
    ensure_reference_only(&runtime)?;
    publish_creation_directory(&runtime, &rollback, None)?;
    if let Err(error) =
        publish_creation_directory(&instance.join(STAGING).join("runtime"), &runtime, None)
    {
        if let Err(restore) = publish_creation_directory(&rollback, &runtime, None) {
            return Err(invalid(
                instance,
                format!(
                    "program publication failed: {error}; restoring the shared reference failed: {restore}"
                ),
            ));
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn recover_detach(instance: &Path, mode: &str) -> Result<(), StorageError> {
    if !matches!(mode, "shared" | "independent") {
        return Err(invalid(instance, "unknown program ownership mode"));
    }
    let rollback = instance.join(SHARED_ROLLBACK);
    match fs::symlink_metadata(&rollback) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return cleanup_staging(instance);
        }
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: rollback,
                source,
            });
        }
        Ok(_) => {}
    }
    ensure_reference_only(&rollback)?;
    let runtime = instance.join("runtime");
    match mode {
        "shared" => {
            if fs::symlink_metadata(&runtime).is_ok() {
                crate::private_runtime::resolve_instance_private_runtime_root(instance)?;
                validate_staging(&instance.join(STAGING))?;
                let staged = instance.join(STAGING).join("runtime");
                if fs::symlink_metadata(&staged).is_ok() {
                    return Err(invalid(
                        &staged,
                        "both detached copies exist; preserving them for recovery",
                    ));
                }
                publish_creation_directory(&runtime, &staged, None)?;
            }
            publish_creation_directory(&rollback, &runtime, None)?;
        }
        "independent" => {
            crate::private_runtime::resolve_instance_private_runtime_root(instance)?;
            fs::remove_file(rollback.join(SHARED_RUNTIME_BINDING)).map_err(|source| {
                StorageError::DeletePath {
                    path: rollback.join(SHARED_RUNTIME_BINDING),
                    source,
                }
            })?;
            fs::remove_dir(&rollback).map_err(|source| StorageError::DeletePath {
                path: rollback,
                source,
            })?;
        }
        _ => return Err(invalid(instance, "unknown program ownership mode")),
    }
    cleanup_staging(instance)
}

fn ensure_reference_only(path: &Path) -> Result<(), StorageError> {
    normalize_path(path)?;
    let entries = fs::read_dir(path).map_err(|source| StorageError::ReadDirectory {
        path: path.to_owned(),
        source,
    })?;
    let mut found = false;
    for entry in entries {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: path.to_owned(),
            source,
        })?;
        if entry.file_name() != SHARED_RUNTIME_BINDING || found {
            return Err(invalid(
                path,
                "shared reference directory contains additional files; preserve them before detaching",
            ));
        }
        crate::instance_isolation::paths::normalize_resource_path(&entry.path())?;
        found = true;
    }
    if !found {
        return Err(invalid(path, "shared program binding is missing"));
    }
    crate::program_runtime::resolve_shared_program_reference(path)?;
    Ok(())
}

fn cleanup_staging(instance: &Path) -> Result<(), StorageError> {
    let staging = instance.join(STAGING);
    match fs::symlink_metadata(&staging) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: staging,
                source,
            });
        }
        Ok(_) => {}
    }
    validate_staging(&staging)?;
    fs::remove_dir_all(&staging).map_err(|source| StorageError::DeletePath {
        path: staging,
        source,
    })
}

fn validate_staging(staging: &Path) -> Result<(), StorageError> {
    normalize_path(staging)?;
    let marker = staging.join(STAGING_MARKER);
    crate::instance_isolation::paths::normalize_resource_path(&marker)?;
    let mut bytes = Vec::new();
    fs::File::open(&marker)
        .and_then(|file| file.take(9).read_to_end(&mut bytes))
        .map_err(|source| StorageError::ReadPath {
            path: marker,
            source,
        })?;
    if bytes != b"managed\n" {
        return Err(invalid(
            staging,
            "detach staging is not owned by this operation",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "program_detach_tests.rs"]
mod tests;
