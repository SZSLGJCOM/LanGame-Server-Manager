//! Local payload ownership is recorded in the same transaction as installation.
use super::*;

const MAX_IMPORT_METADATA_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands::commands_mod_staging) struct LocalImportMetadata {
    pub(in crate::commands::commands_mod_staging) name: String,
    pub(in crate::commands::commands_mod_staging) version: String,
    pub(in crate::commands::commands_mod_staging) dependencies: Vec<String>,
}

pub(in crate::commands::commands_mod_staging) fn manages_local_imports(
    target: &ResolvedManualModTarget,
) -> bool {
    target.id_strategy.is_none()
        && matches!(
            target.module_id.as_str(),
            "abioticfactor"
                | "sevendaystodie"
                | "humanitz"
                | "enshrouded"
                | "valheim"
                | "vrising"
                | "corekeeper"
                | "minecraft"
                | "necesse"
                | "rust"
                | "windrose"
                | "satisfactory"
                | "astroneer"
                | "sonsoftheforest"
        )
}

/// A local manifest supplies dependency evidence, but its display name cannot
/// establish a Thunderstore author identity or claim another package's files.
pub(in crate::commands::commands_mod_staging) fn local_import_metadata(
    source: &Path,
) -> Result<Option<LocalImportMetadata>, String> {
    let metadata = validate_manual_mod_source_filesystem_entry(source)?;
    let bytes = if metadata.is_dir() {
        let path = source.join("manifest.json");
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("cannot inspect imported Mod manifest: {error}")),
            Ok(metadata)
                if !metadata.is_file()
                    || manual_mod_metadata_is_reparse(&metadata)
                    || metadata.len() > MAX_IMPORT_METADATA_BYTES =>
            {
                return Err("imported Mod manifest is unsafe or exceeds its size limit".into());
            }
            Ok(_) => {}
        }
        read_import_metadata_bytes(fs::File::open(&path).map_err(|error| error.to_string())?)?
    } else if source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
    {
        let mut archive = zip::ZipArchive::new(open_preflighted_mod_archive(source)?)
            .map_err(|error| format!("cannot inspect imported Mod archive: {error}"))?;
        let mut bytes = None;
        for index in 0..archive.len() {
            let entry = archive.by_index(index).map_err(|error| error.to_string())?;
            let name = crate::archive_entry::name(&entry)
                .map_err(|error| format!("invalid imported Mod archive entry: {error}"))?
                .replace('\\', "/");
            // Only a package-root manifest is authoritative for all roots in
            // this archive. Nested unrelated manifests do not imply identity.
            if !name.eq_ignore_ascii_case("manifest.json") {
                continue;
            }
            if bytes.is_some() || entry.is_symlink() || entry.size() > MAX_IMPORT_METADATA_BYTES {
                return Err("imported Mod archive contains an unsafe or duplicate manifest".into());
            }
            bytes = Some(read_import_metadata_bytes(entry)?);
        }
        let Some(bytes) = bytes else { return Ok(None) };
        bytes
    } else {
        return Ok(None);
    };
    let document: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid imported Mod manifest: {error}"))?;
    // Other providers also use manifest.json with different schemas.
    if document.get("version_number").is_none() {
        return Ok(None);
    }
    let name = document
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or("Thunderstore import metadata has no package name")?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("Thunderstore import metadata contains an invalid package name".into());
    }
    let version = document
        .get("version_number")
        .and_then(serde_json::Value::as_str)
        .ok_or("Thunderstore import metadata has no version")?;
    dependencies::parse_thunderstore_dependency(&format!("Local-{name}-{version}"))?;
    let dependencies = document
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .ok_or("Thunderstore import metadata has no dependency list")?;
    if dependencies.len() > 128 {
        return Err("Thunderstore import dependency list exceeds its limit".into());
    }
    let dependencies = dependencies
        .iter()
        .map(|dependency| {
            let dependency = dependency
                .as_str()
                .ok_or("Thunderstore import contains an invalid dependency")?;
            dependencies::parse_thunderstore_dependency(dependency)?;
            Ok(dependency.to_string())
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Some(LocalImportMetadata {
        name: name.to_string(),
        version: version.to_string(),
        dependencies,
    }))
}

fn read_import_metadata_bytes(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(MAX_IMPORT_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("cannot read imported Mod metadata: {error}"))?;
    if bytes.len() as u64 > MAX_IMPORT_METADATA_BYTES {
        return Err("imported Mod metadata exceeds its size limit".into());
    }
    Ok(bytes)
}

