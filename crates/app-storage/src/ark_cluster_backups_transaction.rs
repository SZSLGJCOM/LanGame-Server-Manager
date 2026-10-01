//! Cluster snapshot publication and manifest validation.
use std::fs;
use std::io::Read;
use std::path::Path;

use serde::Serialize;
use uuid::Uuid;

use super::{
    ArkClusterBackupMember, ArkClusterBackupSummary, ArkClusterIdentity, Manifest, Plan, Scope,
    files, invalid, now,
};
use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::backups::publication::{Publication, Stamp};
use crate::backups::{
    canonical_plain_directory, create_unique_directory, is_link_or_reparse, move_directory,
    remove_managed_directory,
};

#[cfg(test)]
pub(super) fn snapshot(plan: &Plan, kind: &str) -> Result<Manifest, StorageError> {
    snapshot_with_publication(plan, kind, &Publication::uncoordinated())
}

pub(super) fn snapshot_with_publication(
    plan: &Plan,
    kind: &str,
    publication: &Publication,
) -> Result<Manifest, StorageError> {
    #[cfg(test)]
    crate::instance_archive::test_gate::pause(
        &plan.root,
        crate::instance_archive::test_gate::Point::BackupPreparing,
    );
    ensure_ready(&plan.root)?;
    let mut snapshot_count = 0;
    for entry in fs::read_dir(&plan.root).map_err(|source| StorageError::ReadDirectory {
        path: plan.root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: plan.root.clone(),
            source,
        })?;
        if entry.file_name().to_string_lossy().starts_with("snapshot-") {
            snapshot_count += 1;
        }
        if snapshot_count >= 1000 {
            return Err(invalid(
                &plan.root,
                "Cluster snapshot limit reached (1000); archive older snapshots before creating another",
            ));
        }
    }
    let mut scopes = plan.scopes.clone();
    for scope in &mut scopes {
        scope.entries = files::inventory(&scope.target)?;
        scope.existed = scope.target.exists();
    }
    let (file_count, total_bytes) = totals(&scopes)?;
    files::require_capacity(&plan.root, total_bytes)?;
    let backup_id = format!("snapshot-{}", Uuid::new_v4());
    let destination = plan.root.join(&backup_id);
    let staging = create_unique_directory(&plan.root, ".snapshot")?;
    let identity = plan
        .report
        .identity
        .clone()
        .ok_or_else(|| invalid(&plan.root, "Missing cluster identity"))?;
    let manifest = Manifest {
        version: 1,
        scopes,
        summary: ArkClusterBackupSummary {
            backup_id,
            created_at_unix_ms: now(),
            backup_kind: kind.to_owned(),
            identity,
            backup_path: destination.to_string_lossy().into_owned(),
            file_count,
            total_bytes,
            members: plan
                .report
                .members
                .iter()
                .map(|member| ArkClusterBackupMember {
                    instance_id: member.summary.id.clone(),
                    instance_name: member.summary.name.clone(),
                    map_name: member.map_name.clone(),
                })
                .collect(),
        },
    };
    let result = (|| {
        for scope in &manifest.scopes {
            files::copy_verified(&scope.target, &staging.join(&scope.key), &scope.entries)?;
        }
        let roots = manifest
            .scopes
            .iter()
            .map(|scope| scope.target.clone())
            .collect::<Vec<_>>();
        let stamp = Stamp::capture(&roots)?;
        for scope in &manifest.scopes {
            if files::inventory(&scope.target)? != scope.entries
                || scope.target.exists() != scope.existed
            {
                return Err(invalid(
                    &scope.target,
                    "Cluster source changed while snapshotting; no snapshot was published",
                ));
            }
        }
        write_json(&staging.join("manifest.json"), &manifest)?;
        publication.publish(&stamp, || move_directory(&staging, &destination))?;
        Ok(manifest)
    })();
    match result {
        Ok(manifest) => Ok(manifest),
        Err(error) => {
            if let Err(cleanup) = remove_managed_directory(&staging, &plan.root) {
                return Err(invalid(
                    &staging,
                    format!("Snapshot failed: {error}; staging cleanup failed: {cleanup}"),
                ));
            }
            Err(error)
        }
    }
}

