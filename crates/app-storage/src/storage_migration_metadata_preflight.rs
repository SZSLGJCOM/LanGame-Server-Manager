use super::*;
use std::collections::BTreeSet;

const MAX_TREE_ENTRIES: usize = 1_000_000;

pub(super) fn validate(
    paths: &StoragePaths,
    moves: &[RootMove],
    database: &db::Database,
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    let mut next = paths.clone();
    next.instances_root = remap_path(&paths.instances_root, moves)?;
    next.archives_root = remap_path(&paths.archives_root, moves)?;
    next.games_root = remap_path(&paths.games_root, moves)?;
    next.steamcmd_root = remap_path(&paths.steamcmd_root, moves)?;
    crate::instance_archive_roots::validate_migration_archive_root(&next)?;
    let mut roots = BTreeSet::new();
    for root in [
        &paths.instances_root,
        &paths.archives_root,
        &paths.games_root,
        &paths.steamcmd_root,
    ] {
        roots.insert(normalize_resource_path(root)?);
    }
    let roots: Vec<_> = roots.into_iter().collect();
    for root in &roots {
        if roots
            .iter()
            .any(|parent| parent != root && contains(parent, root))
        {
            continue;
        }
        scan_recovery(root, moves, copied, plan)?;
    }
    for row in &database.rows {
        if row.table != "instances" {
            continue;
        }
        let config = Path::new(row_text(&row.before, "config_path", &paths.database_path)?);
        crate::instance_reconciliation::checked_instance_root(paths, config)?;
        for column in ["env_json", "args_json"] {
            let value: Value = serde_json::from_str(row_text(&row.before, column, config)?)?;
            reject_references(&value, config, column, moves)?;
        }
    }
    for archive in &database.archives {
        if matches!(archive.state.as_str(), "restored" | "purged") {
            continue;
        }
        if archive.state != "archived"
            || archive.purpose != "archive"
            || archive.restore_staging_identity.is_some()
        {
            return Err(invalid(
                &paths.archives_root.join(&archive.leaf),
                format!(
                    "Archive {} is {} ({}). Finish its recovery or resolve its missing metadata before moving storage.",
                    archive.id, archive.state, archive.purpose
                ),
            ));
        }
        crate::instance_archive::validate_archived_files(paths, archive)?;
    }
    let archive_entries = owned::inventory(paths)?;
    reject_archive_leaf_merges(paths, moves, &archive_entries)?;
    for entry in archive_entries {
        if !database.archives.iter().any(|archive| {
            archive.leaf == entry.leaf && !matches!(archive.state.as_str(), "restored" | "purged")
        }) {
            return Err(invalid(
                &paths.archives_root.join(entry.leaf),
                "An unregistered archive entry cannot have its ownership rebound by a storage migration. Resolve it in archive management first.",
            ));
        }
    }
    reject_cluster_snapshots(paths, moves)
}

fn reject_archive_leaf_merges(
    paths: &StoragePaths,
    moves: &[RootMove],
    entries: &[owned::InventoryEntry],
) -> Result<(), StorageError> {
    let historical = paths.instances_root.join(".trash");
    if entries.is_empty() || same_path(&historical, &paths.archives_root)? {
        return Ok(());
    }
    let historical_target = remap_path(&historical, moves)?;
    let archive_target = remap_path(&paths.archives_root, moves)?;
    // Future targets are already normalized by the migration planner. Reopening
    // their old contents would break whole-directory takeovers.
    if !contains(&historical_target, &archive_target)
        || !contains(&archive_target, &historical_target)
        || !owned::plain_directory(&historical)?
    {
        return Ok(());
    }
    // An external archive and a dormant workspace .trash can project two
    // unrelated archive owners onto one leaf without any colliding filenames.
    // Keep their identities separate even when the file copier could merge them.
    for entry in entries {
        let historical_entry = historical.join(&entry.leaf);
        if !exists(&historical_entry)? {
            continue;
        }
        let archive_entry = paths.archives_root.join(&entry.leaf);
        let historical_target = remap_path(&historical_entry, moves)?;
        let archive_target = remap_path(&archive_entry, moves)?;
        if contains(&historical_target, &archive_target)
            && contains(&archive_target, &historical_target)
        {
            return Err(invalid(
                &historical_entry,
                format!(
                    "The workspace's existing .trash and the configured archive directory contain the same archive name. Their contents cannot be combined under one archive identity. Keep the historical entry separate before retrying. Configured archive: {}",
                    archive_entry.display()
                ),
            ));
        }
    }
    Ok(())
}

