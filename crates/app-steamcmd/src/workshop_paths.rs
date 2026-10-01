use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use app_core::AppSettings;

use crate::{
    SteamCmdError, SteamWorkshopDownloadItemResult, SteamWorkshopInstallationItemStatus,
    SteamWorkshopInstallationSnapshot, normalize_steam_workshop_item_ids, steamcmd_status,
};

const DST_STEAM_APP_ID: u32 = 322_330;
const MAX_DST_CLUSTER_ENTRIES: usize = 128;
const MAX_WORKSHOP_DIRECTORY_ENTRIES: usize = 16_384;
const MAX_WORKSHOP_ITEMS: usize = 8_192;

struct WorkshopContentRoot {
    path: PathBuf,
    item_prefix: &'static str,
    native_dst: bool,
}

impl WorkshopContentRoot {
    fn item_path(&self, item_id: &str) -> PathBuf {
        self.path.join(format!("{}{item_id}", self.item_prefix))
    }

    fn item_id<'a>(&self, directory_name: &'a str) -> Option<&'a str> {
        directory_name
            .strip_prefix(self.item_prefix)
            .filter(|id| id.len() >= 6 && id.chars().all(|character| character.is_ascii_digit()))
    }
}

pub fn inspect_workshop_items(
    settings: &AppSettings,
    consumer_app_id: u32,
    install_root: &Path,
    ids: &[String],
) -> Result<SteamWorkshopInstallationSnapshot, SteamCmdError> {
    inspect_workshop_items_with_dst_ugc_roots(settings, consumer_app_id, install_root, &[], ids)
}

pub fn inspect_workshop_items_with_dst_ugc_roots(
    settings: &AppSettings,
    consumer_app_id: u32,
    install_root: &Path,
    additional_dst_ugc_content_roots: &[PathBuf],
    ids: &[String],
) -> Result<SteamWorkshopInstallationSnapshot, SteamCmdError> {
    let steamcmd = steamcmd_status(settings);
    let (workshop_root, alternate_workshop_root) =
        resolve_workshop_content_roots(install_root, Path::new(&steamcmd.root), consumer_app_id);
    let mut roots = vec![
        WorkshopContentRoot {
            path: workshop_root,
            item_prefix: "",
            native_dst: false,
        },
        WorkshopContentRoot {
            path: alternate_workshop_root,
            item_prefix: "",
            native_dst: false,
        },
    ];
    if consumer_app_id == DST_STEAM_APP_ID {
        roots.push(WorkshopContentRoot {
            path: install_root.join("mods"),
            item_prefix: "workshop-",
            native_dst: true,
        });
        roots.extend(
            dst_ugc_content_roots(install_root)?
                .into_iter()
                .map(|path| WorkshopContentRoot {
                    path,
                    item_prefix: "",
                    native_dst: true,
                }),
        );
        roots.extend(
            additional_dst_ugc_content_roots
                .iter()
                .cloned()
                .map(|path| WorkshopContentRoot {
                    path,
                    item_prefix: "",
                    native_dst: true,
                }),
        );
    }

    let mut item_ids = normalize_steam_workshop_item_ids(ids);
    if item_ids.len() > MAX_WORKSHOP_ITEMS {
        return Err(inventory_limit_error(install_root, MAX_WORKSHOP_ITEMS));
    }
    let mut seen = item_ids.iter().cloned().collect::<HashSet<_>>();
    for item_id in discover_workshop_item_ids(&roots)? {
        if seen.insert(item_id.clone()) {
            item_ids.push(item_id);
        }
    }
    if item_ids.len() > MAX_WORKSHOP_ITEMS {
        return Err(inventory_limit_error(install_root, MAX_WORKSHOP_ITEMS));
    }
    // Inventory accepts native DST payload evidence, but never an empty download
    // directory. SteamCMD download/reuse still requires official cache records.
    let inspected = if consumer_app_id == DST_STEAM_APP_ID {
        inspect_dst_item_paths_in_roots(&roots, item_ids)?
    } else {
        crate::workshop_cache::inspect_cached_items(
            consumer_app_id,
            &roots
                .iter()
                .map(|root| root.path.clone())
                .collect::<Vec<_>>(),
            item_ids,
        )?
    };
    let items = inspected
        .into_iter()
        .map(|item| SteamWorkshopInstallationItemStatus {
            item_id: item.item_id,
            path: item.expected_path,
            installed: item.expected_path_exists,
        })
        .collect();

    Ok(SteamWorkshopInstallationSnapshot {
        consumer_app_id,
        searched_roots: roots
            .iter()
            .map(|root| root.path.to_string_lossy().into_owned())
            .collect(),
        items,
    })
}

fn dst_ugc_content_roots(install_root: &Path) -> Result<Vec<PathBuf>, SteamCmdError> {
    let ugc_root = install_root.join("ugc_mods");
    let main_root = ugc_root.join("main");
    let mut clusters = Vec::new();
    let mut remaining = MAX_DST_CLUSTER_ENTRIES;
    for entry in read_bounded_directory(&ugc_root, &mut remaining, MAX_DST_CLUSTER_ENTRIES)? {
        if entry
            .file_type()
            .map_err(|source| inventory_read_error(&ugc_root, source))?
            .is_dir()
            && !entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("main")
        {
            clusters.push(entry.path());
        }
    }
    clusters.sort_by_key(|path| path.to_string_lossy().to_ascii_lowercase());
    clusters.insert(0, main_root);

    // Match DST's per-cluster, per-shard UGC layout without walking mod contents.
    // Keep these inventory roots separate from SteamCMD download verification:
    // an old UGC copy must never hide a failed new SteamCMD download.
    Ok(clusters
        .into_iter()
        .flat_map(|cluster| {
            app_core::dst_shards::DST_SHARDS.map(|shard| {
                cluster
                    .join(shard.directory)
                    .join("content")
                    .join(DST_STEAM_APP_ID.to_string())
            })
        })
        .collect())
}

