use super::*;
use std::time::Instant;

const MAX_INVENTORY_ENTRIES: usize = 50_000;
const MAX_INVENTORY_DEPTH: usize = 32;
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(10);

struct InventoryBudget {
    remaining: usize,
    max_depth: usize,
    deadline: Instant,
}

impl Default for InventoryBudget {
    fn default() -> Self {
        Self {
            remaining: MAX_INVENTORY_ENTRIES,
            max_depth: MAX_INVENTORY_DEPTH,
            deadline: Instant::now() + INVENTORY_TIMEOUT,
        }
    }
}

impl InventoryBudget {
    fn check(&self, path: &Path, depth: usize) -> Result<(), String> {
        if depth > self.max_depth {
            return Err(format!(
                "Mod inventory exceeds its directory depth limit at {}",
                path.display()
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(format!(
                "Mod inventory inspection timed out at {}; narrow the Mod directory before retrying",
                path.display()
            ));
        }
        Ok(())
    }

    fn entry(&mut self, path: &Path, depth: usize) -> Result<(), String> {
        self.check(path, depth)?;
        self.remaining = self.remaining.checked_sub(1).ok_or_else(|| {
            format!(
                "Mod inventory exceeds its entry limit at {}",
                path.display()
            )
        })?;
        Ok(())
    }
}

fn plain_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect Mod path {}: {error}", path.display()))?;
    reject_reparse(path, &metadata)?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(format!(
            "Mod path {} is not a regular file or directory",
            path.display()
        ));
    }
    Ok(metadata)
}

fn reject_reparse(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    let mut reparse = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        reparse |= metadata.file_attributes() & 0x400 != 0;
    }
    if reparse {
        return Err(format!(
            "Mod path {} is a symbolic link or reparse point",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn validate_root_chain(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => reject_reparse(ancestor, &metadata)?,
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect Mod path {}: {error}",
                    ancestor.display()
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn read_inventory(
    target: ResolvedManualModTarget,
) -> Result<ManualModInventoryResult, String> {
    validate_root_chain(&target.target_path)?;
    let target_exists = match fs::symlink_metadata(&target.target_path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(format!(
                    "Mod inventory target {} is not a directory",
                    target.target_path.display()
                ));
            }
            true
        }
        Err(error) if error.kind() == ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "failed to inspect Mod inventory {}: {error}",
                target.target_path.display()
            ));
        }
    };
    let items = if target_exists {
        read_manual_mod_inventory_items(target.id_strategy.as_deref(), &target.target_path)?
    } else {
        Vec::new()
    };
    Ok(ManualModInventoryResult {
        instance_id: target.instance_id,
        module_id: target.module_id,
        source_label: target.source_label,
        target_label: target.target_label,
        target_path: target.target_path.to_string_lossy().into_owned(),
        target_exists,
        items,
    })
}

pub(in crate::commands) fn read_manual_mod_inventory_items(
    id_strategy: Option<&str>,
    target_path: &Path,
) -> Result<Vec<ManualModInventoryItem>, String> {
    read_items_with_budget(id_strategy, target_path, &mut InventoryBudget::default())
}

fn read_items_with_budget(
    id_strategy: Option<&str>,
    target_path: &Path,
    budget: &mut InventoryBudget,
) -> Result<Vec<ManualModInventoryItem>, String> {
    validate_root_chain(target_path)?;
    let entries = fs::read_dir(target_path).map_err(|error| {
        format!(
            "failed to read Mod target {}: {error}",
            target_path.display()
        )
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        budget.entry(target_path, 0)?;
        let entry = entry.map_err(|error| {
            format!(
                "failed to read Mod target {}: {error}",
                target_path.display()
            )
        })?;
        if entry.file_name() != std::ffi::OsStr::new(ONLINE_MOD_MANIFEST) {
            paths.push(entry.path());
        }
    }
    paths.sort_by_key(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = plain_metadata(&path)?;
        let stats = path_stats(&path, budget)?;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        items.push(ManualModInventoryItem {
            inferred_id: infer_manual_mod_inventory_id_from_path(id_strategy, &name, &path),
            name,
            path: path.to_string_lossy().into_owned(),
            item_type: if metadata.is_dir() {
                "directory"
            } else {
                "file"
            }
            .into(),
            file_count: stats.file_count,
            total_bytes: stats.total_bytes,
            modified_unix_ms: metadata
                .modified()
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis()),
        });
    }
    Ok(items)
}

#[cfg(test)]
pub(in crate::commands) fn manual_mod_path_stats(
    path: &Path,
) -> Result<DirectoryCopyStats, String> {
    validate_root_chain(path)?;
    path_stats(path, &mut InventoryBudget::default())
}

fn path_stats(path: &Path, budget: &mut InventoryBudget) -> Result<DirectoryCopyStats, String> {
    let mut stats = DirectoryCopyStats::default();
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    while let Some((current, depth)) = pending.pop() {
        budget.check(&current, depth)?;
        let metadata = plain_metadata(&current)?;
        if metadata.is_file() {
            stats.file_count = stats
                .file_count
                .checked_add(1)
                .ok_or("Mod inventory file count overflowed")?;
            stats.total_bytes = stats
                .total_bytes
                .checked_add(metadata.len())
                .ok_or("Mod inventory byte count overflowed")?;
            continue;
        }
        for entry in fs::read_dir(&current)
            .map_err(|error| format!("failed to read Mod path {}: {error}", current.display()))?
        {
            budget.entry(&current, depth + 1)?;
            let entry = entry.map_err(|error| {
                format!("failed to read Mod path {}: {error}", current.display())
            })?;
            pending.push((entry.path(), depth + 1));
        }
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_mod_inventory_propagates_entry_depth_and_timeout_limits() {
        let root =
            std::env::temp_dir().join(format!("langame-mod-scan-budget-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("package/nested")).unwrap();
        fs::write(root.join("package/nested/mod.dll"), b"payload").unwrap();
        for mut budget in [
            InventoryBudget {
                remaining: 1,
                ..Default::default()
            },
            InventoryBudget {
                max_depth: 0,
                ..Default::default()
            },
            InventoryBudget {
                deadline: Instant::now(),
                ..Default::default()
            },
        ] {
            assert!(read_items_with_budget(None, &root, &mut budget).is_err());
        }
        let inventory = read_manual_mod_inventory_items(None, &root).unwrap();
        assert_eq!(inventory.len(), 1);
        assert_eq!(inventory[0].file_count, 1);
        assert_eq!(inventory[0].total_bytes, 7);
        fs::remove_dir_all(root).unwrap();
    }
}
