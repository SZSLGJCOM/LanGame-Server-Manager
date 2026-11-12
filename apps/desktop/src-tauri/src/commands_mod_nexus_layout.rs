//! Route verified 7 Days to Die package roots inside the existing import transaction.
use super::*;

// Match the maximum depth exposed by the Mod inventory; never scan a tree
// that this workflow could not later inspect and manage.
const MAX_DIRECTORY_DEPTH: usize = 32;

struct ModRoot {
    components: Vec<String>,
    destination: PathBuf,
}

pub(super) fn seven_days_layout(
    source: &Path,
    nexus_project_id: Option<&str>,
) -> Result<ManualModArchiveLayout, String> {
    let root_name = if let Some(project_id) = nexus_project_id {
        let id = project_id
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .filter(|_| project_id.bytes().all(|byte| byte.is_ascii_digit()))
            .ok_or("Nexus package layout requires a valid project ID")?;
        format!("nexus-7daystodie-{id}")
    } else {
        let name = if source.is_dir() {
            source.file_name()
        } else {
            source.file_stem()
        }
        .ok_or("7 Days to Die Mod source has no package name")?;
        validate_manual_mod_windows_component(name)?;
        let name = name
            .to_str()
            .ok_or("7 Days to Die Mod package name is not Unicode")?
            .to_owned();
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "mods" | "release" | "bepinex" | "plugins" | "core" | "patchers" | "config"
        ) {
            format!("7daystodie-{name}")
        } else {
            name
        }
    };
    Ok(ManualModArchiveLayout::SevenDaysToDie {
        root_name,
        require_manifest: nexus_project_id.is_some(),
    })
}

fn mod_roots(
    entries: &[ManualModArchiveEntryPlan],
    root_name: &str,
    require_manifest: bool,
) -> Result<Vec<ModRoot>, String> {
    validate_manual_mod_windows_component(std::ffi::OsStr::new(root_name))?;
    let mut original_paths = ManualModStagingRegistry::default();
    let mut roots = Vec::<ModRoot>::new();
    let mut destinations = HashSet::new();
    for entry in entries {
        // Check the original topology too: removing a wrapper must not conceal
        // a file/directory conflict or an unsafe Windows path.
        match entry.kind {
            ManualModArchiveEntryKind::Directory => {
                original_paths.register_directory(&entry.relative_path)?;
            }
            ManualModArchiveEntryKind::File => {
                original_paths.register_file(&entry.relative_path)?
            }
        }
        if entry.kind != ManualModArchiveEntryKind::File
            || !entry
                .relative_path
                .file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("ModInfo.xml"))
        {
            continue;
        }
        let parent = entry.relative_path.parent().unwrap_or(Path::new(""));
        let components = manual_mod_relative_path_components(parent)?;
        let name = parent
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .filter(|name| !matches!(name.to_ascii_lowercase().as_str(), "mods" | "release"))
            .unwrap_or(root_name);
        let destination = PathBuf::from(name);
        if !destinations.insert(manual_mod_relative_path_key(&destination)?) {
            return Err("7 Days to Die archive contains multiple ModInfo.xml roots with the same package name; select one independent package instead".into());
        }
        roots.push(ModRoot {
            components,
            destination,
        });
    }
    if roots.is_empty() && require_manifest {
        return Err("7 Days to Die website package has no ModInfo.xml; select an archive containing an independent game Mod".into());
    }
    for (index, root) in roots.iter().enumerate() {
        if roots.iter().skip(index + 1).any(|other| {
            root.components.starts_with(&other.components)
                || other.components.starts_with(&root.components)
        }) {
            return Err("7 Days to Die package has nested ModInfo.xml roots; select the independent packages separately".into());
        }
    }
    Ok(roots)
}

fn routed_path(
    entry: &ManualModArchiveEntryPlan,
    roots: &[ModRoot],
) -> Result<Option<PathBuf>, String> {
    let components = manual_mod_relative_path_components(&entry.relative_path)?;
    if let Some(root) = roots
        .iter()
        .find(|root| components.starts_with(&root.components))
    {
        let suffix: PathBuf = entry
            .relative_path
            .components()
            .skip(root.components.len())
            .collect();
        return Ok(Some(root.destination.join(suffix)));
    }
    if entry.kind == ManualModArchiveEntryKind::Directory
        && roots
            .iter()
            .any(|root| root.components.starts_with(&components))
    {
        // Empty ancestor wrappers are not packages. Files outside a recognized
        // Mod stay at their original paths and retain ordinary ownership checks.
        return Ok(None);
    }
    Ok(Some(entry.relative_path.clone()))
}

