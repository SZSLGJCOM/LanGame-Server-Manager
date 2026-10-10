use std::fs;
use std::path::Path;

use app_core::valheim_world::{ValheimWorldRuleSource, ValheimWorldRules};

use crate::instance_file_patch::io::{guard_directories, is_link, read_bytes_with_limit};
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instances::effective_instance_install_root;
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

#[path = "valheim_world_codec.rs"]
mod codec;

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ReadPath {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.into()),
    }
}

pub async fn read_valheim_world_rules(
    paths: &StoragePaths,
    instance_id: &str,
    world_name: &str,
) -> Result<ValheimWorldRules, StorageError> {
    validate_world_name(world_name).map_err(|message| invalid(&paths.instances_root, message))?;
    let pool = connect_pool(paths).await?;
    let context = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != "valheim" {
            return Err(invalid(
                &record.config_dir,
                "World rules belong only to a Valheim instance.",
            ));
        }
        let install = effective_instance_install_root(&record)?;
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
        let mut connection = pool.acquire().await?;
        ensure_instance_paths_available(
            paths,
            &mut connection,
            instance_id,
            &record.summary.module_id,
            &install,
            &record.config_dir,
            &saves,
        )
        .await?;
        let schema: serde_json::Value = serde_json::from_str(
            descriptor
                .as_ref()
                .and_then(|module| module.schema_json.as_deref())
                .ok_or_else(|| {
                    invalid(
                        &record.config_dir,
                        "Valheim configuration schema is unavailable.",
                    )
                })?,
        )?;
        let property = &schema["properties"]["world_name"];
        let default_name = if property["x-lsgm-default-source"].as_str() == Some("instance_name") {
            record.summary.name.clone()
        } else {
            property["default"]
                .as_str()
                .ok_or_else(|| {
                    invalid(
                        &record.config_dir,
                        "Valheim schema has no supported world name default.",
                    )
                })?
                .to_owned()
        };
        let settings_path = record.config_dir.join("instance.json");
        if configured_world_name(&settings_path, &default_name)? != world_name {
            return Err(invalid(
                &settings_path,
                "The selected Valheim world changed. Reload its configuration.",
            ));
        }
        Ok((saves, settings_path, default_name))
    }
    .await;
    pool.close().await;
    let (saves, settings_path, default_name) = context?;
    let name = world_name.to_owned();
    let (source, version, keys) = tokio::task::spawn_blocking(move || scan_world(&saves, &name))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "reading Valheim world rules",
            message: error.to_string(),
        })??;
    if configured_world_name(&settings_path, &default_name)? != world_name {
        return Err(invalid(
            &settings_path,
            "The selected Valheim world changed while reading its rules.",
        ));
    }
    Ok(ValheimWorldRules {
        instance_id: instance_id.to_owned(),
        world_name: world_name.to_owned(),
        source,
        world_version: version,
        saved_keys: keys,
    })
}

fn configured_world_name(path: &Path, default: &str) -> Result<String, StorageError> {
    let _guards = guard_directories(
        path.parent()
            .ok_or_else(|| invalid(path, "Configuration has no parent."))?,
    )?;
    if !exists_ordinary(path)? {
        return Ok(default.to_owned());
    }
    let bytes = read_bytes_with_limit(path, 256 * 1024)?;
    let content = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes);
    let document: serde_json::Value = serde_json::from_slice(content)?;
    let settings = match document.get("settings") {
        None => return Ok(default.to_owned()),
        Some(settings) if settings.is_object() => settings,
        Some(_) => return Err(invalid(path, "Instance settings must be an object.")),
    };
    match settings.get("world_name") {
        Some(serde_json::Value::String(name)) => Ok(name.clone()),
        None => Ok(default.to_owned()),
        Some(_) => Err(invalid(
            path,
            "The committed Valheim world name is not a string.",
        )),
    }
}

fn validate_world_name(name: &str) -> Result<(), &'static str> {
    if name.trim().is_empty()
        || name.len() > 240
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|character| character.is_control() || "/\\:\"<>|?*".contains(character))
        || name.ends_with(['.', ' '])
    {
        return Err("Valheim world name must be a single ordinary filename.");
    }
    Ok(())
}

