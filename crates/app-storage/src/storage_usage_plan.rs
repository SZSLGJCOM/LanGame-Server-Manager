use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use sqlx::Row;

use super::{ScanPlan, StorageUsageEntry, path_key};
use crate::instance_isolation::paths::normalize_resource_path;
use crate::{StorageError, StoragePaths};

pub(super) async fn build(
    paths: &StoragePaths,
    cancellation: Arc<AtomicBool>,
    deadline: Instant,
) -> Result<ScanPlan, StorageError> {
    let mut plan = ScanPlan {
        entries: Vec::new(),
        roots: Vec::new(),
        assignments: HashMap::new(),
        issues: Vec::new(),
        cancellation,
        deadline,
    };
    if plan.halted() {
        return Ok(plan);
    }
    let pool = crate::storage_db::connect_pool(paths).await?;
    let mut tx = pool.begin().await?;
    let instances = crate::storage_db::load_instance_isolation_records(&mut tx).await?;
    let libraries = sqlx::query("SELECT id, module_id, install_root FROM game_installs WHERE scope = 'library' ORDER BY module_id, id LIMIT 4097")
        .fetch_all(&mut *tx).await?;
    if libraries.len() > 4096 {
        return Err(StorageError::InvalidInstancePath {
            path: paths.games_root.clone(),
            message: "storage inspection is limited to 4096 library installations".into(),
        });
    }
    tx.rollback().await?;
    pool.close().await;
    if plan.halted() {
        return Ok(plan);
    }
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || {
        plan.add(
            "library".into(),
            "library",
            "Game library".into(),
            &paths.games_root,
            None,
            None,
        );
        plan.add(
            "unregistered-instances".into(),
            "other",
            "Other instance files".into(),
            &paths.instances_root,
            None,
            None,
        );
        plan.add(
            "archives".into(),
            "archives",
            "Deleted instance archives".into(),
            &paths.archives_root,
            None,
            None,
        );
        plan.add(
            "manager-data".into(),
            "other",
            "Instance management data".into(),
            &paths.instances_root.join(".langame"),
            None,
            None,
        );
        plan.add(
            "application-data".into(),
            "other",
            "Application data".into(),
            &paths.app_data_root,
            None,
            None,
        );
        plan.add(
            "tools".into(),
            "other",
            "SteamCMD tools and downloads".into(),
            &paths.steamcmd_root,
            None,
            None,
        );
        for row in libraries {
            if plan.halted() {
                return Ok(plan);
            }
            let module_id: String = row.get("module_id");
            let install_id: i64 = row.get("id");
            let root = PathBuf::from(row.get::<String, _>("install_root"));
            plan.add(
                format!("library:{install_id}"),
                "library",
                module_id.clone(),
                &root,
                None,
                Some(module_id),
            );
        }
        if plan.halted() {
            return Ok(plan);
        }
        let descriptors = app_modules::discover_modules(&paths.modules_root)?;
        for instance in instances {
            if plan.halted() {
                return Ok(plan);
            }
            let id = &instance.summary.id;
            let module = &instance.summary.module_id;
            let name = &instance.summary.name;
            let Some(root) = instance.config_dir.parent() else {
                plan.issues.push(format!(
                    "Instance {id} has no configuration parent directory."
                ));
                continue;
            };
            if let Err(error) =
                crate::instances::validate_managed_instance_root(root, &paths.instances_root)
            {
                plan.issues.push(format!("Instance {id}: {error}"));
                continue;
            }
            let data = plan.add(
                format!("instance:{id}:data"),
                "instance_data",
                name.clone(),
                root,
                Some(id.clone()),
                Some(module.clone()),
            );
            plan.add(
                format!("instance:{id}:backups"),
                "backups",
                name.clone(),
                &root.join("backups"),
                Some(id.clone()),
                Some(module.clone()),
            );
            match crate::instances::effective_instance_install_root(&instance) {
                Ok(runtime) => {
                    if instance.runtime_mode == "independent"
                        && !crate::instance_uses_library_program(root)?
                    {
                        plan.add(
                            format!("instance:{id}:program"),
                            "instance_program",
                            name.clone(),
                            &runtime,
                            Some(id.clone()),
                            Some(module.clone()),
                        );
                    }
                    let descriptor = descriptors
                        .iter()
                        .find(|descriptor| descriptor.summary.id == *module);
                    match crate::save_paths::effective_instance_saves_dir(
                        descriptor, &runtime, &instance,
                    ) {
                        Ok(saves) => {
                            plan.assign(&saves, data);
                        }
                        Err(error) => plan.entry_issue(data, error.to_string()),
                    }
                    for config in
                        crate::instance_isolation::native::configuration_paths(module, &runtime, id)
                    {
                        plan.assign(&config, data);
                    }
                }
                Err(error) => plan.entry_issue(data, error.to_string()),
            }
            for directory in ["config", "data", "logs"] {
                plan.assign(&root.join(directory), data);
            }
        }
        plan.roots
            .sort_by_key(|path| (path.components().count(), path_key(path)));
        let mut roots: Vec<PathBuf> = Vec::new();
        let mut included = HashSet::new();
        for root in plan.roots.drain(..) {
            if !root
                .ancestors()
                .any(|parent| included.contains(&path_key(parent)))
            {
                included.insert(path_key(&root));
                roots.push(root);
            }
        }
        plan.roots = roots;
        Ok(plan)
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "planning storage inspection",
        message: error.to_string(),
    })?
}

