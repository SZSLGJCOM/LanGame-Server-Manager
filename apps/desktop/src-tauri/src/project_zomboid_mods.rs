use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::text_decode::decode_utf8_or_gb18030_text;
use serde::Serialize;

const MAX_MOD_INFO_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectZomboidWorkshopModsSnapshot {
    pub workshop_root: String,
    pub workshop_root_exists: bool,
    pub items: Vec<ProjectZomboidWorkshopItemSpec>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectZomboidWorkshopItemSpec {
    pub workshop_item_id: String,
    pub item_path: String,
    pub mods_path: Option<String>,
    pub status: String,
    pub message: Option<String>,
    pub mods: Vec<ProjectZomboidLocalModSpec>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProjectZomboidLocalModSpec {
    pub directory_name: String,
    pub mod_id: Option<String>,
    pub mod_name: Option<String>,
    pub mod_path: String,
    pub mod_info_path: Option<String>,
    pub map_ids: Vec<String>,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
struct ParsedModInfo {
    id: Option<String>,
    name: Option<String>,
}

pub fn read_project_zomboid_workshop_mods_snapshot(
    install_root: &Path,
    steamcmd_root: &Path,
    app_id: u32,
    ids: &[String],
) -> ProjectZomboidWorkshopModsSnapshot {
    let normalized_ids = normalize_workshop_ids(ids);
    let workshop_root_candidates =
        build_workshop_root_candidates(install_root, steamcmd_root, app_id);
    let resolved_workshop_root = workshop_root_candidates
        .iter()
        .find(|candidate| candidate.is_dir())
        .cloned()
        .or_else(|| workshop_root_candidates.first().cloned())
        .unwrap_or_else(|| {
            install_root
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(app_id.to_string())
        });

    let workshop_root_exists = resolved_workshop_root.is_dir();
    let items = normalized_ids
        .iter()
        .map(|workshop_item_id| {
            // SteamCMD may place different items in the shared cache or the game's cache.
            let item_root = workshop_root_candidates
                .iter()
                .find(|candidate| candidate.join(workshop_item_id).is_dir())
                .unwrap_or(&resolved_workshop_root);
            read_workshop_item_spec(item_root, item_root.is_dir(), workshop_item_id)
        })
        .collect();

    ProjectZomboidWorkshopModsSnapshot {
        workshop_root: resolved_workshop_root.to_string_lossy().into_owned(),
        workshop_root_exists,
        items,
    }
}

fn read_workshop_item_spec(
    workshop_root: &Path,
    workshop_root_exists: bool,
    workshop_item_id: &str,
) -> ProjectZomboidWorkshopItemSpec {
    let item_path = workshop_root.join(workshop_item_id);
    if !workshop_root_exists {
        return ProjectZomboidWorkshopItemSpec {
            workshop_item_id: workshop_item_id.to_owned(),
            item_path: item_path.to_string_lossy().into_owned(),
            mods_path: None,
            status: String::from("missing_workshop_root"),
            message: Some(format!(
                "Local Workshop cache was not found under {}.",
                workshop_root.display()
            )),
            mods: Vec::new(),
        };
    }

    if !item_path.exists() {
        return ProjectZomboidWorkshopItemSpec {
            workshop_item_id: workshop_item_id.to_owned(),
            item_path: item_path.to_string_lossy().into_owned(),
            mods_path: None,
            status: String::from("missing_item"),
            message: Some(String::from(
                "Workshop item folder was not found under the local Project Zomboid Workshop cache.",
            )),
            mods: Vec::new(),
        };
    }

    let Some(mods_path) = find_local_mods_root(&item_path) else {
        return ProjectZomboidWorkshopItemSpec {
            workshop_item_id: workshop_item_id.to_owned(),
            item_path: item_path.to_string_lossy().into_owned(),
            mods_path: None,
            status: String::from("missing_mods_dir"),
            message: Some(String::from(
                "Downloaded Workshop content exists, but no Contents/mods folder was found inside it.",
            )),
            mods: Vec::new(),
        };
    };

    let mod_dirs = list_directories_sorted(&mods_path);
    if mod_dirs.is_empty() {
        return ProjectZomboidWorkshopItemSpec {
            workshop_item_id: workshop_item_id.to_owned(),
            item_path: item_path.to_string_lossy().into_owned(),
            mods_path: Some(mods_path.to_string_lossy().into_owned()),
            status: String::from("empty_item"),
            message: Some(String::from(
                "Workshop item was found locally, but it did not contain any mod folders.",
            )),
            mods: Vec::new(),
        };
    }

    let mods = mod_dirs
        .iter()
        .map(|mod_dir| read_local_mod_spec(mod_dir))
        .collect::<Vec<_>>();
    let loaded_mod_count = mods.iter().filter(|item| item.status == "loaded").count();
    let warning_count = mods.len().saturating_sub(loaded_mod_count);

    let (status, message) = if warning_count == 0 {
        (String::from("installed"), None)
    } else if loaded_mod_count > 0 {
        (
            String::from("installed_with_warnings"),
            Some(format!(
                "Scanned {} local mod folder(s); {} still need attention before the enabled Mod list is fully trustworthy.",
                mods.len(),
                warning_count
            )),
        )
    } else {
        (
            String::from("warnings_only"),
            Some(String::from(
                "Workshop content was found locally, but none of the embedded mod folders exposed a usable mod.info ID yet.",
            )),
        )
    };

    ProjectZomboidWorkshopItemSpec {
        workshop_item_id: workshop_item_id.to_owned(),
        item_path: item_path.to_string_lossy().into_owned(),
        mods_path: Some(mods_path.to_string_lossy().into_owned()),
        status,
        message,
        mods,
    }
}

fn read_local_mod_spec(mod_dir: &Path) -> ProjectZomboidLocalModSpec {
    let candidate_roots = build_mod_content_roots(mod_dir);
    let selected_mod_info = candidate_roots
        .iter()
        .filter_map(|root| {
            let mod_info_path = root.join("mod.info");
            if !mod_info_path.exists() {
                return None;
            }
            Some((
                content_root_priority(mod_dir, root),
                root.clone(),
                mod_info_path,
            ))
        })
        .max_by_key(|(priority, _, _)| *priority);

    let map_ids = collect_map_ids(&candidate_roots);
    let directory_name = mod_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("mod")
        .to_owned();

    let Some((_, selected_root, mod_info_path)) = selected_mod_info else {
        return ProjectZomboidLocalModSpec {
            directory_name,
            mod_id: None,
            mod_name: None,
            mod_path: mod_dir.to_string_lossy().into_owned(),
            mod_info_path: None,
            map_ids,
            status: String::from("missing_mod_info"),
            message: Some(String::from(
                "No mod.info file was found in this local mod folder or its version subfolders.",
            )),
        };
    };

    match parse_mod_info(&mod_info_path) {
        Ok(parsed) => {
            let mod_id = parsed
                .id
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from);
            let mod_name = parsed
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(String::from);
            let (status, message) = if mod_id.is_some() {
                (String::from("loaded"), None)
            } else {
                (
                    String::from("missing_mod_id"),
                    Some(String::from(
                        "mod.info was found, but it did not expose a usable id= entry.",
                    )),
                )
            };

            ProjectZomboidLocalModSpec {
                directory_name,
                mod_id,
                mod_name,
                mod_path: selected_root.to_string_lossy().into_owned(),
                mod_info_path: Some(mod_info_path.to_string_lossy().into_owned()),
                map_ids,
                status,
                message,
            }
        }
        Err(message) => ProjectZomboidLocalModSpec {
            directory_name,
            mod_id: None,
            mod_name: None,
            mod_path: selected_root.to_string_lossy().into_owned(),
            mod_info_path: Some(mod_info_path.to_string_lossy().into_owned()),
            map_ids,
            status: String::from("parse_error"),
            message: Some(message),
        },
    }
}

fn build_workshop_root_candidates(
    install_root: &Path,
    steamcmd_root: &Path,
    app_id: u32,
) -> Vec<PathBuf> {
    let mut candidates = [install_root, steamcmd_root]
        .into_iter()
        .map(|root| {
            root.join("steamapps")
                .join("workshop")
                .join("content")
                .join(app_id.to_string())
        })
        .collect::<Vec<_>>();
    for base_root in [
        Some(install_root.to_path_buf()),
        install_root.parent().map(Path::to_path_buf),
        install_root
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf),
    ]
    .into_iter()
    .flatten()
    {
        candidates.push(
            base_root
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(app_id.to_string()),
        );
        candidates.push(
            base_root
                .join("steamapps")
                .join("workshop")
                .join(app_id.to_string()),
        );
        candidates.push(
            base_root
                .join("workshop")
                .join("content")
                .join(app_id.to_string()),
        );
        candidates.push(base_root.join("workshop").join(app_id.to_string()));
    }

    dedupe_paths(candidates)
}

fn build_mod_content_roots(mod_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![mod_dir.to_path_buf()];
    roots.extend(list_directories_sorted(mod_dir));
    dedupe_paths(roots)
}

fn collect_map_ids(content_roots: &[PathBuf]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut map_ids = Vec::new();

    for root in content_roots {
        let maps_root = root.join("media").join("maps");
        for map_dir in list_directories_sorted(&maps_root) {
            let Some(name) = map_dir.file_name().and_then(|entry| entry.to_str()) else {
                continue;
            };
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            let normalized = trimmed.to_ascii_lowercase();
            if seen.insert(normalized) {
                map_ids.push(trimmed.to_owned());
            }
        }
    }

    map_ids
}

fn find_local_mods_root(item_path: &Path) -> Option<PathBuf> {
    [
        item_path.join("Contents").join("mods"),
        item_path.join("mods"),
        item_path.join("contents").join("mods"),
    ]
    .into_iter()
    .find(|candidate| candidate.exists() && candidate.is_dir())
}

fn list_directories_sorted(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut directories = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_type = entry.file_type().ok()?;
            file_type.is_dir().then_some(entry.path())
        })
        .collect::<Vec<_>>();

    directories.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let right_name = right
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        left_name.cmp(&right_name)
    });

    directories
}