fn scan_recovery(
    root: &Path,
    moves: &[RootMove],
    copied: bool,
    plan: &mut MetadataPlan,
) -> Result<(), StorageError> {
    if !exists(root)? {
        return Ok(());
    }
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut count = 0usize;
    let mut revisions = BTreeMap::new();
    while let Some((path, depth)) = pending.pop() {
        count += 1;
        if count > MAX_TREE_ENTRIES || depth > 128 {
            return Err(invalid(
                root,
                "Storage recovery preflight exceeds its entry or depth limit.",
            ));
        }
        let metadata = fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink()
            || crate::private_runtime::metadata_is_reparse_point(&metadata)
        {
            return Err(invalid(
                &path,
                "Storage migration cannot follow a symlink or junction.",
            ));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        // Steam preservation retires its active journal into the history
        // namespace after restoration (or before any native write). Those
        // retained review copies do not require recovery; their contents still
        // receive the ordinary recursive path checks below.
        let pending_steam_retention = name.starts_with(".langame-steam-retained-")
            && !name.starts_with(".langame-steam-retained-history-");
        let pending_recovery = matches!(
            name,
            ".langame-program-adoption.json"
                | ".langame-ark-restore.json"
                | ".langame-windrose-bootstrap.json"
                | ".langame-refresh-staging"
                | "runtime.refresh-staging"
                | "runtime.refresh-rollback"
        ) || name.starts_with(".langame-backup-restore-")
            || name.starts_with(".langame-archive-rebuild-")
            || pending_steam_retention
            || (name.starts_with('.')
                && (name.ends_with(".publish.state")
                    || (name.contains(".publish-") && name.ends_with(".state"))));
        if pending_recovery {
            return Err(invalid(
                &path,
                "An unfinished data or program recovery journal must be completed before moving storage.",
            ));
        }
        if name.ends_with(".pending")
            && path.ancestors().any(|ancestor| {
                ancestor
                    .file_name()
                    .is_some_and(|part| part == "install-revisions")
            })
        {
            let parent = path
                .parent()
                .ok_or_else(|| invalid(&path, "Revision marker has no directory."))?;
            let pending = if let Some(value) = revisions.get(parent) {
                *value
            } else {
                let value = latest_revision_is_pending(parent)?;
                revisions.insert(parent.to_owned(), value);
                value
            };
            if pending {
                return Err(invalid(
                    &path,
                    "A program installation revision is still pending. Complete or repair that installation before moving storage.",
                ));
            }
        }
        if metadata.is_dir() {
            if crate::mod_management_state::is_state_directory_name(name) {
                mods::prepare_owner(&path, moves, copied, plan)?;
            }
            for entry in fs::read_dir(&path).map_err(|source| StorageError::ReadPath {
                path: path.clone(),
                source,
            })? {
                let entry = entry.map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
                pending.push((entry.path(), depth + 1));
            }
        } else if !metadata.is_file() {
            return Err(invalid(
                &path,
                "Storage migration encountered a special filesystem entry.",
            ));
        }
    }
    Ok(())
}

fn latest_revision_is_pending(directory: &Path) -> Result<bool, StorageError> {
    let mut latest = (0u64, false);
    for (index, entry) in fs::read_dir(directory)
        .map_err(|source| StorageError::ReadPath {
            path: directory.to_owned(),
            source,
        })?
        .enumerate()
    {
        if index > 100_000 {
            return Err(invalid(
                directory,
                "Too many installation revision entries to inspect.",
            ));
        }
        let entry = entry.map_err(|source| StorageError::ReadPath {
            path: directory.to_owned(),
            source,
        })?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let (stem, pending) = if let Some(stem) = name.strip_suffix(".pending") {
            (stem, true)
        } else if let Some(stem) = name.strip_suffix(".ready") {
            (stem, false)
        } else {
            continue;
        };
        let Some(number) = stem.strip_prefix("rev-") else {
            continue;
        };
        if number.len() != 20 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        if let Ok(number) = number.parse::<u64>()
            && (number > latest.0 || (number == latest.0 && pending))
        {
            latest = (number, pending);
        }
    }
    Ok(latest.1)
}

/// Detection only: configuration strings are never recursively rewritten.
/// Free-form arguments, plugin settings, and game-specific paths need an
/// explicit contract before a migration may change their meaning.
pub(super) fn reject_references(
    value: &Value,
    file: &Path,
    field: &str,
    moves: &[RootMove],
) -> Result<(), StorageError> {
    match value {
        Value::String(value) => {
            let text = path_spelling(value);
            for movement in moves {
                if same_path(&movement.source, &movement.target)? {
                    continue;
                }
                let root = path_spelling(&movement.source.to_string_lossy());
                if text.match_indices(&root).any(|(start, _)| {
                    text.as_bytes().get(start + root.len()).is_none_or(|byte| {
                        matches!(byte, b'/' | b'"' | b'\'' | b' ' | b'\t' | b'\r' | b'\n')
                    })
                }) {
                    return Err(invalid(
                        file,
                        format!(
                            "Configuration field {field} explicitly references the storage root {}. Automatic conversion of this custom path is unsupported; update or remove that reference before moving storage.",
                            movement.source.display()
                        ),
                    ));
                }
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                reject_references(value, file, &format!("{field}[{index}]"), moves)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                reject_references(value, file, &format!("{field}.{key}"), moves)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn path_spelling(value: &str) -> String {
    let value = value.replace('\\', "/");
    let value = value
        .strip_prefix("//?/")
        .unwrap_or(&value)
        .trim_end_matches('/');
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_owned()
    }
}

fn reject_cluster_snapshots(paths: &StoragePaths, moves: &[RootMove]) -> Result<(), StorageError> {
    let root = paths.app_data_root.join("cluster-backups");
    if !exists(&root)? {
        return Ok(());
    }
    let mut pending = vec![(root.clone(), 0usize)];
    let mut count = 0;
    while let Some((directory, depth)) = pending.pop() {
        if !owned::plain_directory(&directory)? {
            continue;
        }
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadPath {
            path: directory.clone(),
            source,
        })? {
            let path = entry
                .map_err(|source| StorageError::ReadPath {
                    path: directory.clone(),
                    source,
                })?
                .path();
            count += 1;
            if count > 100_000 {
                return Err(invalid(
                    &root,
                    "Too many ARK cluster snapshot entries to validate migration.",
                ));
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if name == "restore-pending.json" {
                return Err(invalid(
                    &path,
                    "Complete the pending ARK cluster restore before moving storage.",
                ));
            }
            if name == "manifest.json" {
                let (_, value) = read_json(&path)?;
                let scopes = value
                    .get("scopes")
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(&path, "ARK cluster snapshot scopes are invalid."))?;
                for scope in scopes {
                    let target = Path::new(text(scope, "target", &path)?);
                    if moved(target, moves)? {
                        return Err(invalid(
                            &path,
                            format!(
                                "This ARK cluster snapshot targets {}. Storage migration cannot yet rewrite cluster snapshot membership and checksums; preserve this location or resolve the snapshot first.",
                                target.display()
                            ),
                        ));
                    }
                }
            } else if depth < 2
                && fs::symlink_metadata(&path)
                    .map_err(|source| StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    })?
                    .is_dir()
            {
                pending.push((path, depth + 1));
            }
        }
    }
    Ok(())
}
