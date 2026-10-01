use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use super::dst_save_validation::{DstShardSaveInspection, inspect_dst_shard_save};

const MAX_IMPORT_SAVE_SCAN_ENTRIES: usize = 8192;
const MAX_CLUSTER_ROOT_ENTRIES: usize = 1024;
const MAX_IMPORT_TREE_DEPTH: usize = 64;
const MAX_IMPORT_TREE_ENTRIES: usize = 100_000;
const MAX_IMPORT_TREE_BYTES: u64 = 128 * 1024 * 1024 * 1024;
pub(super) const DST_SHARD_CONFIGURATION_FILES: &[&str] = &[
    "server.ini",
    "worldgenoverride.lua",
    "leveldataoverride.lua",
    "modoverrides.lua",
];

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ValidatedDstImportSource {
    pub(super) cluster_root: PathBuf,
    pub(super) master_root: PathBuf,
    pub(super) caves_root: Option<PathBuf>,
    pub(super) islands_root: Option<PathBuf>,
    pub(super) volcano_root: Option<PathBuf>,
    pub(super) master_saved: bool,
    allow_empty: bool,
    snapshot: Vec<(PathBuf, u64, SystemTime)>,
}

impl ValidatedDstImportSource {
    pub(super) fn shards(&self) -> Vec<(&'static str, &Path)> {
        let mut shards = Vec::new();
        if self.master_saved {
            shards.push(("master", self.master_root.as_path()));
        }
        for (key, root) in [
            ("caves", &self.caves_root),
            ("islands", &self.islands_root),
            ("volcano", &self.volcano_root),
        ] {
            if let Some(root) = root {
                shards.push((key, root.as_path()));
            }
        }
        shards
    }

    pub(super) fn is_island_adventures(&self) -> bool {
        self.islands_root.is_some() && self.volcano_root.is_some()
    }

    pub(super) fn verify_unchanged(&self) -> Result<(), String> {
        if validate_dontstarve_source(&self.cluster_root, self.allow_empty)? != *self {
            return Err(String::from(
                "DST source world changed during import. Stop the source server and select the source again before importing.",
            ));
        }
        Ok(())
    }
}

pub(super) fn dontstarve_cluster_root_from_config_file_path(config_file_path: &str) -> PathBuf {
    let config_path = PathBuf::from(config_file_path);
    let config_dir = config_path.parent().unwrap_or(config_path.as_path());
    config_dir.join("clusters").join("main")
}

#[cfg(test)]
pub(super) fn resolve_dontstarve_cluster_source_root(source_path: &Path) -> Option<PathBuf> {
    validate_dontstarve_import_source(source_path)
        .ok()
        .map(|source| source.cluster_root)
}

pub(super) fn validate_dontstarve_import_source(
    source_path: &Path,
) -> Result<ValidatedDstImportSource, String> {
    validate_dontstarve_source(source_path, false)
}

pub(super) fn validate_dontstarve_restore_source(
    source_path: &Path,
) -> Result<ValidatedDstImportSource, String> {
    validate_dontstarve_source(source_path, true)
}

