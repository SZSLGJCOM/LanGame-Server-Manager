//! A Nexus project owns exact runtime files and one durable inventory anchor.
use super::*;

#[path = "commands_mod_nexus_emf_layout.rs"]
mod emf;
#[path = "commands_mod_nexus_route_layout.rs"]
mod layout;
use layout::profile_for_key;

pub(super) fn is_nexus_key(key: &str) -> bool {
    profile_for_key(key).is_some()
}

pub(super) fn is_metadata(project_key: &str, relative: &str) -> bool {
    profile_for_key(project_key).is_some_and(|profile| layout::nexus_metadata(profile, relative))
}

pub(super) fn runtime_root<'a>(key: &str, target: &'a Path) -> Result<&'a Path, String> {
    profile_for_key(key)
        .ok_or("Invalid routed Nexus project identity")?
        .runtime_root(target)
}

pub(super) fn allowed_route(key: &str, relative: &str) -> bool {
    profile_for_key(key).is_some_and(|profile| profile.allowed(relative))
}

pub(super) fn protected_route(key: &str, relative: &str) -> bool {
    (management::protected_file(relative)
        || profile_for_key(key).is_some_and(|profile| profile.is_lua_dot_data(relative)))
        && !profile_for_key(key).is_some_and(|profile| {
            profile.is_lua_marker(relative)
                || profile.is_eml_metadata(relative)
                || (profile == layout::Profile::Enshrouded && emf::is_payload_descriptor(relative))
        })
}

/// Remove only empty subdirectories below a reviewed container after owned files move.
pub(super) fn prune_route_parents(key: &str, source: &Path, relative: &str) -> Result<(), String> {
    let Some(profile) = profile_for_key(key) else {
        return Ok(());
    };
    let container = profile
        .container(relative)
        .ok_or("Invalid Nexus route container")?;
    let base = source
        .ancestors()
        .nth(Path::new(relative).components().count())
        .ok_or("Nexus route has no storage root")?;
    let stop = base.join(container.trim_end_matches('/'));
    let mut parent = source.parent();
    while let Some(path) = parent {
        if path == stop || !path.starts_with(&stop) {
            break;
        }
        validate_manual_mod_existing_path_components(path, false)?;
        match fs::remove_dir(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) if error.kind() == ErrorKind::DirectoryNotEmpty => break,
            Err(error) => return Err(format!("Cannot remove empty Nexus Mod directory: {error}")),
        }
        parent = path.parent();
    }
    Ok(())
}

pub(in crate::commands) fn stage_nexus_routed_source(
    target: ResolvedManualModTarget,
    source: PathBuf,
    presentation: ManualModImportPresentation,
    project_id: &str,
) -> Result<ManualModStageResult, String> {
    validate_game_mod_mutation(&target.module_id)?;
    let profile = layout::Profile::for_module(&target.module_id)
        .ok_or("This game has no reviewed Nexus runtime routes")?;
    if !layout::valid_project_id(project_id) || target.id_strategy.is_some() {
        return Err("Invalid routed Nexus project identity".into());
    }
    let root = profile.runtime_root(&target.target_path)?.to_path_buf();
    let key = format!("nexus-{}-{project_id}", profile.module());
    let lock = manual_mod_target_lock(&target.target_path)?;
    let _guard = lock.lock().map_err(|_| "Nexus Mod target lock poisoned")?;
    validate_manual_mod_target_path(&target.target_path)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    validate_manual_mod_source_inputs(std::slice::from_ref(&source), &target.accepts)?;
    validate_manual_mod_source_destination(&source, &root)?;
    management::recover_management(&target)?;
    let transaction = create_manual_mod_transaction(&root)?;
    let prepared = prepare(
        &target,
        &source,
        &presentation,
        &key,
        profile,
        project_id,
        &transaction,
    );
    let (files, bytes, replace) = match prepared {
        Ok(value) => value,
        Err(error) => return Err(manual_mod_staging_failure(error, &transaction, true)),
    };
    if let Err(failure) = commit_manual_mod_transaction(&transaction, &root, &replace) {
        return Err(manual_mod_staging_failure(
            failure.message,
            &transaction,
            failure.rollback_complete,
        ));
    }
    let warning = cleanup_manual_mod_transaction(&transaction).err();
    Ok(ManualModStageResult {
        instance_id: target.instance_id,
        module_id: target.module_id,
        source_label: target.source_label,
        target_label: target.target_label,
        target_path: target.target_path.to_string_lossy().into_owned(),
        affected_root_names: vec![key.clone()],
        items: vec![ManualModStageItem {
            source_path: source.to_string_lossy().into_owned(),
            target_path: target.target_path.join(&key).to_string_lossy().into_owned(),
            status: "installed".into(),
            message: warning,
            file_count: files,
            total_bytes: bytes,
        }],
        copied_file_count: files,
        copied_total_bytes: bytes,
    })
}

