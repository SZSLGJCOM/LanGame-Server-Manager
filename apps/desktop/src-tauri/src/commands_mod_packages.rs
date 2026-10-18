use super::*;
use sha2::Digest;
use std::collections::BTreeMap;

pub(in crate::commands) const ONLINE_MOD_MANIFEST: &str = ".langame-online-mods.json";
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PACKAGE_FILES: usize = 20_000;
const MAX_PACKAGE_DEPTH: usize = 64;

#[path = "commands_thunderstore_start_intent.rs"]
mod thunderstore_intent;
pub(in crate::commands) use thunderstore_intent::{
    ThunderstoreInstallIntent, root_thunderstore_intents_with_policy, thunderstore_project,
};

#[path = "commands_thunderstore_start_cache.rs"]
mod thunderstore_start_cache;
pub(in crate::commands) use thunderstore_start_cache::{
    CachedThunderstorePackage, ThunderstoreStartPlan, inspect_thunderstore_startup,
};

#[path = "commands_mod_dependencies.rs"]
mod dependencies;
use dependencies::verify_dependencies;

#[path = "commands_mod_management.rs"]
mod management;
pub(in crate::commands) use management::{decorate_inventory, manage_package, recover_management};

#[path = "commands_mod_routed_packages.rs"]
mod routed;

/// Startup replacement uses the same operator-data boundary as managed Mod
/// updates. Package identity metadata must retain its strict payload validation.
pub(in crate::commands) fn protected_operator_mod_file(relative: &str) -> bool {
    management::protected_file(relative)
        && !routed::is_metadata(relative)
        && !["Info.json", "manifest.json", "ModInfo.xml"]
            .iter()
            .any(|name| relative.eq_ignore_ascii_case(name))
}

pub(in crate::commands) fn untracked_loadable_mod_file(relative: &str) -> bool {
    management::loadable_file(relative) && !protected_operator_mod_file(relative)
}
pub(super) use routed::{
    finalize_thunderstore_transaction, prepare_local_thunderstore_transaction,
    prepare_thunderstore_mod_transaction,
};

#[path = "commands_mod_packages_import.rs"]
mod local_imports;
pub(super) use local_imports::{
    LocalImportMetadata, local_import_metadata, manages_local_imports, prepare_local_mod_ownership,
};

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPackages {
    packages: BTreeMap<String, InstalledPackage>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledPackage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    thunderstore_intent: Option<ThunderstoreInstallIntent>,
    target_name: String,
    files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    routed_files: BTreeMap<String, String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    dependencies: Option<Vec<String>>,
    #[serde(default = "enabled_by_default")]
    enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

/// Package identity and its ownership record are published in the same rollback
/// transaction. Only bytes matching the previous record can be replaced/removed.
pub(in crate::commands) fn stage_online_mod_sources(
    target: ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
) -> Result<ManualModStageResult, String> {
    stage_online_mod_sources_checked(target, sources, None, None)
}

pub(super) fn stage_online_mod_sources_for_startup(
    target: ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    proof: &ThunderstoreStartPlan,
    cancellation: &app_steamcmd::InstallCancellation,
) -> Result<ManualModStageResult, String> {
    let before = || {
        if cancellation.is_cancelled() {
            return Err("Thunderstore Mod startup was cancelled before publication".into());
        }
        proof.verify_unchanged(&target)
    };
    let after = |backup: &Path, roots: &HashSet<PathBuf>| {
        proof.verify_backup(backup, &target.target_path, roots)
    };
    stage_online_mod_sources_checked(target.clone(), sources, Some(&before), Some(&after))
}

type PackageBackupCheck<'a> = dyn Fn(&Path, &HashSet<PathBuf>) -> Result<(), String> + 'a;

fn stage_online_mod_sources_checked(
    target: ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    before: Option<&dyn Fn() -> Result<(), String>>,
    after: Option<&PackageBackupCheck<'_>>,
) -> Result<ManualModStageResult, String> {
    validate_game_mod_mutation(&target.module_id)?;
    if sources.is_empty() {
        return Err("no online mod packages were provided".into());
    }
    let target_lock = manual_mod_target_lock(&target.target_path)?;
    let _guard = target_lock
        .lock()
        .map_err(|_| "manual mod target lock poisoned")?;
    validate_manual_mod_target_path(&target.target_path)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    let transaction = create_manual_mod_transaction(&target.target_path)?;
    let prepared = if before.is_some() {
        prepare_online_mod_transaction_with_policy(
            &target,
            sources,
            &transaction,
            &[],
            PackageUserDataPolicy::PreserveVerifiedStartup,
        )
    } else {
        prepare_online_mod_transaction(&target, sources, &transaction, &[])
    };
    let (items, copied_file_count, copied_total_bytes, replace_roots) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return Err(manual_mod_staging_failure(error, &transaction, true)),
    };
    let affected_root_names = staged_manual_mod_root_names(&transaction)
        .map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    if let Some(check) = before {
        check().map_err(|error| manual_mod_staging_failure(error, &transaction, true))?;
    }
    if let Err(failure) =
        commit_manual_mod_transaction(&transaction, &target.target_path, &replace_roots)
    {
        return Err(manual_mod_staging_failure(
            failure.message,
            &transaction,
            failure.rollback_complete,
        ));
    }
    if let Some(check) = after {
        check(&transaction.backup, &replace_roots)
            .map_err(|error| manual_mod_staging_failure(error, &transaction, false))?;
    }
    let cleanup_warning = cleanup_manual_mod_transaction(&transaction).err();
    Ok(ManualModStageResult {
        instance_id: target.instance_id,
        module_id: target.module_id,
        source_label: target.source_label,
        target_label: target.target_label,
        target_path: target.target_path.to_string_lossy().into_owned(),
        affected_root_names,
        items: items
            .into_iter()
            .map(|mut item| {
                item.message = cleanup_warning.clone();
                item
            })
            .collect(),
        copied_file_count,
        copied_total_bytes,
    })
}

