use std::path::{Path, PathBuf};

use app_modules::{ModuleDescriptor, discover_modules};
use serde::Serialize;
use sqlx::SqliteConnection;

use crate::instances::validate_managed_instance_root;
use crate::program_runtime::{InstanceProgramMode, instance_program_mode};
use crate::save_paths::effective_instance_saves_dir;
use crate::storage_db::{connect_pool, load_instance_isolation_records};
use crate::{StorageError, StoragePaths, StoredInstanceRecord};

#[path = "instance_isolation_archive_reservations.rs"]
mod archive_reservations;
#[path = "instance_isolation_native.rs"]
pub(crate) mod native;
#[path = "instance_isolation_paths.rs"]
pub(crate) mod paths;
use paths::{contains, invalid, normalize_path, normalize_resource_path, overlaps};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstancePathConflict {
    pub instance_id: String,
    pub instance_name: String,
    pub kind: String,
    pub path: String,
    pub other_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceIsolationReport {
    pub instance_id: String,
    pub mode: String,
    pub runtime_path: String,
    pub data_path: String,
    pub config_path: String,
    pub saves_path: String,
    pub conflicts: Vec<InstancePathConflict>,
    pub issues: Vec<String>,
}

#[derive(Serialize)]
struct Layout {
    id: String,
    name: String,
    root: PathBuf,
    config: PathBuf,
    saves: PathBuf,
    runtime: PathBuf,
    native_config: Vec<PathBuf>,
    mode: String,
    issues: Vec<String>,
}

/// Inspect damaged instances without asking the normal detail reader to select
/// a runtime. No files are changed, and settings/token contents are never returned.
pub async fn read_instance_isolation(
    storage: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceIsolationReport, StorageError> {
    let pool = connect_pool(storage).await?;
    let mut tx = pool.begin().await?;
    let result = read_instance_isolation_from_executor(storage, &mut tx, instance_id).await;
    tx.rollback().await?;
    pool.close().await;
    result
}

pub(crate) async fn read_instance_isolation_from_executor(
    storage: &StoragePaths,
    connection: &mut SqliteConnection,
    instance_id: &str,
) -> Result<InstanceIsolationReport, StorageError> {
    let records = load_instance_isolation_records(connection).await?;
    let storage = storage.clone();
    let instance_id = instance_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let layouts = project_layouts(&storage, records)?;
        let target = layouts
            .iter()
            .find(|layout| layout.id == instance_id)
            .ok_or_else(|| StorageError::MissingInstance {
                id: instance_id.clone(),
            })?;
        let mut issues = target.issues.clone();
        let conflicts = match find_conflicts(target, &layouts, true) {
            Ok(conflicts) => conflicts,
            Err(error) => {
                issues.push(error.to_string());
                Vec::new()
            }
        };
        Ok(InstanceIsolationReport {
            instance_id,
            mode: target.mode.clone(),
            runtime_path: target.runtime.to_string_lossy().into_owned(),
            data_path: target.root.to_string_lossy().into_owned(),
            config_path: target.config.to_string_lossy().into_owned(),
            saves_path: target.saves.to_string_lossy().into_owned(),
            conflicts,
            issues,
        })
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "inspecting instance isolation",
        message: error.to_string(),
    })?
}

/// Call inside the owning write transaction before any configuration or save
/// directory is created. Candidate paths come from the incoming settings.
pub(crate) async fn ensure_instance_paths_available(
    storage: &StoragePaths,
    connection: &mut SqliteConnection,
    instance_id: &str,
    module_id: &str,
    install_root: &Path,
    config_dir: &Path,
    saves_dir: &Path,
) -> Result<(), StorageError> {
    let records = load_instance_isolation_records(connection).await?;
    let restoring = archive_reservations::read(connection, instance_id).await?;
    let storage = storage.clone();
    let instance_id = instance_id.to_owned();
    let config = config_dir.to_owned();
    let saves = saves_dir.to_owned();
    let runtime = install_root.to_owned();
    let native_config = native::configuration_paths(module_id, install_root, instance_id.as_str());
    tokio::task::spawn_blocking(move || {
        let mut layouts = project_layouts(&storage, records)?;
        layouts.extend(restoring);
        let root = config
            .parent()
            .ok_or_else(|| invalid(&config, "configuration directory has no instance root"))?
            .to_owned();
        validate_managed_instance_root(&root, &storage.instances_root)?;
        crate::instance_archive_roots::ensure_paths_outside_archive_root(
            &storage,
            [&root, &config, &saves, &runtime]
                .into_iter()
                .chain(native_config.iter())
                .map(PathBuf::as_path),
        )?;
        let candidate = Layout {
            id: instance_id.clone(),
            name: instance_id.clone(),
            root,
            config,
            saves,
            runtime: PathBuf::new(),
            native_config,
            mode: String::new(),
            issues: Vec::new(),
        };
        if let Some(conflict) = find_conflicts(&candidate, &layouts, false)?
            .into_iter()
            .next()
        {
            return Err(StorageError::InstancePathConflict {
                instance_id,
                other_instance_id: conflict.instance_id,
                kind: conflict.kind,
                path: PathBuf::from(conflict.path).into_boxed_path(),
                other_path: PathBuf::from(conflict.other_path).into_boxed_path(),
            });
        }
        Ok(())
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "checking instance path ownership",
        message: error.to_string(),
    })?
}

