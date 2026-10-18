//! Offline cache evidence is bound to installed revisions and actual content.
//! CFCore module fingerprints establish its published content baseline, not
//! cryptographic authenticity: its Murmur2 deliberately ignores whitespace.
//! The SHA-256 snapshot detects every subsequent byte change, including those
//! the SDK fingerprint ignores. Neither proof establishes the latest version.
use serde_json::Value;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "commands_asa_curseforge_cache_files.rs"]
mod files;
#[cfg(test)]
use files::{Murmur, whitespace};
use files::{murmur_bytes, plain_metadata, read_json, read_tree};
const GAME_ID: i64 = 83374;
const MAX_LIBRARY_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 200_000;
const MAX_CONTENT_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileEvidence {
    pub(super) bytes: u64,
    pub(super) sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ModEvidence {
    pub(super) project_id: String,
    pub(super) file_id: i64,
    pub(super) path_on_disk: String,
    pub(super) module_fingerprints: BTreeMap<String, u32>,
    pub(super) directories: BTreeSet<String>,
    pub(super) files: BTreeMap<String, FileEvidence>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Evidence {
    pub(super) sdk_root: PathBuf,
    pub(super) mods: Vec<ModEvidence>,
}

struct Budget<'a> {
    entries: usize,
    bytes: u64,
    deadline: Instant,
    cancelled: &'a dyn Fn() -> bool,
}

impl Budget<'_> {
    fn check(&self) -> Result<(), String> {
        if (self.cancelled)() {
            return Err("ASA cache verification was cancelled.".into());
        }
        if Instant::now() >= self.deadline {
            return Err("ASA cache verification exceeded its time budget.".into());
        }
        Ok(())
    }
}

fn unverifiable(reason: &str) -> String {
    format!("ASA cache_unverifiable: {reason}")
}

