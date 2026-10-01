use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::dst_workshop_cache_vdf::{KvEntry, KvValue, parse_document};
use crate::{SteamCmdError, SteamWorkshopDownloadItemResult};

const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PAYLOAD_ENTRIES: usize = 200_000;
const MAX_PAYLOAD_DEPTH: usize = 64;

// Steam creates content directories before finishing a download. Only an installed
// manifest and its complete byte count authorize reuse or publication of that cache.
pub(super) fn inspect_cached_items(
    app_id: u32,
    content_roots: &[PathBuf],
    item_ids: Vec<String>,
) -> Result<Vec<SteamWorkshopDownloadItemResult>, SteamCmdError> {
    let inventories = content_roots
        .iter()
        .map(|root| read_inventory(root, app_id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut remaining = MAX_PAYLOAD_ENTRIES;
    let mut result = Vec::with_capacity(item_ids.len());
    for item_id in item_ids {
        let mut expected_path = content_roots[0].join(&item_id);
        let mut complete = false;
        for (root, inventory) in content_roots.iter().zip(&inventories) {
            let candidate = root.join(&item_id);
            let Some(expected_bytes) = inventory.get(&item_id) else {
                continue;
            };
            if let Some((files, bytes)) = inspect_payload(&candidate, 0, &mut remaining)?
                && files > 0
                && bytes == *expected_bytes
            {
                expected_path = candidate;
                complete = true;
                break;
            }
        }
        result.push(SteamWorkshopDownloadItemResult {
            item_id,
            expected_path: expected_path.to_string_lossy().into_owned(),
            expected_path_exists: complete,
        });
    }
    Ok(result)
}

fn read_inventory(content_root: &Path, app_id: u32) -> Result<HashMap<String, u64>, SteamCmdError> {
    if !plain_directory_chain(content_root)? {
        return Ok(HashMap::new());
    }
    let Some(workshop_root) = content_root.parent().and_then(Path::parent) else {
        return Ok(HashMap::new());
    };
    let path = workshop_root.join(format!("appworkshop_{app_id}.acf"));
    let Some(metadata) = plain_metadata(&path)? else {
        return Ok(HashMap::new());
    };
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Ok(HashMap::new());
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| file.take(MAX_MANIFEST_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|source| read_error(&path, source))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Ok(HashMap::new());
    }
    let Some(document) = std::str::from_utf8(&bytes)
        .ok()
        .and_then(|source| parse_document(source).ok())
    else {
        return Ok(HashMap::new());
    };
    Ok(installed_sizes(&document, app_id).unwrap_or_default())
}

fn installed_sizes(document: &[KvEntry], app_id: u32) -> Option<HashMap<String, u64>> {
    let app = object(document, "AppWorkshop")?;
    if text(app, "appid")?.parse::<u32>().ok()? != app_id {
        return None;
    }
    let installed = object(app, "WorkshopItemsInstalled")?;
    let details = if app.iter().any(|entry| entry.key == "WorkshopItemDetails") {
        let mut indexed = HashMap::new();
        for entry in object(app, "WorkshopItemDetails")? {
            let KvValue::Object(fields) = &entry.value else {
                return None;
            };
            if indexed
                .insert(entry.key.as_str(), fields.as_slice())
                .is_some()
            {
                return None;
            }
        }
        Some(indexed)
    } else {
        None
    };
    let mut sizes = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    for item in installed {
        if !seen.insert(&item.key) {
            return None;
        }
        let KvValue::Object(fields) = &item.value else {
            continue;
        };
        let Some(manifest) = text(fields, "manifest").and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        if manifest == 0 {
            continue;
        }
        // A newer detail manifest means the installed bytes are still the old
        // package. Some native clients omit details, so installed metadata is
        // sufficient when no detail record for this item exists.
        if let Some(fields) = details
            .as_ref()
            .and_then(|entries| entries.get(item.key.as_str()))
            && text(fields, "manifest").and_then(|value| value.parse::<u64>().ok())
                != Some(manifest)
        {
            continue;
        }
        if let Some(size) = text(fields, "size").and_then(|value| value.parse::<u64>().ok())
            && size > 0
        {
            sizes.insert(item.key.clone(), size);
        }
    }
    Some(sizes)
}

fn unique<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a KvEntry> {
    let mut matches = entries.iter().filter(|entry| entry.key == key);
    let entry = matches.next()?;
    matches.next().is_none().then_some(entry)
}

fn object<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a [KvEntry]> {
    match &unique(entries, key)?.value {
        KvValue::Object(entries) => Some(entries),
        KvValue::Text(_) => None,
    }
}

fn text<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a str> {
    match &unique(entries, key)?.value {
        KvValue::Text(value) => Some(value),
        KvValue::Object(_) => None,
    }
}

fn inspect_payload(
    path: &Path,
    depth: usize,
    remaining: &mut usize,
) -> Result<Option<(u64, u64)>, SteamCmdError> {
    if depth > MAX_PAYLOAD_DEPTH {
        return Err(read_error(
            path,
            io::Error::other("Workshop payload exceeds the 64-level inspection limit"),
        ));
    }
    let Some(metadata) = plain_metadata(path)? else {
        return Ok(None);
    };
    if !metadata.is_dir() {
        return Ok(None);
    }
    let mut files = 0_u64;
    let mut bytes = 0_u64;
    for entry in fs::read_dir(path).map_err(|source| read_error(path, source))? {
        *remaining = remaining.checked_sub(1).ok_or_else(|| {
            read_error(
                path,
                io::Error::other("Workshop payload exceeds the 200000-entry inspection limit"),
            )
        })?;
        let entry = entry.map_err(|source| read_error(path, source))?;
        let child = entry.path();
        let Some(metadata) = plain_metadata(&child)? else {
            return Ok(None);
        };
        let (count, size) = if metadata.is_dir() {
            let Some(totals) = inspect_payload(&child, depth + 1, remaining)? else {
                return Ok(None);
            };
            totals
        } else if metadata.is_file() {
            (1, metadata.len())
        } else {
            return Err(read_error(
                &child,
                io::Error::other("Workshop payload is not a plain file or directory"),
            ));
        };
        files += count;
        bytes = bytes.checked_add(size).ok_or_else(|| {
            read_error(
                &child,
                io::Error::other("Workshop payload byte count overflow"),
            )
        })?;
    }
    Ok(Some((files, bytes)))
}

fn plain_directory_chain(path: &Path) -> Result<bool, SteamCmdError> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        let Some(metadata) = plain_metadata(ancestor)? else {
            return Ok(false);
        };
        if !metadata.is_dir() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn plain_metadata(path: &Path) -> Result<Option<fs::Metadata>, SteamCmdError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(read_error(path, source)),
    };
    if is_link(&metadata) {
        return Err(read_error(
            path,
            io::Error::other("Workshop cache contains a link or reparse point"),
        ));
    }
    Ok(Some(metadata))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes()
            & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
            != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn read_error(path: &Path, source: io::Error) -> SteamCmdError {
    SteamCmdError::WorkshopInventoryRead {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workshop_cache_accepts_existing_relative_directory_roots() {
        assert!(plain_directory_chain(Path::new(".")).unwrap());
    }
}
