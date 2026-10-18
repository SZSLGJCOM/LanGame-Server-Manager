//! Instance-local provenance for Workshop payloads stored in a shared cache.
//! Membership identifies requested items; it does not certify payload integrity.
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{InstallCancellation, steam_depot};

const MAX_MEMBERSHIP_BYTES: usize = 4096;
const MAX_ITEMS: usize = 8192;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Membership {
    version: u32,
    app_id: u32,
    item_id: String,
    manifest: u64,
    native_success: bool,
}

fn check_cancel(cancellation: &InstallCancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Workshop membership operation cancelled",
        ))
    } else {
        Ok(())
    }
}

fn valid_id(id: &str) -> bool {
    (6..=20).contains(&id.len())
        && crate::normalize_steam_workshop_item_ids(&[id.to_owned()]) == [id]
        && id
            .parse::<u64>()
            .ok()
            .is_some_and(|value| value > 0 && value.to_string() == id)
}

fn checked_install(install: &Path, app: u32) -> io::Result<()> {
    if app == 0
        || !install.is_absolute()
        || install
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(steam_depot::invalid(
            "invalid private Workshop install scope",
        ));
    }
    steam_depot::check_plain(install, true)
}

pub(crate) fn membership_path(install: &Path, app: u32, id: &str) -> io::Result<PathBuf> {
    checked_install(install, app)?;
    if !valid_id(id) {
        return Err(steam_depot::invalid(
            "invalid Workshop membership item identity",
        ));
    }
    Ok(install
        .join(".langame-workshop-membership")
        .join(app.to_string())
        .join(format!("{id}.json")))
}

/// The caller must hold the install and SteamCMD leases and have just completed
/// native validation. A global cache directory is never an instance identity.
pub(crate) fn record_native_success(
    install: &Path,
    app: u32,
    id: &str,
    manifest: u64,
    cancellation: &InstallCancellation,
) -> io::Result<()> {
    check_cancel(cancellation)?;
    let path = membership_path(install, app, id)?;
    if manifest == 0 {
        return Err(steam_depot::invalid(
            "Workshop membership has no completed manifest",
        ));
    }
    let bytes = serde_json::to_vec(&Membership {
        version: 1,
        app_id: app,
        item_id: id.into(),
        manifest,
        native_success: true,
    })
    .map_err(|_| steam_depot::invalid("cannot serialize Workshop membership"))?;
    if bytes.len() > MAX_MEMBERSHIP_BYTES {
        return Err(steam_depot::invalid(
            "Workshop membership exceeds its size limit",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| steam_depot::invalid("Workshop membership has no parent"))?;
    crate::dst_workshop_cache_fs::ensure_plain_directory(parent).map_err(|error| match error {
        crate::DstWorkshopCacheError::Io { source, .. } => source,
        other => io::Error::other(other),
    })?;
    steam_depot::check_plain(parent, true)?;
    match fs::symlink_metadata(&path) {
        Ok(_) => steam_depot::check_plain(&path, false)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let temporary = parent.join(format!(
        ".langame-membership-{id}-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    let published = written.and_then(|_| {
        check_cancel(cancellation)?;
        steam_depot::check_plain(parent, true)?;
        match fs::symlink_metadata(&path) {
            Ok(_) => steam_depot::check_plain(&path, false)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // Same-directory rename atomically replaces an existing regular record.
        fs::rename(&temporary, &path)
    });
    if published.is_err() {
        // Never remove the target record after an uncertain publication.
        let _ = fs::remove_file(&temporary);
    }
    published
}

fn read_record(path: &Path, app: u32, id: &str) -> io::Result<()> {
    let bytes = steam_depot::bounded(path, MAX_MEMBERSHIP_BYTES)?;
    let record: Membership = serde_json::from_slice(&bytes)
        .map_err(|_| steam_depot::invalid("invalid Workshop instance membership record"))?;
    if record.version != 1
        || record.app_id != app
        || record.item_id != id
        || record.manifest == 0
        || !record.native_success
    {
        return Err(steam_depot::invalid(
            "Workshop instance membership identity does not match its scope",
        ));
    }
    Ok(())
}

/// Candidates come only from this install's ACF or its native-success records.
/// Shared/global ACFs and payload names are deliberately not inputs. Callers
/// must still verify each selected package against cached native hashes.
pub(crate) fn instance_candidate_ids(
    install: &Path,
    app: u32,
    cancellation: &InstallCancellation,
) -> io::Result<Vec<String>> {
    check_cancel(cancellation)?;
    checked_install(install, app)?;
    let content = install
        .join("steamapps/workshop/content")
        .join(app.to_string());
    let local = crate::workshop_cache::read_installed_inventory(&content, app, false)
        .map_err(io::Error::other)?;
    if local.len() > MAX_ITEMS {
        return Err(steam_depot::invalid(
            "private Workshop ACF exceeds the item limit",
        ));
    }
    let mut ids = BTreeSet::new();
    for id in local.keys() {
        check_cancel(cancellation)?;
        if !valid_id(id) {
            return Err(steam_depot::invalid(
                "private Workshop ACF has an invalid item identity",
            ));
        }
        ids.insert(id.clone());
    }
    let directory = install
        .join(".langame-workshop-membership")
        .join(app.to_string());
    match fs::symlink_metadata(&directory) {
        Ok(_) => steam_depot::check_plain(&directory, true)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // Even an absent app directory cannot hide an existing linked parent.
            for ancestor in directory.ancestors().skip(1) {
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => {
                        steam_depot::check_plain(ancestor, true)?;
                        break;
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            return Ok(ids.into_iter().collect());
        }
        Err(error) => return Err(error),
    }
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        check_cancel(cancellation)?;
        if index >= MAX_ITEMS {
            return Err(steam_depot::invalid(
                "Workshop membership exceeds its scan limit",
            ));
        }
        let entry = entry?;
        steam_depot::check_plain(&entry.path(), false)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| steam_depot::invalid("invalid Workshop membership filename"))?;
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !valid_id(id) {
            return Err(steam_depot::invalid(
                "invalid Workshop membership filename identity",
            ));
        }
        read_record(&entry.path(), app, id)?;
        ids.insert(id.into());
        if ids.len() > MAX_ITEMS {
            return Err(steam_depot::invalid(
                "Workshop candidate union exceeds the item limit",
            ));
        }
    }
    Ok(ids.into_iter().collect())
}

#[cfg(test)]
#[path = "workshop_instance_membership_tests.rs"]
mod tests;