pub(in crate::commands::commands_mod_staging) fn prepare_local_mod_ownership(
    target: &ResolvedManualModTarget,
    transaction: &ManualModTransactionPaths,
    metadata: &BTreeMap<String, Option<LocalImportMetadata>>,
) -> Result<(), String> {
    management::recover_management(target)?;
    let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))?;
    let mut imported_keys = Vec::new();
    validate_import_entry_kinds(&transaction.staging, &target.target_path)?;
    let roots = fs::read_dir(&transaction.staging).map_err(|error| error.to_string())?;
    for root in roots {
        let root = root.map_err(|error| error.to_string())?;
        let name = root
            .file_name()
            .into_string()
            .map_err(|_| "imported Mod name is not Unicode")?;
        let destination = target.target_path.join(&name);
        // A standalone root file is fingerprinted as "". Its real filename
        // still determines configuration and user-data protection.
        let protected_file = |relative: &str| {
            management::protected_file(if relative.is_empty() { &name } else { relative })
        };
        let previous_key = manifest
            .packages
            .iter()
            .find(|(_, package)| package.target_name.eq_ignore_ascii_case(&name))
            .map(|(key, _)| key.clone());
        let previous = previous_key
            .as_ref()
            .and_then(|key| manifest.packages.get(key));
        if previous.is_some_and(|package| !package.enabled) {
            return Err(format!(
                "Mod `{name}` is disabled; enable it before importing a replacement"
            ));
        }
        let incoming = fingerprint_tree(&root.path())?;
        let current = fingerprint_tree(&destination)?;
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "mods" | "bepinex" | "plugins" | "core" | "patchers"
        ) && incoming.keys().any(|relative| {
            (!protected_file(relative) && management::loadable_file(relative))
                || Path::new(relative)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case("ModInfo.xml"))
        }) {
            return Err(format!(
                "import contains shared Mod or loader root `{name}`; select the independent package folder inside it rather than importing the shared root; existing files were preserved"
            ));
        }
        if !excluded_import_root(&name, root.path().is_dir()) {
            for (relative, hash) in &current {
                if previous.and_then(|package| package.files.get(relative)) != Some(hash)
                    && !protected_file(relative)
                {
                    return Err(format!(
                        "import destination {} contains an unowned or modified payload file; use a separate package folder or explicitly manage the existing Mod first; existing files were preserved",
                        destination.join(relative).display()
                    ));
                }
            }
        }
        for (relative, hash) in &incoming {
            if let Some(current_hash) = current.get(relative) {
                if previous.and_then(|package| package.files.get(relative)) != Some(current_hash) {
                    return Err(format!(
                        "import conflicts with existing or modified file {}; existing files were preserved",
                        destination.join(relative).display()
                    ));
                }
                // Configurations already present are retained even if an older
                // package originally shipped them; importing must not reset them.
                if protected_file(relative) && current_hash != hash {
                    return Err(format!(
                        "import would overwrite configuration {}; existing files were preserved",
                        destination.join(relative).display()
                    ));
                }
            }
        }
        if incoming.is_empty() || excluded_import_root(&name, root.path().is_dir()) {
            continue;
        }
        let mut files = previous
            .map(|package| package.files.clone())
            .unwrap_or_default();
        // The merge transaction retains old files. Retain ownership only where
        // the old bytes still match, and add precisely this import's new bytes.
        files.retain(|relative, hash| current.get(relative) == Some(hash));
        files.extend(incoming);
        let evidence = metadata.get(&name.to_lowercase()).and_then(Option::as_ref);
        let key = previous_key.unwrap_or_else(|| format!("local-{name}"));
        imported_keys.push(key.clone());
        manifest.packages.insert(
            key,
            InstalledPackage {
                routed_files: BTreeMap::new(),
                target_name: name,
                files,
                version: evidence.map(|value| value.version.clone()),
                dependencies: evidence.map(|value| value.dependencies.clone()),
                enabled: true,
            },
        );
    }
    management::validate_management_manifest(&manifest)?;
    let changed_projects = imported_keys
        .iter()
        .filter_map(|key| key.strip_prefix("thunderstore-"))
        .collect::<HashSet<_>>();
    let mut verification_keys = imported_keys.clone();
    // A local replacement of a known online identity must also preserve the
    // enabled consumers' exact version contracts. Unrelated records do not gate
    // importing an independent Mod.
    for (key, package) in &manifest.packages {
        if package.enabled
            && package.dependencies.iter().flatten().any(|dependency| {
                dependencies::parse_thunderstore_dependency(dependency)
                    .is_ok_and(|(name, _)| changed_projects.contains(name))
            })
            && !verification_keys.contains(key)
        {
            verification_keys.push(key.clone());
        }
    }
    management::verify_recorded_dependencies(
        target,
        &manifest,
        &verification_keys,
        Some(&transaction.staging),
    )?;
    let bytes = serde_json::to_vec(&manifest).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Mod ownership record exceeds its size limit".into());
    }
    fs::write(transaction.staging.join(ONLINE_MOD_MANIFEST), bytes)
        .map_err(|error| format!("failed to stage Mod ownership record: {error}"))
}

fn validate_import_entry_kinds(staging: &Path, target: &Path) -> Result<(), String> {
    let mut pending = vec![(staging.to_path_buf(), target.to_path_buf())];
    while let Some((source, destination)) = pending.pop() {
        let incoming = validate_manual_mod_source_filesystem_entry(&source)?;
        match fs::symlink_metadata(&destination) {
            Ok(existing) if incoming.is_dir() != existing.is_dir() => {
                return Err(format!(
                    "import would replace a file with a folder or a folder with a file at {}; existing files were preserved",
                    destination.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect import destination: {error}")),
        }
        if incoming.is_dir() {
            for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                pending.push((entry.path(), destination.join(entry.file_name())));
            }
        }
    }
    Ok(())
}

fn excluded_import_root(name: &str, directory: bool) -> bool {
    management::protected_root(name) || (!directory && management::protected_file(name))
}

#[cfg(test)]
#[path = "commands_mod_packages_import_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "commands_mod_packages_import_workflow_tests.rs"]
mod workflow_tests;
