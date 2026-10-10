//! Exact installed Steam depot inventories shared by desktop acquisition and
//! catalog inspection. Local CRC/SHA checks do not authenticate Valve signatures.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

#[path = "steam_depot_manifest.rs"]
pub mod manifest;
#[path = "steam_depot_verification.rs"]
mod verification;
use manifest::{Object, collect_depots, decimal, depot_gid, insert_depot, object, parse_acf, text};
pub use verification::{VerifiedSteamPackage, verify_installed_steam_package};

pub const MAX_MANIFEST: usize = 128 * 1024 * 1024;
pub const MAX_ACF: usize = 4 * 1024 * 1024;
pub const MAX_FILES: usize = 500_000;
pub const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;
type Result<T> = io::Result<T>;
pub fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub size: u64,
    pub sha1: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DepotCandidate {
    pub depot: u64,
    pub manifest: u64,
    #[serde(flatten)]
    pub entry: Entry,
}

pub fn folded(value: &str) -> String {
    value.to_uppercase()
}

pub fn relative(value: &str) -> Result<String> {
    let value = value.replace('\\', "/");
    if value.len() > 2048 || value.split('/').count() > 64 {
        return Err(invalid("excessive cache path"));
    }
    for part in value.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if part.is_empty()
            || matches!(part, "." | "..")
            || part.ends_with([' ', '.'])
            || part
                .chars()
                .any(|c| c.is_control() || ":<>\"|?*".contains(c))
            || part.to_ascii_lowercase().starts_with(".langame-")
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(invalid("unsafe Steam relative path"));
        }
    }
    Ok(value)
}

pub fn check_plain(path: &Path, directory: bool) -> Result<()> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked || metadata.file_type().is_symlink() || (ancestor != path && !metadata.is_dir()) {
            return Err(invalid("linked or non-directory cache ancestor"));
        }
        if ancestor == path
            && (if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            })
        {
            return Err(invalid("unexpected cache entry type"));
        }
    }
    Ok(())
}

pub fn checked_root(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("absolute cache root without traversal required"));
    }
    check_plain(root, true)?;
    fs::canonicalize(root)
}
pub fn open_read(path: &Path) -> Result<File> {
    check_plain(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options
            .share_mode(1)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

pub fn bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    open_read(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("cache metadata exceeds bound"));
    }
    Ok(bytes)
}

#[derive(Default)]
pub struct Inventory {
    pub files: BTreeMap<String, Entry>,
    pub overlapping_files: BTreeMap<String, Vec<DepotCandidate>>,
    pub directories: BTreeSet<String>,
    pub acfs: BTreeMap<String, String>,
    pub missing_manifests: Vec<PathBuf>,
    pub missing_depots: usize,
}

pub fn inventory(root: &Path, steamcmd: &Path, app_id: u32, strict: bool) -> Result<Inventory> {
    read_inventory(root, steamcmd, app_id, strict, false, None)
}

/// Preserve declared official alternatives for observation, without guessing
/// Steam's mount priority. The seed copier continues to reject differing bytes.
pub fn inventory_for_inspection(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    strict: bool,
) -> Result<Inventory> {
    read_inventory(root, steamcmd, app_id, strict, true, None)
}

