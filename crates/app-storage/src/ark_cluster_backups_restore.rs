//! Cluster restoration with durable rollback and recovery.
//! Every live rename follows a verified protection snapshot and a durable journal.
use std::path::Path;

use super::recovery::{self, Journal, Workspace};
use super::{
    ArkClusterBackupRestoreResult, Manifest, Plan, files, invalid, now, path_key, transaction,
};
use crate::StorageError;
use crate::backups::publication::{Publication, Stamp};
use crate::backups::{
    create_unique_directory, ensure_plain_directory, move_directory, remove_managed_directory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Staged,
    OriginalMoved,
    Published,
    Committed,
}

#[cfg(test)]
pub(super) fn restore(
    plan: &Plan,
    id: &str,
) -> Result<ArkClusterBackupRestoreResult, StorageError> {
    restore_with_hook(plan, id, |_, _| Ok(()))
}

#[cfg(test)]
pub(super) fn restore_with_hook(
    plan: &Plan,
    id: &str,
    hook: impl FnMut(Phase, usize) -> Result<(), StorageError>,
) -> Result<ArkClusterBackupRestoreResult, StorageError> {
    restore_with_publication(plan, id, &Publication::uncoordinated(), hook)
}

pub(super) fn restore_with_publication(
    plan: &Plan,
    id: &str,
    publication: &Publication,
    mut hook: impl FnMut(Phase, usize) -> Result<(), StorageError>,
) -> Result<ArkClusterBackupRestoreResult, StorageError> {
    transaction::ensure_ready(&plan.root)?;
    let backup = transaction::load(&plan.root, id)?;
    validate_manifest(plan, &backup)?;
    let source = plan.root.join(id);
    for scope in &backup.scopes {
        if files::inventory(&source.join(&scope.key))? != scope.entries {
            return Err(invalid(
                &source,
                "Snapshot files no longer match the manifest",
            ));
        }
    }
    validate_configurations(plan, &source)?;
    // This remains available even if publication and rollback both encounter I/O failure.
    let safeguard = transaction::snapshot_with_publication(plan, "pre_restore", publication)?;
    let mut workspaces = Vec::new();
    let stage_result: Result<(), StorageError> = (|| {
        for (index, scope) in plan.scopes.iter().enumerate() {
            let parent = scope
                .target
                .parent()
                .ok_or_else(|| invalid(&scope.target, "Restore target has no parent"))?;
            files::plain_ancestors(parent)?;
            ensure_plain_directory(parent)?;
            // The sum is conservative when members use different volumes. It prevents
            // several independent stages from each assuming the same free bytes.
            files::require_capacity(parent, backup.summary.total_bytes)?;
            let workspace = create_unique_directory(parent, ".langame-ark-restore")?;
            workspaces.push(Workspace {
                key: scope.key.clone(),
                path: workspace.clone(),
            });
            recovery::write_workspace_owner(
                workspaces
                    .last()
                    .ok_or_else(|| invalid(parent, "Missing restore workspace"))?,
                &backup,
                &safeguard,
            )?;
            if backup.scopes[index].existed {
                files::copy_verified(
                    &source.join(&scope.key),
                    &workspace.join("new"),
                    &backup.scopes[index].entries,
                )?;
            }
            hook(Phase::Staged, index)?;
        }
        Ok(())
    })();
    if let Err(error) = stage_result {
        let warnings = cleanup_workspaces(&workspaces);
        return Err(invalid(
            &plan.root,
            format!(
                "{error}; protection snapshot: {}; {}",
                safeguard.summary.backup_id,
                warnings.join("; ")
            ),
        ));
    }
    let mut journal = Journal {
        version: 1,
        identity: backup.summary.identity.clone(),
        backup_id: id.to_owned(),
        safeguard_id: safeguard.summary.backup_id.clone(),
        committed: false,
        workspaces,
    };
    let admission = Stamp::capture(
        &plan
            .scopes
            .iter()
            .map(|scope| scope.target.clone())
            .collect::<Vec<_>>(),
    )
    .and_then(|stamp| publication.publish(&stamp, || recovery::write_initial(plan, &journal)));
    if let Err(error) = admission {
        // No target has moved, but a later pointer write can fail after the journal
        // became durable. Complete compensation under the same ownership lease.
        let cleanup = if plan.root.join(recovery::JOURNAL_NAME).exists() {
            recovery::rollback_with_publication(plan, &journal, &backup, &safeguard, publication)
                .map(|warnings| warnings.join("; "))
        } else {
            Ok(cleanup_workspaces(&journal.workspaces).join("; "))
        };
        return Err(invalid(
            &plan.root,
            format!(
                "Restore admission failed before publication: {error}; protection snapshot: {}; cleanup: {}",
                safeguard.summary.backup_id,
                cleanup.unwrap_or_else(|error| error.to_string())
            ),
        ));
    }
    let mut durably_committed = false;
    let published = (|| {
        let roots = recovery::publication_roots(plan, &journal);
        let stamp = Stamp::capture(&roots)?;
        // Content verification precedes the writer reservation. The identity and
        // metadata stamp catches changes while the async ownership check runs.
        for (index, scope) in plan.scopes.iter().enumerate() {
            let before = &safeguard.scopes[index];
            if scope.target.exists() != before.existed
                || files::inventory(&scope.target)? != before.entries
            {
                return Err(invalid(
                    &scope.target,
                    "Cluster files changed after the protection snapshot",
                ));
            }
        }
        publication.publish(&stamp, || {
            for (index, scope) in plan.scopes.iter().enumerate() {
                let before = &safeguard.scopes[index];
                let workspace = &journal.workspaces[index].path;
                if before.existed {
                    move_directory(&scope.target, &workspace.join("old"))?;
                }
                hook(Phase::OriginalMoved, index)?;
                if backup.scopes[index].existed {
                    move_directory(&workspace.join("new"), &scope.target)?;
                }
                hook(Phase::Published, index)?;
            }
            journal.committed = true;
            transaction::write_json(&plan.root.join(recovery::JOURNAL_NAME), &journal)?;
            durably_committed = true;
            Ok(())
        })
    })();
    if let Err(error) = published {
        if durably_committed {
            return Err(invalid(
                &plan.root,
                format!(
                    "Restore committed but publication release failed: {error}; retain the journal and complete interrupted-restore recovery"
                ),
            ));
        }
        journal.committed = false;
        let rollback =
            recovery::rollback_with_publication(plan, &journal, &backup, &safeguard, publication);
        return match rollback {
            Ok(warnings) => Err(invalid(
                &plan.root,
                format!(
                    "Restore failed and every member was rolled back: {error}; protection snapshot: {}; {}",
                    safeguard.summary.backup_id,
                    warnings.join("; ")
                ),
            )),
            Err(rollback) => Err(invalid(
                &plan.root,
                format!(
                    "Restore failed: {error}; rollback is incomplete: {rollback}. Use interrupted-restore recovery. Protection snapshot: {}",
                    safeguard.summary.backup_id
                ),
            )),
        };
    }
    // The hook is only supplied by deterministic fault-injection tests.
    hook(Phase::Committed, plan.scopes.len())?;
    let cleanup_warnings = recovery::finish(plan, &journal);
    Ok(ArkClusterBackupRestoreResult {
        backup: backup.summary,
        safeguard_backup: safeguard.summary,
        restored_at_unix_ms: now(),
        cleanup_warnings,
    })
}

