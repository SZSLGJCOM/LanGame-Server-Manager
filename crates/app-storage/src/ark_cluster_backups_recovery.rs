//! Durable transaction ownership and explicit recovery after process interruption.
use std::fs;
use std::path::{Path, PathBuf};

use app_core::InstanceStatus;
use serde::{Deserialize, Serialize};

use super::{
    ArkClusterBackupSummary, ArkClusterIdentity, Manifest, Plan, backup_root, files, invalid,
    overlaps, path_key, restore, transaction,
};
use crate::ark_clusters::{ArkClusterMember, ArkClusterReport, is_ark_module};
use crate::backups::publication::{Publication, Stamp};
use crate::backups::{canonical_plain_directory, move_directory};
use crate::{StorageError, StoragePaths, StoredInstanceRecord};

pub(super) const JOURNAL_NAME: &str = "restore-pending.json";
pub(super) const POINTER_NAME: &str = ".langame-ark-restore.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Workspace {
    pub key: String,
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WorkspaceOwner {
    identity: ArkClusterIdentity,
    backup_id: String,
    safeguard_id: String,
    key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    pub version: u32,
    pub identity: ArkClusterIdentity,
    pub backup_id: String,
    pub safeguard_id: String,
    pub committed: bool,
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pointer {
    identity: ArkClusterIdentity,
    backup_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingArkClusterRestore {
    pub identity: ArkClusterIdentity,
    pub backup_id: String,
    pub safeguard_backup: ArkClusterBackupSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArkClusterRecoveryResult {
    pub outcome: String,
    pub backup: ArkClusterBackupSummary,
    pub cleanup_warnings: Vec<String>,
}

pub(super) fn write_workspace_owner(
    workspace: &Workspace,
    backup: &Manifest,
    safeguard: &Manifest,
) -> Result<(), StorageError> {
    transaction::write_json(
        &workspace.path.join("owner.json"),
        &WorkspaceOwner {
            identity: backup.summary.identity.clone(),
            backup_id: backup.summary.backup_id.clone(),
            safeguard_id: safeguard.summary.backup_id.clone(),
            key: workspace.key.clone(),
        },
    )
}

pub(super) fn write_initial(plan: &Plan, journal: &Journal) -> Result<(), StorageError> {
    transaction::ensure_ready(&plan.root)?;
    transaction::write_json(&plan.root.join(JOURNAL_NAME), journal)?;
    let pointer = Pointer {
        identity: journal.identity.clone(),
        backup_id: journal.backup_id.clone(),
    };
    for path in pointer_paths(plan)? {
        if path.exists() {
            return Err(invalid(
                &path,
                "This instance already has a pending restore pointer",
            ));
        }
        transaction::write_json(&path, &pointer)?;
    }
    Ok(())
}

pub(super) fn read_pending(
    storage: &StoragePaths,
    record: &StoredInstanceRecord,
) -> Result<Option<PendingArkClusterRestore>, StorageError> {
    let instance_root = record
        .config_dir
        .parent()
        .ok_or_else(|| invalid(&record.config_dir, "Missing instance root"))?;
    crate::instances::validate_managed_instance_root(instance_root, &storage.instances_root)?;
    let pointer_path = instance_root.join(POINTER_NAME);
    let pointer: Pointer = match fs::symlink_metadata(&pointer_path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: pointer_path,
                source,
            });
        }
        Ok(_) => transaction::read_json(&pointer_path, 128 * 1024)?,
    };
    if pointer.identity.module_id != record.summary.module_id
        || !pointer.identity.member_ids.contains(&record.summary.id)
    {
        return Err(invalid(
            &pointer_path,
            "Restore pointer does not belong to this registered instance",
        ));
    }
    let root = backup_root(storage, &pointer.identity)?;
    let pending = read_pending_root(&root, &pointer.identity)?
        .ok_or_else(|| invalid(&pointer_path, "Restore pointer has no journal"))?;
    if pending.backup_id != pointer.backup_id {
        return Err(invalid(
            &pointer_path,
            "Restore pointer and journal disagree",
        ));
    }
    Ok(Some(pending))
}

