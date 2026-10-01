use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use app_core::{InstanceSummary, PortBinding};
use app_modules::{ModuleDescriptor, discover_modules};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::instances::{read_instance_settings_json, validate_managed_instance_root};
use crate::save_paths::{InstanceSavePathContext, plan_instance_saves_dir};
use crate::storage_db::{connect_pool, load_instance_isolation_records};
use crate::{StorageError, StoragePaths, StoredInstanceRecord, read_instance_port_projections};

#[path = "ark_clusters_paths.rs"]
mod paths;
use paths::{canonical_cluster_directory, directory_key};

pub const MAX_ARK_CLUSTER_INSTANCES: usize = 128;
const MAX_SETTINGS_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArkClusterIdentity {
    pub module_id: String,
    pub cluster_id: String,
    pub directory_key: String,
    pub member_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArkClusterMember {
    pub summary: InstanceSummary,
    pub map_name: String,
    pub cluster_id: String,
    pub cluster_directory: Option<String>,
    pub explicit_shared_directory: bool,
    pub config_file_path: String,
    pub saves_path: String,
    pub ports: Vec<PortBinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArkClusterIssue {
    pub code: String,
    pub severity: String,
    pub instance_id: String,
    pub instance_name: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArkClusterReport {
    pub instance_id: String,
    pub identity: Option<ArkClusterIdentity>,
    pub cluster_directory: Option<String>,
    pub members: Vec<ArkClusterMember>,
    pub related_instances: Vec<ArkClusterMember>,
    pub issues: Vec<ArkClusterIssue>,
    pub start_blocked: bool,
}

struct Projection {
    member: ArkClusterMember,
    directory_key: Option<String>,
    issues: Vec<ArkClusterIssue>,
}

pub fn is_ark_module(module_id: &str) -> bool {
    matches!(module_id, "arksurvivalevolved" | "arksurvivalascended")
}

/// Inspect only registered instances. This neither creates a directory nor moves
/// uploads, and a matching ID alone never implies shared transfer storage.
pub async fn read_ark_cluster_report(
    storage: &StoragePaths,
    instance_id: &str,
) -> Result<ArkClusterReport, StorageError> {
    let pool = connect_pool(storage).await?;
    let result = async {
        let mut transaction = pool.begin().await?;
        let records = load_instance_isolation_records(&mut transaction).await?;
        transaction.rollback().await?;
        Ok::<_, StorageError>(records)
    }
    .await;
    pool.close().await;
    let records = result?
        .into_iter()
        .filter(|record| is_ark_module(&record.summary.module_id))
        .collect::<Vec<_>>();
    if records.len() > MAX_ARK_CLUSTER_INSTANCES {
        return Err(invalid(
            &storage.instances_root,
            "ARK cluster inspection is limited to 128 registered ARK instances",
        ));
    }
    if !records
        .iter()
        .any(|record| record.summary.id == instance_id)
    {
        return Err(StorageError::MissingInstance {
            id: instance_id.to_owned(),
        });
    }
    let ids = records
        .iter()
        .map(|record| record.summary.id.clone())
        .collect::<Vec<_>>();
    let ports = read_instance_port_projections(storage, &ids)
        .await?
        .into_iter()
        .map(|entry| (entry.instance_id, entry.ports))
        .collect();
    let storage = storage.clone();
    let instance_id = instance_id.to_owned();
    tokio::task::spawn_blocking(move || {
        let descriptors = discover_modules(&storage.modules_root)?;
        let mut ports: BTreeMap<String, Vec<PortBinding>> = ports;
        let projections = records
            .into_iter()
            .map(|record| {
                let instance_ports = ports.remove(&record.summary.id).unwrap_or_default();
                let descriptor = descriptors
                    .iter()
                    .find(|item| item.summary.id == record.summary.module_id);
                project_member(&storage, record, descriptor, instance_ports)
            })
            .collect::<Vec<_>>();
        assemble_report(&instance_id, projections)
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "inspecting ARK cluster membership",
        message: error.to_string(),
    })?
}

fn project_member(
    storage: &StoragePaths,
    record: StoredInstanceRecord,
    descriptor: Option<&ModuleDescriptor>,
    ports: Vec<PortBinding>,
) -> Projection {
    let config = record.config_dir.join("instance.json");
    let root = record.config_dir.parent().unwrap_or(&record.config_dir);
    let mut projected = Projection {
        member: ArkClusterMember {
            summary: record.summary.clone(),
            map_name: String::new(),
            cluster_id: String::new(),
            cluster_directory: None,
            explicit_shared_directory: false,
            config_file_path: config.to_string_lossy().into_owned(),
            saves_path: record.saves_dir.to_string_lossy().into_owned(),
            ports,
        },
        directory_key: None,
        issues: Vec::new(),
    };
    let inspect = || -> Result<(Value, PathBuf), String> {
        if descriptor.is_none() {
            return Err(String::from(
                "The ARK module is unavailable; the current save and cluster paths cannot be verified",
            ));
        }
        validate_managed_instance_root(root, &storage.instances_root)
            .map_err(|error| error.to_string())?;
        let metadata = std::fs::metadata(&config)
            .map_err(|error| format!("Cannot inspect configuration: {error}"))?;
        if metadata.len() > MAX_SETTINGS_BYTES {
            return Err(String::from(
                "Instance configuration exceeds the 2 MiB inspection limit",
            ));
        }
        let settings = serde_json::from_str::<Value>(
            &read_instance_settings_json(&config).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let runtime = root.join("runtime");
        let saves = plan_instance_saves_dir(
            descriptor,
            &InstanceSavePathContext {
                install_root: &runtime,
                instance_root: root,
                config_dir: &record.config_dir,
                instance_id: &record.summary.id,
                instance_name: &record.summary.name,
                module_id: &record.summary.module_id,
                settings: settings.as_object(),
            },
        )
        .map_err(|error| error.to_string())?;
        Ok((settings, saves))
    };
    let (settings, saves) = match inspect() {
        Ok(value) => value,
        Err(error) => {
            projected.issues.push(issue(
                &projected.member,
                "inspection_failed",
                "error",
                error,
            ));
            return projected;
        }
    };
    projected.member.saves_path = saves.to_string_lossy().into_owned();
    projected.member.map_name = setting(&settings, "map_name");
    projected.member.cluster_id = setting(&settings, "cluster_id").trim().to_owned();
    let directory = setting(&settings, "cluster_directory");
    projected.member.explicit_shared_directory = !directory.trim().is_empty();
    let raw_flags = setting(&settings, "custom_launch_flags").to_ascii_lowercase();
    if raw_flags.contains("-clusterid") || raw_flags.contains("-clusterdir") {
        projected.issues.push(issue(&projected.member, "unmanaged_cluster_options", "error",
            "Cluster options in custom launch flags cannot be grouped safely. Use the managed Cluster ID and shared directory fields."));
        return projected;
    }
    match app_core::ark_cluster::resolve_cluster_directory(
        &projected.member.cluster_id,
        &directory,
        &app_core::ark_maps::primary_saves_dir(&record.summary.id, &saves),
    ) {
        Ok(Some(path)) => match canonical_cluster_directory(&path) {
            Ok(canonical) => {
                projected.directory_key = Some(directory_key(&canonical));
                projected.member.cluster_directory = Some(canonical.to_string_lossy().into_owned());
            }
            Err(error) => projected.issues.push(issue(
                &projected.member,
                "directory_unverified",
                "error",
                error,
            )),
        },
        Ok(None) => {}
        Err(error) => projected.issues.push(issue(
            &projected.member,
            "invalid_cluster_settings",
            "error",
            error.to_string(),
        )),
    }
    projected
}

fn assemble_report(
    instance_id: &str,
    projections: Vec<Projection>,
) -> Result<ArkClusterReport, StorageError> {
    let target = projections
        .iter()
        .find(|entry| entry.member.summary.id == instance_id)
        .ok_or_else(|| StorageError::MissingInstance {
            id: instance_id.to_owned(),
        })?;
    let mut report = ArkClusterReport {
        instance_id: instance_id.to_owned(),
        identity: None,
        cluster_directory: target.member.cluster_directory.clone(),
        members: Vec::new(),
        related_instances: Vec::new(),
        issues: target.issues.clone(),
        start_blocked: false,
    };
    let cluster_id = &target.member.cluster_id;
    if cluster_id.is_empty() || target.directory_key.is_none() {
        report.start_blocked = true;
        return Ok(report);
    }
    for candidate in &projections {
        let same_id = candidate.member.cluster_id == *cluster_id;
        let same_directory =
            candidate.directory_key.is_some() && candidate.directory_key == target.directory_key;
        let overlapping_directory = candidate
            .directory_key
            .as_ref()
            .zip(target.directory_key.as_ref())
            .is_some_and(|(left, right)| {
                left.starts_with(&format!("{right}/")) || right.starts_with(&format!("{left}/"))
            });
        let same_edition = candidate.member.summary.module_id == target.member.summary.module_id;
        if same_id && same_directory && same_edition {
            report.members.push(candidate.member.clone());
            if candidate.member.summary.id != instance_id {
                report.issues.extend(candidate.issues.clone());
            }
        } else if same_id || same_directory || overlapping_directory {
            report.related_instances.push(candidate.member.clone());
            report.issues.extend(candidate.issues.clone());
            let (code, message) = if overlapping_directory {
                (
                    "directory_overlap",
                    "This transfer directory contains or is contained by another ARK instance's transfer directory. Separate these directories before cluster maintenance.",
                )
            } else if same_directory && !same_edition {
                (
                    "directory_edition_conflict",
                    "Different ARK editions share this transfer directory. Use separate directories for ASE and ASA.",
                )
            } else if same_directory {
                (
                    "directory_id_conflict",
                    "This transfer directory is also configured with another Cluster ID. Verify the intended cluster boundary.",
                )
            } else {
                (
                    "id_directory_mismatch",
                    "This Cluster ID uses another transfer directory; uploads are not shared with this instance.",
                )
            };
            report
                .issues
                .push(issue(&candidate.member, code, "error", message));
        } else if !candidate.issues.is_empty() && candidate.member.cluster_id.is_empty() {
            report.issues.push(issue(&candidate.member, "peer_inspection_incomplete", "warning",
                "This ARK instance could not be classified; review its configuration before including it in cluster operations."));
        }
    }
    report
        .members
        .sort_by(|left, right| left.summary.id.cmp(&right.summary.id));
    report.identity = Some(ArkClusterIdentity {
        module_id: target.member.summary.module_id.clone(),
        cluster_id: cluster_id.clone(),
        directory_key: target.directory_key.clone().unwrap_or_default(),
        member_ids: report
            .members
            .iter()
            .map(|entry| entry.summary.id.clone())
            .collect(),
    });
    report.start_blocked = report.issues.iter().any(|entry| entry.severity == "error");
    Ok(report)
}

fn setting(settings: &Value, key: &str) -> String {
    settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn issue(
    member: &ArkClusterMember,
    code: &str,
    severity: &str,
    message: impl Into<String>,
) -> ArkClusterIssue {
    ArkClusterIssue {
        code: code.to_owned(),
        severity: severity.to_owned(),
        instance_id: member.summary.id.clone(),
        instance_name: member.summary.name.clone(),
        message: message.into(),
        path: member.cluster_directory.clone(),
    }
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "ark_clusters_tests.rs"]
mod tests;
