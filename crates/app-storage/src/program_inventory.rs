use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use app_core::{InstallState, InstanceProgramSource};
use app_modules::ModuleDescriptor;
use serde::Serialize;
use sqlx::Row;

use crate::{InstanceProgramMode, StorageError, StoragePaths};

#[derive(Debug, Serialize)]
pub struct ProgramInstallationUser {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ProgramInstallationSummary {
    pub id: i64,
    pub install_root: String,
    pub scope: String,
    pub install_state: InstallState,
    pub current_version: Option<String>,
    pub used_by: Vec<ProgramInstallationUser>,
    pub modification_state: String,
    pub size_bytes: Option<u64>,
    pub pending_removal: bool,
}

#[derive(Debug, Serialize)]
pub struct ModuleProgramInventory {
    pub requires_archive_inventory: bool,
    pub installations: Vec<ProgramInstallationSummary>,
    pub creation: ProgramCreationEstimate,
}

#[derive(Debug, Serialize)]
pub struct ProgramCreationEstimate {
    pub action: String,
    pub program_path: String,
    pub additional_bytes: Option<u64>,
    pub reason: Option<String>,
    pub can_create: bool,
}

/// Advisory metadata inspection. Creation repeats ownership and content checks
/// under its mutation lease; this response never authorizes a filesystem write.
pub async fn inspect_module_programs(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    mode: Option<InstanceProgramMode>,
    source: InstanceProgramSource,
    cancellation: Arc<AtomicBool>,
    include_archived_sources: bool,
) -> Result<ModuleProgramInventory, StorageError> {
    let pool = crate::storage_db::connect_pool(paths).await?;
    let result = async {
        let rows = sqlx::query(
            "SELECT id,install_root,scope,install_state,current_version,
                    EXISTS(SELECT 1 FROM program_removals r WHERE r.install_id=game_installs.id) AS pending_removal FROM game_installs
             WHERE module_id=?1 ORDER BY id LIMIT 4097",
        )
        .bind(&descriptor.summary.id)
        .fetch_all(&pool)
        .await?;
        let users = sqlx::query(
            "SELECT id,name,install_id FROM instances WHERE module_id=?1 ORDER BY id LIMIT 4097",
        )
        .bind(&descriptor.summary.id)
        .fetch_all(&pool)
        .await?;
        if rows.len() > 4096 || users.len() > 4096 {
            return Err(invalid(
                &paths.games_root,
                "Program inventory exceeds 4096 records.",
            ));
        }
        let installations = rows
            .into_iter()
            .map(|row| {
                let id: i64 = row.get("id");
                ProgramInstallationSummary {
                    id,
                    install_root: row.get("install_root"),
                    scope: row.get("scope"),
                    install_state: crate::storage_db::install_state_from_db_value(Some(
                        &row.get::<String, _>("install_state"),
                    )),
                    current_version: row.get("current_version"),
                    used_by: users
                        .iter()
                        .filter(|user| user.get::<Option<i64>, _>("install_id") == Some(id))
                        .map(|user| ProgramInstallationUser {
                            id: user.get("id"),
                            name: user.get("name"),
                        })
                        .collect(),
                    modification_state: "unverified".into(),
                    size_bytes: None,
                    pending_removal: row.get("pending_removal"),
                }
            })
            .collect::<Vec<_>>();
        Ok(installations)
    }
    .await;
    pool.close().await;
    let mut installations = result?;
    let plan = crate::program_exclusive::inspect_creation_sources(
        paths,
        descriptor,
        mode,
        source,
        Some(cancellation.clone()),
        include_archived_sources,
    )
    .await?;
    // A repair plan may name the default destination without having selected
    // any reusable source. Do not mistake an old manifest there for its input.
    let estimate_source = if source == InstanceProgramSource::Verified {
        crate::read_library_program_install(paths, &descriptor.summary.id)
            .await?
            .filter(|installation| installation.install_state == InstallState::Installed)
            .map(|installation| installation.install_root)
    } else {
        None
    };
    let plan_path = PathBuf::from(&plan.program_path);
    let action = plan.action.clone();
    let can_create = plan.can_create;
    let descriptor = descriptor.clone();
    let (installations, additional_bytes) = tokio::task::spawn_blocking(move || {
        // One shared deadline and entry budget bounds the entire inventory,
        // including installations on a slow or unavailable volume.
        let mut budget = MetadataBudget {
            remaining: 200_000,
            deadline: Instant::now() + Duration::from_secs(15),
            cancellation,
        };
        let mut source_bytes = None;
        for installation in &mut installations {
            let root = Path::new(&installation.install_root);
            installation.size_bytes = measure(root, &mut budget);
            // A use marker records exposure to mutable native software. A
            // manifest alone is not evidence of the current bytes being clean.
            if !installation.used_by.is_empty()
                || root
                    .join(".langame-program-usage.json")
                    .try_exists()
                    .unwrap_or(true)
            {
                installation.modification_state = "modified_or_used".into();
            }
            if root == plan_path {
                source_bytes = installation.size_bytes;
            }
        }
        let additional_bytes = if !can_create {
            None
        } else if source == InstanceProgramSource::Verified {
            // The clean allowlist describes the program copy, whereas a used
            // installation's whole tree also includes saves, Mods and settings.
            // Only estimate a confirmed source, comparing normalized paths
            // so ordinary and canonical Windows aliases describe one input.
            use crate::instance_isolation::paths::normalize_path;
            let bytes = if let Some(root) = &estimate_source
                && normalize_path(root)? == normalize_path(&plan_path)?
            {
                estimate_clean_copy(root, &descriptor, &mut budget)?
            } else {
                None
            };
            if action == "independent_install" {
                bytes
            } else {
                bytes.map(|_| 0)
            }
        } else if action == "independent_install" {
            source_bytes
        } else {
            Some(0)
        };
        Ok::<_, StorageError>((installations, additional_bytes))
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "inspecting program installations",
        message: error.to_string(),
    })??;
    Ok(ModuleProgramInventory {
        requires_archive_inventory: plan.requires_archive_inventory,
        installations,
        creation: ProgramCreationEstimate {
            action: plan.action,
            program_path: plan.program_path,
            additional_bytes,
            reason: plan.reason,
            can_create: plan.can_create,
        },
    })
}

struct MetadataBudget {
    remaining: usize,
    deadline: Instant,
    cancellation: Arc<AtomicBool>,
}

impl MetadataBudget {
    fn take(&mut self) -> bool {
        if self.remaining == 0
            || Instant::now() >= self.deadline
            || self.cancellation.load(Ordering::Acquire)
        {
            return false;
        }
        self.remaining -= 1;
        true
    }
}

// Advisory logical size only: hashes are verified under the creation lease,
// not by the inventory preview. Missing payloads need repair, whose size is
// unknown; invalid manifests, unsafe paths and actual IO failures stay errors.
fn estimate_clean_copy(
    root: &Path,
    descriptor: &ModuleDescriptor,
    budget: &mut MetadataBudget,
) -> Result<Option<u64>, StorageError> {
    use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};
    if !budget.take() {
        return Ok(None);
    }
    let root = normalize_path(root)?;
    let Some(package) = crate::program_seed::read_clean_package_inventory(&root, descriptor)?
    else {
        return Ok(None);
    };
    let mut bytes = 0u64;
    for (key, directory) in package
        .directories
        .iter()
        .map(|key| (key, true))
        .chain(package.files.keys().map(|key| (key, false)))
    {
        if !budget.take() {
            return Ok(None);
        }
        let path = normalize_resource_path(&root.join(key))?;
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(StorageError::ReadPath { path, source }),
        };
        if directory {
            if !metadata.is_dir() {
                return Ok(None);
            }
        } else if metadata.is_file() {
            let Some(total) = bytes.checked_add(metadata.len()) else {
                return Ok(None);
            };
            bytes = total;
        } else {
            return Ok(None);
        }
    }
    Ok(Some(bytes))
}

// Actual logical bytes of this installation, never physical free space.
// Return unknown for links, partial scans, unsupported nodes and IO failures.
fn measure(root: &Path, budget: &mut MetadataBudget) -> Option<u64> {
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut bytes = 0u64;
    while let Some((path, depth)) = pending.pop() {
        if depth > 64 || !budget.take() {
            return None;
        }
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return None;
            }
        }
        if metadata.is_file() {
            bytes = bytes.checked_add(metadata.len())?;
        } else if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).ok()? {
                if pending.len() >= budget.remaining {
                    return None;
                }
                pending.push((entry.ok()?.path(), depth + 1));
            }
        } else {
            return None;
        }
    }
    Some(bytes)
}

fn invalid(path: &Path, message: &str) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "program_inventory_tests.rs"]
mod tests;