fn validate_dontstarve_source(
    source_path: &Path,
    allow_empty: bool,
) -> Result<ValidatedDstImportSource, String> {
    let cluster_root = resolve_cluster_root_candidate(source_path).ok_or_else(|| {
        String::from(
            "Choose a DST cluster folder containing a Master shard with a non-empty save/shardindex and session data.",
        )
    })?;
    require_plain_directory(&cluster_root, "DST cluster source")?;
    reject_additional_shard_directories(&cluster_root)?;

    let master_root = find_case_insensitive_child_dir(&cluster_root, "Master")
        .ok_or_else(|| String::from("The selected DST cluster does not contain a Master shard."))?;
    let master_saved = if allow_empty {
        read_optional_saved_shard(&cluster_root, "Master")?.is_some()
    } else {
        validate_dontstarve_shard_save(&master_root, "Master")?;
        true
    };

    let mut optional = Vec::new();
    for name in ["Caves", "Islands", "Volcano"] {
        optional.push(read_optional_saved_shard(&cluster_root, name)?);
    }
    let volcano_root = optional.pop().unwrap();
    let islands_root = optional.pop().unwrap();
    let caves_root = optional.pop().unwrap();
    if !master_saved && (caves_root.is_some() || islands_root.is_some() || volcano_root.is_some()) {
        return Err("A DST backup with generated worlds must contain a valid Master save; missing worlds will not be regenerated.".into());
    }
    if (islands_root.is_some() || volcano_root.is_some())
        && (!master_saved
            || caves_root.is_none()
            || islands_root.is_none()
            || volcano_root.is_none())
    {
        return Err(String::from(
            "Island Adventures imports require all four valid shard saves: Master, Caves, Islands and Volcano. No missing shard will be regenerated.",
        ));
    }

    let snapshot = inspect_plain_tree(&cluster_root, &cluster_root)?;
    Ok(ValidatedDstImportSource {
        cluster_root,
        master_root,
        caves_root,
        islands_root,
        volcano_root,
        master_saved,
        allow_empty,
        snapshot,
    })
}

fn read_optional_saved_shard(root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let Some(path) = find_case_insensitive_child_path(root, name)? else {
        return Ok(None);
    };
    // Managed templates prepare disabled shard configuration too. Such a
    // directory is not a saved world; any other entry, including an incomplete
    // save directory, must still satisfy the native save validator.
    let entries =
        fs::read_dir(&path).map_err(|error| format!("Cannot inspect DST {name} shard: {error}"))?;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_CLUSTER_ROOT_ENTRIES {
            return Err(format!(
                "DST {name} shard exceeds the root entry validation limit."
            ));
        }
        let entry = entry.map_err(|error| format!("Cannot inspect DST {name} shard: {error}"))?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.is_file()
            && !is_link_or_reparse(&metadata)
            && DST_SHARD_CONFIGURATION_FILES.iter().any(|config| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(config)
            })
        {
            continue;
        }
        validate_dontstarve_shard_save(&path, name)?;
        return Ok(Some(path));
    }
    Ok(None)
}

pub(super) fn validate_dontstarve_import_paths(
    source_cluster_root: &Path,
    target_cluster_root: &Path,
) -> Result<(), String> {
    let canonical_source = canonicalize_plain_directory(source_cluster_root)?;
    let canonical_target = canonicalize_existing_prefix(target_cluster_root)?;
    if paths_overlap(&canonical_source, &canonical_target) {
        return Err(format!(
            "Source and target cluster folders must not be the same folder or contain one another (source: {}, target: {}).",
            source_cluster_root.display(),
            target_cluster_root.display()
        ));
    }

    if target_cluster_root.exists() {
        require_plain_directory(target_cluster_root, "DST target cluster")?;
        reject_additional_shard_directories(target_cluster_root)?;
        validate_plain_tree(target_cluster_root, target_cluster_root)?;
    }
    Ok(())
}

pub(super) fn validate_dontstarve_import_policy(
    source: &ValidatedDstImportSource,
    caves_enabled: bool,
) -> Result<(), String> {
    if caves_enabled && source.caves_root.is_none() {
        return Err(String::from(
            "This instance has caves enabled, but the selected save has no valid Caves shard. Import both shards or disable caves first.",
        ));
    }
    Ok(())
}

