//! Recover interrupted cache acquisitions and inspect exact Steam inventories.
//! The caller holds the production lifecycle lease through recovery and installation.
//! Cache CRC/SHA1 checks are integrity checks, not Valve signature authentication.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

#[path = "steam_manifest.rs"]
mod manifest_format;
#[cfg(test)]
use manifest_format::manifest;
use manifest_format::{
    Object, collect_depots, decimal, depot_gid, insert_depot, object, parse_acf, text,
};

const RECEIPT: &str = ".langame-steam-cache-seed.json";
const ACQUISITION: &str = ".langame-program-acquisition.json";
const MAX_MANIFEST: usize = 128 * 1024 * 1024;
const MAX_ACF: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 500_000;
const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024 * 1024;
type Result<T> = io::Result<T>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(super) struct Entry {
    pub(super) size: u64,
    pub(super) sha1: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct DepotCandidate {
    pub(super) depot: u64,
    pub(super) manifest: u64,
    #[serde(flatten)]
    pub(super) entry: Entry,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct SeedSummary {
    pub copied_files: usize,
    pub copied_bytes: u64,
    pub skipped_files: usize,
    pub missing_depots: usize,
    pub verified_files: usize,
    pub verified_bytes: u64,
    pub removed_files: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SeedReceipt {
    version: u32,
    id: String,
    module_id: String,
    app_id: u32,
    source: PathBuf,
    target: PathBuf,
    acquisition: String,
    files: BTreeMap<String, Entry>,
    acfs: BTreeMap<String, String>,
    pub summary: SeedSummary,
}

pub(super) fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Detect this before allowing any ordinary acquisition-to-baseline path.
pub fn load(target: &Path, module_id: &str, app_id: u32) -> Result<Option<SeedReceipt>> {
    match fs::symlink_metadata(target) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(_) => (),
    }
    let target = checked_root(target)?;
    let path = target.join(RECEIPT);
    if !path.try_exists()? {
        return Ok(None);
    }
    let receipt: SeedReceipt = serde_json::from_slice(&bounded(&path, MAX_MANIFEST)?)?;
    if receipt.version != 1
        || receipt.module_id != module_id
        || receipt.app_id != app_id
        || receipt.target != target
        || uuid::Uuid::parse_str(&receipt.id).is_err()
    {
        return Err(invalid("cache seed receipt belongs to another acquisition"));
    }
    validate_entries(&receipt.files)?;
    if receipt.acfs.len() > 128 {
        return Err(invalid("too many receipt ACF files"));
    }
    for (path, contents) in &receipt.acfs {
        let owner = acf_app_id(path)?;
        parse_acf(contents.as_bytes(), owner)?;
    }
    validate_acquisition(&receipt, receipt.acquisition.as_bytes())?;
    if !receipt.target.join(ACQUISITION).try_exists()? {
        // Baseline can succeed before DB registration. Restore only the captured
        // allocation identity; the next installer/finalize must still run fully.
        write_new(
            &receipt.target.join(ACQUISITION),
            receipt.acquisition.as_bytes(),
        )?;
    }
    check_acquisition(&receipt)?;
    Ok(Some(receipt))
}

/// Build historical cache receipts for recovery tests without modifying source files.
/// Missing depot metadata limits the fixture; final verification remains mandatory.
#[cfg(test)]
pub fn prepare(
    source: &Path,
    target: &Path,
    steamcmd: &Path,
    app_id: u32,
    module_id: &str,
) -> Result<SeedReceipt> {
    let source = checked_root(source)?;
    let target = checked_root(target)?;
    if source.starts_with(&target) || target.starts_with(&source) {
        return Err(invalid("cache source and acquisition overlap"));
    }
    let mut receipt = if let Some(receipt) = load(&target, module_id, app_id)? {
        if receipt.source != source {
            return Err(invalid("cache seed source changed"));
        }
        receipt
    } else {
        // No adoption of arbitrary nonempty directories, even with an acquisition marker.
        for entry in fs::read_dir(&target)? {
            if entry?.file_name() != ACQUISITION {
                return Err(invalid("cache target is not a fresh acquisition"));
            }
        }
        let inventory = inventory(&source, steamcmd, app_id, false)?;
        let acquisition =
            String::from_utf8(bounded(&target.join(ACQUISITION), 4096)?).map_err(invalid)?;
        let receipt = SeedReceipt {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            module_id: module_id.into(),
            app_id,
            source,
            target,
            acquisition,
            files: inventory.files,
            acfs: inventory.acfs,
            summary: SeedSummary {
                missing_depots: inventory.missing_depots,
                ..Default::default()
            },
        };
        check_acquisition(&receipt)?;
        let bytes = serde_json::to_vec(&receipt)?;
        if bytes.len() > MAX_MANIFEST {
            return Err(invalid("cache receipt too large"));
        }
        // Persist ownership before the first payload copy, so interruption is resumable.
        write_new(&receipt.target.join(RECEIPT), &bytes)?;
        receipt
    };
    let mut progress = Progress::new("steam_cache_copy", app_id);
    let temporary = receipt
        .target
        .join(format!(".langame-cache-copy-{}", receipt.id));
    if temporary.try_exists()? {
        check_plain(&temporary, false)?;
        fs::remove_file(&temporary)?;
    }
    for (relative, expected) in &receipt.files {
        let destination = receipt.target.join(relative);
        if destination.try_exists()? {
            // Steam may already have updated this file before an interrupted retry.
            check_plain(&destination, false)?;
            continue;
        }
        let copied = copy_matching(
            &receipt.source,
            &receipt.target,
            relative,
            expected,
            &temporary,
            &mut progress,
        )?;
        if copied {
            receipt.summary.copied_files += 1;
            receipt.summary.copied_bytes += expected.size;
        } else {
            receipt.summary.skipped_files += 1;
        }
    }
    // Old ACFs describe a fully installed source, not this partial acquisition.
    // Publishing them makes Steam plan deltas against uncopied old chunks.
    // Keep them only as source evidence; Steam must create the target ACF itself.
    check_acquisition(&receipt)?;
    progress.emit()?;
    Ok(receipt)
}

/// Call only after successful production `app_update ... validate` under the same lease.
/// Re-read the NEW depot set; an old ACF or executable cannot certify this target.
pub fn finalize(receipt: &SeedReceipt, steamcmd: &Path) -> Result<SeedSummary> {
    check_acquisition(receipt)?;
    let current = inventory(&receipt.target, steamcmd, receipt.app_id, true)?;
    let mut summary = SeedSummary::default();
    let mut progress = Progress::new("steam_cache_verify", receipt.app_id);
    for (relative, expected) in &current.files {
        if !matches_file(&receipt.target.join(relative), expected, &mut progress)? {
            return Err(invalid(format!(
                "updated Steam file failed size/SHA1 verification: {relative}"
            )));
        }
        summary.verified_files += 1;
        summary.verified_bytes += expected.size;
    }
    let keys: BTreeSet<_> = current.files.keys().map(|key| folded(key)).collect();
    for (relative, expected) in &receipt.files {
        if keys.contains(&folded(relative)) {
            continue;
        }
        let path = receipt.target.join(relative);
        if !path.try_exists()? {
            continue;
        }
        if !matches_file(&path, expected, &mut progress)? {
            return Err(invalid(format!(
                "retired cache file changed; refusing removal: {relative}"
            )));
        }
        fs::remove_file(path)?;
        summary.removed_files += 1;
    }
    for (relative, original) in &receipt.acfs {
        if current.acfs.contains_key(relative) {
            continue;
        }
        let path = receipt.target.join(relative);
        if path.try_exists()? {
            if bounded(&path, MAX_ACF)? != original.as_bytes() {
                return Err(invalid("retired dependency ACF changed; refusing removal"));
            }
            fs::remove_file(path)?;
        }
    }
    check_tree(receipt, &current)?;
    check_acquisition(receipt)?;
    progress.emit()?;
    Ok(summary)
}

/// Only after baseline recording and DB registration both succeed under the lease.
pub fn finish(receipt: &SeedReceipt) -> Result<()> {
    let current: SeedReceipt =
        serde_json::from_slice(&bounded(&receipt.target.join(RECEIPT), MAX_MANIFEST)?)?;
    if current.version != receipt.version
        || current.id != receipt.id
        || current.module_id != receipt.module_id
        || current.app_id != receipt.app_id
        || current.target != receipt.target
        || current.source != receipt.source
        || current.acquisition != receipt.acquisition
        || current.files != receipt.files
        || current.acfs != receipt.acfs
        || receipt.target.join(ACQUISITION).try_exists()?
    {
        return Err(invalid(
            "cache receipt changed or baseline acquisition is not complete",
        ));
    }
    fs::remove_file(receipt.target.join(RECEIPT))
}

fn check_acquisition(receipt: &SeedReceipt) -> Result<()> {
    let bytes = bounded(&receipt.target.join(ACQUISITION), 4096)?;
    validate_acquisition(receipt, &bytes)
}

fn validate_acquisition(receipt: &SeedReceipt, bytes: &[u8]) -> Result<()> {
    if bytes != receipt.acquisition.as_bytes() {
        return Err(invalid("acquisition ownership changed"));
    }
    if bytes.len() > 4096 {
        return Err(invalid("acquisition metadata exceeds bound"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let path = value["target"]
        .as_str()
        .ok_or_else(|| invalid("acquisition target missing"))?;
    if value["version"].as_u64() != Some(1)
        || value["module_id"].as_str() != Some(receipt.module_id.as_str())
        || uuid::Uuid::parse_str(value["id"].as_str().unwrap_or("")).is_err()
        || checked_root(Path::new(path))? != receipt.target
    {
        return Err(invalid("invalid acquisition ownership"));
    }
    Ok(())
}

pub(super) fn folded(value: &str) -> String {
    value.to_uppercase()
}

fn relative(value: &str) -> Result<String> {
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

pub(super) fn check_plain(path: &Path, directory: bool) -> Result<()> {
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

pub(super) fn checked_root(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute()
        || root
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("absolute cache root without traversal required"));
    }
    check_plain(root, true)?;
    dunce::canonicalize(root)
}

pub(super) fn open_read(path: &Path) -> Result<File> {
    check_plain(path, false)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    options.open(path)
}

pub(super) fn bounded(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    open_read(path)?
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum {
        return Err(invalid("cache metadata exceeds bound"));
    }
    Ok(bytes)
}

#[cfg(test)]
fn create_parents(root: &Path, name: &str) -> Result<()> {
    let parent = Path::new(name)
        .parent()
        .ok_or_else(|| invalid("missing relative parent"))?;
    let mut path = root.to_owned();
    for part in parent.components() {
        path.push(part);
        match fs::create_dir(&path) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error),
        }
        check_plain(&path, true)?;
    }
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    check_plain(
        path.parent().ok_or_else(|| invalid("missing parent"))?,
        true,
    )?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

pub(super) struct Progress {
    stage: &'static str,
    app_id: u32,
    bytes: u64,
    last: Instant,
}
impl Progress {
    pub(super) fn new(stage: &'static str, app_id: u32) -> Self {
        Self {
            stage,
            app_id,
            bytes: 0,
            last: Instant::now(),
        }
    }
    fn add(&mut self, bytes: usize) -> Result<()> {
        self.bytes += bytes as u64;
        if self.last.elapsed() >= Duration::from_secs(10) {
            self.emit()?;
        }
        Ok(())
    }
    pub(super) fn emit(&mut self) -> Result<()> {
        let mut output = io::stdout().lock();
        writeln!(
            output,
            "{}",
            serde_json::json!({"event":self.stage,"app_id":self.app_id,"hashed_bytes":self.bytes})
        )?;
        output.flush()?;
        self.last = Instant::now();
        Ok(())
    }
}

pub(super) fn hash_reader(
    mut reader: impl Read,
    mut output: Option<&mut File>,
    size: u64,
    progress: &mut Progress,
) -> Result<(u64, String)> {
    let mut hash = Sha1::new();
    let mut buffer = [0_u8; 256 * 1024];
    let mut count = 0;
    let mut reader = (&mut reader).take(size + 1);
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        hash.update(&buffer[..n]);
        if let Some(file) = output.as_mut() {
            file.write_all(&buffer[..n])?;
        }
        progress.add(n)?;
    }
    Ok((
        count,
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

fn matches_file(path: &Path, expected: &Entry, progress: &mut Progress) -> Result<bool> {
    let file = open_read(path)?;
    if file.metadata()?.len() != expected.size {
        return Ok(false);
    }
    let (size, hash) = hash_reader(file, None, expected.size, progress)?;
    Ok(size == expected.size && hash == expected.sha1)
}

#[cfg(test)]
fn copy_matching(
    source: &Path,
    target: &Path,
    name: &str,
    expected: &Entry,
    temporary: &Path,
    progress: &mut Progress,
) -> Result<bool> {
    let input = match open_read(&source.join(name)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if input.metadata()?.len() != expected.size {
        return Ok(false);
    }
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)?;
        let (size, hash) = hash_reader(input, Some(&mut output), expected.size, progress)?;
        if size != expected.size || hash != expected.sha1 {
            return Ok(false);
        }
        output.sync_all()?;
        drop(output);
        create_parents(target, name)?;
        let destination = target.join(name);
        if destination.try_exists()? {
            return Err(invalid("cache destination appeared during copy"));
        }
        fs::rename(temporary, destination)?;
        Ok(true)
    })();
    if temporary.try_exists()? {
        fs::remove_file(temporary)?;
    }
    result
}

#[derive(Default)]
pub(super) struct Inventory {
    pub(super) files: BTreeMap<String, Entry>,
    pub(super) overlapping_files: BTreeMap<String, Vec<DepotCandidate>>,
    pub(super) directories: BTreeSet<String>,
    pub(super) acfs: BTreeMap<String, String>,
    pub(super) missing_manifests: Vec<PathBuf>,
    missing_depots: usize,
}

pub(super) fn inventory(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    strict: bool,
) -> Result<Inventory> {
    read_inventory(root, steamcmd, app_id, strict, false)
}

/// Preserve declared official alternatives for observation, without guessing
/// Steam's mount priority. The seed copier continues to reject differing bytes.
pub(super) fn inventory_for_inspection(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    strict: bool,
) -> Result<Inventory> {
    read_inventory(root, steamcmd, app_id, strict, true)
}

fn read_inventory(
    root: &Path,
    steamcmd: &Path,
    app_id: u32,
    strict: bool,
    retain_overlaps: bool,
) -> Result<Inventory> {
    let cache = checked_root(steamcmd)?.join("depotcache");
    let mut result = Inventory::default();
    let app = read_acf(root, app_id, &mut result.acfs)?;
    let mut depots = BTreeMap::new();
    collect_depots(&app, &mut depots)?;
    if let Some(shared) = app.get("shareddepots") {
        for (depot, owner) in object(shared)? {
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
        let (files, directories) = manifest_format::manifest_inventory(&bytes, depot, gid)?;
        result.directories.extend(directories);
        if result.directories.len() > MAX_FILES {
            return Err(invalid("too many mounted Steam directories"));
        }
        for (name, entry) in files {
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

fn validate_entries(files: &BTreeMap<String, Entry>) -> Result<()> {
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

fn check_tree(receipt: &SeedReceipt, inventory: &Inventory) -> Result<()> {
    let mut allowed: BTreeSet<_> = inventory
        .files
        .keys()
        .chain(inventory.acfs.keys())
        .map(|name| folded(name))
        .collect();
    allowed.extend([
        folded(ACQUISITION),
        folded(RECEIPT),
        folded(".langame-initial-package.json"),
        folded(".langame-clean-package.json"),
    ]);
    let mut directories = vec![receipt.target.clone()];
    let mut count = 0;
    while let Some(directory) = directories.pop() {
        check_plain(&directory, true)?;
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.components().count() > receipt.target.components().count() + 64 {
                return Err(invalid("acquisition tree exceeds depth bound"));
            }
            count += 1;
            if count > MAX_FILES * 2 {
                return Err(invalid("acquisition tree exceeds entry bound"));
            }
            if entry.file_type()?.is_dir() {
                check_plain(&path, true)?;
                directories.push(path);
                continue;
            }
            check_plain(&path, false)?;
            let key = path
                .strip_prefix(&receipt.target)
                .map_err(invalid)?
                .to_string_lossy()
                .replace('\\', "/");
            if !allowed.contains(&folded(&key)) {
                return Err(invalid(format!(
                    "unaccounted acquisition file prevents clean baseline: {key}"
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn acf_app_id(path: &str) -> Result<u32> {
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

#[cfg(test)]
#[path = "steam_seed_tests.rs"]
mod tests;