pub(super) type PreparedPackages = (Vec<ManualModStageItem>, usize, u64, HashSet<PathBuf>);

pub(super) fn prepare_online_mod_transaction(
    target: &ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    transaction: &ManualModTransactionPaths,
    runtime_dependencies: &[OnlineModIdentity],
) -> Result<PreparedPackages, String> {
    prepare_online_mod_transaction_with_policy(
        target,
        sources,
        transaction,
        runtime_dependencies,
        PackageUserDataPolicy::RejectConflicts,
    )
}

/// Ordinary installs reject configuration collisions. Startup has a full
/// preimage proof plus precommit/moved-original guards, so it may retain mutable
/// operator bytes while replacing only the verified package payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PackageUserDataPolicy {
    RejectConflicts,
    PreserveVerifiedStartup,
}

fn prepare_online_mod_transaction_with_policy(
    target: &ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    transaction: &ManualModTransactionPaths,
    runtime_dependencies: &[OnlineModIdentity],
    policy: PackageUserDataPolicy,
) -> Result<PreparedPackages, String> {
    management::recover_management(target)?;
    reject_untracked_downloads(&target.target_path, sources)?;
    let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))?;
    verify_dependencies(
        &target.target_path,
        &manifest,
        &sources
            .iter()
            .map(|source| source.identity.clone())
            .collect::<Vec<_>>(),
        runtime_dependencies,
    )?;
    prepare_packages(target, sources, transaction, &mut manifest, policy)
}

