//! Bounded ordinary-file reads and the official CFCore module fingerprint.
use super::{
    Budget, FileEvidence, MAX_CONTENT_BYTES, MAX_DEPTH, MAX_ENTRIES, safe_component, unverifiable,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path};
pub(super) fn plain_metadata(path: &Path) -> Result<Metadata, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(unverifiable("the cache path is not absolute and ordinary."));
    }
    crate::commands::commands_mods::validate_mod_inventory_root_chain(path)?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| unverifiable("a cached path is missing or inaccessible."))?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(unverifiable(
            "a cached path is not an ordinary file or directory.",
        ));
    }
    Ok(metadata)
}

fn open_file(path: &Path) -> Result<(File, Vec<File>), String> {
    if !plain_metadata(path)?.is_file() {
        return Err(unverifiable("a cached document is not a regular file."));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    let mut parents = Vec::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::fs::OpenOptionsExt;
        // Pin every ancestor against rename/deletion while the file is read.
        // OPEN_REPARSE_POINT prevents a raced-in link from being followed.
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            let guard = OpenOptions::new()
                .read(true)
                .share_mode(3)
                .custom_flags(0x0220_0000)
                .open(parent)
                .map_err(|_| unverifiable("a cache ancestor is unavailable or being replaced."))?;
            let metadata = guard
                .metadata()
                .map_err(|_| unverifiable("a cache ancestor cannot be inspected."))?;
            if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                return Err(unverifiable(
                    "a cache ancestor is a reparse point or not a directory.",
                ));
            }
            parents.push(guard);
        }
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|_| unverifiable("a cached file is unavailable or being modified."))?;
    let metadata = file
        .metadata()
        .map_err(|_| unverifiable("a cached file cannot be inspected."))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(unverifiable("a cached file is a reparse point."));
        }
    }
    if !metadata.is_file() {
        return Err(unverifiable("a cached file is not regular."));
    }
    Ok((file, parents))
}

pub(super) fn read_json(path: &Path, limit: u64) -> Result<Value, String> {
    let (file, _parents) = open_file(path)?;
    if file
        .metadata()
        .map_err(|_| unverifiable("a cached document cannot be inspected."))?
        .len()
        > limit
    {
        return Err(unverifiable("a cached document exceeds its size budget."));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unverifiable("a cached document cannot be read."))?;
    if bytes.len() as u64 > limit {
        return Err(unverifiable(
            "a cached document grew beyond its size budget.",
        ));
    }
    serde_json::from_slice(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes))
        .map_err(|_| unverifiable("a cached document is not valid JSON."))
}

pub(super) fn read_tree(
    root: &Path,
    files: &mut BTreeMap<String, FileEvidence>,
    directories: &mut BTreeSet<String>,
    hashes: &mut Vec<u32>,
    total_size: &mut u64,
    budget: &mut Budget<'_>,
) -> Result<(), String> {
    if !plain_metadata(root)?.is_dir() {
        return Err(unverifiable("a declared module directory is missing."));
    }
    let mut pending = vec![(root.to_path_buf(), String::new(), 0_usize)];
    let mut names = BTreeSet::new();
    while let Some((path, relative, depth)) = pending.pop() {
        budget.check()?;
        budget.entries += 1;
        if budget.entries > MAX_ENTRIES || depth > MAX_DEPTH {
            return Err(unverifiable("the cache exceeds its entry or depth budget."));
        }
        let metadata = plain_metadata(&path)?;
        if metadata.is_dir() {
            directories.insert(relative.clone());
            for entry in fs::read_dir(&path)
                .map_err(|_| unverifiable("a module directory cannot be read."))?
            {
                budget.check()?;
                let entry = entry.map_err(|_| unverifiable("a module entry cannot be read."))?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| unverifiable("a module path is not Unicode."))?;
                safe_component(&name)?;
                let nested = if relative.is_empty() {
                    name
                } else {
                    format!("{relative}/{name}")
                };
                if !names.insert(nested.to_ascii_lowercase())
                    || pending.len() + budget.entries >= MAX_ENTRIES
                {
                    return Err(unverifiable(
                        "the cache has colliding paths or exceeds its entry budget.",
                    ));
                }
                pending.push((entry.path(), nested, depth + 1));
            }
        } else {
            budget.bytes = budget
                .bytes
                .checked_add(metadata.len())
                .filter(|bytes| *bytes <= MAX_CONTENT_BYTES)
                .ok_or_else(|| unverifiable("the cache exceeds its content byte budget."))?;
            let (evidence, hash) = fingerprint_file(&path, metadata.len(), budget)?;
            if !relative
                .rsplit('/')
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case("assetregistry.bin"))
            {
                *total_size = total_size
                    .checked_add(evidence.bytes)
                    .ok_or_else(|| unverifiable("cache size accounting overflowed."))?;
                hashes.push(hash);
            }
            files.insert(relative, evidence);
        }
    }
    Ok(())
}

