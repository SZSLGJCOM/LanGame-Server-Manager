//! File Mod lifecycle. Ownership is evidence about bytes, never a recursive delete grant.
use super::*;

#[path = "commands_mod_management_io.rs"]
pub(super) mod storage;
use storage::{ManagementPaths, ManagementTransaction};

pub(in crate::commands) fn recover_management(
    target: &ResolvedManualModTarget,
) -> Result<(), String> {
    storage::recover(&ManagementPaths::new(&target.target_path)?)
}

pub(in crate::commands) fn manage_package(
    target: ResolvedManualModTarget,
    package_id: &str,
    action: &str,
) -> Result<ManagedModActionResult, String> {
    if !matches!(action, "adopt" | "enable" | "disable" | "delete") {
        return Err("Unknown Mod management action".into());
    }
    validate_game_mod_mutation(&target.module_id)?;
    let lock = manual_mod_target_lock(&target.target_path)?;
    let guard = lock
        .lock()
        .map_err(|_| "Mod management target lock poisoned")?;
    validate_manual_mod_target_path(&target.target_path)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    let paths = ManagementPaths::new(&target.target_path)?;
    storage::recover(&paths)?;
    let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))?;
    validate_management_manifest(&manifest)?;
    let mut warnings = Vec::new();
    let (key, intended_package) = if action == "adopt" {
        let package = adopt_package(&target.target_path, package_id, &manifest)?;
        (format!("local-{package_id}"), Some(package))
    } else {
        (package_id.to_owned(), None)
    };
    if action != "adopt" {
        let package = manifest
            .packages
            .get(&key)
            .ok_or("Mod is not managed; adopt its files first")?;
        let dependents = dependent_packages(&manifest, &key, action == "disable")?;
        if matches!(action, "disable" | "delete") && !dependents.is_empty() {
            return Err(format!(
                "Mod is required by {}; manage those Mods first",
                dependents.join(", ")
            ));
        }
        if action == "enable" {
            verify_enable_dependencies(&target, &manifest, &key)?;
        }
        let package_path = if package.enabled {
            target.target_path.join(&package.target_name)
        } else {
            paths.disabled.join(&package.target_name)
        };
        validate_exclusive_payload(package, &package_path)?;
        if package.dependencies.is_none() {
            warnings.push("This package has no complete dependency declaration; unknown dependencies could not be verified".into());
        }
        if (action == "enable" && package.enabled) || (action == "disable" && !package.enabled) {
            drop(guard);
            return Ok(ManagedModActionResult {
                inventory: crate::commands::commands_mods::mod_inventory::read_inventory(target)?,
                preserved_path: None,
                removed_file_count: 0,
                warnings,
            });
        }
    }
    let transaction = ManagementTransaction::new(key, action, &mut manifest, intended_package)?;
    let outcome = storage::execute(&paths, &transaction)?;
    warnings.extend(outcome.warnings);
    drop(guard);
    Ok(ManagedModActionResult {
        inventory: crate::commands::commands_mods::mod_inventory::read_inventory(target)?,
        preserved_path: outcome.preserved_path,
        removed_file_count: outcome.removed_file_count,
        warnings,
    })
}