pub(super) fn validate_manifest(plan: &Plan, manifest: &Manifest) -> Result<(), StorageError> {
    if plan.report.identity.as_ref() != Some(&manifest.summary.identity)
        || manifest.scopes.len() != plan.scopes.len()
    {
        return Err(invalid(
            &plan.root,
            "Snapshot membership differs from the current cluster",
        ));
    }
    for (current, stored) in plan.scopes.iter().zip(&manifest.scopes) {
        if current.key != stored.key || path_key(&current.target) != path_key(&stored.target) {
            return Err(invalid(
                &stored.target,
                "Snapshot target does not match this registered instance",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_configurations(plan: &Plan, source: &Path) -> Result<(), StorageError> {
    let identity = plan
        .report
        .identity
        .as_ref()
        .ok_or_else(|| invalid(source, "Missing cluster identity"))?;
    for (index, member) in plan.report.members.iter().enumerate() {
        let path = source.join(format!("member-{index:03}-config/instance.json"));
        let config: serde_json::Value = transaction::read_json(&path, 2 * 1024 * 1024)?;
        let settings = config
            .get("settings")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| invalid(&path, "Snapshot has no managed instance settings"))?;
        let text = |key: &str| {
            settings
                .get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .trim()
        };
        let flags = text("custom_launch_flags").to_ascii_lowercase();
        let ports: Vec<app_core::PortBinding> = serde_json::from_value(
            config
                .get("ports")
                .cloned()
                .ok_or_else(|| invalid(&path, "Snapshot has no registered port mirror"))?,
        )?;
        let port_keys = |ports: &[app_core::PortBinding]| {
            let mut keys = ports
                .iter()
                .map(|port| {
                    (
                        port.name.trim().to_owned(),
                        port.protocol.trim().to_ascii_lowercase(),
                        port.port,
                    )
                })
                .collect::<Vec<_>>();
            keys.sort();
            keys
        };
        if config
            .get("instance_name")
            .and_then(serde_json::Value::as_str)
            != Some(member.summary.name.as_str())
            || config.get("autostart").and_then(serde_json::Value::as_bool)
                != Some(member.summary.autostart)
            || text("bind_ip") != member.summary.bind_ip
            || port_keys(&ports) != port_keys(&member.ports)
        {
            return Err(invalid(
                &path,
                "Snapshot registration differs from the current instance. Match its name, ports, bind address and autostart before restoring the complete configuration.",
            ));
        }
        if config
            .get("instance_id")
            .and_then(serde_json::Value::as_str)
            != Some(member.summary.id.as_str())
            || config.get("module_id").and_then(serde_json::Value::as_str)
                != Some(identity.module_id.as_str())
            || text("cluster_id") != identity.cluster_id
            || text("cluster_directory").is_empty()
            || path_key(Path::new(text("cluster_directory")))
                != path_key(Path::new(&identity.directory_key))
            || flags.contains("-clusterid")
            || flags.contains("-clusterdir")
        {
            return Err(invalid(
                &path,
                "Snapshot configuration belongs to another instance or cluster",
            ));
        }
    }
    Ok(())
}

pub(super) fn cleanup_workspaces(workspaces: &[Workspace]) -> Vec<String> {
    let mut warnings = Vec::new();
    for workspace in workspaces {
        if !workspace.path.exists() {
            continue;
        }
        let Some(parent) = workspace.path.parent() else {
            warnings.push(String::from("Restore workspace has no parent"));
            continue;
        };
        if let Err(error) = remove_managed_directory(&workspace.path, parent) {
            warnings.push(format!(
                "Restore workspace retained at {}: {error}",
                workspace.path.display()
            ));
        }
    }
    warnings
}
