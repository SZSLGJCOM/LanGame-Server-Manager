//! Optional public project presentation data, separate from Mod ownership and game settings.
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use app_network::media_cache::{MediaKind, media_cache_identity};
use serde::{Deserialize, Serialize};

use super::ManualModPresentation;

const MAX_ENTRIES: usize = 2048;
const MAX_BYTES: usize = 1024 * 1024;
static CACHE_ACCESS: Mutex<()> = Mutex::new(());

#[derive(Deserialize, Serialize)]
struct Cache {
    #[serde(default = "cache_version")]
    schema_version: u32,
    #[serde(default)]
    entries: Vec<ManualModPresentation>,
}

fn cache_version() -> u32 {
    1
}

pub(crate) fn normalize_display_name(value: Option<&str>) -> Option<String> {
    let mut result = String::new();
    let mut pending_space = false;
    let mut count = 0;
    for character in value?.chars() {
        if character.is_whitespace() {
            pending_space = !result.is_empty();
        } else if !character.is_control() {
            if pending_space && count < 240 {
                result.push(' ');
                count += 1;
            }
            if count == 240 {
                break;
            }
            result.push(character);
            count += 1;
            pending_space = false;
        }
    }
    let result = result.trim_end().to_owned();
    (!result.is_empty()).then_some(result)
}

pub(crate) fn verified_icon_url(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.len() > 2048 {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some()
        || media_cache_identity(url.as_str(), MediaKind::Image).is_none()
    {
        return None;
    }
    Some(url.to_string())
}

fn valid_id(id: &str) -> bool {
    id.len() <= 10
        && matches!(id.as_bytes().first(), Some(b'1'..=b'9'))
        && id.bytes().all(|byte| byte.is_ascii_digit())
        && id.parse::<u32>().is_ok()
}

fn cache_path(data_root: &Path) -> Result<PathBuf, String> {
    if !data_root.is_absolute()
        || data_root
            .components()
            .any(|part| part == Component::ParentDir)
    {
        return Err("Mod presentation storage requires an absolute data root".into());
    }
    Ok(data_root.join("cache/mod-presentations/curseforge.json"))
}

fn validate_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors().filter(|part| !part.as_os_str().is_empty()) {
        let metadata = match fs::symlink_metadata(ancestor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Cannot inspect Mod presentation storage: {error}")),
        };
        let mut reparse = metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            reparse |= metadata.file_attributes() & 0x400 != 0;
        }
        if reparse
            || (ancestor == path && !metadata.is_file())
            || (ancestor != path && !metadata.is_dir())
        {
            return Err("Mod presentation storage contains a non-regular path".into());
        }
    }
    Ok(())
}

fn read_snapshot(path: &Path) -> Result<(Option<Vec<u8>>, Cache), String> {
    validate_path(path)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok((
                None,
                Cache {
                    schema_version: cache_version(),
                    entries: Vec::new(),
                },
            ));
        }
        Err(error) => return Err(format!("Cannot read Mod presentation cache: {error}")),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read Mod presentation cache: {error}"))?;
    if bytes.len() > MAX_BYTES {
        return Err("Mod presentation cache exceeds its size limit".into());
    }
    let mut cache: Cache = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid Mod presentation cache: {error}"))?;
    if cache.schema_version != cache_version() || cache.entries.len() > MAX_ENTRIES {
        return Err("Unsupported or oversized Mod presentation cache".into());
    }
    let mut ids = HashSet::new();
    for entry in &mut cache.entries {
        if !valid_id(&entry.id) || !ids.insert(entry.id.clone()) {
            return Err("Mod presentation cache contains invalid project identities".into());
        }
        entry.title = normalize_display_name(entry.title.as_deref());
        entry.icon_url = verified_icon_url(entry.icon_url.as_deref());
    }
    Ok((Some(bytes), cache))
}

pub(super) fn read_curseforge_presentations(
    data_root: &Path,
) -> Result<HashMap<String, ManualModPresentation>, String> {
    let _guard = CACHE_ACCESS
        .lock()
        .map_err(|_| "Mod presentation cache lock poisoned")?;
    let (_, cache) = read_snapshot(&cache_path(data_root)?)?;
    Ok(cache
        .entries
        .into_iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect())
}

pub(super) fn remember_curseforge_presentations(
    data_root: &Path,
    entries: &[ManualModPresentation],
) -> Result<(), String> {
    if entries.len() > MAX_ENTRIES || entries.iter().any(|entry| !valid_id(&entry.id)) {
        return Err("Invalid Mod presentation batch".into());
    }
    if entries.is_empty() {
        return Ok(());
    }
    let _guard = CACHE_ACCESS
        .lock()
        .map_err(|_| "Mod presentation cache lock poisoned")?;
    let path = cache_path(data_root)?;
    let (original, mut cache) = read_snapshot(&path)?;
    for entry in entries {
        let title = normalize_display_name(entry.title.as_deref());
        let icon_url = verified_icon_url(entry.icon_url.as_deref());
        if title.is_none() && icon_url.is_none() {
            continue;
        }
        let previous = cache
            .entries
            .iter()
            .position(|cached| cached.id == entry.id)
            .map(|position| cache.entries.remove(position));
        cache.entries.push(ManualModPresentation {
            id: entry.id.clone(),
            title: title.or_else(|| previous.as_ref().and_then(|entry| entry.title.clone())),
            icon_url: icon_url.or_else(|| previous.and_then(|entry| entry.icon_url)),
        });
    }
    if cache.entries.is_empty() && original.is_none() {
        return Ok(());
    }
    let overflow = cache.entries.len().saturating_sub(MAX_ENTRIES);
    cache.entries.drain(..overflow);
    let mut encoded_size = serde_json::to_vec(&Cache {
        schema_version: cache_version(),
        entries: Vec::new(),
    })
    .map_err(|error| error.to_string())?
    .len();
    let mut retained = 0;
    for entry in cache.entries.iter().rev() {
        let size = serde_json::to_vec(entry)
            .map_err(|error| error.to_string())?
            .len()
            + usize::from(retained > 0);
        if encoded_size + size > MAX_BYTES {
            break;
        }
        encoded_size += size;
        retained += 1;
    }
    cache.entries.drain(..cache.entries.len() - retained);
    let replacement = serde_json::to_vec(&cache).map_err(|error| error.to_string())?;
    if original.as_deref() == Some(replacement.as_slice()) {
        return Ok(());
    }
    fs::create_dir_all(path.parent().expect("cache path has a parent"))
        .map_err(|error| format!("Cannot create Mod presentation cache: {error}"))?;
    validate_path(&path)?;
    publish(&path, original.as_deref(), &replacement)
}

fn publish(path: &Path, original: Option<&[u8]>, replacement: &[u8]) -> Result<(), String> {
    validate_path(path)?;
    match original {
        Some(original) => {
            if !app_storage::compare_and_swap_file_atomically(path, original, replacement)
                .map_err(|error| format!("Cannot save Mod presentation cache: {error}"))?
            {
                return Err("Mod presentation cache changed during publication".into());
            }
        }
        None => app_storage::create_file_atomically(path, replacement)
            .map_err(|error| format!("Cannot save Mod presentation cache: {error}"))?,
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_mod_presentations_tests.rs"]
mod tests;