fn prepare(
    target: &ResolvedManualModTarget,
    source: &Path,
    presentation: &ManualModImportPresentation,
    key: &str,
    profile: layout::Profile,
    project_id: &str,
    transaction: &ManualModTransactionPaths,
) -> Result<(usize, u64, HashSet<PathBuf>), String> {
    let root = profile.runtime_root(&target.target_path)?;
    let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))?;
    management::validate_management_manifest(&manifest)?;
    for other_key in manifest
        .packages
        .keys()
        .filter(|key| profile_for_key(key).is_some())
    {
        runtime_root(other_key, &target.target_path)?;
    }
    let previous = manifest.packages.get(key);
    if previous.is_some_and(|package| !package.enabled) {
        return Err("This Nexus Mod is disabled; enable it before updating".into());
    }
    let extracted = ManualModTransactionPaths {
        root: transaction.root.clone(),
        staging: transaction.partials.join("unpacked"),
        partials: transaction.partials.join("extracting"),
        backup: transaction.backup.clone(),
    };
    fs::create_dir(&extracted.staging).map_err(|error| error.to_string())?;
    fs::create_dir(&extracted.partials).map_err(|error| error.to_string())?;
    stage_manual_mod_source_to_transaction(
        source,
        &extracted,
        &mut ManualModStagingRegistry::default(),
        ManualModArchiveLayout::Preserve,
        None,
    )?;
    let mut payload = fingerprint_tree(&extracted.staging)?;
    let mut routes = layout::plan(profile, &payload, project_id)?;
    emf::prepare(
        root,
        previous,
        &extracted.staging,
        &mut payload,
        &mut routes,
    )?;
    add_enable_markers(profile, root, &extracted.staging, &mut payload, &mut routes)?;
    let migration = if previous.is_none() {
        exact_local_migration(target, profile, &manifest, &payload, &routes)?
    } else {
        LocalMigration::default()
    };
    let anchor_relative = Path::new(profile.target()).join(key);
    let anchor = transaction.staging.join(&anchor_relative);
    fs::create_dir_all(&anchor).map_err(|error| error.to_string())?;
    let mut routed_files = BTreeMap::new();
    let mut replace = migration.replace.clone();
    replace.insert(anchor_relative);
    let mut count = 0usize;
    let mut bytes = 0u64;
    for route in routes {
        let input = extracted.staging.join(&route.source);
        let hash = payload
            .get(&route.source)
            .ok_or("Missing Nexus payload fingerprint")?;
        let destination = if let Some(relative) = route.destination {
            if profile.is_eml_metadata(&relative) {
                validate_eml_metadata(&input)?;
            }
            if manifest.packages.iter().any(|(other, package)| {
                other != key
                    && package
                        .routed_files
                        .keys()
                        .any(|path| path.eq_ignore_ascii_case(&relative))
            }) {
                return Err(format!(
                    "Runtime file {relative} already belongs to another Mod"
                ));
            }
            let live = root.join(&relative);
            validate_manual_mod_existing_path_components(&live, false)?;
            let current = fingerprint_tree(&live)?;
            if !current.is_empty()
                && previous
                    .and_then(|package| package.routed_files.get(&relative))
                    .or_else(|| migration.files.get(&relative))
                    != current.get("")
            {
                return Err(format!(
                    "Runtime file {relative} is unowned or modified; legacy local ownership can migrate only when every recorded byte matches the incoming package. Existing files were preserved"
                ));
            }
            if live.exists() && current.is_empty() {
                return Err(format!(
                    "Runtime destination {relative} is not an owned file"
                ));
            }
            if protected_route(key, &relative)
                && current.get("").is_some_and(|current| current != hash)
            {
                return Err(format!(
                    "Nexus update would overwrite configuration {relative}; existing files were preserved"
                ));
            }
            replace.insert(PathBuf::from(&relative));
            routed_files.insert(relative.clone(), hash.clone());
            transaction.staging.join(relative)
        } else {
            let relative = match route.metadata_path {
                Some(path) => normalize_manual_mod_relative_path(Path::new(&path))?,
                None => PathBuf::from(
                    Path::new(&route.source)
                        .file_name()
                        .ok_or("Nexus metadata has no name")?,
                ),
            };
            let destination = anchor.join(relative);
            if destination.exists() {
                return Err("Nexus package contains duplicate metadata files".into());
            }
            destination
        };
        fs::create_dir_all(destination.parent().ok_or("Nexus payload has no parent")?)
            .map_err(|error| error.to_string())?;
        bytes = bytes
            .checked_add(
                fs::metadata(&input)
                    .map_err(|error| error.to_string())?
                    .len(),
            )
            .ok_or("Nexus package size overflowed")?;
        fs::rename(&input, &destination).map_err(|error| error.to_string())?;
        count += 1;
    }
    let incoming = fingerprint_tree(&anchor)?;
    let live_anchor = target.target_path.join(key);
    if let Some(previous) = previous {
        preserve_user_files_with_policy(
            &live_anchor,
            &anchor,
            previous,
            &incoming,
            PackageUserDataPolicy::RejectConflicts,
        )?;
        for (relative, hash) in &previous.routed_files {
            if !routed_files.contains_key(relative) {
                let live = root.join(relative);
                let current = fingerprint_tree(&live)?;
                if live.exists() && current.get("") != Some(hash) {
                    return Err(format!(
                        "Removed Nexus file {relative} was modified; existing files were preserved"
                    ));
                }
                if protected_route(key, relative) && live.exists() {
                    return Err(format!(
                        "Nexus update would remove configuration {relative}; existing files were preserved"
                    ));
                }
                replace.insert(PathBuf::from(relative));
            }
        }
    } else if live_anchor.exists() {
        return Err("Nexus package anchor is unowned; existing files were preserved".into());
    }
    let package = InstalledPackage {
        display_name: presentation
            .display_name
            .clone()
            .or_else(|| previous.and_then(|value| value.display_name.clone())),
        icon_url: presentation
            .icon_url
            .clone()
            .or_else(|| previous.and_then(|value| value.icon_url.clone())),
        thunderstore_intent: None,
        target_name: key.into(),
        files: incoming,
        routed_files,
        version: None,
        dependencies: None,
        enabled: true,
    };
    for legacy in migration.keys {
        manifest.packages.remove(&legacy);
    }
    manifest.packages.insert(key.into(), package);
    management::validate_management_manifest(&manifest)?;
    let encoded = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    if encoded.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Nexus ownership record exceeds its size limit".into());
    }
    let path = transaction
        .staging
        .join(profile.target())
        .join(ONLINE_MOD_MANIFEST);
    fs::write(path, encoded).map_err(|error| error.to_string())?;
    Ok((count, bytes, replace))
}

