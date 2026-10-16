//! Root loader files and plugin ownership publish in the same rollback transaction.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[path = "commands_mod_loader_profiles.rs"]
mod profiles;
pub(in crate::commands) use profiles::{
    BepInExProfile, ThunderstoreGameProfile, reviewed_loader_satisfies, thunderstore_game_profile,
};

const CONFIG: &str = "doorstop_config.ini";
const BEPINEX_CONFIG: &str = "BepInEx/config/BepInEx.cfg";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LoaderReceipt {
    schema: u8,
    package: String,
    version: String,
    archive_sha256: String,
    files: BTreeMap<String, String>,
}

fn plain_file(root: &Path, relative: &str) -> Result<Option<fs::File>, String> {
    let path = root.join(relative);
    validate_manual_mod_existing_path_components(&path, false)?;
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect loader file {relative}: {error}")),
        Ok(metadata) if !metadata.is_file() || manual_mod_metadata_is_reparse(&metadata) => {
            return Err(format!("Unsafe loader file: {relative}"));
        }
        Ok(_) => {}
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1);
    }
    options
        .open(path)
        .map(Some)
        .map_err(|error| format!("Cannot read loader file {relative}: {error}"))
}

fn read_document(root: &Path, relative: &str, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let Some(file) = plain_file(root, relative)? else {
        return Ok(None);
    };
    if file.metadata().map_err(|error| error.to_string())?.len() > limit {
        return Err(format!(
            "Loader document exceeds its size limit: {relative}"
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(format!(
            "Loader document exceeds its size limit: {relative}"
        ));
    }
    Ok(Some(bytes))
}

fn fingerprint_file(mut file: fs::File, limit: u64) -> Result<(String, u64), String> {
    if file.metadata().map_err(|error| error.to_string())?.len() > limit {
        return Err("Loader file exceeds the reviewed release's size bound".into());
    }
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        bytes = bytes
            .checked_add(read as u64)
            .ok_or("Loader file size overflowed")?;
        if bytes > limit {
            return Err("Loader file exceeds the reviewed release's size bound".into());
        }
        hasher.update(&buffer[..read]);
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    ))
}

fn verify_immutable_files(root: &Path, loader: &BepInExProfile) -> Result<(), String> {
    let limit = loader
        .archive_bytes
        .checked_mul(200)
        .ok_or("Loader size accounting overflowed")?;
    for (path, expected) in &loader.files {
        let actual = plain_file(root, path)?
            .map(|file| fingerprint_file(file, limit))
            .transpose()?;
        if actual.as_ref().map(|(hash, _)| hash) != Some(expected) {
            return Err(format!(
                "Loader conflicts with the supported {}-{}: {path}. Existing files were preserved; no unknown or older loader was overwritten.",
                loader.project, loader.version
            ));
        }
    }
    Ok(())
}

fn verified_receipt(root: &Path, loader: &BepInExProfile) -> Result<bool, String> {
    let Some(bytes) = read_document(root, loader.receipt, 256 * 1024)? else {
        return Ok(false);
    };
    let receipt: LoaderReceipt = serde_json::from_slice(&bytes)
        .map_err(|_| "Loader receipt is invalid; existing files were preserved")?;
    if receipt.schema != 1
        || receipt.package != loader.project
        || receipt.version != loader.version
        || receipt.archive_sha256 != loader.archive_sha256
        || receipt.files != loader.files
    {
        return Err(
            "Loader receipt does not match the supported release; existing files were preserved"
                .into(),
        );
    }
    verify_immutable_files(root, loader)?;
    Ok(true)
}

fn verify_server(root: &Path, game: &ThunderstoreGameProfile) -> Result<(), String> {
    let mut file =
        plain_file(root, game.executable)?.ok_or("Game server executable is unavailable")?;
    let mut header = [0; 2];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    if header != *b"MZ" {
        return Err("Game server executable is not a Windows runtime binary".into());
    }
    Ok(())
}

