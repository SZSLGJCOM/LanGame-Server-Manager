use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use sha1::{Digest, Sha1};
use sha2::Sha256;

use super::manifest::{decimal, parse_acf, text};
use super::{
    Entry, MAX_ACF, bounded, check_plain, checked_root, invalid, open_read, read_inventory,
};

/// An exact official file allowlist, including the app's selected ACFs.
/// Unrecognized files are preserved on disk and never enter this inventory.
#[derive(Debug)]
pub struct VerifiedSteamPackage {
    pub files: BTreeMap<String, String>,
    pub directories: BTreeSet<String>,
}

/// Call after successful native installation/validation, while retaining the
/// installation lifecycle lease through baseline publication and persistence.
/// Cached depot CRC/SHA1 checks are integrity checks, not signature verification.
/// The storage consumer must verify these SHA256 values when recording its
/// baseline, so edits after these handles close cannot become trusted content.
pub fn verify_installed_steam_package(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    expected_version: &str,
    cancellation: Option<&AtomicBool>,
) -> io::Result<VerifiedSteamPackage> {
    cancelled(cancellation)?;
    let canonical_root = checked_root(root)?;
    let root = canonical_root.as_path();
    let expected_build = decimal(expected_version)?;
    if app_id == 0 || expected_build == 0 {
        return Err(invalid(
            "Steam certification requires an app ID and build ID",
        ));
    }
    let expected = read_inventory(root, steamcmd, app_id, true, true, cancellation)?;
    let main = format!("steamapps/appmanifest_{app_id}.acf");
    let app = parse_acf(
        expected
            .acfs
            .get(&main)
            .ok_or_else(|| invalid("app ACF missing"))?
            .as_bytes(),
        app_id,
    )?;
    let version = text(
        app.get("buildid")
            .ok_or_else(|| invalid("Steam build ID missing"))?,
    )?;
    if decimal(version)? != expected_build {
        return Err(invalid(
            "installed Steam build differs from completed installation",
        ));
    }
    if let Some(target) = app.get("targetbuildid") {
        let target = decimal(text(target)?)?;
        if target != 0 && target != expected_build {
            return Err(invalid("Steam target build differs from installed build"));
        }
    }
    let mut verified = VerifiedSteamPackage {
        files: BTreeMap::new(),
        directories: expected.directories.clone(),
    };
    for (relative, entry) in &expected.files {
        cancelled(cancellation)?;
        let alternatives = expected.overlapping_files.get(relative);
        let candidates = alternatives
            .map(|items| items.iter().map(|item| &item.entry).collect::<Vec<_>>())
            .unwrap_or_else(|| vec![entry]);
        let digest = verify_file(&root.join(relative), &candidates, cancellation)
            .map_err(|error| io::Error::new(error.kind(), format!("{relative}: {error}")))?;
        verified.files.insert(relative.clone(), digest);
        add_parents(&mut verified.directories, relative);
    }
    for relative in &expected.directories {
        cancelled(cancellation)?;
        check_plain(&root.join(relative), true)?;
        add_parents(&mut verified.directories, relative);
    }
    for (relative, original) in &expected.acfs {
        cancelled(cancellation)?;
        let current = bounded(&root.join(relative), MAX_ACF)?;
        if current != original.as_bytes() {
            return Err(invalid("Steam ACF changed during package verification"));
        }
        verified
            .files
            .insert(relative.clone(), hex(&Sha256::digest(&current)));
        add_parents(&mut verified.directories, relative);
    }
    cancelled(cancellation)?;
    Ok(verified)
}

pub(super) fn cancelled(cancellation: Option<&AtomicBool>) -> io::Result<()> {
    if cancellation.is_some_and(|signal| signal.load(Ordering::Acquire)) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Steam package verification cancelled",
        ))
    } else {
        Ok(())
    }
}

fn add_parents(directories: &mut BTreeSet<String>, relative: &str) {
    for parent in Path::new(relative).ancestors().skip(1) {
        if !parent.as_os_str().is_empty() {
            directories.insert(parent.to_string_lossy().replace('\\', "/"));
        }
    }
}

#[derive(PartialEq, Eq)]
struct Stamp {
    length: u64,
    modified: SystemTime,
    created: Option<SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn stamp(file: &File) -> io::Result<Stamp> {
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(invalid("Steam package handle refers to a reparse point"));
        }
    }
    if !metadata.is_file() {
        return Err(invalid("Steam package handle is not a plain file"));
    }
    Ok(Stamp {
        length: metadata.len(),
        modified: metadata.modified()?,
        created: metadata.created().ok(),
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        },
    })
}

fn verify_file(
    path: &Path,
    candidates: &[&Entry],
    cancellation: Option<&AtomicBool>,
) -> io::Result<String> {
    let mut file = open_read(path)?;
    let before = stamp(&file)?;
    if !candidates.iter().any(|entry| entry.size == before.length) {
        return Err(invalid("official Steam file size mismatch"));
    }
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    loop {
        cancelled(cancellation)?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| invalid("Steam file size overflow"))?;
        if size > before.length {
            return Err(invalid("Steam file grew during verification"));
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    if stamp(&file)? != before {
        return Err(invalid("Steam file metadata changed during verification"));
    }
    check_plain(path, false)?;
    let current = open_read(path)?;
    if stamp(&current)? != before {
        return Err(invalid("Steam file path changed during verification"));
    }
    let digest = hex(&sha1.finalize());
    if !candidates
        .iter()
        .any(|entry| entry.size == size && entry.sha1 == digest)
    {
        return Err(invalid("official Steam file SHA1 mismatch"));
    }
    Ok(hex(&sha256.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "steam_depot_verification_tests.rs"]
mod tests;