pub(super) fn read_pending_root(
    root: &Path,
    identity: &ArkClusterIdentity,
) -> Result<Option<PendingArkClusterRestore>, StorageError> {
    match fs::symlink_metadata(root.join(JOURNAL_NAME)) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: root.join(JOURNAL_NAME),
                source,
            });
        }
        Ok(_) => {}
    }
    let journal: Journal = transaction::read_json(&root.join(JOURNAL_NAME), 2 * 1024 * 1024)?;
    if journal.version != 1 || journal.identity != *identity {
        return Err(invalid(root, "Restore pointer and journal disagree"));
    }
    let safeguard = transaction::load(root, &journal.safeguard_id)?;
    if safeguard.summary.identity != journal.identity
        || safeguard.summary.backup_kind != "pre_restore"
    {
        return Err(invalid(
            root,
            "Pending restore safeguard belongs to another cluster",
        ));
    }
    Ok(Some(PendingArkClusterRestore {
        identity: journal.identity,
        backup_id: journal.backup_id,
        safeguard_backup: safeguard.summary,
    }))
}

pub(super) fn report_for_recovery(
    storage: &StoragePaths,
    records: &[StoredInstanceRecord],
    ports: &[crate::InstancePortProjection],
    expected: &ArkClusterIdentity,
    id: &str,
) -> Result<ArkClusterReport, StorageError> {
    let root = backup_root(storage, expected)?;
    let journal: Journal = transaction::read_json(&root.join(JOURNAL_NAME), 2 * 1024 * 1024)?;
    if journal.version != 1
        || journal.identity != *expected
        || journal.backup_id != id
        || expected.member_ids.is_empty()
        || expected
            .member_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(invalid(
            &root,
            "Recovery confirmation does not match the interrupted restore",
        ));
    }
    let safeguard = transaction::load(&root, &journal.safeguard_id)?;
    if safeguard.summary.identity != *expected || safeguard.summary.backup_kind != "pre_restore" {
        return Err(invalid(
            &root,
            "Recovery safeguard belongs to another cluster",
        ));
    }
    let shared = safeguard
        .scopes
        .last()
        .ok_or_else(|| invalid(&root, "Missing recovery transfer scope"))?
        .target
        .clone();
    if !shared.is_absolute() || path_key(&shared) != path_key(Path::new(&expected.directory_key)) {
        return Err(invalid(
            &shared,
            "Recovery transfer scope differs from the confirmed cluster directory",
        ));
    }
    let mut members = Vec::new();
    for (index, member_id) in expected.member_ids.iter().enumerate() {
        let record = records
            .iter()
            .find(|record| &record.summary.id == member_id)
            .ok_or_else(|| invalid(&root, "A recovery member is no longer registered"))?;
        if record.summary.module_id != expected.module_id
            || !matches!(record.summary.status, InstanceStatus::Stopped)
            || record.summary.active_process_count != 0
        {
            return Err(invalid(
                &root,
                "Every original recovery member must remain registered and stopped",
            ));
        }
        members.push(ArkClusterMember {
            summary: record.summary.clone(),
            map_name: safeguard.summary.members[index].map_name.clone(),
            cluster_id: expected.cluster_id.clone(),
            cluster_directory: Some(shared.to_string_lossy().into_owned()),
            explicit_shared_directory: true,
            config_file_path: record
                .config_dir
                .join("instance.json")
                .to_string_lossy()
                .into_owned(),
            saves_path: record.saves_dir.to_string_lossy().into_owned(),
            ports: ports
                .iter()
                .find(|projection| projection.instance_id == *member_id)
                .ok_or_else(|| {
                    invalid(&root, "A recovery member has no registered port projection")
                })?
                .ports
                .clone(),
        });
    }
    // Membership files can be temporarily absent for owned members. Every other
    // registered ARK instance must still be inspectable and outside this shared root.
    for record in records.iter().filter(|record| {
        is_ark_module(&record.summary.module_id)
            && !expected.member_ids.contains(&record.summary.id)
    }) {
        let config = record.config_dir.join("instance.json");
        let value: serde_json::Value = transaction::read_json(&config, 2 * 1024 * 1024)?;
        let settings = value
            .get("settings")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                invalid(
                    &config,
                    "Another ARK instance cannot be classified for recovery",
                )
            })?;
        let directory = settings
            .get("cluster_directory")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        let flags = settings
            .get("custom_launch_flags")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase();
        if flags.contains("-clusterid")
            || flags.contains("-clusterdir")
            || (!directory.is_empty() && overlaps(&shared, Path::new(directory)))
        {
            return Err(invalid(
                &config,
                "Another ARK instance may use the interrupted cluster's transfer root",
            ));
        }
    }
    Ok(ArkClusterReport {
        instance_id: expected.member_ids[0].clone(),
        identity: Some(expected.clone()),
        cluster_directory: Some(shared.to_string_lossy().into_owned()),
        members,
        related_instances: Vec::new(),
        issues: Vec::new(),
        start_blocked: false,
    })
}

#[cfg(test)]
pub(super) fn recover(plan: &Plan, id: &str) -> Result<ArkClusterRecoveryResult, StorageError> {
    recover_with_publication(plan, id, &Publication::uncoordinated())
}

