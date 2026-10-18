//! Old Workshop packages require file hashes, not merely ACF total byte counts.
//! A cached native manifest certifies a legacy package locally. Later native
//! `validate` successes persist exact SHA256 receipts outside the payload tree.
//! Neither source proves that an offline package is the latest remote version.
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha1::{Digest as _, Sha1};
use sha2::Sha256;

use crate::steam_depot::{self, Entry};
use crate::workshop_cache::InstalledItem;
use crate::{InstallCancellation, SteamWorkshopDownloadResult};

const MAX_RECEIPT: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 200_000;
static RECEIPT_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct FileDigest {
    pub(super) size: u64,
    pub(super) sha256: String,
}

pub(super) type Files = BTreeMap<String, FileDigest>;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    version: u32,
    app_id: u32,
    item_id: String,
    manifest: u64,
    files: Files,
}

fn check_cancel(cancellation: &InstallCancellation) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Workshop verification cancelled",
        ))
    } else {
        Ok(())
    }
}

fn file_digests(
    path: &Path,
    cancellation: &InstallCancellation,
) -> io::Result<(FileDigest, String)> {
    let mut file = steam_depot::open_read(path)?;
    let before = file.metadata()?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        check_cancel(cancellation)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| steam_depot::invalid("Workshop file overflow"))?;
        if size > before.len() {
            return Err(steam_depot::invalid(
                "Workshop file changed during verification",
            ));
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    let after = file.metadata()?;
    if size != before.len()
        || before.len() != after.len()
        || before.modified()? != after.modified()?
    {
        return Err(steam_depot::invalid(
            "Workshop file changed during verification",
        ));
    }
    // On Windows open_read excludes writers/replacements until this handle is
    // released. Recheck the plain path as well; lifecycle leases exclude app IO.
    steam_depot::check_plain(path, false)?;
    Ok((
        FileDigest {
            size,
            sha256: hex(&sha256.finalize()),
        },
        hex(&sha1.finalize()),
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn inventory(path: &Path, cancellation: &InstallCancellation) -> io::Result<Files> {
    let files = inventory_allow_empty(path, cancellation)?;
    if files.is_empty() {
        return Err(steam_depot::invalid("Workshop package has no files"));
    }
    Ok(files)
}

pub(super) fn inventory_allow_empty(
    path: &Path,
    cancellation: &InstallCancellation,
) -> io::Result<Files> {
    let mut files = BTreeMap::new();
    let mut entries = MAX_ENTRIES;
    walk(path, path, 0, &mut entries, &mut files, cancellation)?;
    let mut spellings = std::collections::HashSet::new();
    if files
        .keys()
        .any(|name| !spellings.insert(steam_depot::folded(name)))
    {
        return Err(steam_depot::invalid("case-conflicting Workshop files"));
    }
    Ok(files)
}

fn walk(
    root: &Path,
    path: &Path,
    depth: usize,
    remaining: &mut usize,
    files: &mut Files,
    cancellation: &InstallCancellation,
) -> io::Result<()> {
    check_cancel(cancellation)?;
    if depth > 64 {
        return Err(steam_depot::invalid(
            "Workshop package nesting exceeds its limit",
        ));
    }
    steam_depot::check_plain(path, true)?;
    for entry in fs::read_dir(path)? {
        check_cancel(cancellation)?;
        *remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| steam_depot::invalid("Workshop package entry limit exceeded"))?;
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.is_dir() {
            walk(root, &child, depth + 1, remaining, files, cancellation)?;
        } else {
            let relative = child.strip_prefix(root).map_err(steam_depot::invalid)?;
            let relative = steam_depot::relative(
                relative
                    .to_str()
                    .ok_or_else(|| steam_depot::invalid("non-Unicode Workshop path"))?,
            )?;
            let digest = file_digests(&child, cancellation)?.0;
            files.insert(relative, digest);
        }
    }
    Ok(())
}

pub(super) fn receipt_path(content: &Path, app: u32, id: &str) -> io::Result<PathBuf> {
    let workshop = content
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| steam_depot::invalid("invalid Workshop content root"))?;
    Ok(workshop
        .join(".langame-validated")
        .join(app.to_string())
        .join(format!("{id}.json")))
}

pub(super) fn verify_old_item(
    content: &Path,
    steamcmd: &Path,
    install: &Path,
    app: u32,
    id: &str,
    installed: InstalledItem,
    cancellation: &InstallCancellation,
) -> io::Result<Files> {
    let path = content.join(id);
    let actual = inventory(&path, cancellation)?;
    if actual
        .values()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.size))
        != Some(installed.bytes)
    {
        return Err(steam_depot::invalid(
            "Workshop package size differs from installed manifest",
        ));
    }
    let receipt = receipt_path(content, app, id)?;
    match steam_depot::bounded(&receipt, MAX_RECEIPT) {
        Ok(bytes) => {
            let receipt: Receipt = serde_json::from_slice(&bytes).map_err(steam_depot::invalid)?;
            if receipt.version != 1
                || receipt.app_id != app
                || receipt.item_id != id
                || receipt.manifest != installed.manifest
                || receipt.files != actual
            {
                return Err(steam_depot::invalid(
                    "Workshop package differs from its native validation receipt",
                ));
            }
            return Ok(actual);
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    // The UGC app/manifest identity must match inside the native manifest too.
    // Unsupported/encrypted/missing manifests never authorize legacy reuse.
    // https://github.com/SteamRE/DepotDownloader/blob/master/DepotDownloader/ContentDownloader.cs
    let mut expected: Option<BTreeMap<String, Entry>> = None;
    for base in [steamcmd, install] {
        let manifest_path = base
            .join("depotcache")
            .join(format!("{app}_{}.manifest", installed.manifest));
        let bytes = match steam_depot::bounded(&manifest_path, steam_depot::MAX_MANIFEST) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let files = steam_depot::manifest::manifest(&bytes, u64::from(app), installed.manifest)?;
        if expected.as_ref().is_some_and(|old| old != &files) {
            return Err(steam_depot::invalid(
                "conflicting cached Workshop manifests",
            ));
        }
        expected = Some(files);
    }
    let expected = expected.ok_or_else(|| steam_depot::invalid("Workshop cache has no file integrity evidence; validate it once while Steam is available"))?;
    if actual.len() != expected.len() || !actual.keys().eq(expected.keys()) {
        return Err(steam_depot::invalid(
            "Workshop package file list differs from native manifest",
        ));
    }
    for (relative, entry) in expected {
        let (digest, sha1) = file_digests(&path.join(&relative), cancellation)?;
        if digest.size != entry.size || sha1 != entry.sha1 {
            return Err(steam_depot::invalid(
                "Workshop file SHA1 differs from native manifest",
            ));
        }
    }
    Ok(actual)
}

/// Publish receipts only for the exact result of a successful native validate,
/// while both lifecycle leases remain held. Call from the caller's owned IO worker.
pub fn record_validated_workshop_items(
    result: &SteamWorkshopDownloadResult,
    guard: &crate::GameInstallLifecycleGuard,
    steamcmd: &crate::SteamCmdLifecycleGuard,
    settings: &app_core::AppSettings,
    cancellation: &InstallCancellation,
) -> Result<(), crate::SteamCmdError> {
    guard.0.ensure_root(Path::new(&result.install_root))?;
    steamcmd.ensure_settings(settings)?;
    for item in &result.items {
        if item.item_id.len() < 6 || !item.item_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(crate::SteamCmdError::MissingWorkshopItemIds);
        }
        let path = Path::new(&item.expected_path);
        let content = path
            .parent()
            .ok_or(crate::SteamCmdError::MissingWorkshopItemIds)?;
        let known = crate::workshop_paths::resolve_workshop_content_roots(
            Path::new(&result.install_root),
            Path::new(&crate::steamcmd_status(settings).root),
            result.consumer_app_id,
        );
        if ![known.0, known.1]
            .iter()
            .any(|root| path == root.join(&item.item_id))
            || !item.expected_path_exists
        {
            return Err(crate::SteamCmdError::MissingWorkshopItemIds);
        }
        record_item(content, result.consumer_app_id, &item.item_id, cancellation).map_err(
            |source| crate::SteamCmdError::WorkshopInventoryRead {
                path: path.to_path_buf(),
                source,
            },
        )?;
        let installed =
            crate::workshop_cache::read_installed_inventory(content, result.consumer_app_id, true)?;
        let installed = installed.get(&item.item_id).ok_or_else(|| {
            crate::SteamCmdError::WorkshopInventoryRead {
                path: path.to_path_buf(),
                source: steam_depot::invalid(
                    "missing native-success Workshop manifest for instance membership",
                ),
            }
        })?;
        crate::workshop_instance_membership::record_native_success(
            Path::new(&result.install_root),
            result.consumer_app_id,
            &item.item_id,
            installed.manifest,
            cancellation,
        )
        .map_err(|source| crate::SteamCmdError::WorkshopInventoryRead {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

fn record_item(
    content: &Path,
    app: u32,
    id: &str,
    cancellation: &InstallCancellation,
) -> io::Result<()> {
    let installed = crate::workshop_cache::read_installed_inventory(content, app, true)
        .map_err(io::Error::other)?;
    let installed = installed
        .get(id)
        .ok_or_else(|| steam_depot::invalid("missing completed Workshop manifest"))?;
    let files = inventory(&content.join(id), cancellation)?;
    if files
        .values()
        .try_fold(0_u64, |sum, file| sum.checked_add(file.size))
        != Some(installed.bytes)
    {
        return Err(steam_depot::invalid(
            "Workshop validation byte count changed",
        ));
    }
    let receipt = Receipt {
        version: 1,
        app_id: app,
        item_id: id.into(),
        manifest: installed.manifest,
        files,
    };
    let bytes = serde_json::to_vec(&receipt).map_err(steam_depot::invalid)?;
    if bytes.len() > MAX_RECEIPT {
        return Err(steam_depot::invalid("Workshop receipt exceeds bound"));
    }
    let path = receipt_path(content, app, id)?;
    crate::dst_workshop_cache_fs::ensure_plain_directory(
        path.parent()
            .ok_or_else(|| steam_depot::invalid("Workshop receipt has no parent"))?,
    )
    .map_err(io::Error::other)?;
    let sequence = RECEIPT_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = path.with_extension(format!("json-{}-{sequence}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let write_result = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    // Existing receipts are only replaced after the new, bounded file is durable.
    let previous = path.with_extension(format!("previous-{}-{sequence}", std::process::id()));
    let had_previous = path.try_exists()?;
    if had_previous {
        steam_depot::check_plain(&path, false)?;
        fs::rename(&path, &previous)?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        if had_previous {
            fs::rename(&previous, &path)?;
        }
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if had_previous {
        fs::remove_file(previous)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "workshop_cache_integrity_tests.rs"]
mod tests;