fn add_enable_markers(
    profile: layout::Profile,
    runtime: &Path,
    unpacked: &Path,
    payload: &mut BTreeMap<String, String>,
    routes: &mut Vec<layout::Route>,
) -> Result<(), String> {
    let needed = routes
        .iter()
        .filter_map(|route| route.destination.as_deref())
        .filter_map(|path| profile.lua_marker(path))
        .filter(|marker| {
            !routes.iter().any(|route| {
                route
                    .destination
                    .as_deref()
                    .is_some_and(|path| path.eq_ignore_ascii_case(marker))
            })
        })
        .collect::<Vec<_>>();
    if needed.is_empty() {
        return Ok(());
    }
    let list_path = runtime.join(profile.mod_list());
    validate_manual_mod_existing_path_components(&list_path, false)?;
    let names = match fs::symlink_metadata(&list_path) {
        Err(error) if error.kind() == ErrorKind::NotFound => HashSet::new(),
        Err(error) => return Err(error.to_string()),
        Ok(metadata) => {
            if !metadata.is_file()
                || manual_mod_metadata_is_reparse(&metadata)
                || metadata.len() > 65536
            {
                return Err(
                    "UE4SS mod list is unsafe or exceeds its size limit; it was preserved".into(),
                );
            }
            let mut bytes = Vec::new();
            fs::File::open(&list_path)
                .and_then(|file| file.take(65537).read_to_end(&mut bytes))
                .map_err(|error| error.to_string())?;
            if bytes.len() > 65536 {
                return Err("UE4SS mod list exceeds its size limit".into());
            }
            let content = std::str::from_utf8(&bytes)
                .map_err(|_| "UE4SS mod list is not UTF-8; it was preserved")?;
            content
                .trim_start_matches('\u{feff}')
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with(';'))
                .map(|line| {
                    line.split_once(':')
                        .map_or(line, |(name, _)| name)
                        .trim()
                        .to_ascii_lowercase()
                })
                .collect()
        }
    };
    for marker in needed {
        let name = Path::new(&marker)
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .ok_or("UE4SS Mod has no name")?;
        // An explicit operator list entry (including disabled) remains authoritative.
        if names.contains(&name.to_ascii_lowercase()) {
            continue;
        }
        let source = format!(".langame-generated-enabled-{}", routes.len());
        let path = unpacked.join(&source);
        fs::File::options()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        let hash = fingerprint_tree(&path)?
            .remove("")
            .ok_or("Missing UE4SS marker fingerprint")?;
        payload.insert(source.clone(), hash);
        routes.push(layout::Route {
            source,
            destination: Some(marker),
            metadata_path: None,
        });
    }
    Ok(())
}