/// The caller owns the target lock; pending recovery is rejected.
pub(in crate::commands) fn decorate_inventory(
    target: &ResolvedManualModTarget,
    items: &mut Vec<ManualModInventoryItem>,
) -> Result<(), String> {
    let paths = ManagementPaths::new(&target.target_path)?;
    if paths.journal.exists() {
        return Err("A Mod operation needs recovery before inventory can be read".into());
    }
    let manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))?;
    validate_management_manifest(&manifest)?;
    let disabled_items =
        if manifest.packages.values().any(|package| !package.enabled) && paths.disabled.exists() {
            crate::commands::commands_mods::mod_inventory::read_manual_mod_inventory_items(
                None,
                &paths.disabled,
            )?
        } else {
            Vec::new()
        };
    for (key, package) in &manifest.packages {
        let state = ManagedModState {
            package_id: key.clone(),
            enabled: package.enabled,
            dependencies: package.dependencies.clone().unwrap_or_default(),
            dependents: dependent_packages(&manifest, key, false)?,
            dependency_status: if package.dependencies.is_some() {
                "complete"
            } else {
                "unknown"
            }
            .into(),
        };
        if package.enabled
            && let Some(item) = items
                .iter_mut()
                .find(|item| item.name.eq_ignore_ascii_case(&package.target_name))
        {
            item.management = Some(state);
            continue;
        }
        let path = if package.enabled {
            target.target_path.join(&package.target_name)
        } else {
            paths.disabled.join(&package.target_name)
        };
        if !package.enabled
            && let Some(item) = disabled_items
                .iter()
                .find(|item| item.name.eq_ignore_ascii_case(&package.target_name))
        {
            let mut item = item.clone();
            item.management = Some(state);
            items.push(item);
            continue;
        }
        items.push(ManualModInventoryItem {
            name: package.target_name.clone(),
            path: path.to_string_lossy().into_owned(),
            item_type: "file".into(),
            inferred_id: None,
            file_count: 0,
            total_bytes: 0,
            modified_unix_ms: None,
            management: Some(state),
        });
    }
    items.sort_by_key(|item| item.name.to_lowercase());
    Ok(())
}

pub(super) fn copy_package(package: &InstalledPackage) -> InstalledPackage {
    InstalledPackage {
        routed_files: package.routed_files.clone(),
        target_name: package.target_name.clone(),
        files: package.files.clone(),
        version: package.version.clone(),
        dependencies: package.dependencies.clone(),
        enabled: package.enabled,
    }
}

pub(super) fn validate_management_manifest(manifest: &InstalledPackages) -> Result<(), String> {
    if manifest.packages.len() > MAX_PACKAGE_FILES {
        return Err("Mod ownership record contains too many packages".into());
    }
    let mut targets = HashSet::new();
    let mut routed_paths = HashSet::new();
    for (key, package) in &manifest.packages {
        routed::validate_routes(package, &mut routed_paths)?;
        validate_component(key)?;
        validate_component(&package.target_name)?;
        if protected_root(&package.target_name)
            || !targets.insert(package.target_name.to_lowercase())
        {
            return Err("Mod ownership record contains a protected or duplicate target".into());
        }
        if package.files.len() > MAX_PACKAGE_FILES {
            return Err("Mod ownership record contains too many files".into());
        }
        let mut registry = ManualModStagingRegistry::default();
        for (relative, hash) in &package.files {
            if !relative.is_empty() {
                let normalized = normalize_manual_mod_relative_path(Path::new(relative))?;
                if normalized.to_string_lossy().replace('\\', "/") != *relative {
                    return Err("Invalid relative path in Mod ownership record".into());
                }
                registry.register_file(&normalized)?;
            } else if package.files.len() != 1 {
                return Err("Mod ownership record mixes a root file with directory entries".into());
            }
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Invalid fingerprint in Mod ownership record".into());
            }
        }
        if package
            .dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.len() > 128)
        {
            return Err("Mod dependency declaration exceeds its limit".into());
        }
    }
    Ok(())
}

pub(super) fn validate_component(value: &str) -> Result<(), String> {
    validate_manual_mod_windows_component(std::ffi::OsStr::new(value))?;
    if value.contains(['/', '\\']) || value.starts_with('.') || value.len() > 240 {
        return Err("Invalid Mod management path component".into());
    }
    Ok(())
}

pub(super) fn protected_root(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with('.')
        || matches!(
            name.as_str(),
            "0_tfp_harmony"
                | "bepinex"
                | "core"
                | "patchers"
                | "plugins"
                | "mods"
                | "config"
                | "configs"
                | "configuration"
                | "settings"
                | "save"
                | "saves"
                | "worlds"
                | "userdata"
                | "profiles"
        )
}

