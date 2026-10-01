use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use app_core::InstanceStatus;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ark_clusters::{
    ArkClusterIdentity, ArkClusterReport, is_ark_module, read_ark_cluster_report,
};
use crate::backups::publication::{self, Publication};
use crate::backups::{canonical_plain_directory, ensure_plain_directory};
use crate::instance_settings_lock::{
    InstanceSettingsLock, acquire_instance_settings_mutation_lock,
    acquire_module_instance_creation_lock_blocking,
};
use crate::storage_db::{connect_pool, load_instance_isolation_records};
use crate::{StorageError, StoragePaths};

#[path = "ark_cluster_backups_fs.rs"]
mod files;
#[path = "ark_cluster_backups_recovery.rs"]
mod recovery;
#[path = "ark_cluster_backups_restore.rs"]
mod restore;
#[path = "ark_cluster_backups_transaction.rs"]
mod transaction;
pub use recovery::{ArkClusterRecoveryResult, PendingArkClusterRestore};
#[cfg(test)]
#[path = "ark_cluster_backups_tests.rs"]
mod tests;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkClusterBackupMember {
    pub instance_id: String,
    pub instance_name: String,
    pub map_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArkClusterBackupSummary {
    pub backup_id: String,
    pub created_at_unix_ms: u128,
    pub backup_kind: String,
    pub identity: ArkClusterIdentity,
    pub backup_path: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub members: Vec<ArkClusterBackupMember>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArkClusterBackupRestoreResult {
    pub backup: ArkClusterBackupSummary,
    pub safeguard_backup: ArkClusterBackupSummary,
    pub restored_at_unix_ms: u128,
    pub cleanup_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Scope {
    key: String,
    target: PathBuf,
    existed: bool,
    entries: Vec<files::Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    version: u32,
    summary: ArkClusterBackupSummary,
    scopes: Vec<Scope>,
}

#[derive(Debug, Clone)]
struct Plan {
    root: PathBuf,
    report: ArkClusterReport,
    scopes: Vec<Scope>,
}

pub async fn create_ark_cluster_backup(
    storage: &StoragePaths,
    instance_id: &str,
    expected: &ArkClusterIdentity,
    exclusive_root_confirmed: bool,
) -> Result<ArkClusterBackupSummary, StorageError> {
    operate(
        storage,
        instance_id,
        expected,
        exclusive_root_confirmed,
        None,
        |plan, publication| {
            transaction::snapshot_with_publication(&plan, "manual", publication)
                .map(|manifest| manifest.summary)
        },
    )
    .await
}

pub async fn restore_ark_cluster_backup(
    storage: &StoragePaths,
    instance_id: &str,
    expected: &ArkClusterIdentity,
    backup_id: &str,
    exclusive_root_confirmed: bool,
) -> Result<ArkClusterBackupRestoreResult, StorageError> {
    let backup_id = backup_id.to_owned();
    operate(
        storage,
        instance_id,
        expected,
        exclusive_root_confirmed,
        None,
        move |plan, publication| {
            restore::restore_with_publication(&plan, &backup_id, publication, |_, _| Ok(()))
        },
    )
    .await
}

pub async fn recover_ark_cluster_restore(
    storage: &StoragePaths,
    instance_id: &str,
    expected: &ArkClusterIdentity,
    backup_id: &str,
    exclusive_root_confirmed: bool,
) -> Result<ArkClusterRecoveryResult, StorageError> {
    let backup_id = backup_id.to_owned();
    operate(
        storage,
        instance_id,
        expected,
        exclusive_root_confirmed,
        Some(backup_id.clone()),
        move |plan, publication| recovery::recover_with_publication(&plan, &backup_id, publication),
    )
    .await
}

pub async fn read_pending_ark_cluster_restore(
    storage: &StoragePaths,
    instance_id: &str,
) -> Result<Option<PendingArkClusterRestore>, StorageError> {
    let pool = connect_pool(storage).await?;
    let result = crate::storage_db::fetch_instance_record(&pool, instance_id).await;
    pool.close().await;
    let record = result?;
    if !is_ark_module(&record.summary.module_id) {
        return Ok(None);
    }
    let read_storage = storage.clone();
    let pending =
        tokio::task::spawn_blocking(move || recovery::read_pending(&read_storage, &record))
            .await
            .map_err(|error| {
                invalid(
                    Path::new(instance_id),
                    format!("Reading pending cluster restore failed: {error}"),
                )
            })??;
    if pending.is_some() {
        return Ok(pending);
    }
    // Initial marker publication and final cleanup can stop between two pointer
    // writes. The intact config still provides the cluster identity in that window.
    let report = read_ark_cluster_report(storage, instance_id).await?;
    let Some(identity) = report.identity else {
        return Ok(None);
    };
    let root = backup_root(storage, &identity)?;
    tokio::task::spawn_blocking(move || recovery::read_pending_root(&root, &identity))
        .await
        .map_err(|error| invalid(Path::new(instance_id), error.to_string()))?
}

/// Starting any member while a restore journal remains could write a mixed generation.
pub fn ensure_ark_cluster_backup_ready(
    storage: &StoragePaths,
    identity: &ArkClusterIdentity,
) -> Result<(), StorageError> {
    transaction::ensure_ready(&backup_root(storage, identity)?)
}

pub async fn list_ark_cluster_backups(
    storage: &StoragePaths,
    instance_id: &str,
) -> Result<Vec<ArkClusterBackupSummary>, StorageError> {
    let report = read_ark_cluster_report(storage, instance_id).await?;
    let identity = report.identity.ok_or_else(|| {
        invalid(
            &storage.instances_root,
            "Configure an explicit ARK cluster before listing snapshots",
        )
    })?;
    let root = backup_root(storage, &identity)?;
    tokio::task::spawn_blocking(move || transaction::list(&root, &identity))
        .await
        .map_err(|error| {
            invalid(
                &storage.instances_root,
                format!("Cluster snapshot listing failed: {error}"),
            )
        })?
}

async fn operate<T, F>(
    storage: &StoragePaths,
    instance_id: &str,
    expected: &ArkClusterIdentity,
    confirmed: bool,
    recovery_id: Option<String>,
    operation: F,
) -> Result<T, StorageError>
where
    T: Send + 'static,
    F: FnOnce(Plan, &Publication) -> Result<T, StorageError> + Send + 'static,
{
    if !confirmed {
        return Err(invalid(
            &storage.instances_root,
            "Confirm that the selected shared root belongs exclusively to this ARK cluster",
        ));
    }
    if expected.member_ids.is_empty()
        || expected.member_ids.len() > 128
        || expected
            .member_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 512)
        || expected.directory_key.len() > 32768
        || expected.cluster_id.len() > 1024
    {
        return Err(invalid(
            &storage.instances_root,
            "Invalid ARK cluster member snapshot",
        ));
    }
    if !expected.member_ids.iter().any(|id| id == instance_id)
        || !is_ark_module(&expected.module_id)
    {
        return Err(invalid(
            &storage.instances_root,
            "The anchor does not belong to the confirmed ARK cluster",
        ));
    }
    let storage = storage.clone();
    let expected = expected.clone();
    let instance_id = instance_id.to_owned();
    let lock_paths = storage.clone();
    let module_locks = tokio::task::spawn_blocking(move || {
        ["arksurvivalascended", "arksurvivalevolved"]
            .into_iter()
            .map(|module| acquire_module_instance_creation_lock_blocking(&lock_paths, module))
            .collect::<Result<Vec<_>, _>>()
    })
    .await
    .map_err(|error| invalid(&storage.instances_root, error.to_string()))??;
    let owner = module_locks[0].clone();
    owner.complete_mutation("ARK cluster snapshot transaction", async move {
        let _module_locks = module_locks;
        let pool = connect_pool(&storage).await?;
        let result = async {
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        let records = load_instance_isolation_records(&mut tx).await?;
        let mut ids = records.iter().filter(|record| is_ark_module(&record.summary.module_id)).map(|record| record.summary.id.clone()).collect::<Vec<_>>();
        if ids.len() > 128 { return Err(invalid(&storage.instances_root, "ARK cluster snapshot is limited to 128 registered ARK instances")); }
        ids.sort();
        let locks = ids.iter().map(|id| acquire_instance_settings_mutation_lock(&storage, id)).collect::<Result<Vec<InstanceSettingsLock>, _>>()?;
        let recovering = recovery_id.is_some();
        if !recovering {
            let pending_storage = storage.clone();
            let pending_records = records.iter().filter(|record| expected.member_ids.contains(&record.summary.id)).cloned().collect::<Vec<_>>();
            tokio::task::spawn_blocking(move || {
                for record in &pending_records {
                    if recovery::read_pending(&pending_storage, record)?.is_some() {
                        return Err(invalid(&record.config_dir, "Recover the member's interrupted cluster restore before creating or restoring another snapshot"));
                    }
                }
                Ok::<_, StorageError>(())
            }).await.map_err(|error| invalid(&storage.instances_root, error.to_string()))??;
        }
        let report = if let Some(id) = recovery_id {
            let recovery_storage = storage.clone();
            let recovery_records = records.clone();
            let recovery_expected = expected.clone();
            // The write reservation keeps these batched DB projections stable even
            // when an interrupted publication temporarily removed instance.json.
            let recovery_ports = crate::read_instance_port_projections(&storage, &expected.member_ids).await?;
            tokio::task::spawn_blocking(move || recovery::report_for_recovery(&recovery_storage, &recovery_records, &recovery_ports, &recovery_expected, &id)).await
                .map_err(|error| invalid(&storage.instances_root, error.to_string()))??
        } else { read_ark_cluster_report(&storage, &instance_id).await? };
        validate_report(&report, &expected)?;
        for member in &report.members {
            let config = Path::new(&member.config_file_path).parent().ok_or_else(|| invalid(&storage.instances_root, "Missing member configuration root"))?;
            let runtime = config.parent().ok_or_else(|| invalid(config, "Missing member root"))?.join("runtime");
            crate::instance_isolation::ensure_instance_paths_available(&storage, &mut tx, &member.summary.id, &member.summary.module_id, &runtime, config, Path::new(&member.saves_path)).await?;
        }
        let forbidden = records.iter().flat_map(|record| [record.config_dir.clone(), record.saves_dir.clone()]).collect::<Vec<_>>();
        let plan_storage = storage.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let plan = build_plan(&plan_storage, report, &forbidden)?;
            if !recovering { transaction::ensure_ready(&plan.root)?; }
            Ok::<_, StorageError>(plan)
        }).await.map_err(|error| invalid(&storage.instances_root, format!("Cluster plan worker failed: {error}")))??;
        tx.rollback().await?;
        let checked_storage = storage.clone();
        let checked_plan = plan.clone();
        publication::run(&pool, move |connection| {
            let storage = checked_storage.clone();
            let plan = checked_plan.clone();
            let ids = ids.clone();
            Box::pin(async move { validate_publication(&storage, connection, &plan, &ids).await })
        }, move |publication| {
            let _locks = locks;
            operation(plan, &publication)
        }).await
        }.await;
        pool.close().await;
        result
    }).await
}

