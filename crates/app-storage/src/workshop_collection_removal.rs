//! Squad loads every plugin directory: removal must move payloads as well as metadata.
use std::collections::BTreeSet;
use std::path::Path;

use app_core::{InstanceDetails, InstanceProgramMode, InstanceStatus, UpdateInstanceInput};
use serde_json::Value;

use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::instances::{effective_instance_install_root, read_instance_details};
use crate::runtime::load_active_instance_run;
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

#[path = "workshop_collection_removal_files.rs"]
mod files;
pub(crate) use files::{ensure_no_pending, recover};

const FIELD: &str = "steam_workshop_collections";

/// The desktop also owns the instance lifecycle lock. The database reservation
/// and settings lease protect the same operation from other local storage users.
pub async fn remove_instance_workshop_collection(
    paths: &StoragePaths,
    input: UpdateInstanceInput,
    expected_settings_json: String,
    collection_id: String,
    member_ids: Vec<String>,
    retain_collection: bool,
) -> Result<InstanceDetails, StorageError> {
    let lock = acquire_instance_settings_mutation_lock(paths, &input.id)?;
    let worker_lock = lock.clone();
    let paths = paths.clone();
    lock.complete_mutation("removing instance Workshop collection", async move {
        let pool = connect_pool(&paths).await?;
        let result = async {
            let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
            let record = fetch_instance_record(&mut *tx, &input.id).await?;
            let root = record
                .config_dir
                .parent()
                .ok_or_else(|| invalid("invalid instance root"))?
                .to_path_buf();
            if record.summary.module_id != "squad" || record.runtime_mode != "independent" {
                return Err(invalid(
                    "file collection removal requires an independent Squad instance",
                ));
            }
            if record.summary.active_process_count > 0
                || matches!(
                    record.summary.status,
                    InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
                )
                || load_active_instance_run(&mut *tx, &input.id)
                    .await?
                    .is_some()
            {
                return Err(invalid(
                    "stop the instance before removing its Workshop collection",
                ));
            }
            let recovery_root = root.clone();
            let instances_root = paths.instances_root.clone();
            worker_lock
                .spawn_blocking(move || {
                    crate::instances::validate_managed_instance_root(
                        &recovery_root,
                        &instances_root,
                    )?;
                    files::recover(&recovery_root, false)
                })
                .await
                .map_err(task_error)??;
            let runtime = effective_instance_install_root(&record)?;
            if crate::instance_program_mode(&root)? != InstanceProgramMode::Independent {
                return Err(invalid("collection payloads must belong to the instance"));
            }
            let current = read_instance_details(&paths, &input.id).await?;
            validate_metadata(&current, &input)?;
            let settings: Value = serde_json::from_str(&current.settings_json)?;
            let expected: Value = serde_json::from_str(&expected_settings_json)?;
            if settings != expected {
                return Err(StorageError::InstanceSettingsPreconditionFailed {
                    id: input.id.clone(),
                });
            }
            let replacement: Value = serde_json::from_str(&input.settings_json)?;
            let members = validate_removal(
                &settings,
                &replacement,
                &collection_id,
                &member_ids,
                retain_collection,
            )?;
            let instance_id = input.id.clone();
            worker_lock
                .spawn_blocking(move || {
                    files::remove(
                        &root,
                        &runtime,
                        &instance_id,
                        &settings,
                        &replacement,
                        members,
                    )
                })
                .await
                .map_err(task_error)??;
            // No database or native configuration value changes in this operation.
            tx.commit().await?;
            read_instance_details(&paths, &input.id).await
        }
        .await;
        pool.close().await;
        result
    })
    .await
}

fn validate_metadata(
    current: &InstanceDetails,
    input: &UpdateInstanceInput,
) -> Result<(), StorageError> {
    if current.summary.id != input.id
        || current.summary.bind_ip != input.bind_ip
        || current.auto_backup_on_stop != input.auto_backup_on_stop
        || current.backup_retention_count != input.backup_retention_count
        || serde_json::to_value(&current.ports)? != serde_json::to_value(&input.ports)?
    {
        return Err(invalid(
            "collection removal cannot change instance metadata, ports or backup policy",
        ));
    }
    Ok(())
}

fn validate_removal(
    original: &Value,
    replacement: &Value,
    collection_id: &str,
    requested: &[String],
    retain_collection: bool,
) -> Result<Vec<String>, StorageError> {
    if !valid_id(collection_id)
        || requested.is_empty()
        || requested.len() > 8192
        || (retain_collection && requested.len() != 1)
    {
        return Err(invalid("invalid collection removal request"));
    }
    let records = original
        .get(FIELD)
        .and_then(Value::as_array)
        .filter(|records| records.len() <= 128)
        .ok_or_else(|| invalid("saved collection records are missing"))?;
    let mut retained = Vec::new();
    let mut selected = None;
    let mut others = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut count = 0;
    for record in records {
        let id = record
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_id(id))
            .ok_or_else(|| invalid("invalid saved collection ID"))?;
        if !ids.insert(id) {
            return Err(invalid("duplicate saved collection ID"));
        }
        let members = record
            .get("member_ids")
            .and_then(Value::as_array)
            .filter(|members| members.len() <= 8192)
            .ok_or_else(|| invalid("invalid saved collection members"))?;
        count += members.len();
        if count > 65_536 {
            return Err(invalid("too many saved collection members"));
        }
        let members = members
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|id| valid_id(id))
                    .map(str::to_owned)
                    .ok_or_else(|| invalid("invalid saved member ID"))
            })
            .collect::<Result<BTreeSet<_>, _>>()?;
        if id == collection_id {
            selected = Some(members);
        } else {
            if members.is_empty() && !retain_collection {
                return Err(invalid("another collection has an unknown member snapshot"));
            }
            others.extend(members);
            retained.push(record.clone());
        }
    }
    let selected =
        selected.ok_or_else(|| invalid("the collection is not saved in this instance"))?;
    // An explicit single-member action removes that instance deployment even
    // when another saved collection references it. Whole collections protect it.
    let removable = if retain_collection {
        selected
    } else {
        selected
            .difference(&others)
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let requested_set = requested
        .iter()
        .filter(|id| valid_id(id))
        .cloned()
        .collect::<BTreeSet<_>>();
    if requested_set.len() != requested.len() || !requested_set.is_subset(&removable) {
        return Err(invalid(
            "member IDs must belong to the saved collection; whole removal must preserve shared members",
        ));
    }
    let mut intended = original.clone();
    if !retain_collection {
        intended[FIELD] = Value::Array(retained);
    }
    if &intended != replacement {
        return Err(invalid(
            "single-member removal must preserve all settings; whole removal may only remove its saved collection record",
        ));
    }
    Ok(requested_set.into_iter().collect())
}

fn valid_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && !id.starts_with('0')
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u64>().is_ok()
}

fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::InvalidModuleSetting {
        module_id: "squad".into(),
        field: FIELD.into(),
        message: message.into(),
    }
}

fn failure(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::PrivateRuntimeRefresh {
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

fn task_error(error: tokio::task::JoinError) -> StorageError {
    StorageError::BlockingTaskFailed {
        operation: "removing instance Workshop collection",
        message: error.to_string(),
    }
}

#[cfg(test)]
#[path = "workshop_collection_removal_tests.rs"]
mod tests;