fn project_layouts(
    storage: &StoragePaths,
    records: Vec<StoredInstanceRecord>,
) -> Result<Vec<Layout>, StorageError> {
    let descriptors = discover_modules(&storage.modules_root)?;
    Ok(records
        .into_iter()
        .map(|record| {
            let descriptor = descriptors
                .iter()
                .find(|descriptor| descriptor.summary.id == record.summary.module_id);
            project_layout(storage, record, descriptor)
        })
        .collect())
}

fn project_layout(
    storage: &StoragePaths,
    record: StoredInstanceRecord,
    descriptor: Option<&ModuleDescriptor>,
) -> Layout {
    let root = record
        .config_dir
        .parent()
        .unwrap_or(&record.config_dir)
        .to_owned();
    let mut issues = Vec::new();
    let (mode, runtime) = match crate::instances::effective_instance_install_root(&record) {
        Ok(runtime) => (
            if matches!(
                instance_program_mode(&root),
                Ok(InstanceProgramMode::Shared)
            ) {
                "shared"
            } else {
                "private"
            },
            runtime,
        ),
        Err(error) => {
            issues.push(error.to_string());
            ("damaged", root.join("runtime"))
        }
    };
    let saves = match effective_instance_saves_dir(descriptor, &runtime, &record) {
        Ok(path) => path,
        Err(error) => {
            issues.push(error.to_string());
            record.saves_dir.clone()
        }
    };
    if let Err(error) = validate_managed_instance_root(&root, &storage.instances_root) {
        issues.push(error.to_string());
    }
    for path in [&root, &record.config_dir, &saves, &runtime] {
        if let Err(error) = normalize_path(path) {
            issues.push(error.to_string());
        }
    }
    let native_config =
        native::configuration_paths(&record.summary.module_id, &runtime, &record.summary.id);
    for path in &native_config {
        if let Err(error) = normalize_resource_path(path) {
            issues.push(error.to_string());
        }
    }
    if let Err(error) = crate::instance_archive_roots::ensure_paths_outside_archive_root(
        storage,
        [&root, &record.config_dir, &saves, &runtime]
            .into_iter()
            .chain(native_config.iter())
            .map(PathBuf::as_path),
    ) {
        issues.push(error.to_string());
    }
    Layout {
        id: record.summary.id,
        name: record.summary.name,
        root,
        config: record.config_dir,
        saves,
        runtime,
        native_config,
        mode: mode.into(),
        issues,
    }
}

fn find_conflicts(
    candidate: &Layout,
    peers: &[Layout],
    include_runtime: bool,
) -> Result<Vec<InstancePathConflict>, StorageError> {
    let root = normalize_path(&candidate.root)?;
    let config = normalize_path(&candidate.config)?;
    let saves = normalize_path(&candidate.saves)?;
    if !contains(&root, &config) {
        return Err(invalid(
            &candidate.config,
            "configuration directory escapes its instance root",
        ));
    }
    // Resolve each path once, not once per candidate/peer resource pair.
    let mut claims = vec![
        ("configuration", &candidate.config, config),
        ("saves", &candidate.saves, saves),
    ];
    for native in &candidate.native_config {
        claims.push(("configuration", native, normalize_resource_path(native)?));
    }
    let runtime = include_runtime
        .then(|| normalize_path(&candidate.runtime))
        .transpose()?;
    let mut conflicts = Vec::new();
    for peer in peers.iter().filter(|peer| peer.id != candidate.id) {
        let peer_paths = [&peer.root, &peer.config, &peer.saves]
            .into_iter()
            .chain(peer.native_config.iter())
            .map(|path| Ok((path, normalize_resource_path(path)?)))
            .collect::<Result<Vec<_>, StorageError>>()?;
        // Native configuration outside config_dir is also an exclusive claim.
        for (kind, actual, normalized) in &claims {
            for (other, other_normalized) in &peer_paths {
                if overlaps(normalized, other_normalized) {
                    if !conflicts.iter().any(|conflict: &InstancePathConflict| {
                        conflict.instance_id == peer.id && conflict.kind == *kind
                    }) {
                        conflicts.push(InstancePathConflict {
                            instance_id: peer.id.clone(),
                            instance_name: peer.name.clone(),
                            kind: (*kind).into(),
                            path: actual.to_string_lossy().into_owned(),
                            other_path: other.to_string_lossy().into_owned(),
                        });
                    }
                    break;
                }
            }
        }
        if let Some(runtime) = &runtime
            && !(candidate.mode == "shared" && peer.mode == "shared")
            && overlaps(runtime, &normalize_path(&peer.runtime)?)
        {
            conflicts.push(InstancePathConflict {
                instance_id: peer.id.clone(),
                instance_name: peer.name.clone(),
                kind: "runtime".into(),
                path: candidate.runtime.to_string_lossy().into_owned(),
                other_path: peer.runtime.to_string_lossy().into_owned(),
            });
        }
    }
    Ok(conflicts)
}

#[cfg(test)]
#[path = "instance_isolation_tests.rs"]
mod tests;