fn prepare_packages(
    target: &ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    transaction: &ManualModTransactionPaths,
    manifest: &mut InstalledPackages,
    policy: PackageUserDataPolicy,
) -> Result<PreparedPackages, String> {
    let mut registry = ManualModStagingRegistry::default();
    let mut replace_roots = HashSet::new();
    let mut items = Vec::new();
    let mut file_count = 0;
    let mut total_bytes = 0;
    for source in sources {
        let metadata = validate_manual_mod_source_filesystem_entry(&source.path)?;
        if (source.identity.provider == "modrinth" && !metadata.is_file())
            || (source.identity.provider == "thunderstore" && !metadata.is_dir())
        {
            return Err("online mod payload does not match its provider format".into());
        }
        validate_manual_mod_source_destination(&source.path, &target.target_path)?;
        let key =
            package_key(&source.identity).map_err(|error| format!("{}: {error}", source.label))?;
        if manifest
            .packages
            .get(&key)
            .is_some_and(|package| !package.enabled)
        {
            return Err(format!(
                "mod package `{key}` is disabled; enable it before reinstalling or updating so a duplicate cannot become active"
            ));
        }
        let target_name = if metadata.is_dir() {
            key.clone()
        } else {
            let extension = source
                .path
                .extension()
                .and_then(|value| value.to_str())
                .ok_or("online mod package has no file extension")?;
            if !extension.eq_ignore_ascii_case("jar") {
                return Err("online Modrinth installation requires a JAR package".into());
            }
            format!("{key}.jar")
        };
        if !replace_roots.insert(PathBuf::from(&target_name)) {
            return Err(format!(
                "the online request includes package `{key}` more than once"
            ));
        }
        let staged = stage_manual_mod_source_to_transaction(
            &source.path,
            transaction,
            &mut registry,
            ManualModArchiveLayout::Preserve,
            Some(std::ffi::OsStr::new(&target_name)),
        )?;
        let staged_path = transaction.staging.join(&target_name);
        if staged.stats.file_count == 0 {
            return Err("online mod package contains no files".into());
        }
        let incoming = fingerprint_tree(&staged_path)?;
        let destination = target.target_path.join(&target_name);
        if let Some(previous) = manifest.packages.get(&key) {
            if previous.target_name != target_name {
                return Err(format!(
                    "online package `{key}` changed its installation layout; existing files were preserved"
                ));
            }
            preserve_user_files_with_policy(
                &destination,
                &staged_path,
                previous,
                &incoming,
                policy,
            )?;
        } else if destination.exists() {
            return Err(format!(
                "online package destination {} is not owned by the manager; existing files were preserved",
                destination.display()
            ));
        }
        let intent = thunderstore_intent::retained_intent(
            manifest.packages.get(&key),
            source.identity.thunderstore_intent.as_ref(),
            &source.identity.version,
        )?;
        manifest.packages.insert(
            key,
            InstalledPackage {
                thunderstore_intent: intent,
                target_name: target_name.clone(),
                files: incoming,
                routed_files: BTreeMap::new(),
                version: Some(source.identity.version.clone()),
                dependencies: Some(source.identity.dependencies.clone()),
                enabled: true,
            },
        );
        file_count += staged.stats.file_count;
        total_bytes += staged.stats.total_bytes;
        items.push(ManualModStageItem {
            source_path: source.path.to_string_lossy().into_owned(),
            target_path: destination.to_string_lossy().into_owned(),
            status: "installed".into(),
            message: None,
            file_count: staged.stats.file_count,
            total_bytes: staged.stats.total_bytes,
        });
    }
    management::validate_management_manifest(manifest)?;
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("online mod ownership record exceeds its size limit".into());
    }
    fs::write(transaction.staging.join(ONLINE_MOD_MANIFEST), bytes)
        .map_err(|error| format!("failed to stage online mod ownership record: {error}"))?;
    Ok((items, file_count, total_bytes, replace_roots))
}

fn package_key(identity: &OnlineModIdentity) -> Result<String, String> {
    if !matches!(identity.provider.as_str(), "modrinth" | "thunderstore")
        || identity.project.is_empty()
        || identity.project.len() > 128
        || !identity
            .project
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("online mod metadata contains an invalid package identity".into());
    }
    // Modrinth's canonical IDs are case-sensitive. Do not fold IDs or use a
    // mutable display name/version as an ownership boundary.
    Ok(format!("{}-{}", identity.provider, identity.project))
}