pub(crate) fn read_inventory(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    strict: bool,
    retain_overlaps: bool,
    cancellation: Option<&std::sync::atomic::AtomicBool>,
) -> Result<Inventory> {
    verification::cancelled(cancellation)?;
    let cache = checked_root(steamcmd)?.join("depotcache");
    let mut result = Inventory::default();
    let app = read_acf(root, app_id, &mut result.acfs)?;
    let mut depots = BTreeMap::new();
    collect_depots(&app, &mut depots)?;
    if let Some(shared) = app.get("shareddepots") {
        for (depot, owner) in object(shared)? {
            verification::cancelled(cancellation)?;
            let owner = decimal(text(owner)?)?;
            let owner = u32::try_from(owner).map_err(invalid)?;
            let dependency = read_acf(root, owner, &mut result.acfs)?;
            let installed = object(
                dependency
                    .get("installeddepots")
                    .ok_or_else(|| invalid("shared depots missing"))?,
            )?;
            let details = installed
                .get(depot)
                .ok_or_else(|| invalid("shared depot absent from owner ACF"))?;
            insert_depot(&mut depots, decimal(depot)?, depot_gid(details)?)?;
        }
    }
    if depots.is_empty() || depots.len() > 128 {
        return Err(invalid("missing or excessive installed depots"));
    }
    let mut origins = BTreeMap::new();
    let mut candidate_count = 0_usize;
    for (depot, gid) in depots {
        verification::cancelled(cancellation)?;
        let path = cache.join(format!("{depot}_{gid}.manifest"));
        let bytes = match bounded(&path, MAX_MANIFEST) {
            Ok(bytes) => bytes,
            Err(error) if !strict && error.kind() == io::ErrorKind::NotFound => {
                result.missing_depots += 1;
                result.missing_manifests.push(path);
                continue;
            }
            Err(error) => return Err(error),
        };
        let (files, directories) = manifest::manifest_inventory(&bytes, depot, gid)?;
        result.directories.extend(directories);
        if result.directories.len() > MAX_FILES {
            return Err(invalid("too many mounted Steam directories"));
        }
        for (name, entry) in files {
            verification::cancelled(cancellation)?;
            if let Some(old) = result.files.get(&name) {
                if !retain_overlaps && old != &entry {
                    return Err(invalid(format!(
                        "conflicting content in mounted depots: {name}"
                    )));
                }
                if retain_overlaps {
                    let candidates =
                        result
                            .overlapping_files
                            .entry(name.clone())
                            .or_insert_with(|| {
                                candidate_count += 1;
                                let (depot, manifest) = origins[&name];
                                vec![DepotCandidate {
                                    depot,
                                    manifest,
                                    entry: old.clone(),
                                }]
                            });
                    candidates.push(DepotCandidate {
                        depot,
                        manifest: gid,
                        entry,
                    });
                    candidate_count += 1;
                    if candidate_count > MAX_FILES * 2 {
                        return Err(invalid("too many overlapping Steam file candidates"));
                    }
                }
            } else {
                origins.insert(name.clone(), (depot, gid));
                result.files.insert(name, entry);
            }
            if result.files.len() > MAX_FILES {
                return Err(invalid("too many mounted Steam files"));
            }
        }
    }
    validate_entries(&result.files)?;
    if retain_overlaps {
        // Individually bounded depots can have different large files at the
        // same paths. Bound every possible selected combination, not just the
        // first representative entries retained for path validation.
        let mut maximum_bytes = 0_u64;
        for (name, entry) in &result.files {
            let maximum = result
                .overlapping_files
                .get(name)
                .and_then(|candidates| {
                    candidates
                        .iter()
                        .map(|candidate| candidate.entry.size)
                        .max()
                })
                .unwrap_or(entry.size);
            maximum_bytes = maximum_bytes
                .checked_add(maximum)
                .ok_or_else(|| invalid("Steam candidate size overflow"))?;
            if maximum_bytes > MAX_BYTES {
                return Err(invalid("Steam candidate package exceeds byte bound"));
            }
        }
    }
    let file_keys: BTreeSet<_> = result.files.keys().map(|path| folded(path)).collect();
    let mut spellings = BTreeMap::new();
    for path in result.files.keys().chain(result.directories.iter()) {
        let mut prefix = String::new();
        for part in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            let key = folded(&prefix);
            if let Some(old) = spellings.insert(key.clone(), prefix.clone())
                && old != prefix
            {
                return Err(invalid("case-conflicting mounted directory"));
            }
            if spellings.len() > MAX_FILES * 2 {
                return Err(invalid("too many mounted directory prefixes"));
            }
            if (prefix != *path || result.directories.contains(path)) && file_keys.contains(&key) {
                return Err(invalid("mounted directory conflicts with a file"));
            }
        }
    }
    if strict && result.files.is_empty() {
        return Err(invalid("updated package has no verified files"));
    }
    Ok(result)
}

pub fn validate_entries(files: &BTreeMap<String, Entry>) -> Result<()> {
    if files.len() > MAX_FILES {
        return Err(invalid("too many Steam files"));
    }
    let mut spellings = BTreeMap::new();
    let mut total = 0_u64;
    let file_keys: BTreeSet<_> = files.keys().map(|name| folded(name)).collect();
    for (name, entry) in files {
        if relative(name)? != *name
            || entry.size > MAX_BYTES
            || entry.sha1.len() != 40
            || !entry
                .sha1
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(invalid("invalid Steam inventory entry"));
        }
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| invalid("Steam size overflow"))?;
        if total > MAX_BYTES {
            return Err(invalid("Steam package exceeds byte bound"));
        }
        let mut prefix = String::new();
        for part in name.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if let Some(old) = spellings.insert(folded(&prefix), prefix.clone())
                && old != prefix
            {
                return Err(invalid("case-conflicting Steam paths"));
            }
            if spellings.len() > MAX_FILES * 2 {
                return Err(invalid("too many Steam path prefixes"));
            }
            if prefix != *name && file_keys.contains(&folded(&prefix)) {
                return Err(invalid("Steam file is also a directory ancestor"));
            }
        }
    }
    Ok(())
}

pub fn acf_app_id(path: &str) -> Result<u32> {
    let id = path
        .strip_prefix("steamapps/appmanifest_")
        .and_then(|s| s.strip_suffix(".acf"))
        .ok_or_else(|| invalid("invalid dependency ACF path"))?;
    u32::try_from(decimal(id)?).map_err(invalid)
}

fn read_acf(root: &Path, id: u32, acfs: &mut BTreeMap<String, String>) -> Result<Object> {
    if acfs.len() > 128 {
        return Err(invalid("too many dependency ACF files"));
    }
    let relative = format!("steamapps/appmanifest_{id}.acf");
    let bytes = bounded(&root.join(&relative), MAX_ACF)?;
    let app = parse_acf(&bytes, id)?;
    acfs.insert(relative, String::from_utf8(bytes).map_err(invalid)?);
    Ok(app)
}