/// Reads SDK-owned metadata without changing library.json or user contexts.
/// Call before native mutation, or after that mutation has reached a terminal
/// state; a previous snapshot cannot prove a failed install preserved its files.
pub(super) fn inspect(
    sdk_root: &Path,
    project_ids: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<Evidence, String> {
    let mut budget = Budget {
        entries: 0,
        bytes: 0,
        deadline: Instant::now() + Duration::from_secs(1200),
        cancelled,
    };
    budget.check()?;
    if project_ids.is_empty() || project_ids.len() > 8192 {
        return Err(unverifiable("the selected Mod list is empty or too large."));
    }
    let ids = project_ids
        .iter()
        .map(|id| {
            id.parse::<i64>()
                .ok()
                .filter(|number| *number > 0 && number.to_string() == *id)
                .ok_or_else(|| unverifiable("a selected Mod identity is invalid."))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if ids.len() != project_ids.len() || !sdk_root.is_absolute() {
        return Err(unverifiable(
            "Mod identities or the SDK root are ambiguous.",
        ));
    }
    plain_metadata(sdk_root)?;
    let sdk_root = dunce::canonicalize(sdk_root)
        .map_err(|_| unverifiable("the native SDK root is unavailable."))?;
    let library = read_json(
        &sdk_root.join("ModsUserData/83374/library.json"),
        MAX_LIBRARY_BYTES,
    )?;
    let records = library["installedMods"]
        .as_array()
        .filter(|items| items.len() <= 8192)
        .ok_or_else(|| unverifiable("the installed library has no bounded Mod records."))?;
    let mut mods = Vec::new();
    for id in ids {
        budget.check()?;
        let mut matches = records
            .iter()
            .filter(|item| item["details"]["iD"].as_i64() == Some(id));
        let record = matches
            .next()
            .ok_or_else(|| unverifiable("a selected Mod has no installation record."))?;
        if matches.next().is_some() {
            return Err(unverifiable(
                "a selected Mod has duplicate installation records.",
            ));
        }
        mods.push(inspect_mod(&sdk_root, id, record, &mut budget)?);
    }
    Ok(Evidence { sdk_root, mods })
}

/// Rechecks SDK record identity, the official content fingerprints and the exact
/// byte snapshot. An update may remove the old payload before its move fails.
pub(super) fn verify_current(
    snapshot: &Evidence,
    cancelled: &dyn Fn() -> bool,
) -> Result<(), String> {
    let ids = snapshot
        .mods
        .iter()
        .map(|item| item.project_id.clone())
        .collect::<Vec<_>>();
    if inspect(&snapshot.sdk_root, &ids, cancelled)? != *snapshot {
        return Err(unverifiable(
            "the cached revision or its exact content changed after verification.",
        ));
    }
    Ok(())
}

fn inspect_mod(
    root: &Path,
    id: i64,
    record: &Value,
    budget: &mut Budget<'_>,
) -> Result<ModEvidence, String> {
    let installed = &record["installedFile"];
    let file_id = installed["iD"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(|| unverifiable("a cached installed-file identity is missing."))?;
    if record["details"]["gameId"].as_i64() != Some(GAME_ID)
        || installed["gameId"].as_i64() != Some(GAME_ID)
        || installed["modId"].as_i64() != Some(id)
        || record["unmanaged"] != false
        || !matches!(record["status"].as_str(), Some("Normal" | "OutOfDate"))
    {
        return Err(unverifiable(
            "a cached Mod is foreign, unmanaged, incomplete or invalid.",
        ));
    }
    let relative = format!("{GAME_ID}/{id}_{file_id}");
    let path = record["pathOnDisk"]
        .as_str()
        .ok_or_else(|| unverifiable("a cached Mod path is missing."))?;
    if path.replace('\\', "/") != relative {
        return Err(unverifiable(
            "a cached Mod path does not match its exact installed revision.",
        ));
    }
    let expected_size = installed["fileSizeOnDisk"]
        .as_u64()
        .filter(|size| *size > 0)
        .ok_or_else(|| unverifiable("the published installed-file size is missing."))?;
    let modules = installed["modules"]
        .as_array()
        .filter(|items| !items.is_empty() && items.len() <= 1024)
        .ok_or_else(|| unverifiable("the published module fingerprints are missing."))?;
    let payload = root.join("Mods").join(&relative);
    let mut files = BTreeMap::new();
    let mut directories = BTreeSet::new();
    let mut fingerprints = BTreeMap::new();
    let mut total_size = 0_u64;
    let mut names = BTreeSet::new();
    for module in modules {
        let name = module["name"]
            .as_str()
            .ok_or_else(|| unverifiable("a module name is missing."))?;
        safe_component(name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return Err(unverifiable("module names collide on Windows."));
        }
        let fingerprint = module["fingerprint"]
            .as_u64()
            .filter(|hash| *hash > 0 && *hash <= u32::MAX as u64)
            .ok_or_else(|| unverifiable("a published module fingerprint is unavailable."))?
            as u32;
        let mut module_files = BTreeMap::new();
        let mut hashes = Vec::new();
        let mut module_directories = BTreeSet::new();
        read_tree(
            &payload.join(name),
            &mut module_files,
            &mut module_directories,
            &mut hashes,
            &mut total_size,
            budget,
        )?;
        require_server_payload(name, &module_files)?;
        hashes.sort_unstable();
        let combined = hashes.iter().map(u32::to_string).collect::<String>();
        if hashes.is_empty() || murmur_bytes(combined.as_bytes()) != fingerprint {
            return Err(unverifiable(
                "cached module content does not match its published fingerprint.",
            ));
        }
        for (relative, evidence) in module_files {
            files.insert(format!("{name}/{relative}"), evidence);
        }
        for relative in module_directories {
            directories.insert(if relative.is_empty() {
                name.into()
            } else {
                format!("{name}/{relative}")
            });
        }
        fingerprints.insert(name.to_owned(), fingerprint);
    }
    // Only declared modules may supply native content. Extra files inside a
    // module are accepted when its published fingerprint covers them.
    let mut found = BTreeSet::new();
    for entry in
        fs::read_dir(&payload).map_err(|_| unverifiable("the cached payload is missing."))?
    {
        budget.check()?;
        let entry = entry.map_err(|_| unverifiable("the cached payload cannot be enumerated."))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| unverifiable("a payload path is not Unicode."))?;
        let metadata = plain_metadata(&entry.path())?;
        if !metadata.is_dir() || !found.insert(name.to_ascii_lowercase()) {
            return Err(unverifiable(
                "the payload contains undeclared content or colliding paths.",
            ));
        }
    }
    if found != names || total_size != expected_size {
        return Err(unverifiable(
            "the cached payload modules or total size are incomplete.",
        ));
    }
    Ok(ModEvidence {
        project_id: id.to_string(),
        file_id,
        path_on_disk: relative,
        module_fingerprints: fingerprints,
        directories,
        files,
    })
}

fn require_server_payload(
    name: &str,
    files: &BTreeMap<String, FileEvidence>,
) -> Result<(), String> {
    let files = files
        .iter()
        .map(|(path, file)| (path.to_ascii_lowercase(), file))
        .collect::<BTreeMap<_, _>>();
    let descriptor = format!("{name}.uplugin").to_ascii_lowercase();
    if files.get(&descriptor).is_none_or(|file| file.bytes == 0) {
        return Err(unverifiable("the native module descriptor is missing."));
    }
    let server_files = files
        .iter()
        .filter(|(path, _)| path.starts_with("content/paks/windowsserver/"));
    let mut has_payload = false;
    for (path, file) in server_files {
        if file.bytes == 0
            && [".pak", ".utoc", ".ucas"]
                .iter()
                .any(|extension| path.ends_with(extension))
        {
            return Err(unverifiable("a server content package is empty."));
        }
        if let Some(stem) = path.strip_suffix(".utoc") {
            if files
                .get(&format!("{stem}.ucas"))
                .is_none_or(|file| file.bytes == 0)
            {
                return Err(unverifiable("a server IoStore package is incomplete."));
            }
            has_payload = true;
        } else if let Some(stem) = path.strip_suffix(".ucas") {
            let base = stem
                .rsplit_once("_s")
                .filter(|(_, partition)| {
                    !partition.is_empty() && partition.bytes().all(|byte| byte.is_ascii_digit())
                })
                .map_or(stem, |(base, _)| base);
            if [format!("{stem}.utoc"), format!("{base}.utoc")]
                .iter()
                .all(|path| files.get(path).is_none_or(|file| file.bytes == 0))
            {
                return Err(unverifiable("a server IoStore table is missing."));
            }
        } else if path.ends_with(".pak") {
            has_payload = true;
        }
    }
    if !has_payload {
        return Err(unverifiable(
            "no recognized native WindowsServer content package is available.",
        ));
    }
    Ok(())
}

fn safe_component(name: &str) -> Result<(), String> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(unverifiable("a cache path component is unsafe on Windows."));
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_asa_curseforge_cache_tests.rs"]
mod tests;