fn verify_doorstop(bytes: &[u8], loader: &BepInExProfile) -> Result<(), String> {
    let config = std::str::from_utf8(bytes).map_err(|_| "Doorstop configuration is not UTF-8")?;
    let assembly = crate::commands::commands_mods::mod_online::doorstop_assembly(config)?;
    if !assembly
        .to_string_lossy()
        .replace('\\', "/")
        .eq_ignore_ascii_case(loader.expected_assembly)
    {
        return Err("Doorstop targets another loader; existing configuration was preserved".into());
    }
    if loader.il2cpp {
        let mut relevant = false;
        let mut paths = BTreeMap::new();
        for line in config
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with(['#', ';']))
        {
            if line.starts_with('[') && line.ends_with(']') {
                relevant = line.eq_ignore_ascii_case("[Il2Cpp]");
            } else if relevant && let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                if matches!(key.as_str(), "coreclr_path" | "corlib_dir")
                    && paths
                        .insert(key, value.trim().replace('\\', "/").to_ascii_lowercase())
                        .is_some()
                {
                    return Err("Doorstop IL2CPP runtime paths are ambiguous".into());
                }
            }
        }
        if paths.get("coreclr_path").map(String::as_str) != Some("dotnet/coreclr.dll")
            || paths.get("corlib_dir").map(String::as_str) != Some("dotnet")
        {
            return Err("Doorstop IL2CPP must use this instance's verified dotnet runtime".into());
        }
    }
    Ok(())
}

// Replace only explicit enablement; retain the operator's comments and settings.
fn enable_doorstop(bytes: &[u8], loader: &BepInExProfile) -> Result<(Vec<u8>, bool), String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Doorstop configuration is not UTF-8")?;
    if bytes.len() > 64 * 1024 {
        return Err("Doorstop configuration exceeds its size limit".into());
    }
    let mut relevant = false;
    let mut offset = 0;
    let mut value_range = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            relevant = matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "[general]" | "[unitydoorstop]"
            );
        } else if relevant
            && !trimmed.starts_with(['#', ';'])
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("enabled")
        {
            if value_range.is_some() {
                return Err("Doorstop enablement is ambiguous".into());
            }
            let value = value.trim();
            if !value.eq_ignore_ascii_case("true") && !value.eq_ignore_ascii_case("false") {
                return Err("Doorstop enablement is invalid".into());
            }
            let start = offset + key.len() + 1 + line[key.len() + 1..].len()
                - line[key.len() + 1..].trim_start().len();
            value_range = Some((
                start..start + value.len(),
                value.eq_ignore_ascii_case("false"),
            ));
        }
        offset += line.len();
    }
    let (range, changed) = value_range.ok_or("Doorstop has no explicit enablement setting")?;
    let mut enabled = text.to_string();
    if changed {
        enabled.replace_range(range, "true");
    }
    verify_doorstop(enabled.as_bytes(), loader)?;
    Ok((enabled.into_bytes(), changed))
}

pub(in crate::commands) fn runtime_loader_requires_archive(
    module_id: &str,
    root: &Path,
) -> Result<bool, String> {
    let game = thunderstore_game_profile(module_id)?;
    validate_manual_mod_target_path(root)?;
    verify_server(root, game)?;
    let Some(loader) = game.loader else {
        crate::commands::commands_mods::mod_online::validate_runtime(
            module_id,
            "thunderstore",
            root,
            &root.join(game.target_relative),
        )?;
        return Ok(false);
    };
    if !verified_receipt(root, loader)? {
        return Ok(true);
    }
    let config =
        read_document(root, CONFIG, 64 * 1024)?.ok_or("Doorstop configuration is missing")?;
    Ok(enable_doorstop(&config, loader)?.1)
}