fn resolve_cluster_root_candidate(source_path: &Path) -> Option<PathBuf> {
    if source_path.is_dir() && find_case_insensitive_child_dir(source_path, "Master").is_some() {
        return Some(source_path.to_path_buf());
    }

    let nested_cluster_root = source_path.join("clusters").join("main");
    if nested_cluster_root.is_dir()
        && find_case_insensitive_child_dir(&nested_cluster_root, "Master").is_some()
    {
        return Some(nested_cluster_root);
    }

    let shard_name = source_path.file_name().and_then(|name| name.to_str())?;
    if !app_core::dst_shards::DST_SHARDS
        .iter()
        .any(|spec| shard_name.eq_ignore_ascii_case(spec.directory))
    {
        return None;
    }

    let parent = source_path.parent()?;
    if parent.is_dir() && find_case_insensitive_child_dir(parent, "Master").is_some() {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

pub(super) fn validate_dontstarve_shard_save(
    shard_root: &Path,
    shard_name: &str,
) -> Result<(), String> {
    require_plain_directory(shard_root, &format!("DST {shard_name} shard"))?;
    let save_root = shard_root.join("save");
    require_plain_directory(&save_root, &format!("DST {shard_name} save"))?;

    let mut budget = MAX_IMPORT_SAVE_SCAN_ENTRIES;
    if let DstShardSaveInspection::Invalid(reason) =
        inspect_dst_shard_save(&save_root, &mut budget)?
    {
        return Err(format!(
            "DST {shard_name} shard is not a valid save: {reason}."
        ));
    }
    Ok(())
}

fn reject_additional_shard_directories(cluster_root: &Path) -> Result<(), String> {
    let entries = fs::read_dir(cluster_root).map_err(|error| {
        format!(
            "Failed to read DST cluster directory {}: {error}",
            cluster_root.display()
        )
    })?;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_CLUSTER_ROOT_ENTRIES {
            return Err(format!(
                "DST cluster {} exceeds the root entry validation limit.",
                cluster_root.display()
            ));
        }
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect DST cluster directory {}: {error}",
                cluster_root.display()
            )
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.is_dir() && !is_link_or_reparse(&metadata) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !app_core::dst_shards::DST_SHARDS
                .iter()
                .any(|spec| name.eq_ignore_ascii_case(spec.directory))
            {
                return Err(format!(
                    "Unsupported DST shard directory {name}; supported shards are Master and Caves, and the Island Adventures Islands and Volcano shards."
                ));
            }
        }
    }
    Ok(())
}

fn find_case_insensitive_child_path(
    root: &Path,
    target_name: &str,
) -> Result<Option<PathBuf>, String> {
    let entries = fs::read_dir(root)
        .map_err(|error| format!("Failed to read directory {}: {}", root.display(), error))?;
    let mut found = None;
    for entry_result in entries {
        let entry = entry_result.map_err(|error| {
            format!("Failed to inspect directory {}: {}", root.display(), error)
        })?;
        if !entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(target_name)
        {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {}", path.display(), error))?;
        if is_link_or_reparse(&metadata) || !metadata.is_dir() {
            return Err(format!(
                "DST shard path {} must be a plain directory.",
                path.display()
            ));
        }
        if found.is_some() {
            return Err(format!(
                "DST cluster has duplicate {target_name} shard directories."
            ));
        }
        found = Some(path);
    }
    Ok(found)
}

pub(super) fn find_case_insensitive_child_dir(root: &Path, target_name: &str) -> Option<PathBuf> {
    find_case_insensitive_child_path(root, target_name)
        .ok()
        .flatten()
}

fn canonicalize_plain_directory(path: &Path) -> Result<PathBuf, String> {
    require_plain_directory(path, "directory")?;
    fs::canonicalize(path)
        .map_err(|error| format!("Failed to resolve {}: {}", path.display(), error))
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf, String> {
    let mut missing_components = Vec::new();
    let mut cursor = path;
    while !cursor.exists() {
        let name = cursor.file_name().ok_or_else(|| {
            format!(
                "Could not resolve an existing parent for {}.",
                path.display()
            )
        })?;
        missing_components.push(name.to_os_string());
        cursor = cursor.parent().ok_or_else(|| {
            format!(
                "Could not resolve an existing parent for {}.",
                path.display()
            )
        })?;
    }
    require_plain_directory(cursor, "target ancestor")?;
    let mut canonical = fs::canonicalize(cursor)
        .map_err(|error| format!("Failed to resolve {}: {}", cursor.display(), error))?;
    for component in missing_components.iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        let normalize = |path: &Path| {
            path.components()
                .filter_map(|component| match component {
                    Component::Prefix(prefix) => {
                        Some(prefix.as_os_str().to_string_lossy().to_lowercase())
                    }
                    Component::RootDir => Some(String::from("\\")),
                    Component::Normal(value) => Some(value.to_string_lossy().to_lowercase()),
                    Component::CurDir => None,
                    Component::ParentDir => Some(String::from("..")),
                })
                .collect::<Vec<_>>()
        };
        let left = normalize(left);
        let right = normalize(right);
        left.starts_with(&right) || right.starts_with(&left)
    }

    #[cfg(not(windows))]
    {
        left.starts_with(right) || right.starts_with(left)
    }
}

pub(super) fn require_plain_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Failed to inspect {label} {}: {}", path.display(), error))?;
    if is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(format!(
            "{label} {} must be a plain directory.",
            path.display()
        ));
    }
    Ok(())
}