pub(super) fn recover_with_publication(
    plan: &Plan,
    id: &str,
    publication: &Publication,
) -> Result<ArkClusterRecoveryResult, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &plan.root,
        crate::instance_archive::test_gate::Point::BackupPreparing,
    );
    let journal: Journal = transaction::read_json(&plan.root.join(JOURNAL_NAME), 2 * 1024 * 1024)?;
    if journal.version != 1
        || plan.report.identity.as_ref() != Some(&journal.identity)
        || journal.backup_id != id
    {
        return Err(invalid(
            &plan.root,
            "Recovery confirmation differs from the pending transaction",
        ));
    }
    let backup = transaction::load(&plan.root, &journal.backup_id)?;
    let safeguard = transaction::load(&plan.root, &journal.safeguard_id)?;
    validate(plan, &journal, &backup, &safeguard)?;
    let resulting = if journal.committed {
        &backup
    } else {
        &safeguard
    };
    let source = plan.root.join(&resulting.summary.backup_id);
    for scope in resulting
        .scopes
        .iter()
        .filter(|scope| scope.key.ends_with("-config"))
    {
        if files::inventory(&source.join(&scope.key))? != scope.entries {
            return Err(invalid(
                &source,
                "Recovery configuration no longer matches its snapshot",
            ));
        }
    }
    // A user may edit registration after an interrupted process exits. Never
    // remove the journal while recovering a conflicting configuration mirror.
    restore::validate_configurations(plan, &source)?;
    if journal.committed {
        // A durable commit follows all replacements. Never undo that commit merely
        // because the application exited during disposal of old directory copies.
        let stamp = Stamp::capture(
            &plan
                .scopes
                .iter()
                .map(|scope| scope.target.clone())
                .collect::<Vec<_>>(),
        )?;
        for (scope, expected) in plan.scopes.iter().zip(&backup.scopes) {
            require_contents(&scope.target, expected)?;
        }
        publication.publish(&stamp, || Ok(()))?;
        Ok(ArkClusterRecoveryResult {
            outcome: String::from("completed"),
            backup: backup.summary,
            cleanup_warnings: finish(plan, &journal),
        })
    } else {
        let cleanup_warnings =
            rollback_with_publication(plan, &journal, &backup, &safeguard, publication)?;
        Ok(ArkClusterRecoveryResult {
            outcome: String::from("rolled_back"),
            backup: safeguard.summary,
            cleanup_warnings,
        })
    }
}

pub(super) fn rollback_with_publication(
    plan: &Plan,
    journal: &Journal,
    backup: &Manifest,
    safeguard: &Manifest,
    publication: &Publication,
) -> Result<Vec<String>, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &plan.root,
        crate::instance_archive::test_gate::Point::BackupRollback,
    );
    validate(plan, journal, backup, safeguard)?;
    let roots = publication_roots(plan, journal);
    let stamp = Stamp::capture(&roots)?;
    let mut errors = Vec::new();
    let mut moves = Vec::new();
    for index in (0..plan.scopes.len()).rev() {
        if let Err(error) =
            prepare_rollback_scope(plan, journal, backup, safeguard, index, &mut moves)
        {
            errors.push(error.to_string());
        }
    }
    if !errors.is_empty() {
        return Err(invalid(&plan.root, errors.join("; ")));
    }
    publication.publish(&stamp, || {
        for (from, to) in &moves {
            move_directory(from, to)?;
        }
        Ok(())
    })?;
    for (scope, original) in plan.scopes.iter().zip(&safeguard.scopes) {
        require_contents(&scope.target, original)?;
    }
    Ok(finish(plan, journal))
}

pub(super) fn publication_roots(plan: &Plan, journal: &Journal) -> Vec<PathBuf> {
    plan.scopes
        .iter()
        .map(|scope| scope.target.clone())
        .chain(journal.workspaces.iter().flat_map(|workspace| {
            ["new", "old", "failed", "owner.json"].map(|child| workspace.path.join(child))
        }))
        .collect()
}