pub(super) fn protected_file(relative: &str) -> bool {
    let lower = relative.replace('\\', "/").to_ascii_lowercase();
    lower.split('/').any(|part| {
        part == ".env"
            || part.starts_with(".env.")
            || matches!(
                part,
                "config"
                    | "configs"
                    | "configuration"
                    | "settings"
                    | "save"
                    | "saves"
                    | "worlds"
                    | "profiles"
                    | "userdata"
            )
    }) || Path::new(&lower)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "cfg"
                    | "conf"
                    | "config"
                    | "cnf"
                    | "properties"
                    | "hjson"
                    | "jsonc"
                    | "snbt"
                    | "nbt"
                    | "env"
                    | "prefs"
                    | "ini"
                    | "json"
                    | "json5"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "xml"
                    | "txt"
                    | "log"
                    | "db"
                    | "sqlite"
                    | "sqlite3"
                    | "sav"
                    | "save"
                    | "dat"
                    | "world"
                    | "wld"
                    | "map"
                    | "bak"
            )
        })
}

fn adopt_package(
    target: &Path,
    name: &str,
    manifest: &InstalledPackages,
) -> Result<InstalledPackage, String> {
    validate_component(name)?;
    if protected_root(name)
        || manifest
            .packages
            .values()
            .any(|package| package.target_name.eq_ignore_ascii_case(name))
    {
        return Err("This path is protected or already belongs to a managed Mod".into());
    }
    let path = target.join(name);
    let evidence = local_import_metadata(&path)?;
    let mut files = fingerprint_tree(&path)?;
    if files.is_empty() {
        return Err("The selected Mod contains no files".into());
    }
    let xml_mod = files
        .keys()
        .any(|relative| relative.eq_ignore_ascii_case("ModInfo.xml"));
    // Explicit adoption grants ownership only over recognizable payloads. User
    // documents and settings in an existing directory remain unowned.
    files.retain(|relative, _| {
        let candidate = if relative.is_empty() { name } else { relative };
        (xml_mod && relative.eq_ignore_ascii_case("ModInfo.xml"))
            || (!protected_file(candidate) && loadable_file(candidate))
    });
    if files.is_empty() {
        return Err("No recognizable Mod payload was found; unrelated files were preserved".into());
    }
    // Adoption records files already in the loading tree. Retain declarations
    // even for a broken install so it can be disabled; enabling validates them.
    Ok(InstalledPackage {
        routed_files: BTreeMap::new(),
        target_name: name.into(),
        files,
        version: evidence.as_ref().map(|metadata| metadata.version.clone()),
        dependencies: evidence.map(|metadata| metadata.dependencies),
        enabled: true,
    })
}

pub(super) fn loadable_file(relative: &str) -> bool {
    Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "dll" | "jar" | "pak" | "lua" | "bundle" | "so" | "cs" | "smod" | "ucas" | "utoc"
            )
        })
}

fn validate_exclusive_payload(package: &InstalledPackage, path: &Path) -> Result<(), String> {
    for relative in fingerprint_tree(path)?.keys() {
        let candidate = if relative.is_empty() {
            package.target_name.as_str()
        } else {
            relative.as_str()
        };
        if !package.files.contains_key(relative)
            && !protected_file(candidate)
            && loadable_file(candidate)
        {
            return Err(format!(
                "Mod directory contains an unowned payload {candidate}; split unrelated Mods into separate directories before managing this package"
            ));
        }
    }
    Ok(())
}

fn dependent_packages(
    manifest: &InstalledPackages,
    key: &str,
    enabled_only: bool,
) -> Result<Vec<String>, String> {
    let Some(name) = key.strip_prefix("thunderstore-") else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for (consumer, package) in &manifest.packages {
        if consumer == key || (enabled_only && !package.enabled) {
            continue;
        }
        for dependency in package.dependencies.iter().flatten() {
            let (dependency_name, _) = dependencies::parse_thunderstore_dependency(dependency)?;
            if dependency_name == name {
                result.push(consumer.clone());
                break;
            }
        }
    }
    Ok(result)
}

fn verify_enable_dependencies(
    target: &ResolvedManualModTarget,
    manifest: &InstalledPackages,
    key: &str,
) -> Result<(), String> {
    let package = manifest
        .packages
        .get(key)
        .ok_or("Managed Mod does not exist")?;
    let paths = ManagementPaths::new(&target.target_path)?;
    let source = if package.enabled {
        target.target_path.join(&package.target_name)
    } else {
        paths.disabled.join(&package.target_name)
    };
    verify_package_files(package, &source)?;
    if !package.routed_files.is_empty() {
        for (relative, hash) in package
            .routed_files
            .iter()
            .filter(|(path, _)| routed::is_patcher(path))
        {
            let file = if package.enabled {
                routed::runtime_root(&target.target_path)?.join(relative)
            } else {
                paths.root.join("disabled-routes").join(key).join(relative)
            };
            if fingerprint_tree(&file)?.get("") != Some(hash) {
                return Err(format!(
                    "Patcher {relative} is missing or changed; files were preserved"
                ));
            }
        }
    }
    verify_recorded_dependencies(target, manifest, &[key.into()], None)
}