pub(super) fn list(
    root: &Path,
    identity: &ArkClusterIdentity,
) -> Result<Vec<ArkClusterBackupSummary>, StorageError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    canonical_plain_directory(root, root)?;
    let mut backups = Vec::new();
    for entry in fs::read_dir(root).map_err(|source| StorageError::ReadDirectory {
        path: root.to_owned(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: root.to_owned(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("snapshot-") {
            continue;
        }
        if backups.len() >= 1000 {
            return Err(invalid(
                root,
                "Cluster backup listing exceeds 1000 snapshots",
            ));
        }
        let manifest = load(root, &name)?;
        if manifest.summary.identity.module_id != identity.module_id
            || manifest.summary.identity.cluster_id != identity.cluster_id
            || manifest.summary.identity.directory_key != identity.directory_key
        {
            return Err(invalid(
                &entry.path(),
                "Snapshot does not belong to this cluster directory",
            ));
        }
        backups.push(manifest.summary);
    }
    backups.sort_by_key(|backup| std::cmp::Reverse(backup.created_at_unix_ms));
    Ok(backups)
}

pub(super) fn load(root: &Path, id: &str) -> Result<Manifest, StorageError> {
    let suffix = id
        .strip_prefix("snapshot-")
        .ok_or_else(|| invalid(root, "Invalid cluster snapshot ID"))?;
    let parsed =
        Uuid::parse_str(suffix).map_err(|_| invalid(root, "Invalid cluster snapshot ID"))?;
    if suffix != parsed.to_string() {
        return Err(invalid(root, "Snapshot ID must use its canonical form"));
    }
    let directory = root.join(id);
    canonical_plain_directory(&directory, root)?;
    let mut manifest: Manifest = read_json(&directory.join("manifest.json"), 128 * 1024 * 1024)?;
    if manifest.version != 1
        || manifest.summary.backup_id != id
        || !matches!(
            manifest.summary.backup_kind.as_str(),
            "manual" | "pre_restore"
        )
    {
        return Err(invalid(&directory, "Invalid cluster snapshot manifest"));
    }
    if totals(&manifest.scopes)? != (manifest.summary.file_count, manifest.summary.total_bytes) {
        return Err(invalid(
            &directory,
            "Snapshot totals do not match its manifest",
        ));
    }
    let members = &manifest.summary.identity.member_ids;
    if members.is_empty()
        || members.len() > 128
        || members.windows(2).any(|pair| pair[0] >= pair[1])
        || manifest
            .summary
            .members
            .iter()
            .map(|member| &member.instance_id)
            .collect::<Vec<_>>()
            != members.iter().collect::<Vec<_>>()
        || manifest.scopes.len() != members.len() * 2 + 1
    {
        return Err(invalid(&directory, "Snapshot membership is malformed"));
    }
    for (index, scope) in manifest.scopes.iter().enumerate() {
        let expected = if index == members.len() * 2 {
            String::from("transfer")
        } else {
            format!(
                "member-{:03}-{}",
                index / 2,
                if index % 2 == 0 { "config" } else { "saved" }
            )
        };
        if scope.key != expected {
            return Err(invalid(&directory, "Snapshot scope ordering is malformed"));
        }
    }
    manifest.summary.backup_path = directory.to_string_lossy().into_owned();
    Ok(manifest)
}

fn totals(scopes: &[Scope]) -> Result<(u64, u64), StorageError> {
    let entries = scopes
        .iter()
        .map(|scope| scope.entries.len())
        .sum::<usize>();
    if scopes.len() > 257 || entries > files::MAX_ENTRIES {
        return Err(invalid(
            Path::new(""),
            "Cluster snapshot exceeds scope or entry limits",
        ));
    }
    let mut files = 0;
    let mut bytes = 0_u64;
    for entry in scopes.iter().flat_map(|scope| &scope.entries) {
        if let Some(size) = entry.bytes {
            files += 1;
            bytes = bytes
                .checked_add(size)
                .filter(|value| *value <= 1024_u64.pow(4))
                .ok_or_else(|| {
                    invalid(Path::new(""), "Cluster snapshot exceeds the 1 TiB limit")
                })?;
        }
    }
    if scopes
        .iter()
        .any(|scope| !scope.existed && !scope.entries.is_empty())
    {
        return Err(invalid(
            Path::new(""),
            "An absent snapshot scope cannot contain files",
        ));
    }
    Ok((files, bytes))
}

pub(super) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), StorageError> {
    write_file_atomically(path, &serde_json::to_vec_pretty(value)?).map_err(|source| {
        StorageError::WriteConfig {
            path: path.to_owned(),
            source,
        }
    })
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    maximum: u64,
) -> Result<T, StorageError> {
    if let Some(parent) = path.parent() {
        files::plain_ancestors(parent)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) || metadata.len() > maximum {
        return Err(invalid(
            path,
            "Snapshot metadata is not a bounded plain file",
        ));
    }
    let file = fs::File::open(path).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(maximum.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > maximum {
        return Err(invalid(
            path,
            "Snapshot metadata grew beyond its size limit",
        ));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn ensure_ready(root: &Path) -> Result<(), StorageError> {
    let path = root.join("restore-pending.json");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::ReadPath { path, source }),
        Ok(_) => Err(invalid(
            &path,
            "An interrupted cluster restore must be explicitly recovered before starting or changing this cluster",
        )),
    }
}