fn prepare_rollback_scope(
    plan: &Plan,
    journal: &Journal,
    backup: &Manifest,
    safeguard: &Manifest,
    index: usize,
    moves: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), StorageError> {
    let target = &plan.scopes[index].target;
    let workspace = &journal.workspaces[index].path;
    let old = workspace.join("old");
    let failed = workspace.join("failed");
    let original = &safeguard.scopes[index];
    if old.exists() {
        require_contents(&old, original)?;
        if target.exists() {
            require_contents(target, &backup.scopes[index])?;
            if failed.exists() {
                return Err(invalid(&failed, "Unexpected duplicate rollback directory"));
            }
            moves.push((target.clone(), failed));
        }
        moves.push((old, target.clone()));
    } else if original.existed {
        // Either untouched or already rolled back by an earlier recovery attempt.
        require_contents(target, original)?;
    } else if target.exists() {
        if workspace.join("new").exists() {
            return Err(invalid(
                target,
                "An unexpected target appeared before publication",
            ));
        }
        require_contents(target, &backup.scopes[index])?;
        if failed.exists() {
            return Err(invalid(&failed, "Unexpected duplicate rollback directory"));
        }
        moves.push((target.clone(), failed));
    }
    Ok(())
}

fn require_contents(path: &Path, scope: &super::Scope) -> Result<(), StorageError> {
    if path.exists() != scope.existed || files::inventory(path)? != scope.entries {
        return Err(invalid(
            path,
            "Restore transaction data changed; retained every recovery copy",
        ));
    }
    Ok(())
}

fn validate(
    plan: &Plan,
    journal: &Journal,
    backup: &Manifest,
    safeguard: &Manifest,
) -> Result<(), StorageError> {
    restore::validate_manifest(plan, backup)?;
    restore::validate_manifest(plan, safeguard)?;
    if journal.workspaces.len() != plan.scopes.len()
        || safeguard.summary.backup_kind != "pre_restore"
    {
        return Err(invalid(&plan.root, "Malformed restore journal"));
    }
    for path in pointer_paths(plan)? {
        if path.exists() {
            let pointer: Pointer = transaction::read_json(&path, 128 * 1024)?;
            if pointer.identity != journal.identity || pointer.backup_id != journal.backup_id {
                return Err(invalid(
                    &path,
                    "Member recovery pointer belongs to another transaction",
                ));
            }
        }
    }
    for (scope, workspace) in plan.scopes.iter().zip(&journal.workspaces) {
        let parent = scope
            .target
            .parent()
            .ok_or_else(|| invalid(&scope.target, "Restore target has no parent"))?;
        let name = workspace
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let suffix = name.strip_prefix(".langame-ark-restore-").unwrap_or("");
        if workspace.key != scope.key
            || suffix.len() != 12
            || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
            || workspace.path.parent().map(path_key) != Some(path_key(parent))
        {
            return Err(invalid(
                &workspace.path,
                "Journal workspace is outside its registered target",
            ));
        }
        files::plain_ancestors(&workspace.path)?;
        if workspace.path.exists() {
            canonical_plain_directory(&workspace.path, parent)?;
            let owner: WorkspaceOwner =
                transaction::read_json(&workspace.path.join("owner.json"), 128 * 1024)?;
            let expected = WorkspaceOwner {
                identity: journal.identity.clone(),
                backup_id: journal.backup_id.clone(),
                safeguard_id: journal.safeguard_id.clone(),
                key: scope.key.clone(),
            };
            if owner != expected {
                return Err(invalid(
                    &workspace.path,
                    "Restore workspace belongs to another transaction",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn finish(plan: &Plan, journal: &Journal) -> Vec<String> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &plan.root,
        crate::instance_archive::test_gate::Point::BackupCleanup,
    );
    let mut warnings = restore::cleanup_workspaces(&journal.workspaces);
    if !warnings.is_empty() {
        return warnings;
    }
    let paths = match pointer_paths(plan) {
        Ok(paths) => paths,
        Err(error) => return vec![error.to_string()],
    };
    for path in paths {
        if let Err(error) = remove_plain_file(&path) {
            warnings.push(error.to_string());
        }
    }
    if warnings.is_empty()
        && let Err(error) = remove_plain_file(&plan.root.join(JOURNAL_NAME))
    {
        warnings.push(error.to_string());
    }
    warnings
}

fn pointer_paths(plan: &Plan) -> Result<Vec<PathBuf>, StorageError> {
    plan.scopes
        .iter()
        .filter(|scope| scope.key.ends_with("-config"))
        .map(|scope| {
            scope
                .target
                .parent()
                .map(|parent| parent.join(POINTER_NAME))
                .ok_or_else(|| invalid(&scope.target, "Missing instance root for recovery pointer"))
        })
        .collect()
}

fn remove_plain_file(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
        Ok(metadata) if !metadata.is_file() || crate::backups::is_link_or_reparse(&metadata) => {
            return Err(invalid(path, "Recovery marker is not a plain file"));
        }
        Ok(_) => {}
    }
    fs::remove_file(path).map_err(|source| StorageError::DeletePath {
        path: path.to_owned(),
        source,
    })
}