fn stage_loader_archive(
    root: &Path,
    archive: &Path,
    transaction: &ManualModTransactionPaths,
    loader: &BepInExProfile,
) -> Result<DirectoryCopyStats, String> {
    // Keep the download pinned while creating one bounded-memory snapshot.
    let archive_parent = archive.parent().ok_or("Loader archive has no parent")?;
    let archive_name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid loader archive filename")?;
    let _source_pin =
        plain_file(archive_parent, archive_name)?.ok_or("Loader archive disappeared")?;
    if _source_pin
        .metadata()
        .map_err(|error| error.to_string())?
        .len()
        != loader.archive_bytes
    {
        return Err("Loader archive size does not match the pinned release".into());
    }
    let verified_archive = transaction.root.join("verified-loader.zip");
    copy_mod_payload(archive, &verified_archive)?;
    let verified = plain_file(&transaction.root, "verified-loader.zip")?
        .ok_or("Loader snapshot disappeared")?;
    let (hash, bytes) = fingerprint_file(
        verified.try_clone().map_err(|error| error.to_string())?,
        loader.archive_bytes,
    )?;
    if bytes != loader.archive_bytes || hash != loader.archive_sha256 {
        return Err("Loader archive checksum does not match the pinned release".into());
    }
    let unpacked = transaction.root.join("loader-archive");
    fs::create_dir(&unpacked).map_err(|error| error.to_string())?;
    extract_zip_archive_to_directory(
        &verified_archive,
        &unpacked,
        &transaction.partials,
        &mut ManualModStagingRegistry::default(),
        ManualModArchiveLayout::Preserve,
    )?;
    let payload = unpacked.join(&loader.prefix);
    let files = online_packages::fingerprint_tree(&payload)?;
    for path in files.keys() {
        if path != CONFIG
            && path != BEPINEX_CONFIG
            && !loader.files.contains_key(path)
            && !loader.ignored_payload.contains(&path.as_str())
        {
            return Err(format!("Unexpected loader payload: {path}"));
        }
    }
    if loader
        .files
        .iter()
        .any(|(path, hash)| files.get(path) != Some(hash))
    {
        return Err("Loader payload does not match the reviewed immutable file manifest".into());
    }
    let payload_config = read_document(&payload, CONFIG, 64 * 1024)?
        .ok_or("Loader archive has no Doorstop configuration")?;
    let payload_bepinex = read_document(&payload, BEPINEX_CONFIG, 64 * 1024)?
        .ok_or("Loader archive has no BepInEx configuration")?;
    enable_doorstop(&payload_config, loader)?;
    let has_loader = ["winhttp.dll", CONFIG, ".doorstop_version", "BepInEx/core"]
        .iter()
        .any(|path| root.join(path).exists())
        || (loader.il2cpp && root.join("dotnet").exists());
    if has_loader {
        verify_immutable_files(root, loader)?;
    }
    let mut stats = DirectoryCopyStats::default();
    if !has_loader {
        for (path, expected) in &loader.files {
            if let Some(existing) = plain_file(root, path)? {
                if fingerprint_file(existing, loader.archive_bytes * 200)?.0 != *expected {
                    return Err(format!(
                        "Loader destination {path} conflicts with an existing file; existing files were preserved"
                    ));
                }
                continue;
            }
            let destination = transaction.staging.join(path);
            fs::create_dir_all(destination.parent().ok_or("Loader file has no parent")?)
                .map_err(|error| error.to_string())?;
            let bytes = copy_mod_payload(&payload.join(path), &destination)?;
            stats.file_count += 1;
            stats.total_bytes += bytes;
        }
    }
    let original = read_document(root, CONFIG, 64 * 1024)?;
    let (enabled, changed) =
        enable_doorstop(original.as_deref().unwrap_or(&payload_config), loader)?;
    if original.is_none() || changed {
        stage_bytes(transaction, CONFIG, &enabled, &mut stats)?;
    }
    if read_document(root, BEPINEX_CONFIG, 64 * 1024)?.is_none() {
        stage_bytes(transaction, BEPINEX_CONFIG, &payload_bepinex, &mut stats)?;
    }
    let receipt = serde_json::to_vec(&LoaderReceipt {
        schema: 1,
        package: loader.project.clone(),
        version: loader.version.clone(),
        archive_sha256: loader.archive_sha256.clone(),
        files: loader.files.clone(),
    })
    .map_err(|error| error.to_string())?;
    stage_bytes(transaction, loader.receipt, &receipt, &mut stats)?;
    drop(verified);
    Ok(stats)
}

fn stage_bytes(
    transaction: &ManualModTransactionPaths,
    path: &str,
    bytes: &[u8],
    stats: &mut DirectoryCopyStats,
) -> Result<(), String> {
    let destination = transaction.staging.join(path);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut budget = ModSpaceBudget::new(&destination)?;
    budget.before_write(bytes.len())?;
    fs::write(destination, bytes).map_err(|error| error.to_string())?;
    stats.file_count += 1;
    stats.total_bytes += bytes.len() as u64;
    Ok(())
}

pub(in crate::commands) fn stage_thunderstore_online_mod_sources(
    target: ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    archive: Option<&Path>,
) -> Result<ManualModStageResult, String> {
    let game = thunderstore_game_profile(&target.module_id)?;
    let root = runtime_root(&target, game)?;
    let Some(loader) = game.loader else {
        if archive.is_some() {
            return Err("Native Mod runtimes cannot install a BepInEx archive".into());
        }
        crate::commands::commands_mods::mod_online::validate_runtime(
            game.module_id,
            "thunderstore",
            &root,
            &target.target_path,
        )?;
        for source in sources {
            crate::commands::commands_mods::mod_online::validate_corekeeper_native_payload(
                &source.path,
            )?;
        }
        return online_packages::stage_online_mod_sources(target, sources);
    };
    stage_with_profile(target, sources, archive, game, loader)
}