fn validate_eml_metadata(path: &Path) -> Result<(), String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(65537).read_to_end(&mut bytes))
        .map_err(|error| error.to_string())?;
    if bytes.len() > 65536 {
        return Err("EML mod.json exceeds its size limit".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "EML mod.json is not valid JSON")?;
    if !value.is_object() {
        return Err("EML mod.json must be an object paired with its src/mod.lua entry".into());
    }
    Ok(())
}

#[derive(Default)]
struct LocalMigration {
    keys: Vec<String>,
    files: BTreeMap<String, String>,
    replace: HashSet<PathBuf>,
}

/// Old imports have no website identity. Only their entire, unchanged ownership
/// tree matching incoming bytes can move to a Nexus identity; titles are unused.
fn exact_local_migration(
    target: &ResolvedManualModTarget,
    profile: layout::Profile,
    manifest: &InstalledPackages,
    payload: &BTreeMap<String, String>,
    routes: &[layout::Route],
) -> Result<LocalMigration, String> {
    let incoming = routes
        .iter()
        .filter_map(|route| Some((route.destination.as_deref()?, payload.get(&route.source)?)))
        .collect::<BTreeMap<_, _>>();
    let mut result = LocalMigration::default();
    for (key, package) in &manifest.packages {
        if !key.starts_with("local-")
            || !package.enabled
            || package.files.is_empty()
            || !package.routed_files.is_empty()
            || package.thunderstore_intent.is_some()
            || package.dependencies.is_some()
        {
            continue;
        }
        let base = Path::new(profile.target()).join(&package.target_name);
        let mut matched = BTreeMap::new();
        for (relative, hash) in &package.files {
            let original = base
                .join(relative)
                .to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_owned();
            let direct = incoming
                .get(original.as_str())
                .is_some_and(|expected| *expected == hash);
            // A previously misplaced standalone Pak can move from the original
            // module target to its reviewed Pak container using exact name+bytes.
            let standalone_pak = relative.is_empty()
                && package.files.len() == 1
                && package.target_name.to_ascii_lowercase().ends_with(".pak")
                && incoming
                    .iter()
                    .filter(|(path, expected)| {
                        Path::new(path).file_name().is_some_and(|name| {
                            name.to_string_lossy()
                                .eq_ignore_ascii_case(&package.target_name)
                        }) && expected.as_str() == hash.as_str()
                    })
                    .count()
                    == 1;
            if !direct && !standalone_pak {
                break;
            }
            matched.insert(original, hash.clone());
        }
        if matched.len() != package.files.len() {
            continue;
        }
        let live = target.target_path.join(&package.target_name);
        if fingerprint_tree(&live)? != package.files {
            continue;
        }
        result.files.extend(matched);
        result.replace.insert(base);
        result.keys.push(key.clone());
    }
    Ok(result)
}