fn fingerprint_file(
    path: &Path,
    declared_bytes: u64,
    budget: &Budget<'_>,
) -> Result<(FileEvidence, u32), String> {
    let (mut file, _parents) = open_file(path)?;
    let before = file
        .metadata()
        .map_err(|_| unverifiable("a cached file cannot be inspected."))?;
    if before.len() != declared_bytes || before.len() > MAX_CONTENT_BYTES {
        return Err(unverifiable(
            "cached content changed before verification or exceeds its byte budget.",
        ));
    }
    let mut buffer = [0_u8; 64 * 1024];
    let mut sha = Sha256::new();
    let mut bytes = 0_u64;
    let mut normalized = 0_u64;
    loop {
        budget.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| unverifiable("cached content cannot be read."))?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        if bytes > before.len() {
            return Err(unverifiable("cached content changed during verification."));
        }
        normalized += buffer[..count]
            .iter()
            .filter(|byte| !whitespace(**byte))
            .count() as u64;
        sha.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| unverifiable("cached content cannot be re-read."))?;
    let mut murmur = Murmur::new(normalized);
    let mut reread = 0_u64;
    loop {
        budget.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(|_| unverifiable("cached content cannot be re-read."))?;
        if count == 0 {
            break;
        }
        reread += count as u64;
        if reread > bytes {
            return Err(unverifiable("cached content changed during verification."));
        }
        murmur.update(&buffer[..count]);
    }
    let after = file
        .metadata()
        .map_err(|_| unverifiable("cached content cannot be inspected after reading."))?;
    if bytes != before.len()
        || reread != bytes
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(unverifiable("cached content changed during verification."));
    }
    Ok((
        FileEvidence {
            bytes,
            sha256: sha
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        },
        murmur.finish(),
    ))
}

// CFCore 1.47 hashing_service_impl.cpp and fingerprints_service_impl.cpp use
// Murmur2 seed 1 on bytes excluding 09/0A/0D/20. Per-file values are sorted
// numerically, concatenated as decimal UTF-8, then hashed by the same algorithm.
pub(super) fn whitespace(byte: u8) -> bool {
    matches!(byte, 9 | 10 | 13 | 32)
}
pub(super) struct Murmur {
    hash: u32,
    word: u32,
    shift: u32,
}
impl Murmur {
    pub(super) fn new(length: u64) -> Self {
        Self {
            hash: 1 ^ length as u32,
            word: 0,
            shift: 0,
        }
    }
    pub(super) fn update(&mut self, bytes: &[u8]) {
        const MULTIPLIER: u32 = 1_540_483_477;
        for byte in bytes.iter().copied().filter(|byte| !whitespace(*byte)) {
            self.word |= (byte as u32) << self.shift;
            self.shift += 8;
            if self.shift == 32 {
                let mixed = self.word.wrapping_mul(MULTIPLIER);
                let mixed = (mixed ^ (mixed >> 24)).wrapping_mul(MULTIPLIER);
                self.hash = self.hash.wrapping_mul(MULTIPLIER) ^ mixed;
                self.word = 0;
                self.shift = 0;
            }
        }
    }
    pub(super) fn finish(mut self) -> u32 {
        const MULTIPLIER: u32 = 1_540_483_477;
        if self.shift > 0 {
            self.hash = (self.hash ^ self.word).wrapping_mul(MULTIPLIER);
        }
        let hash = (self.hash ^ (self.hash >> 13)).wrapping_mul(MULTIPLIER);
        hash ^ (hash >> 15)
    }
}
pub(super) fn murmur_bytes(bytes: &[u8]) -> u32 {
    let mut murmur = Murmur::new(bytes.iter().filter(|byte| !whitespace(**byte)).count() as u64);
    murmur.update(bytes);
    murmur.finish()
}
