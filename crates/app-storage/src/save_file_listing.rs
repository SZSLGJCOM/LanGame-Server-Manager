//! Read-only listing of an instance's live saves or one of its backups.
//!
//! Both roots are resolved from stored instance state, never from caller paths.
//! The walk never follows links, junctions or other reparse points and stops at
//! a fixed entry budget. Directory enumeration retains only the first names
//! needed for that budget instead of allocating the complete directory.

use super::*;
use serde::Serialize;
use std::collections::BinaryHeap;

#[path = "save_slots.rs"]
pub(crate) mod slots;

/// Upper bound on returned files, directories and skipped links together.
pub const MAX_SAVE_FILE_LISTING_ENTRIES: usize = 2_000;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SaveFileEntry {
    /// Path relative to the listed root, always `/`-separated.
    pub relative_path: String,
    pub is_directory: bool,
    pub size_bytes: u64,
    pub modified_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SaveFileListing {
    pub instance_id: String,
    pub backup_id: Option<String>,
    pub root_path: String,
    pub exists: bool,
    pub entries: Vec<SaveFileEntry>,
    pub truncated: bool,
    /// Links and reparse points found under the root; listed by name only, never entered.
    pub skipped_links: Vec<String>,
    /// `native` when the game defines save slots, otherwise `fallback`
    /// (one row per top-level directory plus loose top-level files).
    pub slot_model: &'static str,
    /// Every entry belongs to exactly one slot. Totals cover listed entries only,
    /// so they are partial when `truncated` is set.
    pub slots: Vec<slots::SaveSlot>,
}

/// Lists the instance's effective saves directory, or the backup `backup_id`
/// when given. The backup must belong to `instance_id` by its manifest.
pub async fn list_instance_save_files(
    paths: &StoragePaths,
    instance_id: &str,
    backup_id: Option<&str>,
) -> Result<SaveFileListing, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    pool.close().await;
    let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
    let saves_dir = match backup_id {
        Some(_) => None,
        None => {
            let install_root = effective_instance_install_root(&record)?;
            Some(effective_instance_saves_dir(
                descriptor.as_ref(),
                &install_root,
                &record,
            )?)
        }
    };
    // Only the live saves can be "current"; a backup keeps its own layout.
    let settings = match backup_id {
        Some(_) => serde_json::Map::new(),
        None => save_import::effective_instance_settings(descriptor.as_ref(), &record)?.0,
    };
    let module_id = record.summary.module_id.clone();

    let instance_id = String::from(instance_id);
    let backup_id = backup_id.map(String::from);
    run_instance_transaction(instance_root(&record), move || {
        let root = match (&backup_id, saves_dir) {
            (Some(backup_id), _) => {
                let path = validated_backup_path(&record, backup_id)?;
                load_instance_backup_for_instance(&path, &instance_id, backup_id)?;
                path
            }
            (None, Some(saves_dir)) => {
                recovery::ensure_record_ready(&record, &saves_dir)?;
                saves_dir
            }
            (None, None) => unreachable!("saves directory is resolved when no backup is requested"),
        };
        let mut listing = list_directory_tree(&root, MAX_SAVE_FILE_LISTING_ENTRIES)?;
        let (native, slots) = slots::group_save_slots(&module_id, &settings, &listing.entries);
        listing.slot_model = if native { "native" } else { "fallback" };
        listing.slots = slots;
        listing.instance_id = instance_id;
        listing.backup_id = backup_id;
        Ok(listing)
    })
    .await
}

fn list_directory_tree(root: &Path, limit: usize) -> Result<SaveFileListing, StorageError> {
    let mut listing = SaveFileListing {
        instance_id: String::new(),
        backup_id: None,
        root_path: root.to_string_lossy().into_owned(),
        exists: false,
        entries: Vec::new(),
        truncated: false,
        skipped_links: Vec::new(),
        slot_model: "fallback",
        slots: Vec::new(),
    };
    match fs::symlink_metadata(root) {
        Ok(metadata) => require_plain_directory(root, root, &metadata)?,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: root.to_path_buf(),
                source,
            });
        }
    }
    let canonical_root = canonical_plain_directory(root, root)?;
    listing.exists = true;

    // Depth-first, name-ordered, so truncation keeps a stable prefix.
    let mut pending = vec![(canonical_root.clone(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let mut nested = Vec::new();
        let remaining = limit.saturating_sub(listing.entries.len() + listing.skipped_links.len());
        // One extra name distinguishes an exact fit from a truncated listing.
        for (name, path) in read_sorted_children(&directory, remaining.saturating_add(1))? {
            if listing.entries.len() + listing.skipped_links.len() >= limit {
                listing.truncated = true;
                return Ok(listing);
            }
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
            if is_link_or_reparse(&metadata) {
                listing.skipped_links.push(relative);
                continue;
            }
            let is_directory = metadata.is_dir();
            if is_directory {
                // A plain directory under a canonical root cannot leave it, but keep the
                // containment check explicit in case of a racing replacement.
                let canonical = canonical_plain_directory(&path, &canonical_root)?;
                if !canonical.starts_with(&canonical_root) {
                    return Err(StorageError::UnsafeManagedPath {
                        path: canonical,
                        root: canonical_root,
                    });
                }
                nested.push((canonical, relative.clone()));
            }
            listing.entries.push(SaveFileEntry {
                relative_path: relative,
                is_directory,
                size_bytes: if is_directory { 0 } else { metadata.len() },
                modified_at_unix_ms: metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .and_then(|duration| u64::try_from(duration.as_millis()).ok()),
            });
        }
        // Reverse so the stack pops directories in name order.
        pending.extend(nested.into_iter().rev());
    }
    Ok(listing)
}

fn read_sorted_children(
    directory: &Path,
    limit: usize,
) -> Result<Vec<(String, PathBuf)>, StorageError> {
    let entries = fs::read_dir(directory).map_err(|source| StorageError::ReadPath {
        path: directory.to_path_buf(),
        source,
    })?;
    // Reading names is necessary to preserve the alphabetic prefix on filesystems
    // with unordered iteration. Keep only the smallest names, without collecting
    // metadata or allocating storage proportional to the full directory size.
    let mut children = BinaryHeap::new();
    for entry in entries {
        let entry = entry.map_err(|source| StorageError::ReadPath {
            path: directory.to_path_buf(),
            source,
        })?;
        let child = (
            entry.file_name().to_string_lossy().into_owned(),
            entry.path(),
        );
        if children.len() < limit {
            children.push(child);
        } else if let Some(mut largest) = children.peek_mut()
            && child < *largest
        {
            *largest = child;
        }
    }
    Ok(children.into_sorted_vec())
}

#[cfg(test)]
#[path = "save_file_listing_tests.rs"]
mod tests;