/// Inventory reports the actual owned routes, not the empty anchor directory.
pub(super) fn decorate_inventory(
    target: &ResolvedManualModTarget,
    manifest: &InstalledPackages,
    state_root: &Path,
    items: &mut Vec<ManualModInventoryItem>,
) -> Result<(), String> {
    for (key, package) in &manifest.packages {
        let Some(profile) = profile_for_key(key) else {
            continue;
        };
        if target.module_id != profile.module() {
            return Err("Nexus ownership does not belong to this game".into());
        }
        let runtime = profile.runtime_root(&target.target_path)?;
        let routed_root = if package.enabled {
            runtime.to_path_buf()
        } else {
            state_root.join("disabled-routes").join(key)
        };
        let mut count = 0;
        let mut bytes = 0u64;
        for relative in package.routed_files.keys() {
            let path = routed_root.join(relative);
            validate_manual_mod_existing_path_components(&path, false)?;
            match fs::symlink_metadata(&path) {
                Ok(metadata)
                    if metadata.is_file() && !manual_mod_metadata_is_reparse(&metadata) =>
                {
                    count += 1;
                    bytes = bytes
                        .checked_add(metadata.len())
                        .ok_or("Nexus inventory size overflowed")?;
                }
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Ok(_) => return Err("Nexus route is not a regular owned file".into()),
                Err(error) => return Err(error.to_string()),
            }
        }
        if package.enabled {
            let prefix = format!("{}/", profile.target());
            let relative_routes = package
                .routed_files
                .keys()
                .filter_map(|path| path.strip_prefix(&prefix))
                .collect::<HashSet<_>>();
            let mut remove = HashSet::new();
            for item in items.iter().filter(|item| item.management.is_none()) {
                let prefix = format!("{}/", item.name);
                if !relative_routes
                    .iter()
                    .any(|path| *path == item.name || path.starts_with(&prefix))
                {
                    continue;
                }
                if covered_item(&target.target_path, &item.name, &relative_routes)? {
                    remove.insert(item.name.clone());
                }
            }
            items.retain(|item| !remove.contains(&item.name));
        }
        if let Some(item) = items.iter_mut().find(|item| {
            item.management
                .as_ref()
                .is_some_and(|state| state.package_id == *key)
        }) {
            item.file_count += count;
            item.total_bytes = item
                .total_bytes
                .checked_add(bytes)
                .ok_or("Nexus inventory size overflowed")?;
            item.item_type = "directory".into();
        }
    }
    Ok(())
}

fn covered_item(target: &Path, name: &str, routes: &HashSet<&str>) -> Result<bool, String> {
    let mut pending = vec![(PathBuf::from(name), 0usize)];
    let mut files = 0;
    let mut visited = 0;
    while let Some((relative, depth)) = pending.pop() {
        visited += 1;
        if depth > MAX_PACKAGE_DEPTH || visited + pending.len() > MAX_PACKAGE_FILES {
            return Err("Nexus inventory route coverage exceeds its limit".into());
        }
        let path = target.join(&relative);
        let metadata = validate_manual_mod_source_filesystem_entry(&path)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                pending.push((relative.join(entry.file_name()), depth + 1));
            }
        } else {
            if !routes.contains(relative.to_string_lossy().replace('\\', "/").as_str()) {
                return Ok(false);
            }
            files += 1;
        }
    }
    Ok(files > 0)
}

#[cfg(test)]
#[path = "commands_mod_nexus_routes_tests.rs"]
mod tests;