fn runtime_root(
    target: &ResolvedManualModTarget,
    game: &ThunderstoreGameProfile,
) -> Result<PathBuf, String> {
    let depth = Path::new(game.target_relative).components().count();
    let root = target
        .target_path
        .ancestors()
        .nth(depth)
        .ok_or("Mod target has no runtime root")?;
    if target.module_id != game.module_id || target.target_path != root.join(game.target_relative) {
        return Err("Thunderstore target does not match this game's verified runtime".into());
    }
    Ok(root.to_path_buf())
}

fn stage_with_profile(
    target: ResolvedManualModTarget,
    sources: &[DownloadedManualModSource],
    archive: Option<&Path>,
    game: &ThunderstoreGameProfile,
    loader: &BepInExProfile,
) -> Result<ManualModStageResult, String> {
    let root = runtime_root(&target, game)?;
    let root_lock = manual_mod_target_lock(&root)?;
    let _root_guard = root_lock
        .lock()
        .map_err(|_| "Runtime Mod target lock poisoned")?;
    let plugin_lock = manual_mod_target_lock(&target.target_path)?;
    let _plugin_guard = plugin_lock
        .lock()
        .map_err(|_| "Plugin Mod target lock poisoned")?;
    validate_manual_mod_target_path(&root)?;
    verify_server(&root, game)?;
    validate_manual_mod_target_path(&target.target_path)?;
    validate_manual_mod_target_tree(&target.target_path)?;
    verified_receipt(&root, loader)?;
    let transaction = create_manual_mod_transaction(&root)?;
    let prepared = (|| {
        let loader_stats = if let Some(archive) = archive {
            stage_loader_archive(&root, archive, &transaction, loader)?
        } else {
            if !verified_receipt(&root, loader)? {
                return Err("Loader requires its pinned archive before installing plugins".into());
            }
            let config = read_document(&root, CONFIG, 64 * 1024)?
                .ok_or("Doorstop configuration is missing")?;
            if enable_doorstop(&config, loader)?.1 {
                return Err("Disabled loader requires verified preparation".into());
            }
            DirectoryCopyStats::default()
        };
        let plugin_staging = transaction.staging.join(game.target_relative);
        fs::create_dir_all(&plugin_staging).map_err(|error| error.to_string())?;
        let plugin_transaction = ManualModTransactionPaths {
            root: transaction.root.clone(),
            staging: plugin_staging,
            partials: transaction.partials.clone(),
            backup: transaction.backup.clone(),
        };
        let (mut items, files, bytes, replace_roots) = if sources.is_empty() {
            (Vec::new(), 0, 0, HashSet::new())
        } else {
            online_packages::prepare_thunderstore_mod_transaction(
                &target,
                sources,
                &transaction,
                &[loader.identity()],
            )?
        };
        let names = staged_manual_mod_root_names(&plugin_transaction)?;
        if loader_stats.file_count > 0 {
            items.push(ManualModStageItem {
                source_path: archive
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                target_path: root.to_string_lossy().into_owned(),
                status: "installed".into(),
                message: None,
                file_count: loader_stats.file_count,
                total_bytes: loader_stats.total_bytes,
            });
        }
        Ok::<_, String>((
            items,
            files + loader_stats.file_count,
            bytes + loader_stats.total_bytes,
            replace_roots,
            names,
        ))
    })();
    let (items, copied_file_count, copied_total_bytes, replace_roots, affected_root_names) =
        match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err(manual_mod_staging_failure(error, &transaction, true)),
        };
    if let Err(failure) = commit_manual_mod_transaction(&transaction, &root, &replace_roots) {
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
        affected_root_names,
        items: items
            .into_iter()
            .map(|mut item| {
                if let Some(warning) = &warning {
                    item.message = Some(match item.message.take() {
                        Some(message) => format!("{message}; {warning}"),
                        None => warning.clone(),
                    });
                }
                item
            })
            .collect(),
        copied_file_count,
        copied_total_bytes,
    })
}

#[cfg(test)]
#[path = "commands_mod_bepinex_loader_tests.rs"]
mod tests;
