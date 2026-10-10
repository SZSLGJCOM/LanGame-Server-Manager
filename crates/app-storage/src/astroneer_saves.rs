use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use app_core::astroneer_saves::{AstroneerSaveCatalog, AstroneerSaveEntry};

use crate::instance_file_patch::io::{guard_directories, is_link};
use crate::instance_isolation::ensure_instance_paths_available;
use crate::instances::effective_instance_install_root;
use crate::save_paths::{effective_instance_saves_dir, load_module_descriptor};
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{StorageError, StoragePaths};

const MAX_DIRECTORY_ENTRIES: usize = 4096;
const MAX_DESCRIPTIVE_SLOTS: usize = 256;

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::ReadPath {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.into()),
    }
}

/// Reads native save slot names without opening, modifying or switching worlds.
/// Selecting an entry still uses the ordinary instance settings save pipeline.
pub async fn read_astroneer_save_catalog(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<AstroneerSaveCatalog, StorageError> {
    let pool = connect_pool(paths).await?;
    let context = async {
        let record = fetch_instance_record(&pool, instance_id).await?;
        if record.summary.module_id != "astroneer" {
            return Err(invalid(
                &record.config_dir,
                "Save selection belongs only to an ASTRONEER instance.",
            ));
        }
        let install = effective_instance_install_root(&record)?;
        let descriptor = load_module_descriptor(paths, &record.summary.module_id)?;
        let saves = effective_instance_saves_dir(descriptor.as_ref(), &install, &record)?;
        let settings_path = record.config_dir.join("instance.json");
        let settings: serde_json::Value = serde_json::from_str(
            &crate::instances::read_instance_settings_json(&settings_path)?,
        )?;
        let configured_name = match settings.get("active_save_file_name") {
            Some(serde_json::Value::String(name)) => name.clone(),
            None => String::from("SAVE_1"),
            Some(_) => {
                return Err(invalid(
                    &settings_path,
                    "The configured ASTRONEER save name is not a string.",
                ));
            }
        };
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
        Ok((saves, configured_name))
    }
    .await;
    pool.close().await;
    let (saves, configured_name) = context?;
    let entries = tokio::task::spawn_blocking(move || scan_slots(&saves))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "reading ASTRONEER save slots",
            message: error.to_string(),
        })??;
    Ok(AstroneerSaveCatalog {
        instance_id: instance_id.to_owned(),
        configured_name,
        entries,
    })
}

fn scan_slots(root: &Path) -> Result<Vec<AstroneerSaveEntry>, StorageError> {
    // A fresh instance may have no SaveGames directory. Validate existing
    // ancestors first, so a dangling link is an error rather than an empty list.
    let mut ancestor = root;
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| invalid(root, "Save directory has no existing ancestor."))?;
            }
            Err(error) => return Err(invalid(ancestor, error.to_string())),
        }
    }
    let _ancestor_guards = guard_directories(ancestor)?;
    if ancestor != root {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(root).map_err(|source| StorageError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;
    let mut slots = BTreeMap::<String, AstroneerSaveEntry>::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_DIRECTORY_ENTRIES {
            return Err(invalid(root, "The save directory exceeds 4096 entries."));
        }
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|error| invalid(&path, error.to_string()))?;
        if is_link(&metadata) {
            return Err(invalid(
                &path,
                "Save directories cannot contain links or reparse points.",
            ));
        }
        if !metadata.is_file() || metadata.len() == 0 {
            continue;
        }
        let Some((name, saved_at)) = entry
            .file_name()
            .to_str()
            .and_then(parse_save_filename)
            .map(|(name, saved_at)| (name.to_owned(), saved_at.to_owned()))
        else {
            continue;
        };
        let slot = slots.entry(name.clone()).or_insert(AstroneerSaveEntry {
            descriptive_name: name,
            latest_saved_at: saved_at.clone(),
            versions: 0,
            total_bytes: 0,
        });
        slot.versions += 1;
        slot.total_bytes = slot
            .total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| invalid(root, "The save inventory size exceeds its numeric range."))?;
        if saved_at > slot.latest_saved_at {
            slot.latest_saved_at = saved_at;
        }
        if slots.len() > MAX_DESCRIPTIVE_SLOTS {
            return Err(invalid(
                root,
                "The save directory exceeds 256 descriptive slots.",
            ));
        }
    }
    let mut result = slots.into_values().collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .latest_saved_at
            .cmp(&left.latest_saved_at)
            .then_with(|| left.descriptive_name.cmp(&right.descriptive_name))
    });
    Ok(result)
}

fn parse_save_filename(filename: &str) -> Option<(&str, &str)> {
    let (stem, extension) = filename.rsplit_once('.')?;
    if !extension.eq_ignore_ascii_case("savegame") {
        return None;
    }
    let (name, timestamp) = stem.rsplit_once('$')?;
    if name.trim().is_empty()
        || name.chars().count() > 256
        || name
            .chars()
            .any(|character| character.is_control() || "$/\\:\"<>|?*".contains(character))
        || !valid_native_timestamp(timestamp)
    {
        return None;
    }
    Some((name, timestamp))
}

fn valid_native_timestamp(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 19
        || bytes.iter().enumerate().any(|(index, byte)| match index {
            4 | 7 | 13 | 16 => *byte != b'.',
            10 => *byte != b'-',
            _ => !byte.is_ascii_digit(),
        })
    {
        return false;
    }
    let component = |start, end| timestamp[start..end].parse::<u32>().ok();
    let [
        Some(year),
        Some(month),
        Some(day),
        Some(hour),
        Some(minute),
        Some(second),
    ] = [
        component(0, 4),
        component(5, 7),
        component(8, 10),
        component(11, 13),
        component(14, 16),
        component(17, 19),
    ]
    else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => return false,
    };
    year != 0 && day != 0 && day <= days && hour <= 23 && minute <= 59 && second <= 59
}

#[cfg(test)]
#[path = "astroneer_saves_tests.rs"]
mod tests;
