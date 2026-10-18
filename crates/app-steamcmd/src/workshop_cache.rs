use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::dst_workshop_cache_vdf::{KvEntry, KvValue, parse_document};
use crate::{SteamCmdError, SteamWorkshopDownloadItemResult};

const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PAYLOAD_ENTRIES: usize = 200_000;
const MAX_PAYLOAD_DEPTH: usize = 64;

#[derive(Clone, Copy)]
pub(super) struct InstalledItem {
    pub(super) manifest: u64,
    pub(super) bytes: u64,
}

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
            let Some(installed) = inventory.get(&item_id) else {
                continue;
            };
            if let Some((files, bytes)) = inspect_payload(&candidate, 0, &mut remaining)?
                && files > 0
                && bytes == installed.bytes
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

/// A successful native item location identifies the cache validated by this
/// invocation. Another complete cache must never replace an incomplete or failed
/// reported location. Without that evidence, conflicting manifests are ambiguous.
pub(super) fn inspect_downloaded_items(
    app_id: u32,
    content_roots: &[PathBuf],
    item_ids: Vec<String>,
    evidence: &crate::workshop_download::WorkshopEvidence,
) -> Result<Vec<SteamWorkshopDownloadItemResult>, SteamCmdError> {
    let inventories = content_roots
        .iter()
        .map(|root| read_inventory(root, app_id))
        .collect::<Result<Vec<_>, _>>()?;
    let mut remaining = MAX_PAYLOAD_ENTRIES;
    let mut result = Vec::with_capacity(item_ids.len());
    for item_id in item_ids {
        if !evidence.item_download_succeeded(&item_id) {
            return Err(SteamCmdError::SteamCmdCommandFailed {
                output_excerpt: format!(
                    "SteamCMD did not confirm Workshop item {item_id} downloaded or validated during this invocation."
                ),
            });
        }
        let selected = match evidence.downloaded_item_path(&item_id) {
            Some(reported) => Some(
                content_roots
                    .iter()
                    .position(|root| same_cache_path(reported, &root.join(&item_id)))
                    .ok_or_else(|| SteamCmdError::SteamCmdCommandFailed {
                        output_excerpt: format!(
                            "SteamCMD reported Workshop item {item_id} outside its managed cache roots: {}",
                            reported.display()
                        ),
                    })?,
            ),
            None => None,
        };
        let mut complete: Option<(PathBuf, u64)> = None;
        for (index, (root, inventory)) in content_roots.iter().zip(&inventories).enumerate() {
            if selected.is_some_and(|selected| selected != index) {
                continue;
            }
            let candidate = root.join(&item_id);
            let Some(installed) = inventory.get(&item_id) else {
                continue;
            };
            let Some((files, bytes)) = inspect_payload(&candidate, 0, &mut remaining)? else {
                continue;
            };
            if files == 0 || bytes != installed.bytes {
                continue;
            }
            if complete
                .as_ref()
                .is_some_and(|(_, manifest)| *manifest != installed.manifest)
            {
                return Err(SteamCmdError::SteamCmdCommandFailed {
                    output_excerpt: format!(
                        "Workshop item {item_id} has conflicting complete cached manifests and SteamCMD did not identify the validated download location."
                    ),
                });
            }
            complete.get_or_insert((candidate, installed.manifest));
        }
        let expected_path = complete
            .as_ref()
            .map(|(path, _)| path.clone())
            .unwrap_or_else(|| content_roots[selected.unwrap_or(0)].join(&item_id));
        result.push(SteamWorkshopDownloadItemResult {
            item_id,
            expected_path: expected_path.to_string_lossy().into_owned(),
            expected_path_exists: complete.is_some(),
        });
    }
    Ok(result)
}

fn same_cache_path(reported: &Path, expected: &Path) -> bool {
    let reported = fs::canonicalize(reported).unwrap_or_else(|_| reported.to_path_buf());
    let expected = fs::canonicalize(expected).unwrap_or_else(|_| expected.to_path_buf());
    #[cfg(windows)]
    {
        crate::command_path_string(&reported)
            .replace('\\', "/")
            .eq_ignore_ascii_case(&crate::command_path_string(&expected).replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        reported == expected
    }
}

fn read_inventory(
    content_root: &Path,
    app_id: u32,
) -> Result<HashMap<String, InstalledItem>, SteamCmdError> {
    read_installed_inventory(content_root, app_id, true)
}

pub(super) fn read_installed_inventory(
    content_root: &Path,
    app_id: u32,
    require_current: bool,
) -> Result<HashMap<String, InstalledItem>, SteamCmdError> {
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
    Ok(installed_records(&document, app_id, require_current).unwrap_or_default())
}

fn installed_records(
    document: &[KvEntry],
    app_id: u32,
    require_current: bool,
) -> Option<HashMap<String, InstalledItem>> {
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
        if require_current
            && let Some(fields) = details
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
            sizes.insert(
                item.key.clone(),
                InstalledItem {
                    manifest,
                    bytes: size,
                },
            );
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

    #[test]
    fn workshop_download_rejects_native_locations_outside_both_cache_roots() {
        let root = crate::tests::unique_test_root();
        let caches = [
            root.join("install/content/42"),
            root.join("steamcmd/content/42"),
        ];
        for cache in &caches {
            fs::create_dir_all(cache).unwrap();
        }
        let mut evidence = crate::workshop_download::WorkshopEvidence::default();
        evidence.observe_line(&format!(
            "Success. Downloaded item 123456 to \"{}\" (6 bytes)",
            root.join("unmanaged/123456").display()
        ));
        let result = inspect_downloaded_items(42, &caches, vec!["123456".into()], &evidence);
        fs::remove_dir_all(root).unwrap();
        let Err(SteamCmdError::SteamCmdCommandFailed { output_excerpt }) = result else {
            panic!("native output must not authorize an unmanaged cache path");
        };
        assert!(output_excerpt.contains("outside its managed cache roots"));
    }
}
