use std::fs;
use std::path::Path;

use super::StorageError;
use crate::atomic_file::compare_and_swap_file_atomically;
use crate::instance_file_patch::io::{guard_directories, read_bytes_with_limit};
use crate::instance_file_patch::sha256;
use crate::private_runtime_refresh::hash_file;
use crate::templates::templates_materialize::package_staging::copy_package_file;

const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 24;

#[derive(Clone, Copy)]
enum FilePolicy {
    Program,
    PublisherData,
    Seed,
    CampaignPresets,
}

// The published v1.13.5.0 presets, with LF or Windows CRLF, and the exact
// Windows replacement from build 26920110. Operator edits have other hashes.
const PRIOR_CAMPAIGN_PRESETS: [&str; 2] = [
    "46a2573ca53e83dbb660130eb770c37b7282279aeeebe5d3946d6209555d6dec",
    "bf4b7dddae0c7970419769e467695b1f0a0df4d43bbae45fc1e76d1f675caec4",
];
const UPDATED_CAMPAIGN_PRESETS: &str =
    "cb1e3a5780f7af989b00f20babac35addb48340b55df59f5fdb0a3e6beb56801";

/// Barotrauma resets cwd to its entry assembly directory. The actual executable,
/// dependencies and content must therefore live beside the instance-owned XML.
pub(super) fn materialize_runtime(install: &Path, instance: &Path) -> Result<(), StorageError> {
    if !install.join("DedicatedServer.exe").is_file() {
        return Ok(());
    }
    let _package_guard = guard_directories(install)?;
    let existing_parent = instance
        .ancestors()
        .find(|path| path.exists())
        .unwrap_or(instance);
    let _parent_guard = guard_directories(existing_parent)?;
    fs::create_dir_all(instance).map_err(|error| failure(instance, error))?;
    let _instance_guard = guard_directories(instance)?;
    let source_root = fs::canonicalize(install).map_err(|error| failure(install, error))?;
    let target_root = fs::canonicalize(instance).map_err(|error| failure(instance, error))?;
    if target_root.starts_with(&source_root) || source_root.starts_with(&target_root) {
        return Err(failure(
            instance,
            "The Barotrauma instance and package roots must be separate.",
        ));
    }
    let mut files = 0;
    for entry in fs::read_dir(install).map_err(|error| failure(install, error))? {
        let entry = entry.map_err(|error| failure(install, error))?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let lower = name_text.to_ascii_lowercase();
        if lower.ends_with(".dll")
            || lower.ends_with(".exe")
            || lower.ends_with(".deps.json")
            || lower.ends_with(".runtimeconfig.json")
            || lower == "steam_appid.txt"
        {
            copy_file(
                &entry.path(),
                &instance.join(name),
                FilePolicy::Program,
                &mut files,
            )?;
        }
    }
    copy_directory(
        &install.join("Content"),
        &instance.join("Content"),
        0,
        &mut files,
    )?;
    // Only named shipped Data files belong in the instance. A shared package
    // may also contain a previous server's permissions, bans or saves.
    let source_data = install.join("Data");
    let target_data = instance.join("Data");
    let _source_guard = guard_directories(&source_data)?;
    fs::create_dir_all(&target_data).map_err(|error| failure(&target_data, error))?;
    let _data_guard = guard_directories(&target_data)?;
    for entry in fs::read_dir(&source_data).map_err(|error| failure(&source_data, error))? {
        let entry = entry.map_err(|error| failure(&source_data, error))?;
        let name = entry.file_name();
        let Some(name_text) = name.to_str() else {
            continue;
        };
        let lower = name_text.to_ascii_lowercase();
        let policy = match lower.as_str() {
            "dlc_catalog.xml" | "publickey.dat" => FilePolicy::PublisherData,
            "campaignsettings.xml" => FilePolicy::CampaignPresets,
            "forbiddenwordlist.txt" | "karmasettings.xml" | "languageoptions.xml" => {
                FilePolicy::Seed
            }
            name if name.starts_with("permissionpresets") && name.ends_with(".xml") => {
                FilePolicy::Seed
            }
            _ => continue,
        };
        copy_file(&entry.path(), &target_data.join(name), policy, &mut files)?;
    }
    Ok(())
}

fn copy_directory(
    source: &Path,
    target: &Path,
    depth: usize,
    files: &mut usize,
) -> Result<(), StorageError> {
    if depth >= MAX_DEPTH {
        return Err(failure(
            source,
            "Package directory depth exceeds the limit.",
        ));
    }
    reject_link(source)?;
    if target.exists() {
        reject_link(target)?;
    }
    fs::create_dir_all(target).map_err(|error| failure(target, error))?;
    for entry in fs::read_dir(source).map_err(|error| failure(source, error))? {
        let entry = entry.map_err(|error| failure(source, error))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|error| failure(&source_path, error))?;
        if kind.is_dir() {
            copy_directory(&source_path, &target_path, depth + 1, files)?;
        } else {
            copy_file(&source_path, &target_path, FilePolicy::Program, files)?;
        }
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    target: &Path,
    policy: FilePolicy,
    files: &mut usize,
) -> Result<(), StorageError> {
    *files += 1;
    if *files > MAX_FILES {
        return Err(failure(source, "Package file count exceeds the limit."));
    }
    let source_metadata = reject_link(source)?;
    if !source_metadata.is_file() {
        return Err(failure(source, "Package entry is not a regular file."));
    }
    match fs::symlink_metadata(target) {
        Ok(_) => {
            let existing = reject_link(target)?;
            if !existing.is_file() {
                return Err(failure(target, "Instance entry is not a regular file."));
            }
            let same_length = existing.len() == source_metadata.len();
            match policy {
                FilePolicy::Seed => return Ok(()),
                FilePolicy::CampaignPresets => {
                    return upgrade_stock_presets(
                        source,
                        target,
                        source_metadata.len(),
                        existing.len(),
                    );
                }
                FilePolicy::PublisherData
                    if same_length && hash_file(source, None)? == hash_file(target, None)? =>
                {
                    return Ok(());
                }
                FilePolicy::Program
                    if same_length
                        && existing
                            .modified()
                            .ok()
                            .zip(source_metadata.modified().ok())
                            .is_some_and(|(target, source)| target == source) =>
                {
                    return Ok(());
                }
                _ => {}
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure(target, error)),
    }
    copy_package_file("barotrauma", source, target)
}

fn upgrade_stock_presets(
    source: &Path,
    target: &Path,
    source_size: u64,
    target_size: u64,
) -> Result<(), StorageError> {
    if source_size != 2840 || !matches!(target_size, 2755 | 2840) {
        return Ok(());
    }
    let original = read_bytes_with_limit(target, 2840)?;
    if !PRIOR_CAMPAIGN_PRESETS.contains(&sha256(&original).as_str()) {
        return Ok(());
    }
    let replacement = read_bytes_with_limit(source, 2840)?;
    if sha256(&replacement) == UPDATED_CAMPAIGN_PRESETS {
        // A concurrent edit that changed the expected bytes wins.
        compare_and_swap_file_atomically(target, &original, &replacement)
            .map_err(|error| failure(target, error))?;
    }
    Ok(())
}

fn reject_link(path: &Path) -> Result<fs::Metadata, StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| failure(path, error))?;
    let link = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        link || metadata.file_attributes() & 0x400 != 0
    };
    if link {
        return Err(failure(
            path,
            "Linked package or instance entries cannot be materialized.",
        ));
    }
    Ok(metadata)
}

fn failure(path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: "barotrauma".into(),
        path: path.to_owned(),
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "barotrauma_tests.rs"]
mod tests;
