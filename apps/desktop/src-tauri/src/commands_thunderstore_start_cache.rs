//! Read-only startup proof over the existing ownership record and actual
//! enabled payloads. Caller owns instance/program/storage-context leases.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::commands) struct CachedThunderstorePackage {
    pub(in crate::commands) project: String,
    pub(in crate::commands) version: String,
    pub(in crate::commands) dependencies: Vec<String>,
}

#[derive(Clone)]
pub(in crate::commands) struct ThunderstoreStartPlan {
    pub(in crate::commands) references: Vec<String>,
    pub(in crate::commands) packages: Vec<CachedThunderstorePackage>,
    pub(in crate::commands) source_required: Vec<String>,
    snapshots: BTreeMap<PathBuf, BTreeMap<String, String>>,
    record: BTreeMap<String, String>,
    record_path: PathBuf,
}

impl ThunderstoreStartPlan {
    /// Verify the originals moved by publication before transaction cleanup.
    /// A racing operator write preserves both the new payload and old recovery
    /// tree; it cannot authorize a cached-start warning.
    pub(in crate::commands) fn verify_backup(
        &self,
        backup: &Path,
        publication_root: &Path,
        replacements: &HashSet<PathBuf>,
    ) -> Result<(), String> {
        let original = self.original_files();
        let relative = self
            .record_path
            .strip_prefix(publication_root)
            .map_err(|_| "Thunderstore ownership record belongs to another publication root")?;
        if fingerprint_tree(&backup.join(relative))? != self.record {
            return Err("Thunderstore original ownership record changed during publication; recovery files were retained".into());
        }
        for (relative, hash) in fingerprint_tree(backup)? {
            if original.get(&publication_root.join(relative)) != Some(&hash) {
                return Err("Thunderstore operator data changed during publication; original recovery files were retained".into());
            }
        }
        for relative in replacements {
            let source = publication_root.join(relative);
            let expected = original
                .iter()
                .filter_map(|(path, hash)| {
                    path.strip_prefix(&source).ok().map(|relative| {
                        (relative.to_string_lossy().replace('\\', "/"), hash.clone())
                    })
                })
                .collect::<BTreeMap<_, _>>();
            if !expected.is_empty() && fingerprint_tree(&backup.join(relative))? != expected {
                return Err("Thunderstore original payload changed during publication; recovery files were retained".into());
            }
        }
        Ok(())
    }
    fn original_files(&self) -> BTreeMap<PathBuf, String> {
        let mut files = BTreeMap::new();
        for (path, entries) in &self.snapshots {
            for (relative, hash) in entries {
                files.insert(
                    if relative.is_empty() {
                        path.clone()
                    } else {
                        path.join(relative)
                    },
                    hash.clone(),
                );
            }
        }
        files
    }
    pub(in crate::commands) fn item_ids(&self) -> Vec<String> {
        self.packages
            .iter()
            .map(|package| package.project.clone())
            .collect()
    }
    pub(in crate::commands) fn verify_unchanged(
        &self,
        target: &ResolvedManualModTarget,
    ) -> Result<(), String> {
        if fingerprint_tree(&target.target_path.join(ONLINE_MOD_MANIFEST))? != self.record {
            return Err("Thunderstore Mod ownership or version intent changed during startup; retry after confirming the Mod selection".into());
        }
        for (path, expected) in &self.snapshots {
            if fingerprint_tree(path)? != *expected {
                return Err("Thunderstore actual Mod payload or operator data changed during the update; automatic publication and cached startup were refused".into());
            }
        }
        Ok(())
    }
}

