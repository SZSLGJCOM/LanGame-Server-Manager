//! Exact runtime routes share package ownership with the recursive plugin tree.
use super::super::thunderstore_layout::{ThunderstorePayloadKind, thunderstore_payload_routes};
use super::*;

pub(super) fn runtime_root(target: &Path) -> Result<&Path, String> {
    let root = target
        .parent()
        .and_then(Path::parent)
        .ok_or("Plugin target has no runtime root")?;
    if target != root.join("BepInEx/plugins") {
        return Err("Routed payload requires the verified BepInEx plugins target".into());
    }
    Ok(root)
}

pub(super) fn is_patcher(path: &str) -> bool {
    path.starts_with("BepInEx/patchers/")
}

pub(super) fn is_metadata(path: &str) -> bool {
    !path.contains(['/', '\\'])
        && matches!(
            path.to_ascii_lowercase().as_str(),
            "manifest.json"
                | "icon.png"
                | "readme"
                | "readme.md"
                | "readme.txt"
                | "license"
                | "license.md"
                | "license.txt"
                | "licence"
                | "licence.md"
                | "licence.txt"
                | "changelog"
                | "changelog.md"
                | "changelog.txt"
                | "notice"
                | "notice.md"
                | "notice.txt"
        )
}

pub(super) fn validate_routes(
    package: &InstalledPackage,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    if package.files.len() + package.routed_files.len() > MAX_PACKAGE_FILES {
        return Err("Mod ownership record contains too many routed files".into());
    }
    for (path, hash) in &package.routed_files {
        let normalized = normalize_manual_mod_relative_path(Path::new(path))?;
        if normalized.to_string_lossy().replace('\\', "/") != *path
            || !(is_patcher(path) || path.starts_with("BepInEx/config/"))
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !seen.insert(path.to_lowercase())
        {
            return Err("Invalid or overlapping routed Mod ownership".into());
        }
    }
    Ok(())
}

fn staged_manifest(
    target: &ResolvedManualModTarget,
    transaction: &ManualModTransactionPaths,
) -> Result<InstalledPackages, String> {
    let staged = transaction
        .staging
        .join("BepInEx/plugins")
        .join(ONLINE_MOD_MANIFEST);
    if staged.exists() {
        read_manifest(&staged)
    } else {
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
    }
}

fn publish_record(
    transaction: &ManualModTransactionPaths,
    manifest: &InstalledPackages,
) -> Result<(), String> {
    management::validate_management_manifest(manifest)?;
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("Mod ownership record exceeds its size limit".into());
    }
    let path = transaction
        .staging
        .join("BepInEx/plugins")
        .join(ONLINE_MOD_MANIFEST);
    fs::create_dir_all(path.parent().ok_or("Missing ownership parent")?)
        .map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

pub(in crate::commands::commands_mod_staging) fn prepare_thunderstore_mod_transaction(
    target: &ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    transaction: &ManualModTransactionPaths,
    runtime_dependencies: &[OnlineModIdentity],
) -> Result<PreparedPackages, String> {
    management::recover_management(target)?;
    reject_untracked_downloads(&target.target_path, sources)?;
    let mut manifest = staged_manifest(target, transaction)?;
    verify_dependencies(
        &target.target_path,
        &manifest,
        &sources
            .iter()
            .map(|source| source.identity.clone())
            .collect::<Vec<_>>(),
        runtime_dependencies,
    )?;
    let mut result = (Vec::new(), 0, 0, HashSet::new());
    for source in sources {
        let key = package_key(&source.identity)?;
        let prepared = prepare_package(
            target,
            &source.path,
            &key,
            Some(source.identity.version.clone()),
            Some(source.identity.dependencies.clone()),
            transaction,
            &mut manifest,
        )?;
        result.0.extend(prepared.0);
        result.1 += prepared.1;
        result.2 += prepared.2;
        result.3.extend(prepared.3);
    }
    publish_record(transaction, &manifest)?;
    Ok(result)
}

pub(in crate::commands::commands_mod_staging) fn prepare_local_thunderstore_transaction(
    target: &ResolvedManualModTarget,
    source: &Path,
    metadata: Option<&LocalImportMetadata>,
    transaction: &ManualModTransactionPaths,
) -> Result<PreparedPackages, String> {
    management::recover_management(target)?;
    let name = metadata
        .map(|metadata| metadata.name.as_str())
        .or_else(|| source.file_name().and_then(|name| name.to_str()))
        .ok_or("Imported Mod has no name")?;
    management::validate_component(name)?;
    let mut manifest = staged_manifest(target, transaction)?;
    let result = prepare_package(
        target,
        source,
        &format!("local-local-{name}"),
        metadata.map(|metadata| metadata.version.clone()),
        metadata.map(|metadata| metadata.dependencies.clone()),
        transaction,
        &mut manifest,
    )?;
    publish_record(transaction, &manifest)?;
    Ok(result)
}

