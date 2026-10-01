use std::fs;
use std::path::{Path, PathBuf};

use crate::StorageError;
use crate::atomic_file::compare_and_swap_optional_file_atomically;

pub(crate) struct ManagedConfigMergePlan {
    pub(crate) destination_path: PathBuf,
    pub(crate) replacement: Vec<u8>,
    pub(crate) original: Option<Vec<u8>>,
}

struct ManagedConfigChange {
    path: PathBuf,
    original: Option<Vec<u8>>,
    replacement: Option<Vec<u8>>,
}

#[must_use = "commit or roll back the managed configuration changes"]
pub(crate) struct ManagedConfigMutation {
    changes: Vec<ManagedConfigChange>,
    module_id: String,
}

impl ManagedConfigMutation {
    pub(crate) fn new(module_id: &str) -> Self {
        Self {
            changes: Vec::new(),
            module_id: module_id.to_owned(),
        }
    }

    pub(crate) fn write(&mut self, path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
        self.replace(path, Some(bytes.to_vec()))
    }

    pub(crate) fn remove(&mut self, path: &Path) -> Result<(), StorageError> {
        self.replace(path, None)
    }

    pub(crate) fn copy(&mut self, source: &Path, destination: &Path) -> Result<(), StorageError> {
        let bytes = fs::read(source).map_err(|source_error| StorageError::ReadConfig {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        self.write(destination, &bytes)
    }

    fn replace(&mut self, path: &Path, replacement: Option<Vec<u8>>) -> Result<(), StorageError> {
        let original = super::read_optional_bytes(path)?;
        self.verify_previous_write(path, original.as_deref())?;
        if original == replacement {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let changed = compare_and_swap_optional_file_atomically(
            path,
            original.as_deref(),
            replacement.as_deref(),
        )
        .map_err(|source| StorageError::WriteConfig {
            path: path.to_path_buf(),
            source,
        })?;
        if !changed {
            return Err(materialization_error(
                &self.module_id,
                path,
                "refusing to overwrite a configuration file changed concurrently".to_owned(),
            ));
        }
        self.record(ManagedConfigChange {
            path: path.to_path_buf(),
            original,
            replacement,
        });
        Ok(())
    }

    pub(crate) fn apply(&mut self, plans: Vec<ManagedConfigMergePlan>) -> Result<(), StorageError> {
        for plan in plans {
            self.verify_previous_write(&plan.destination_path, plan.original.as_deref())?;
            write_managed_config_plan(&plan, &self.module_id)?;
            self.record(ManagedConfigChange {
                path: plan.destination_path,
                original: plan.original,
                replacement: Some(plan.replacement),
            });
        }
        Ok(())
    }

    fn record(&mut self, change: ManagedConfigChange) {
        if let Some(previous) = self
            .changes
            .iter_mut()
            .find(|previous| previous.path == change.path)
        {
            // One owner retains the initial bytes and the latest published state.
            // A failed later write must not expose an older rollback expectation.
            previous.replacement = change.replacement;
        } else {
            self.changes.push(change);
        }
    }

    fn verify_previous_write(
        &self,
        path: &Path,
        original: Option<&[u8]>,
    ) -> Result<(), StorageError> {
        if self
            .changes
            .iter()
            .rev()
            .find(|change| change.path == path)
            .is_some_and(|change| change.replacement.as_deref() != original)
        {
            return Err(materialization_error(
                &self.module_id,
                path,
                "refusing to overwrite a configuration file changed during this transaction"
                    .to_owned(),
            ));
        }
        Ok(())
    }

    pub(crate) fn commit(self) {}

    pub(crate) fn rollback(self) -> Result<(), StorageError> {
        let failures = self
            .changes
            .iter()
            .rev()
            .filter_map(|change| {
                match compare_and_swap_optional_file_atomically(
                    &change.path,
                    change.replacement.as_deref(),
                    change.original.as_deref(),
                ) {
                    Ok(true) => None,
                    Ok(false) => Some(format!(
                        "{}: destination changed concurrently",
                        change.path.display()
                    )),
                    Err(error) => Some(format!("{}: {error}", change.path.display())),
                }
            })
            .collect::<Vec<_>>();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(materialization_error(
                &self.module_id,
                &self.changes[0].path,
                format!("configuration rollback failed: {}", failures.join("; ")),
            ))
        }
    }

    pub(crate) fn rollback_after(self, original: StorageError) -> StorageError {
        let module_id = self.module_id.clone();
        let path = self
            .changes
            .first()
            .map(|change| change.path.clone())
            .unwrap_or_default();
        match self.rollback() {
            Ok(()) => original,
            Err(error) => materialization_error(
                &module_id,
                &path,
                format!("instance transaction failed ({original}); {error}"),
            ),
        }
    }
}

#[cfg(test)]
pub(crate) fn apply_pending_managed_config_plans(
    plans: Vec<ManagedConfigMergePlan>,
    module_id: &str,
) -> Result<ManagedConfigMutation, StorageError> {
    apply_pending_managed_config_plans_with_writer(plans, module_id, |plan| {
        write_managed_config_plan(plan, module_id)
    })
}

#[cfg(test)]
pub(crate) fn apply_managed_config_plans_with_writer(
    plans: Vec<ManagedConfigMergePlan>,
    module_id: &str,
    writer: impl FnMut(&ManagedConfigMergePlan) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    apply_pending_managed_config_plans_with_writer(plans, module_id, writer)
        .map(ManagedConfigMutation::commit)
}

#[cfg(test)]
fn apply_pending_managed_config_plans_with_writer(
    plans: Vec<ManagedConfigMergePlan>,
    module_id: &str,
    mut writer: impl FnMut(&ManagedConfigMergePlan) -> Result<(), StorageError>,
) -> Result<ManagedConfigMutation, StorageError> {
    for (index, plan) in plans.iter().enumerate() {
        if let Err(error) = writer(plan) {
            let rollback_failures = plans[..index]
                .iter()
                .rev()
                .filter_map(|applied| rollback_managed_config_plan(applied, module_id).err())
                .map(|rollback_error| rollback_error.to_string())
                .collect::<Vec<_>>();
            if rollback_failures.is_empty() {
                return Err(error);
            }
            return Err(materialization_error(
                module_id,
                &plan.destination_path,
                format!(
                    "managed configuration write failed: {error}; rollback also failed: {}",
                    rollback_failures.join("; ")
                ),
            ));
        }
    }
    Ok(ManagedConfigMutation {
        changes: plans
            .into_iter()
            .map(|plan| ManagedConfigChange {
                path: plan.destination_path,
                original: plan.original,
                replacement: Some(plan.replacement),
            })
            .collect(),
        module_id: module_id.to_owned(),
    })
}

pub(crate) fn write_managed_config_plan(
    plan: &ManagedConfigMergePlan,
    module_id: &str,
) -> Result<(), StorageError> {
    let parent = plan.destination_path.parent().ok_or_else(|| {
        materialization_error(
            module_id,
            &plan.destination_path,
            String::from("materialized support destination has no parent directory"),
        )
    })?;
    fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
        path: parent.to_path_buf(),
        source,
    })?;
    let replaced = compare_and_swap_optional_file_atomically(
        &plan.destination_path,
        plan.original.as_deref(),
        Some(&plan.replacement),
    )
    .map_err(|source| StorageError::WriteConfig {
        path: plan.destination_path.clone(),
        source,
    })?;
    if replaced {
        Ok(())
    } else {
        Err(materialization_error(
            module_id,
            &plan.destination_path,
            String::from("refusing to overwrite a configuration file changed concurrently"),
        ))
    }
}

#[cfg(test)]
fn rollback_managed_config_plan(
    plan: &ManagedConfigMergePlan,
    module_id: &str,
) -> Result<(), StorageError> {
    let restored = compare_and_swap_optional_file_atomically(
        &plan.destination_path,
        Some(&plan.replacement),
        plan.original.as_deref(),
    )
    .map_err(|source| StorageError::WriteConfig {
        path: plan.destination_path.clone(),
        source,
    })?;
    if restored {
        Ok(())
    } else {
        Err(materialization_error(
            module_id,
            &plan.destination_path,
            String::from("refusing rollback because the destination changed concurrently"),
        ))
    }
}

fn materialization_error(module_id: &str, path: &Path, message: String) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: module_id.to_string(),
        path: path.to_path_buf(),
        message,
    }
}