fn discover_workshop_item_ids(roots: &[WorkshopContentRoot]) -> Result<Vec<String>, SteamCmdError> {
    let mut item_ids = HashSet::new();
    let mut remaining = MAX_WORKSHOP_DIRECTORY_ENTRIES;
    for root in roots {
        for entry in
            read_bounded_directory(&root.path, &mut remaining, MAX_WORKSHOP_DIRECTORY_ENTRIES)?
        {
            if !entry
                .file_type()
                .map_err(|source| inventory_read_error(&root.path, source))?
                .is_dir()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(item_id) = root.item_id(&name) {
                item_ids.insert(item_id.to_owned());
                if item_ids.len() > MAX_WORKSHOP_ITEMS {
                    return Err(inventory_limit_error(&root.path, MAX_WORKSHOP_ITEMS));
                }
            }
        }
    }
    let mut sorted_ids = item_ids.into_iter().collect::<Vec<_>>();
    sorted_ids.sort_unstable();
    Ok(sorted_ids)
}

fn read_bounded_directory(
    root: &Path,
    remaining: &mut usize,
    limit: usize,
) -> Result<Vec<fs::DirEntry>, SteamCmdError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(inventory_read_error(root, source)),
    };
    let mut result = Vec::new();
    for entry in entries {
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| inventory_limit_error(root, limit))?;
        result.push(entry.map_err(|source| inventory_read_error(root, source))?);
    }
    Ok(result)
}

fn inventory_read_error(path: &Path, source: io::Error) -> SteamCmdError {
    SteamCmdError::WorkshopInventoryRead {
        path: path.to_path_buf(),
        source,
    }
}

fn inventory_limit_error(path: &Path, limit: usize) -> SteamCmdError {
    inventory_read_error(
        path,
        io::Error::other(format!(
            "Workshop inventory exceeds the {limit}-entry inspection limit"
        )),
    )
}

pub(super) fn resolve_workshop_content_roots(
    install_root: &Path,
    steamcmd_root: &Path,
    consumer_app_id: u32,
) -> (PathBuf, PathBuf) {
    let relative_root = Path::new("steamapps")
        .join("workshop")
        .join("content")
        .join(consumer_app_id.to_string());
    let primary_workshop_root = install_root.join(&relative_root);
    let fallback_workshop_root = steamcmd_root.join(relative_root);
    if primary_workshop_root.exists() || !fallback_workshop_root.exists() {
        (primary_workshop_root, fallback_workshop_root)
    } else {
        (fallback_workshop_root, primary_workshop_root)
    }
}

pub(super) fn inspect_workshop_item_paths(
    consumer_app_id: u32,
    workshop_root: &Path,
    alternate_workshop_root: &Path,
    item_ids: Vec<String>,
) -> Result<Vec<SteamWorkshopDownloadItemResult>, SteamCmdError> {
    crate::workshop_cache::inspect_cached_items(
        consumer_app_id,
        &[
            workshop_root.to_path_buf(),
            alternate_workshop_root.to_path_buf(),
        ],
        item_ids,
    )
}

fn inspect_dst_item_paths_in_roots(
    roots: &[WorkshopContentRoot],
    item_ids: Vec<String>,
) -> Result<Vec<SteamWorkshopDownloadItemResult>, SteamCmdError> {
    let cache_roots = roots
        .iter()
        .filter(|root| root.item_prefix.is_empty())
        .map(|root| root.path.clone())
        .collect::<Vec<_>>();
    let mut items =
        crate::workshop_cache::inspect_cached_items(DST_STEAM_APP_ID, &cache_roots, item_ids)?;
    let mut native_roots = Vec::new();
    for root in roots.iter().filter(|root| root.native_dst) {
        if root.item_prefix.is_empty() {
            let manifest = root
                .path
                .parent()
                .and_then(Path::parent)
                .map(|path| path.join(format!("appworkshop_{DST_STEAM_APP_ID}.acf")));
            if let Some(path) = manifest {
                match fs::symlink_metadata(&path) {
                    // A present but incomplete manifest must not be bypassed by
                    // treating its partial payload as a legacy native Mod.
                    Ok(_) => continue,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(source) => return Err(inventory_read_error(&path, source)),
                }
            }
        }
        native_roots.push(root);
    }
    for item in items.iter_mut().filter(|item| !item.expected_path_exists) {
        for root in &native_roots {
            let candidate = root.item_path(&item.item_id);
            if native_dst_payload_exists(&candidate)? {
                item.expected_path = candidate.to_string_lossy().into_owned();
                item.expected_path_exists = true;
                break;
            }
        }
    }
    Ok(items)
}

fn native_dst_payload_exists(path: &Path) -> Result<bool, SteamCmdError> {
    let modinfo = path.join("modinfo.lua");
    let metadata = match fs::symlink_metadata(&modinfo) {
        Ok(metadata) => metadata,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(false);
        }
        Err(source) => return Err(inventory_read_error(&modinfo, source)),
    };
    if !metadata.is_file() || metadata.len() == 0 {
        return Ok(false);
    }
    crate::dst_workshop_cache_fs::require_plain_file(&modinfo)
        .map_err(|error| inventory_read_error(&modinfo, io::Error::other(error)))?;
    let payload = crate::dst_workshop_cache_fs::inspect_tree(path, 0)
        .map_err(|error| inventory_read_error(path, io::Error::other(error)))?;
    Ok(payload.files > 0 && payload.bytes > 0)
}

#[cfg(test)]
#[path = "workshop_cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "workshop_dst_inventory_tests.rs"]
mod dst_inventory_tests;