pub(in crate::commands::commands_mod_staging) fn finalize_thunderstore_transaction(
    target: &ResolvedManualModTarget,
    transaction: &ManualModTransactionPaths,
    runtime_dependencies: &[OnlineModIdentity],
) -> Result<(), String> {
    let manifest = staged_manifest(target, transaction)?;
    let mut runtime = runtime_dependencies.to_vec();
    if runtime.is_empty()
        && runtime_root(&target.target_path)?
            .join("doorstop_config.ini")
            .exists()
        && manifest.packages.values().any(|package| {
            package
                .dependencies
                .as_ref()
                .is_some_and(|dependencies| !dependencies.is_empty())
        })
    {
        let root = runtime_root(&target.target_path)?;
        if !crate::commands::commands_mod_staging::runtime_loader_requires_archive(
            &target.module_id,
            root,
        )? && let Some(loader) =
            crate::commands::commands_mod_staging::thunderstore_game_profile(&target.module_id)?
                .loader
        {
            runtime.push(loader.identity());
        }
    }
    // Dependency IO sees the staged final plugin/patcher state, with unchanged
    // packages copied only as identities after exact live-byte verification.
    let mut identities = Vec::new();
    for (key, package) in &manifest.packages {
        if !package.enabled {
            continue;
        }
        if let Some(dependencies) = &package.dependencies {
            if let Some(project) = key.strip_prefix("thunderstore-") {
                identities.push(OnlineModIdentity {
                    provider: "thunderstore".into(),
                    project: project.into(),
                    version: package.version.clone().unwrap_or_default(),
                    dependencies: dependencies.clone(),
                    historical_names: Vec::new(),
                });
            } else {
                // Local author identity is unknown; validate its declared requirements only.
                identities.push(OnlineModIdentity {
                    provider: "thunderstore".into(),
                    project: format!("Local-{key}"),
                    version: package.version.clone().unwrap_or_default(),
                    dependencies: dependencies.clone(),
                    historical_names: Vec::new(),
                });
            }
        }
        let plugin_staged = transaction
            .staging
            .join("BepInEx/plugins")
            .join(&package.target_name);
        let live = target.target_path.join(&package.target_name);
        management::verify_package_files(
            package,
            if plugin_staged.exists() {
                &plugin_staged
            } else {
                &live
            },
        )?;
        verify_patchers(package, &target.target_path, Some(&transaction.staging))?;
    }
    // Incoming identities mask already verified packages so dependency checks
    // do not accidentally hash the old version during a batch update.
    verify_dependencies(&target.target_path, &manifest, &identities, &runtime)?;
    Ok(())
}