pub(super) fn route_seven_days_entries(
    entries: Vec<ManualModArchiveEntryPlan>,
    root_name: &str,
    require_manifest: bool,
) -> Result<Vec<ManualModArchiveEntryPlan>, String> {
    let roots = mod_roots(&entries, root_name, require_manifest)?;
    if roots.is_empty() {
        // Existing local file imports are not reinterpreted as game packages.
        return Ok(entries);
    }
    entries
        .into_iter()
        .filter_map(|mut entry| match routed_path(&entry, &roots) {
            Ok(Some(path)) => {
                entry.relative_path = path;
                Some(Ok(entry))
            }
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}

fn inspect_directory(
    source: &Path,
    relative: &Path,
    depth: usize,
    scanned: &mut usize,
    entries: &mut Vec<ManualModArchiveEntryPlan>,
    sources: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err("7 Days to Die Mod directory exceeds its depth limit".into());
    }
    let mut children = Vec::new();
    for child in fs::read_dir(source)
        .map_err(|error| format!("Cannot inspect 7 Days to Die Mod directory: {error}"))?
    {
        if *scanned >= MANUAL_MOD_ARCHIVE_MAX_ENTRY_COUNT || depth >= MAX_DIRECTORY_DEPTH {
            return Err("7 Days to Die Mod directory exceeds its entry or depth limit".into());
        }
        let child = child
            .map_err(|error| format!("Cannot inspect 7 Days to Die Mod directory: {error}"))?;
        *scanned += 1;
        children.push(child);
    }
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let metadata = validate_manual_mod_source_filesystem_entry(&path)?;
        let relative_path = relative.join(child.file_name());
        let kind = if metadata.is_dir() {
            ManualModArchiveEntryKind::Directory
        } else {
            ManualModArchiveEntryKind::File
        };
        entries.push(ManualModArchiveEntryPlan {
            index: sources.len(),
            relative_path: relative_path.clone(),
            kind,
            compressed_bytes: 0,
            uncompressed_bytes: metadata.len(),
        });
        sources.push(path.clone());
        if metadata.is_dir() {
            inspect_directory(&path, &relative_path, depth + 1, scanned, entries, sources)?;
        }
    }
    Ok(())
}

pub(super) fn stage_seven_days_directory(
    source: &Path,
    transaction: &ManualModTransactionPaths,
    registry: &mut ManualModStagingRegistry,
    root_name: &str,
    require_manifest: bool,
) -> Result<Option<StagedManualModSource>, String> {
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    inspect_directory(source, Path::new(""), 0, &mut 0, &mut entries, &mut sources)?;
    if mod_roots(&entries, root_name, require_manifest)?.is_empty() {
        return Ok(None);
    }
    let entries = route_seven_days_entries(entries, root_name, require_manifest)?;
    let mut planned_registry = registry.clone();
    for entry in &entries {
        match entry.kind {
            ManualModArchiveEntryKind::Directory => {
                planned_registry.register_directory(&entry.relative_path)?;
            }
            ManualModArchiveEntryKind::File => {
                planned_registry.register_file(&entry.relative_path)?;
            }
        }
    }
    let mut stats = DirectoryCopyStats::default();
    for entry in entries {
        let destination = transaction.staging.join(&entry.relative_path);
        if entry.kind == ManualModArchiveEntryKind::Directory {
            fs::create_dir_all(&destination)
                .map_err(|error| format!("Cannot prepare staged Mod directory: {error}"))?;
        } else {
            let metadata = validate_manual_mod_source_filesystem_entry(&sources[entry.index])?;
            if !metadata.is_file() {
                return Err("7 Days to Die Mod source changed while being staged".into());
            }
            let bytes = copy_manual_mod_file_to_staging(
                &sources[entry.index],
                &destination,
                &transaction.partials,
            )?;
            stats.file_count = stats
                .file_count
                .checked_add(1)
                .ok_or("manual mod folder file count overflowed")?;
            stats.total_bytes = stats
                .total_bytes
                .checked_add(bytes)
                .ok_or("manual mod folder byte count overflowed")?;
        }
    }
    *registry = planned_registry;
    Ok(Some(StagedManualModSource {
        source_path: source.to_owned(),
        target_relative_path: None,
        stats,
    }))
}

#[cfg(test)]
#[path = "commands_mod_nexus_layout_tests.rs"]
mod tests;