fn ordinary_directory(path: &Path) -> Result<Option<Vec<fs::File>>, StorageError> {
    let mut ancestor = path;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| invalid(path, "Save root has no existing ancestor."))?;
            }
            Err(error) => return Err(invalid(ancestor, error.to_string())),
        }
    }
    let guards = guard_directories(ancestor)?;
    Ok((ancestor == path).then_some(guards))
}

fn exists_ordinary(path: &Path) -> Result<bool, StorageError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(true),
        Ok(_) => Err(invalid(
            path,
            "World metadata and commit markers must be ordinary files.",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(invalid(path, error.to_string())),
    }
}

fn scan_world(
    root: &Path,
    world_name: &str,
) -> Result<(ValheimWorldRuleSource, Option<i32>, Vec<String>), StorageError> {
    // Dedicated-server -world calls World.GetCreateWorld, which indexes
    // SaveCollection by the disk save name (GetSaveInfo/GetChunkedSaveName),
    // not the editable display name stored inside the .fwl package.
    let directory = root.join("worlds_local");
    let Some(_guards) = ordinary_directory(&directory)? else {
        return Ok((ValheimWorldRuleSource::NewWorld, None, Vec::new()));
    };
    let chunked = directory.join(world_name);
    let target = if let Some(_chunk_guards) = ordinary_directory(&chunked)? {
        let mut latest = None;
        for (index, entry) in fs::read_dir(&chunked)
            .map_err(|error| invalid(&chunked, error.to_string()))?
            .enumerate()
        {
            if index >= 4096 {
                return Err(invalid(
                    &chunked,
                    "World save directory exceeds 4096 entries.",
                ));
            }
            let entry = entry.map_err(|error| invalid(&chunked, error.to_string()))?;
            let metadata = fs::symlink_metadata(entry.path())
                .map_err(|error| invalid(&entry.path(), error.to_string()))?;
            if is_link(&metadata) {
                return Err(invalid(
                    &entry.path(),
                    "World saves cannot contain links or reparse points.",
                ));
            }
            let name = entry.file_name();
            let Some(number) = name
                .to_str()
                .and_then(|name| name.strip_prefix("_main."))
                .and_then(|name| name.strip_suffix(".ok"))
                .and_then(|number| number.parse::<u32>().ok())
            else {
                continue;
            };
            if !metadata.is_file() {
                return Err(invalid(
                    &entry.path(),
                    "World commit marker must be a file.",
                ));
            }
            latest = Some(latest.map_or(number, |previous: u32| previous.max(number)));
        }
        let Some(number) = latest else {
            return Ok((ValheimWorldRuleSource::MissingMetadata, None, Vec::new()));
        };
        let target = chunked.join(format!("_main.{number}.fwl2"));
        if !exists_ordinary(&target)?
            || !exists_ordinary(&chunked.join(format!("_main.{number}.db2")))?
        {
            return Ok((ValheimWorldRuleSource::MissingMetadata, None, Vec::new()));
        }
        target
    } else {
        let target = directory.join(format!("{world_name}.fwl"));
        if !exists_ordinary(&target)? {
            let source = if exists_ordinary(&directory.join(format!("{world_name}.db")))? {
                ValheimWorldRuleSource::MissingMetadata
            } else {
                ValheimWorldRuleSource::NewWorld
            };
            return Ok((source, None, Vec::new()));
        }
        target
    };
    let bytes = read_bytes_with_limit(&target, 64 * 1024)?;
    let (version, keys, needs_db) =
        codec::decode_keys(&bytes).map_err(|message| invalid(&target, message))?;
    if target
        .extension()
        .is_some_and(|extension| extension == "fwl")
        && needs_db
        && !exists_ordinary(&directory.join(format!("{world_name}.db")))?
    {
        return Ok((ValheimWorldRuleSource::MissingMetadata, None, Vec::new()));
    }
    Ok((ValheimWorldRuleSource::Saved, Some(version), keys))
}

#[cfg(test)]
#[path = "valheim_world_tests.rs"]
mod tests;