pub(in crate::commands) fn inspect_thunderstore_startup(
    target: &ResolvedManualModTarget,
) -> Result<ThunderstoreStartPlan, String> {
    validate_manual_mod_target_path(&target.target_path)?;
    management::ensure_no_pending_recovery(&target.target_path)?;
    let manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
        .map_err(|_| "Cannot verify this instance's managed Mod ownership record".to_owned())?;
    management::validate_management_manifest(&manifest)?;
    let active = manifest
        .packages
        .iter()
        .filter(|(key, package)| package.enabled && key.starts_with("thunderstore-"))
        .collect::<Vec<_>>();
    if active.len() > 128 {
        return Err("Thunderstore startup exceeds its 128-package bound".into());
    }
    let mut result = ThunderstoreStartPlan {
        references: Vec::new(),
        packages: Vec::new(),
        source_required: Vec::new(),
        snapshots: BTreeMap::new(),
        record: fingerprint_tree(&target.target_path.join(ONLINE_MOD_MANIFEST))?,
        record_path: target.target_path.join(ONLINE_MOD_MANIFEST),
    };
    if active.is_empty() {
        return Ok(result);
    }
    let game = thunderstore_game_profile(&target.module_id)?;
    let root = target
        .target_path
        .ancestors()
        .nth(Path::new(game.target_relative).components().count())
        .ok_or("Thunderstore target has no runtime")?;
    if target.target_path != root.join(game.target_relative) {
        return Err("Thunderstore target belongs to a different runtime".into());
    }
    crate::commands::commands_mods::mod_online::validate_runtime(
        &target.module_id,
        "thunderstore",
        root,
        &target.target_path,
    )?;
    if game.loader.is_some() && runtime_loader_requires_archive(&target.module_id, root)? {
        return Err("Thunderstore startup requires this instance's verified compatible loader; install or verify it in the Mod page".into());
    }
    result.snapshots.insert(
        target.target_path.clone(),
        fingerprint_tree(&target.target_path)?,
    );
    if game.loader.is_some() {
        for relative in ["BepInEx/config", "BepInEx/patchers"] {
            let path = root.join(relative);
            result
                .snapshots
                .insert(path.clone(), fingerprint_tree(&path)?);
        }
    }
    let mut roots = Vec::new();
    for (key, package) in &active {
        let project = thunderstore_project(key)?;
        let path = target.target_path.join(&package.target_name);
        let current = fingerprint_tree(&path)?;
        management::validate_exclusive_payload(package, &path)?;
        management::verify_package_files(package, &path)?;
        for (relative, hash) in &package.files {
            if (!protected_operator_mod_file(relative)
                || relative.eq_ignore_ascii_case("ModManifest.json"))
                && current.get(relative) != Some(hash)
            {
                return Err("A managed Thunderstore package has missing or changed payload/identity metadata; its bytes were preserved".into());
            }
        }
        if target.module_id == "corekeeper" {
            crate::commands::commands_mods::mod_online::validate_corekeeper_native_payload(&path)?;
        }
        routed::verify_patchers(package, &target.target_path, None)?;
        result.snapshots.insert(path, current);
        for relative in package.routed_files.keys() {
            let path = root.join(relative);
            result
                .snapshots
                .insert(path.clone(), fingerprint_tree(&path)?);
        }
        let version = package.version.as_deref().filter(|version| {
            crate::commands::commands_mods::mod_online::valid_thunderstore_version(version)
        });
        let dependencies = package.dependencies.as_ref();
        if version.is_none() || dependencies.is_none() {
            result.source_required.push(project.into());
        }
        result.packages.push(CachedThunderstorePackage {
            project: project.into(),
            version: version.unwrap_or_default().into(),
            dependencies: dependencies.cloned().unwrap_or_default(),
        });
        match &package.thunderstore_intent {
            Some(ThunderstoreInstallIntent::Root { version }) => {
                roots.push((*key).clone());
                let (namespace, name) = project
                    .split_once('-')
                    .ok_or("Invalid Thunderstore identity")?;
                result.references.push(format!(
                    "https://thunderstore.io/package/{namespace}/{name}/{}",
                    version
                        .as_ref()
                        .map(|version| format!("{version}/"))
                        .unwrap_or_default()
                ));
            }
            None => result.source_required.push(project.into()),
            Some(ThunderstoreInstallIntent::Dependency) => {}
        }
    }
    management::verify_recorded_dependencies(
        target,
        &manifest,
        &active
            .iter()
            .map(|(key, _)| (*key).clone())
            .collect::<Vec<_>>(),
        None,
    )?;
    let mut reached = HashSet::new();
    let mut pending = roots;
    while let Some(key) = pending.pop() {
        if !reached.insert(key.clone()) {
            continue;
        }
        if reached.len() > 128 {
            return Err("Thunderstore dependency graph exceeds its bound".into());
        }
        if let Some(package) = manifest.packages.get(&key) {
            for dependency in package.dependencies.iter().flatten() {
                let (project, _) = dependencies::parse_thunderstore_dependency(dependency)?;
                pending.push(format!("thunderstore-{project}"));
            }
        }
    }
    for (key, package) in &active {
        if matches!(
            package.thunderstore_intent,
            Some(ThunderstoreInstallIntent::Dependency)
        ) && !reached.contains(*key)
        {
            result
                .source_required
                .push(thunderstore_project(key)?.into());
        }
    }
    result.source_required.sort();
    result.source_required.dedup();
    if result.references.len() > 32 {
        return Err("Thunderstore startup exceeds its 32-root-request bound".into());
    }
    Ok(result)
}

#[cfg(test)]
#[path = "commands_thunderstore_start_cache_tests.rs"]
mod tests;