fn read_manifest(path: &Path) -> Result<InstalledPackages, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(InstalledPackages::default());
        }
        Err(error) => {
            return Err(format!(
                "failed to inspect online mod ownership record: {error}"
            ));
        }
        Ok(metadata)
            if !metadata.is_file()
                || manual_mod_metadata_is_reparse(&metadata)
                || metadata.len() > MAX_MANIFEST_BYTES =>
        {
            return Err("invalid online mod ownership record".into());
        }
        Ok(_) => {}
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("failed to read online mod ownership record: {error}"))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("online mod ownership record exceeds its size limit".into());
    }
    let manifest: InstalledPackages = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid online mod ownership record: {error}"))?;
    let mut targets = HashSet::new();
    for (key, package) in &manifest.packages {
        if key.is_empty() || key.len() > 512 || !targets.insert(package.target_name.to_lowercase())
        {
            return Err("invalid or duplicate package in online mod ownership record".into());
        }
        validate_manual_mod_windows_component(std::ffi::OsStr::new(&package.target_name))?;
        let mut components = Path::new(&package.target_name).components();
        if package.target_name.contains(['/', '\\'])
            || !matches!(components.next(), Some(std::path::Component::Normal(_)))
            || components.next().is_some()
        {
            return Err("invalid target in online mod ownership record".into());
        }
        if package.files.len() > MAX_PACKAGE_FILES {
            return Err("online mod ownership record exceeds its file limit".into());
        }
        let mut paths = ManualModStagingRegistry::default();
        for (relative, hash) in &package.files {
            if !relative.is_empty() {
                let normalized = normalize_manual_mod_relative_path(Path::new(relative))?;
                if normalized.to_string_lossy().replace('\\', "/") != *relative {
                    return Err("invalid path in online mod ownership record".into());
                }
                paths.register_file(&normalized)?;
            } else if package.files.len() != 1 {
                return Err(
                    "file and directory paths conflict in online mod ownership record".into(),
                );
            }
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("invalid hash in online mod ownership record".into());
            }
        }
    }
    management::validate_management_manifest(&manifest)?;
    Ok(manifest)
}

pub(super) fn fingerprint_tree(path: &Path) -> Result<BTreeMap<String, String>, String> {
    fingerprint_tree_with_limits(path, MAX_PACKAGE_FILES, MAX_PACKAGE_DEPTH, u64::MAX)
}

fn fingerprint_tree_with_limits(
    path: &Path,
    max_entries: usize,
    max_depth: usize,
    max_bytes: u64,
) -> Result<BTreeMap<String, String>, String> {
    let mut result = BTreeMap::new();
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(result),
        Err(error) => return Err(format!("failed to inspect mod package: {error}")),
        Ok(_) => {}
    }
    let mut visited = 0usize;
    let mut declared_bytes = 0u64;
    let mut received_bytes = 0u64;
    let mut pending = vec![(path.to_path_buf(), String::new(), 0usize)];
    while let Some((current, relative, depth)) = pending.pop() {
        visited += 1;
        if visited > max_entries || depth > max_depth {
            return Err("online mod package exceeds its entry or depth limit".into());
        }
        let metadata = validate_manual_mod_source_filesystem_entry(&current)?;
        if metadata.is_dir() {
            for entry in fs::read_dir(&current).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "mod path is not Unicode")?;
                validate_manual_mod_windows_component(std::ffi::OsStr::new(&name))?;
                let nested = if relative.is_empty() {
                    name
                } else {
                    format!("{relative}/{name}")
                };
                pending.push((entry.path(), nested, depth + 1));
                if pending.len() + visited > max_entries {
                    return Err("online mod package exceeds its entry limit".into());
                }
            }
        } else {
            declared_bytes = declared_bytes
                .checked_add(metadata.len())
                .ok_or("online mod package byte count overflowed")?;
            if declared_bytes > max_bytes {
                return Err("online mod package exceeds its byte limit".into());
            }
            let mut file = fs::File::open(&current).map_err(|error| error.to_string())?;
            let mut hasher = sha2::Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
                if count == 0 {
                    break;
                }
                received_bytes = received_bytes
                    .checked_add(count as u64)
                    .ok_or("online mod package byte count overflowed")?;
                if received_bytes > max_bytes {
                    return Err("online mod package exceeds its byte limit while reading".into());
                }
                hasher.update(&buffer[..count]);
            }
            let digest = hasher.finalize();
            let hex = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            result.insert(relative, hex);
        }
    }
    Ok(result)
}