pub(super) fn validate_plain_tree(path: &Path, root: &Path) -> Result<(), String> {
    inspect_plain_tree(path, root).map(|_| ())
}

fn inspect_plain_tree(path: &Path, root: &Path) -> Result<Vec<(PathBuf, u64, SystemTime)>, String> {
    let mut pending = vec![(path.to_path_buf(), 0usize)];
    let mut snapshot = Vec::new();
    let mut visited_entries = 0usize;
    let mut total_bytes = 0u64;
    while let Some((directory, depth)) = pending.pop() {
        require_plain_directory(&directory, "directory")?;
        let entries = fs::read_dir(&directory).map_err(|error| {
            format!("Failed to read directory {}: {error}", directory.display())
        })?;
        for entry in entries {
            visited_entries += 1;
            if visited_entries > MAX_IMPORT_TREE_ENTRIES {
                return Err(format!(
                    "DST folder {} exceeds the import validation entry limit.",
                    root.display()
                ));
            }
            let entry = entry.map_err(|error| {
                format!(
                    "Failed to inspect directory {}: {error}",
                    directory.display()
                )
            })?;
            let entry_path = entry.path();
            let metadata = fs::symlink_metadata(&entry_path)
                .map_err(|error| format!("Failed to inspect {}: {error}", entry_path.display()))?;
            if is_link_or_reparse(&metadata) {
                return Err(format!(
                    "DST folder {} contains a linked or reparse-point path {}.",
                    root.display(),
                    entry_path.display()
                ));
            }
            snapshot.push((
                entry_path
                    .strip_prefix(root)
                    .map_err(|error| error.to_string())?
                    .to_path_buf(),
                metadata.len(),
                metadata.modified().map_err(|error| {
                    format!(
                        "Cannot inspect source modification time at {}: {error}",
                        entry_path.display()
                    )
                })?,
            ));
            if metadata.is_dir() {
                if depth >= MAX_IMPORT_TREE_DEPTH {
                    return Err(format!(
                        "DST folder {} exceeds the import validation depth limit.",
                        root.display()
                    ));
                }
                pending.push((entry_path, depth + 1));
            } else if metadata.is_file() {
                total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                    format!(
                        "DST folder {} exceeds the import validation size limit.",
                        root.display()
                    )
                })?;
                if total_bytes > MAX_IMPORT_TREE_BYTES {
                    return Err(format!(
                        "DST folder {} exceeds the import validation size limit.",
                        root.display()
                    ));
                }
            } else {
                return Err(format!(
                    "DST folder {} contains an unsupported filesystem entry {}.",
                    root.display(),
                    entry_path.display()
                ));
            }
        }
    }
    snapshot.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(snapshot)
}

#[cfg(windows)]
pub(super) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(super) fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
