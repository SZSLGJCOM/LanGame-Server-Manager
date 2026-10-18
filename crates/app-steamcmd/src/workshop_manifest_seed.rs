//! A retry can reuse item metadata without logging a changed manifest. Seed its
//! exact target from the requested item's cache details; this is a mapping only,
//! never evidence that Steam denied access during the current invocation.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use crate::dst_workshop_cache_vdf::{KvEntry, KvValue, parse_document};

const MAX_SCRIPT_BYTES: usize = 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUESTS: usize = 8192;

pub(crate) async fn requested_item_manifests(
    script_path: &Path,
    root: &Path,
) -> Vec<(u32, u64, u64)> {
    let script_path = script_path.to_owned();
    let root = root.to_owned();
    tokio::task::spawn_blocking(move || read_requested_item_manifests(&script_path, &root))
        .await
        .unwrap_or_default()
}

fn read_requested_item_manifests(script_path: &Path, root: &Path) -> Vec<(u32, u64, u64)> {
    let Some((install, requests)) = crate::steam_depot::bounded(script_path, MAX_SCRIPT_BYTES)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|script| script_requests(&script))
    else {
        return Vec::new();
    };
    let Ok(root) = crate::steam_depot::checked_root(root) else {
        return Vec::new();
    };
    let Ok(install) = crate::steam_depot::checked_root(&install) else {
        return Vec::new();
    };
    let roots = BTreeSet::from([install, root]);
    let apps = requests
        .iter()
        .map(|(app, _)| *app)
        .collect::<BTreeSet<_>>();
    let mut mappings: BTreeMap<(u32, u64), BTreeSet<u64>> = BTreeMap::new();
    for app in apps {
        for root in &roots {
            let path = root
                .join("steamapps/workshop")
                .join(format!("appworkshop_{app}.acf"));
            let Some(document) = crate::steam_depot::bounded(&path, MAX_MANIFEST_BYTES)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .and_then(|source| parse_document(&source).ok())
            else {
                continue;
            };
            let Some(details) = item_details(&document, app) else {
                continue;
            };
            for (item, manifest) in requested_details(details, app, &requests) {
                mappings.entry((app, item)).or_default().insert(manifest);
            }
        }
    }
    mappings
        .into_iter()
        .filter_map(|((app, item), manifests)| {
            (manifests.len() == 1).then(|| (app, item, *manifests.first().unwrap()))
        })
        .collect()
}

fn requested_details(
    details: &[KvEntry],
    app: u32,
    requests: &BTreeSet<(u32, u64)>,
) -> Vec<(u64, u64)> {
    let mut targets = BTreeMap::new();
    for entry in details {
        let Some(item) = number(&entry.key).filter(|item| entry.key == item.to_string()) else {
            continue;
        };
        if !requests.contains(&(app, item)) {
            continue;
        }
        let manifest = match &entry.value {
            KvValue::Object(fields) => unique_text(fields, "manifest").and_then(number),
            KvValue::Text(_) => None,
        };
        // Duplicate item keys are ambiguous even when one duplicate is invalid.
        targets
            .entry(item)
            .and_modify(|target| *target = None)
            .or_insert(manifest);
    }
    targets
        .into_iter()
        .filter_map(|(item, manifest)| manifest.map(|manifest| (item, manifest)))
        .collect()
}

fn script_requests(script: &str) -> Option<(PathBuf, BTreeSet<(u32, u64)>)> {
    let mut install = None;
    let mut requests = BTreeSet::new();
    for line in script.lines().map(str::trim) {
        if let Some(path) = line.strip_prefix("force_install_dir ") {
            if install.is_some() || !requests.is_empty() {
                return None;
            }
            let path = path.strip_prefix('"')?.strip_suffix('"')?;
            if path.is_empty() || path.contains('"') || path.chars().any(char::is_control) {
                return None;
            }
            let path = PathBuf::from(path);
            if !path.is_absolute()
                || path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return None;
            }
            install = Some(path);
        } else if let Some(arguments) = line.strip_prefix("workshop_download_item ") {
            let mut fields = arguments.split_ascii_whitespace();
            let app = u32::try_from(number(fields.next()?)?).ok()?;
            let item = number(fields.next()?)?;
            if !matches!(fields.next(), None | Some("validate")) || fields.next().is_some() {
                return None;
            }
            requests.insert((app, item));
            if requests.len() > MAX_REQUESTS {
                return None;
            }
        }
    }
    Some((install?, requests))
}

fn item_details(document: &[KvEntry], app: u32) -> Option<&[KvEntry]> {
    let workshop = unique_object(document, "AppWorkshop")?;
    if number(unique_text(workshop, "appid")?)? != u64::from(app) {
        return None;
    }
    unique_object(workshop, "WorkshopItemDetails")
}

fn unique<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a KvValue> {
    let mut matches = entries.iter().filter(|entry| entry.key == key);
    let entry = matches.next()?;
    matches.next().is_none().then_some(&entry.value)
}

fn unique_object<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a [KvEntry]> {
    match unique(entries, key)? {
        KvValue::Object(fields) => Some(fields),
        KvValue::Text(_) => None,
    }
}

fn unique_text<'a>(entries: &'a [KvEntry], key: &str) -> Option<&'a str> {
    match unique(entries, key)? {
        KvValue::Text(value) => Some(value),
        KvValue::Object(_) => None,
    }
}

fn number(value: &str) -> Option<u64> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse::<u64>().ok())
        .flatten()
        .filter(|value| *value != 0)
}

#[cfg(test)]
#[path = "workshop_manifest_seed_tests.rs"]
mod tests;