async fn validate_publication(
    storage: &StoragePaths,
    connection: &mut sqlx::SqliteConnection,
    plan: &Plan,
    frozen_ark_ids: &[String],
) -> Result<(), StorageError> {
    let records = load_instance_isolation_records(&mut *connection).await?;
    let mut ids = records
        .iter()
        .filter(|record| is_ark_module(&record.summary.module_id))
        .map(|record| record.summary.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    if ids != frozen_ark_ids {
        return Err(invalid(
            &plan.root,
            "ARK membership changed while preparing backup publication",
        ));
    }
    for member in &plan.report.members {
        let record = records
            .iter()
            .find(|record| record.summary.id == member.summary.id)
            .ok_or_else(|| invalid(&plan.root, "Backup member is no longer registered"))?;
        let ports =
            crate::storage_db::load_instance_ports(&mut *connection, &member.summary.id).await?;
        if record.config_dir.join("instance.json") != Path::new(&member.config_file_path)
            || serde_json::to_value(&record.summary)? != serde_json::to_value(&member.summary)?
            || serde_json::to_value(&ports)? != serde_json::to_value(&member.ports)?
        {
            return Err(invalid(
                &record.config_dir,
                "Instance registration, ports or runtime state changed during backup preparation",
            ));
        }
        let runtime = crate::instances::effective_instance_install_root(record)?;
        crate::instance_isolation::ensure_instance_paths_available(
            storage,
            connection,
            &member.summary.id,
            &member.summary.module_id,
            &runtime,
            &record.config_dir,
            Path::new(&member.saves_path),
        )
        .await?;
    }
    let member = &plan.report.members[0];
    let record = records
        .iter()
        .find(|record| record.summary.id == member.summary.id)
        .ok_or_else(|| invalid(&plan.root, "Backup anchor is no longer registered"))?;
    let runtime = crate::instances::effective_instance_install_root(record)?;
    let transfer = plan
        .scopes
        .last()
        .ok_or_else(|| invalid(&plan.root, "Missing transfer scope"))?;
    // Transfer directories are deliberately shareable within ARK, but must not
    // replace a saves/configuration tree claimed by any other instance.
    crate::instance_isolation::ensure_instance_paths_available(
        storage,
        connection,
        &member.summary.id,
        &member.summary.module_id,
        &runtime,
        &record.config_dir,
        &transfer.target,
    )
    .await
}

fn validate_report(
    report: &ArkClusterReport,
    expected: &ArkClusterIdentity,
) -> Result<(), StorageError> {
    let path = Path::new(report.cluster_directory.as_deref().unwrap_or(""));
    if report.identity.as_ref() != Some(expected) || report.members.is_empty() {
        return Err(invalid(
            path,
            "ARK cluster membership changed; refresh the cluster before retrying",
        ));
    }
    if report.start_blocked
        || report
            .issues
            .iter()
            .any(|issue| issue.code == "peer_inspection_incomplete")
    {
        return Err(invalid(
            path,
            "Resolve all ARK cluster membership and directory inspection conflicts before taking or restoring a snapshot",
        ));
    }
    for member in &report.members {
        if !member.explicit_shared_directory {
            return Err(invalid(
                path,
                "Cluster snapshots require an explicitly selected, exclusive shared root",
            ));
        }
        if !matches!(member.summary.status, InstanceStatus::Stopped)
            || member.summary.active_process_count != 0
        {
            return Err(invalid(
                path,
                format!(
                    "Stop every cluster member before snapshot operations; {} is not stopped",
                    member.summary.name
                ),
            ));
        }
    }
    Ok(())
}

fn build_plan(
    storage: &StoragePaths,
    report: ArkClusterReport,
    forbidden: &[PathBuf],
) -> Result<Plan, StorageError> {
    let identity = report
        .identity
        .as_ref()
        .ok_or_else(|| invalid(&storage.instances_root, "Missing cluster identity"))?;
    let shared = PathBuf::from(
        report
            .cluster_directory
            .as_ref()
            .ok_or_else(|| invalid(&storage.instances_root, "Missing transfer root"))?,
    );
    let roots = [
        &storage.app_data_root,
        &storage.instances_root,
        &storage.games_root,
        &storage.modules_root,
        &storage.steamcmd_root,
    ];
    for protected in roots.into_iter().chain(forbidden.iter()) {
        if overlaps(&shared, protected) {
            return Err(invalid(
                &shared,
                format!(
                    "Choose a dedicated cluster directory outside protected application and instance data: {}",
                    protected.display()
                ),
            ));
        }
    }
    files::plain_ancestors(&shared)?;
    let shared = if shared.exists() {
        canonical_plain_directory(&shared, &shared)?
    } else {
        shared
    };
    let mut scopes = Vec::new();
    for (index, member) in report.members.iter().enumerate() {
        let config = Path::new(&member.config_file_path)
            .parent()
            .ok_or_else(|| invalid(&shared, "Missing configuration directory"))?
            .to_owned();
        let instance_root = config
            .parent()
            .ok_or_else(|| invalid(&config, "Missing instance root"))?;
        crate::instances::validate_managed_instance_root(instance_root, &storage.instances_root)?;
        let saved = instance_root.join("runtime/ShooterGame/Saved");
        if !Path::new(&member.saves_path).starts_with(&saved) {
            return Err(invalid(
                &saved,
                "ARK world is outside its private Saved directory",
            ));
        }
        for (kind, target) in [("config", config), ("saved", saved)] {
            scopes.push(Scope {
                key: format!("member-{index:03}-{kind}"),
                target,
                existed: false,
                entries: Vec::new(),
            });
        }
    }
    scopes.push(Scope {
        key: "transfer".to_owned(),
        target: shared,
        existed: false,
        entries: Vec::new(),
    });
    let root = backup_root(storage, identity)?;
    files::plain_ancestors(&root)?;
    ensure_plain_directory(&root)?;
    Ok(Plan {
        root,
        report,
        scopes,
    })
}

fn backup_root(
    storage: &StoragePaths,
    identity: &ArkClusterIdentity,
) -> Result<PathBuf, StorageError> {
    let bytes = serde_json::to_vec(&(
        &identity.module_id,
        &identity.cluster_id,
        &identity.directory_key,
    ))?;
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok(storage.app_data_root.join("cluster-backups").join(digest))
}

fn overlaps(left: &Path, right: &Path) -> bool {
    let left = path_key(left);
    let right = path_key(right);
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn path_key(path: &Path) -> String {
    let mut existing = if cfg!(windows) {
        PathBuf::from(path.to_string_lossy().replace('/', "\\"))
    } else {
        path.to_owned()
    };
    let mut tail = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            break;
        };
        tail.push(name.to_owned());
        if !existing.pop() {
            break;
        }
    }
    let mut resolved = fs::canonicalize(&existing).unwrap_or(existing);
    for name in tail.into_iter().rev() {
        resolved.push(name);
    }
    let value = resolved
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_owned();
    if cfg!(windows) {
        value
            .strip_prefix("//?/UNC/")
            .map(|value| format!("//{value}"))
            .unwrap_or_else(|| value.strip_prefix("//?/").unwrap_or(&value).to_owned())
            .to_lowercase()
    } else {
        value
    }
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}