pub(super) fn verify_recorded_dependencies(
    target: &ResolvedManualModTarget,
    manifest: &InstalledPackages,
    keys: &[String],
    staging: Option<&Path>,
) -> Result<(), String> {
    let mut pending = Vec::new();
    for key in keys {
        let package = manifest
            .packages
            .get(key)
            .ok_or("Managed Mod does not exist")?;
        pending.extend(
            package
                .dependencies
                .iter()
                .flatten()
                .map(|dependency| (dependency.clone(), 0)),
        );
        if pending.len() > 128 {
            return Err("Mod dependency graph exceeds its limit".into());
        }
    }
    let mut visited = HashSet::new();
    while let Some((dependency, depth)) = pending.pop() {
        if !visited.insert(dependency.clone()) {
            continue;
        }
        if visited.len() > 128 || depth > 16 {
            return Err("Mod dependency graph exceeds its limit".into());
        }
        let (name, version) = dependencies::parse_thunderstore_dependency(&dependency)?;
        if loader_satisfies(target, name, version)? {
            continue;
        }
        let dependency_package = manifest.packages.get(&format!("thunderstore-{name}"))
            .filter(|package| package.enabled && package.version.as_deref() == Some(version) && package.dependencies.is_some())
            .ok_or_else(|| format!("Required dependency {dependency} is missing, disabled, or has an unverified version"))?;
        let staged = staging.map(|root| root.join(&dependency_package.target_name));
        let dependency_path = staged
            .filter(|path| path.exists())
            .unwrap_or_else(|| target.target_path.join(&dependency_package.target_name));
        verify_package_files(dependency_package, &dependency_path)?;
        routed::verify_patchers(dependency_package, &target.target_path, None)?;
        pending.extend(
            dependency_package
                .dependencies
                .iter()
                .flatten()
                .map(|dependency| (dependency.clone(), depth + 1)),
        );
        if pending.len() > 128 {
            return Err("Mod dependency graph exceeds its limit".into());
        }
    }
    Ok(())
}

pub(super) fn verify_package_files(package: &InstalledPackage, path: &Path) -> Result<(), String> {
    let current = fingerprint_tree(path)?;
    if (package.files.is_empty() && package.routed_files.is_empty())
        || !package
            .files
            .iter()
            .filter(|(path, _)| {
                let relative = if path.is_empty() {
                    package.target_name.as_str()
                } else {
                    path.as_str()
                };
                !protected_file(relative) || relative.eq_ignore_ascii_case("ModInfo.xml")
            })
            .all(|(path, hash)| current.get(path) == Some(hash))
        || (current.is_empty() && !package.files.is_empty())
    {
        return Err(format!(
            "Mod payload {} is missing or changed; its files were preserved",
            package.target_name
        ));
    }
    Ok(())
}

fn loader_satisfies(
    target: &ResolvedManualModTarget,
    name: &str,
    version: &str,
) -> Result<bool, String> {
    let Ok(game) = thunderstore_game_profile(&target.module_id) else {
        return Ok(false);
    };
    let Some(loader) = game
        .loader
        .filter(|loader| loader.supports_dependency(name, version))
    else {
        return Ok(false);
    };
    let depth = Path::new(game.target_relative).components().count();
    let root = target
        .target_path
        .ancestors()
        .nth(depth)
        .ok_or("Mod target has no runtime root")?;
    if target.target_path != root.join(game.target_relative) {
        return Ok(false);
    }
    Ok(!runtime_loader_requires_archive(&target.module_id, root)?
        && loader.supports_dependency(name, version))
}

#[cfg(test)]
#[path = "commands_mod_management_tests.rs"]
mod tests;