fn preserve_user_files_with_policy(
    destination: &Path,
    staging: &Path,
    previous: &InstalledPackage,
    incoming: &BTreeMap<String, String>,
    policy: PackageUserDataPolicy,
) -> Result<(), String> {
    let current = fingerprint_tree(destination)?;
    for (relative, hash) in &current {
        let candidate = if relative.is_empty() {
            previous.target_name.as_str()
        } else {
            relative.as_str()
        };
        let protected = if policy == PackageUserDataPolicy::PreserveVerifiedStartup {
            protected_operator_mod_file(candidate)
                && !candidate.eq_ignore_ascii_case("ModManifest.json")
        } else {
            management::protected_file(candidate) && !routed::is_metadata(candidate)
        };
        let preserve = if policy == PackageUserDataPolicy::PreserveVerifiedStartup && protected {
            true
        } else if let Some(original) = previous.files.get(relative) {
            if original != hash {
                return Err(format!(
                    "online mod file {} was modified outside the manager; update stopped and existing files were preserved",
                    destination.join(relative).display()
                ));
            }
            if protected {
                if incoming
                    .get(relative)
                    .is_some_and(|incoming_hash| incoming_hash != hash)
                {
                    return Err(format!(
                        "online mod update would overwrite configuration {}; existing files were preserved",
                        destination.join(relative).display()
                    ));
                }
                !incoming.contains_key(relative)
            } else {
                false
            }
        } else {
            if incoming.contains_key(relative) || relative.is_empty() {
                return Err(format!(
                    "online mod update conflicts with user file {}; existing files were preserved",
                    destination.join(relative).display()
                ));
            }
            true
        };
        if preserve {
            let output = staging.join(relative);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("failed to preserve user mod file: {error}"))?;
            }
            if output
                .try_exists()
                .map_err(|error| format!("failed to inspect staged user file: {error}"))?
            {
                let metadata = validate_manual_mod_source_filesystem_entry(&output)?;
                if !metadata.is_file() {
                    return Err("Operator data conflicts with a staged directory; existing bytes were preserved".into());
                }
                if metadata.permissions().readonly() {
                    #[cfg(windows)]
                    {
                        use std::os::windows::ffi::OsStrExt;
                        use std::os::windows::fs::MetadataExt;
                        use windows_sys::Win32::Storage::FileSystem::{
                            FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
                        };

                        let attributes = metadata.file_attributes() & !FILE_ATTRIBUTE_READONLY;
                        let wide: Vec<u16> =
                            output.as_os_str().encode_wide().chain(Some(0)).collect();
                        // The staged file is private; retain its ACLs and every other
                        // attribute. The path buffer stays alive through the call.
                        if unsafe { SetFileAttributesW(wide.as_ptr(), attributes) } == 0 {
                            return Err(format!(
                                "failed to prepare staged user file: {}",
                                std::io::Error::last_os_error()
                            ));
                        }
                    }
                    #[cfg(not(windows))]
                    {
                        let mut permissions = metadata.permissions();
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            permissions.set_mode(permissions.mode() | 0o200);
                        }
                        fs::set_permissions(&output, permissions).map_err(|error| {
                            format!("failed to prepare staged user file: {error}")
                        })?;
                    }
                }
            }
            copy_mod_payload(&destination.join(relative), &output)
                .map_err(|error| format!("failed to preserve user mod file: {error}"))?;
        }
    }
    Ok(())
}

fn reject_untracked_downloads(
    target: &Path,
    sources: &[DownloadedManualModSource],
) -> Result<(), String> {
    if !target.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(target).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // Match only this request's known provider identities (including the
        // canonical Modrinth slug/ID), never lock unrelated package installs.
        // Old names still do not grant ownership: leave matches for review.
        let old_download = sources.iter().any(|source| {
            source.identity.historical_names.iter().any(|alias| {
                let prefix = format!(
                    "langame-mod-{}-",
                    super::super::commands_mods::sanitize_mod_site_filename(alias)
                );
                let Some(suffix) = name.get(prefix.len()..).filter(|_| {
                    name.get(..prefix.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(&prefix))
                }) else {
                    return false;
                };
                let parts = suffix.split('-').collect::<Vec<_>>();
                parts.iter().enumerate().any(|(index, part)| {
                    index > 0
                        && part.len() == 32
                        && part.bytes().all(|byte| byte.is_ascii_hexdigit())
                        && if source.identity.provider == "thunderstore" {
                            index + 1 == parts.len()
                        } else {
                            index + 1 < parts.len() && name.to_ascii_lowercase().ends_with(".jar")
                        }
                })
            })
        });
        if old_download {
            return Err(format!(
                "untracked previous copy of this online mod package at {}; review and move it out of the mod loading directory before installing this package, so duplicate versions cannot be loaded; no existing files were changed",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_mod_packages_tests.rs"]
mod tests;