impl ScanPlan {
    fn halted(&mut self) -> bool {
        let reason = if self.cancellation.load(Ordering::Acquire) {
            Some("Storage planning was cancelled; the displayed values are partial.")
        } else if Instant::now() >= self.deadline {
            Some("Storage planning reached the scan deadline; the displayed values are partial.")
        } else {
            None
        };
        if let Some(reason) = reason {
            if !self.issues.iter().any(|issue| issue == reason) {
                self.issues.push(reason.into());
            }
            true
        } else {
            false
        }
    }

    fn entry_issue(&mut self, index: usize, message: String) {
        let entry = &mut self.entries[index];
        entry.status = "partial".into();
        if entry.issues.len() < 8 {
            entry.issues.push(message);
        }
    }

    fn add(
        &mut self,
        id: String,
        category: &str,
        label: String,
        path: &Path,
        instance_id: Option<String>,
        module_id: Option<String>,
    ) -> usize {
        let index = self.entries.len();
        let halted = self.halted();
        let missing = !halted
            && std::fs::symlink_metadata(path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        self.entries.push(StorageUsageEntry {
            id,
            category: category.into(),
            label,
            path: path.to_string_lossy().into_owned(),
            instance_id,
            module_id,
            logical_bytes: 0,
            allocated_bytes: Some(0),
            file_count: 0,
            status: if halted {
                "partial"
            } else if missing {
                "missing"
            } else {
                "complete"
            }
            .into(),
            issues: Vec::new(),
        });
        self.assign(path, index);
        index
    }

    fn assign(&mut self, path: &Path, index: usize) {
        if self.halted() {
            return;
        }
        match normalize_resource_path(path) {
            Ok(path) => {
                match std::fs::symlink_metadata(&path) {
                    Ok(_) => {
                        if let Some(previous) = self.assignments.get(&path_key(&path)).copied()
                            && previous != index
                            && let (Some(first), Some(second)) = (
                                &self.entries[previous].instance_id,
                                &self.entries[index].instance_id,
                            )
                            && first != second
                        {
                            let message = format!(
                                "{} is claimed by instances {first} and {second}; it is counted only once.",
                                path.display()
                            );
                            self.entry_issue(previous, message.clone());
                            self.entry_issue(index, message);
                            return;
                        }
                        self.assignments.insert(path_key(&path), index);
                        self.roots.push(path);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        // Missing optional subdirectories do not invalidate an existing owner's totals.
                    }
                    Err(error) => self.entry_issue(index, format!("{}: {error}", path.display())),
                }
            }
            Err(error) => self.entry_issue(index, error.to_string()),
        }
    }
}