pub(super) fn verify_patchers(
    package: &InstalledPackage,
    target: &Path,
    staged: Option<&Path>,
) -> Result<(), String> {
    if package.routed_files.is_empty() {
        return Ok(());
    }
    let root = runtime_root(target)?;
    for (relative, hash) in package
        .routed_files
        .iter()
        .filter(|(path, _)| is_patcher(path))
    {
        let live = root.join(relative);
        let staged_file = staged.map(|root| root.join(relative));
        let path = staged_file
            .as_ref()
            .filter(|path| path.exists())
            .unwrap_or(&live);
        if fingerprint_tree(path)?.get("") != Some(hash) {
            return Err(format!(
                "Managed patcher is missing or changed: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

fn prepare_package(
    target: &ResolvedManualModTarget,
    source: &Path,
    key: &str,
    version: Option<String>,
    dependencies: Option<Vec<String>>,
    transaction: &ManualModTransactionPaths,
    manifest: &mut InstalledPackages,
) -> Result<PreparedPackages, String> {
    let root = runtime_root(&target.target_path)?;
    management::validate_component(key)?;
    if manifest
        .packages
        .get(key)
        .is_some_and(|package| !package.enabled)
    {
        return Err(format!("Mod {key} is disabled; enable it before updating"));
    }
    let payload = fingerprint_tree(source)?;
    let routes =
        thunderstore_payload_routes(&payload.keys().map(PathBuf::from).collect::<Vec<_>>())?;
    if routes.is_empty() {
        return Err("Mod package contains no files".into());
    }
    let target_name = key.strip_prefix("local-").unwrap_or(key);
    let plugin_relative = Path::new("BepInEx/plugins").join(target_name);
    let plugin_staged = transaction.staging.join(&plugin_relative);
    if plugin_staged.exists() {
        return Err(format!(
            "Package {key} occurs more than once in this request"
        ));
    }
    fs::create_dir_all(&plugin_staged).map_err(|error| error.to_string())?;
    let mut routed_files = BTreeMap::new();
    let mut files = 0;
    let mut bytes = 0;
    let mut warnings = Vec::new();
    let mut replace = HashSet::from([plugin_relative]);
    for route in routes {
        let input = source.join(&route.source_relative);
        let hash = payload
            .get(&route.source_relative.to_string_lossy().replace('\\', "/"))
            .ok_or("Missing routed source fingerprint")?;
        let destination = match route.kind {
            ThunderstorePayloadKind::Plugin | ThunderstorePayloadKind::Metadata => {
                plugin_staged.join(&route.relative)
            }
            kind => {
                let container = if kind == ThunderstorePayloadKind::Config {
                    "BepInEx/config"
                } else {
                    "BepInEx/patchers"
                };
                let relative = Path::new(container).join(&route.relative);
                let relative = relative.to_string_lossy().replace('\\', "/");
                if manifest.packages.iter().any(|(other, package)| {
                    other != key
                        && package
                            .routed_files
                            .keys()
                            .any(|path| path.eq_ignore_ascii_case(&relative))
                }) {
                    return Err(format!(
                        "Runtime file {relative} already belongs to another package"
                    ));
                }
                routed_files.insert(relative.clone(), hash.clone());
                let live = root.join(&relative);
                let staged = transaction.staging.join(&relative);
                if staged.exists() {
                    if kind == ThunderstorePayloadKind::Config {
                        warnings.push(format!("Preserved prepared configuration {relative}"));
                        continue;
                    }
                    return Err(format!(
                        "Runtime file {relative} occurs more than once in this request"
                    ));
                }
                if live.exists() {
                    let current = fingerprint_tree(&live)?;
                    if kind == ThunderstorePayloadKind::Config {
                        if !current.contains_key("") {
                            return Err(format!("Configuration is not a regular file: {relative}"));
                        }
                        warnings.push(format!("Preserved existing configuration {relative}"));
                        continue;
                    }
                    let previous = manifest
                        .packages
                        .get(key)
                        .and_then(|package| package.routed_files.get(&relative));
                    if previous.is_none() || current.get("") != previous {
                        return Err(format!(
                            "Patcher {relative} is unowned or modified; existing files were preserved"
                        ));
                    }
                }
                replace.insert(PathBuf::from(&relative));
                staged
            }
        };
        fs::create_dir_all(destination.parent().ok_or("Routed payload has no parent")?)
            .map_err(|error| error.to_string())?;
        bytes += copy_mod_payload(&input, &destination)?;
        files += 1;
    }
    let incoming = fingerprint_tree(&plugin_staged)?;
    let destination = target.target_path.join(target_name);
    if let Some(previous) = manifest.packages.get(key) {
        preserve_user_files(&destination, &plugin_staged, previous, &incoming)?;
        for (relative, hash) in previous
            .routed_files
            .iter()
            .filter(|(path, _)| is_patcher(path))
        {
            if !routed_files.contains_key(relative) {
                let path = root.join(relative);
                if path.exists() && fingerprint_tree(&path)?.get("") != Some(hash) {
                    return Err(format!(
                        "Removed patcher {relative} was modified; existing files were preserved"
                    ));
                }
                replace.insert(PathBuf::from(relative));
            }
        }
    } else if destination.exists() {
        return Err(format!(
            "Unowned plugin destination {} was preserved",
            destination.display()
        ));
    }
    // Config-only/patcher-only packages still have a durable, empty plugin root
    // that inventory and the package journal can move together with patchers.
    manifest.packages.insert(
        key.into(),
        InstalledPackage {
            target_name: target_name.into(),
            files: incoming,
            routed_files,
            version,
            dependencies,
            enabled: true,
        },
    );
    let item = ManualModStageItem {
        source_path: source.to_string_lossy().into_owned(),
        target_path: destination.to_string_lossy().into_owned(),
        status: "installed".into(),
        message: (!warnings.is_empty()).then(|| warnings.join("; ")),
        file_count: files,
        total_bytes: bytes,
    };
    Ok((vec![item], files, bytes, replace))
}

#[cfg(test)]
#[path = "commands_mod_routed_packages_tests.rs"]
mod tests;