fn parse_mod_info(path: &Path) -> Result<ParsedModInfo, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if bytes.len() > MAX_MOD_INFO_BYTES {
        return Err(format!(
            "{} is too large to inspect safely ({} bytes).",
            path.display(),
            bytes.len()
        ));
    }

    let source = decode_utf8_or_gb18030_text(&bytes)
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");

    let mut id = None;
    let mut name = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        let Some((raw_key, raw_value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_key.trim().to_ascii_lowercase();
        let value = trim_wrapping_quotes(raw_value.trim()).trim();
        if value.is_empty() {
            continue;
        }

        match key.as_str() {
            "id" if id.is_none() => id = Some(String::from(value)),
            "name" if name.is_none() => name = Some(String::from(value)),
            _ => {}
        }
    }

    Ok(ParsedModInfo { id, name })
}

fn trim_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return &value[1..value.len() - 1];
        }
    }

    value
}

fn content_root_priority(mod_dir: &Path, candidate_root: &Path) -> i32 {
    if candidate_root == mod_dir {
        return 100;
    }

    let Some(folder_name) = candidate_root.file_name().and_then(|name| name.to_str()) else {
        return 0;
    };
    if let Ok(version) = folder_name.parse::<i32>() {
        return 200 + version;
    }
    if folder_name.eq_ignore_ascii_case("common") {
        return 50;
    }

    25
}

fn normalize_workshop_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for id in ids {
        let digits = id
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect::<String>();
        if digits.len() < 6 {
            continue;
        }
        if seen.insert(digits.clone()) {
            normalized.push(digits);
        }
    }

    normalized
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for path in paths {
        let key = path.to_string_lossy().to_ascii_lowercase();
        if seen.insert(key) {
            deduped.push(path);
        }
    }

    deduped
}

#[cfg(test)]
#[path = "project_zomboid_mods/tests.rs"]
mod tests;
