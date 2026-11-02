//! Blocking, bounded file work for storage relocation. The caller commits its
//! metadata/settings before explicitly requesting source cleanup.
use std::cmp::Ordering as CmpOrdering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::StorageError;
use crate::instance_archive_files::native::{self, OwnedNode};
use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};
use crate::managed_console_log::owned_fs::{FileIdentity, identity, reject_links};

#[path = "storage_migration_takeover.rs"]
mod takeover;
pub(crate) use takeover::RootTakeover;

const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 128;
const MAX_CONFLICT_DETAILS: usize = 100;
const BUFFER_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RootMove {
    pub source: PathBuf,
    pub target: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileDigest {
    pub size: u64,
    pub sha256: String,
}

pub(crate) type DestinationHashes = BTreeMap<PathBuf, FileDigest>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlannedDirectory {
    pub source: PathBuf,
    pub target: PathBuf,
    pub identity: FileIdentity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct PlannedFile {
    pub source: PathBuf,
    pub target: PathBuf,
    pub identity: FileIdentity,
    pub digest: FileDigest,
    pub reused: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FileConflict {
    pub source: PathBuf,
    pub target: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct FilePlan {
    pub roots: Vec<RootMove>,
    // Missing in the original merge-style journal. Such journals may finish
    // their existing commit, but must never start a whole-directory takeover.
    #[serde(default)]
    pub whole_directory: bool,
    #[serde(default)]
    pub takeovers: Vec<RootTakeover>,
    pub excluded_paths: Vec<PathBuf>,
    pub directories: Vec<PlannedDirectory>,
    pub files: Vec<PlannedFile>,
    pub total_bytes: u64,
    pub conflicts: Vec<FileConflict>,
    pub conflict_count: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileProgress {
    pub phase: &'static str,
    pub total_bytes: u64,
    pub total_files: u64,
    pub completed_bytes: u64,
    pub completed_files: u64,
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidStorageMigration {
        path: path.to_owned(),
        message: message.into(),
    }
}

fn read_error(path: &Path, source: io::Error) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source,
    }
}

fn cancelled(cancellation: Option<&AtomicBool>) -> Result<(), StorageError> {
    if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(StorageError::StorageMigrationCancelled);
    }
    Ok(())
}

// Ordinal component comparison follows Windows filename casing without Unicode
// lowercase expansion, and never confuses `games` with `games-extra`.
#[cfg(windows)]
fn compare_names(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> CmpOrdering {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "CompareStringOrdinal"]
        fn compare_ordinal(a: *const u16, an: i32, b: *const u16, bn: i32, ignore_case: i32)
        -> i32;
    }
    let a = left.encode_wide().collect::<Vec<_>>();
    let b = right.encode_wide().collect::<Vec<_>>();
    // Filesystem paths are bounded below i32::MAX by Windows itself.
    match unsafe { compare_ordinal(a.as_ptr(), a.len() as i32, b.as_ptr(), b.len() as i32, 1) } {
        1 => CmpOrdering::Less,
        2 => CmpOrdering::Equal,
        3 => CmpOrdering::Greater,
        _ => a.cmp(&b),
    }
}

#[cfg(not(windows))]
fn compare_names(left: &std::ffi::OsStr, right: &std::ffi::OsStr) -> CmpOrdering {
    left.cmp(right)
}

#[derive(Clone, Debug, Eq)]
struct PathKey(PathBuf);

impl Ord for PathKey {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        let mut left = self.0.components();
        let mut right = other.0.components();
        loop {
            match (left.next(), right.next()) {
                (Some(left), Some(right)) => {
                    let ordering = compare_names(left.as_os_str(), right.as_os_str());
                    if ordering != CmpOrdering::Equal {
                        return ordering;
                    }
                }
                (Some(_), None) => return CmpOrdering::Greater,
                (None, Some(_)) => return CmpOrdering::Less,
                (None, None) => return CmpOrdering::Equal,
            }
        }
    }
}

impl PartialOrd for PathKey {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PathKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == CmpOrdering::Equal
    }
}

fn contains(parent: &Path, child: &Path) -> bool {
    let mut children = child.components();
    parent.components().all(|part| {
        children.next().is_some_and(|child| {
            compare_names(part.as_os_str(), child.as_os_str()) == CmpOrdering::Equal
        })
    })
}

struct DirectoryAlias {
    identity: FileIdentity,
    tail: PathBuf,
}

fn directory_aliases(path: &Path) -> Result<Vec<DirectoryAlias>, StorageError> {
    let mut aliases = Vec::new();
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(error) => return Err(read_error(ancestor, error)),
            Ok(metadata) if !metadata.is_dir() => continue,
            Ok(_) => (),
        }
        let observed = if ancestor.parent().is_none() {
            volume_root_identity(ancestor).map_err(|error| read_error(ancestor, error))?
        } else {
            native::open(ancestor, true, false)
                .and_then(|node| node.identity())
                .map_err(|error| read_error(ancestor, error))?
        };
        aliases.push(DirectoryAlias {
            identity: observed,
            tail: path
                .components()
                .skip(ancestor.components().count())
                .collect(),
        });
    }
    Ok(aliases)
}

pub(crate) fn volume_root_identity(path: &Path) -> io::Result<FileIdentity> {
    identity(&volume_root_handle(path)?)
}

fn volume_root_handle(path: &Path) -> io::Result<File> {
    // OwnedNode pins a node's parents and intentionally requires a parent.
    // A drive/UNC root has none, so retain a plain read-only root handle here.
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || crate::private_runtime::metadata_is_reparse_point(&metadata)
    {
        return Err(io::Error::other(
            "Migration volume root is not a plain directory.",
        ));
    }
    Ok(file)
}

struct DirectoryGuard {
    _node: Option<OwnedNode>,
    _volume: Option<File>,
}

fn directory_guard(path: &Path) -> Result<DirectoryGuard, StorageError> {
    if path.parent().is_none() {
        Ok(DirectoryGuard {
            _node: None,
            _volume: Some(volume_root_handle(path).map_err(|error| read_error(path, error))?),
        })
    } else {
        Ok(DirectoryGuard {
            _node: Some(native::open(path, true, false).map_err(|error| read_error(path, error))?),
            _volume: None,
        })
    }
}

fn physical_contains(parent: &[DirectoryAlias], child: &[DirectoryAlias]) -> bool {
    parent.first().is_some_and(|anchor| {
        child.iter().any(|ancestor| {
            ancestor.identity == anchor.identity && contains(&anchor.tail, &ancestor.tail)
        })
    })
}

fn physical_overlap(left: &[DirectoryAlias], right: &[DirectoryAlias]) -> bool {
    physical_contains(left, right) || physical_contains(right, left)
}

/// Compare component boundaries and filesystem aliases, including missing tails.
pub(crate) fn paths_overlap(left: &Path, right: &Path) -> Result<bool, StorageError> {
    let left = normalize_resource_path(left)?;
    let right = normalize_resource_path(right)?;
    Ok(contains(&left, &right)
        || contains(&right, &left)
        || physical_overlap(&directory_aliases(&left)?, &directory_aliases(&right)?))
}

/// Normalize future destination trees without inspecting old contents below
/// their outermost takeover boundary. Root nodes and external ancestors remain strict.
pub(crate) fn normalize_destinations(paths: &[PathBuf]) -> Result<Vec<PathBuf>, StorageError> {
    takeover::normalize_destinations(paths)
}

fn normalized_roots(roots: &[RootMove]) -> Result<Vec<RootMove>, StorageError> {
    let mut result = Vec::<RootMove>::new();
    let targets = normalize_destinations(
        &roots
            .iter()
            .map(|root| root.target.clone())
            .collect::<Vec<_>>(),
    )?;
    for (root, target) in roots.iter().zip(targets) {
        let source = normalize_path(&root.source)?;
        if result
            .iter()
            .any(|item| PathKey(item.source.clone()) == PathKey(source.clone()))
        {
            return Err(invalid(
                &source,
                "The same source root was specified more than once.",
            ));
        }
        result.push(RootMove { source, target });
    }
    for target in &result {
        if result.iter().any(|source| {
            contains(&source.source, &target.target) || contains(&target.target, &source.source)
        }) {
            return Err(invalid(
                &target.target,
                "A migration destination overlaps a source root.",
            ));
        }
    }
    // Canonical spelling alone does not collapse drive mappings, bind mounts,
    // or local/UNC aliases. Compare identities of existing ancestors plus each
    // uncreated suffix before treating apparently separate trees as independent.
    let sources = result
        .iter()
        .map(|root| directory_aliases(&root.source))
        .collect::<Result<Vec<_>, _>>()?;
    let targets = result
        .iter()
        .map(|root| takeover::target_aliases(&root.target, &result))
        .collect::<Result<Vec<_>, _>>()?;
    for (index, target) in targets.iter().enumerate() {
        if sources
            .iter()
            .any(|source| physical_overlap(source, target))
        {
            return Err(invalid(
                &result[index].target,
                "A migration destination physically aliases or overlaps a source root.",
            ));
        }
        for other in 0..index {
            if (physical_overlap(target, &targets[other])
                && !contains(&result[index].target, &result[other].target)
                && !contains(&result[other].target, &result[index].target))
                || (physical_overlap(&sources[index], &sources[other])
                    && !contains(&result[index].source, &result[other].source)
                    && !contains(&result[other].source, &result[index].source))
            {
                return Err(invalid(
                    &result[index].source,
                    "Migration roots use overlapping filesystem aliases; use one canonical namespace.",
                ));
            }
        }
    }
    Ok(result)
}

fn map_normalized(path: &Path, roots: &[RootMove]) -> PathBuf {
    match roots
        .iter()
        .filter(|root| contains(&root.source, path))
        .max_by_key(|root| root.source.components().count())
    {
        Some(root) => {
            let tail = path
                .components()
                .skip(root.source.components().count())
                .collect::<PathBuf>();
            if tail.as_os_str().is_empty() {
                root.target.clone()
            } else {
                root.target.join(tail)
            }
        }
        None => path.to_owned(),
    }
}

/// Canonicalize existing ancestors and apply the longest component prefix.
/// Roots are the normalized mappings returned in FilePlan; unmatched absolute
/// paths remain unchanged apart from canonical spelling.
pub(crate) fn remap_path(path: &Path, roots: &[RootMove]) -> Result<PathBuf, StorageError> {
    // Root relationship/identity validation belongs to the file-plan boundary;
    // metadata may remap thousands of references using the same proven roots.
    let targets = normalize_destinations(
        &roots
            .iter()
            .map(|root| root.target.clone())
            .collect::<Vec<_>>(),
    )?;
    let roots = roots
        .iter()
        .zip(targets)
        .map(|(root, target)| {
            Ok(RootMove {
                source: normalize_path(&root.source)?,
                target,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    Ok(map_normalized(&normalize_resource_path(path)?, &roots))
}

/// Preserve the configured destination spelling for persisted references whose
/// consumers compare them lexically with settings. Matching and validation
/// still use canonical paths; a Windows verbatim prefix is not added to output.
pub(crate) fn remap_stored_path(path: &Path, roots: &[RootMove]) -> Result<PathBuf, StorageError> {
    normalize_destinations(
        &roots
            .iter()
            .map(|root| root.target.clone())
            .collect::<Vec<_>>(),
    )?;
    let roots = roots
        .iter()
        .map(|root| {
            Ok(RootMove {
                source: normalize_path(&root.source)?,
                target: root.target.clone(),
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    Ok(map_normalized(&normalize_resource_path(path)?, &roots))
}

fn excluded(path: &Path, exclusions: &[PathBuf]) -> bool {
    exclusions.iter().any(|excluded| contains(excluded, path))
}

fn hash_reader(
    reader: &mut impl Read,
    path: &Path,
    cancellation: Option<&AtomicBool>,
    mut observed: impl FnMut(u64),
) -> Result<FileDigest, StorageError> {
    let mut buffer = vec![0; BUFFER_BYTES];
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    loop {
        cancelled(cancellation)?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| read_error(path, error))?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or_else(|| invalid(path, "File size overflow."))?;
        hasher.update(&buffer[..count]);
        observed(count as u64);
    }
    Ok(FileDigest {
        size,
        sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

fn checked_file(
    path: &Path,
    mutate: bool,
    expected: Option<&FileIdentity>,
    digest: Option<&FileDigest>,
    cancellation: Option<&AtomicBool>,
) -> Result<OwnedNode, StorageError> {
    cancelled(cancellation)?;
    let mut node =
        native::open_verified_file(path, mutate).map_err(|error| read_error(path, error))?;
    if let Some(expected) = expected
        && node.identity().map_err(|error| read_error(path, error))? != *expected
    {
        return Err(invalid(
            path,
            "File identity changed; the source was preserved.",
        ));
    }
    if let Some(expected) = digest
        && hash_reader(node.reader(), path, cancellation, |_| {})? != *expected
    {
        return Err(invalid(
            path,
            "File contents changed; the source was preserved.",
        ));
    }
    Ok(node)
}

fn scan_sources(
    roots: Vec<RootMove>,
    excluded_paths: Vec<PathBuf>,
    cancellation: Option<&AtomicBool>,
    progress: &mut dyn FnMut(FileProgress),
) -> Result<FilePlan, StorageError> {
    let mut pending = roots
        .iter()
        .filter(|candidate| {
            !roots.iter().any(|other| {
                PathKey(other.source.clone()) != PathKey(candidate.source.clone())
                    && contains(&other.source, &candidate.source)
            })
        })
        .map(|root| (root.source.clone(), 0_usize))
        .collect::<Vec<_>>();
    let mut plan = FilePlan {
        roots,
        whole_directory: true,
        takeovers: Vec::new(),
        excluded_paths,
        directories: Vec::new(),
        files: Vec::new(),
        total_bytes: 0,
        conflicts: Vec::new(),
        conflict_count: 0,
    };
    let mut status = FileProgress {
        phase: "planning",
        ..FileProgress::default()
    };
    while let Some((path, depth)) = pending.pop() {
        cancelled(cancellation)?;
        if excluded(&path, &plan.excluded_paths) {
            continue;
        }
        if depth > MAX_DEPTH || plan.files.len() + plan.directories.len() >= MAX_ENTRIES {
            return Err(invalid(
                &path,
                "Migration exceeds the bounded traversal limit (200000 entries or 128 levels).",
            ));
        }
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound && depth == 0 => continue,
            Err(error) => return Err(read_error(&path, error)),
        };
        reject_links(&path).map_err(|error| read_error(&path, error))?;
        let target = map_normalized(&path, &plan.roots);
        if metadata.is_dir() {
            let guard =
                native::open(&path, true, false).map_err(|error| read_error(&path, error))?;
            let identity = guard.identity().map_err(|error| read_error(&path, error))?;
            for child in fs::read_dir(&path).map_err(|error| read_error(&path, error))? {
                let child = child.map_err(|error| read_error(&path, error))?;
                if pending.len() + plan.files.len() + plan.directories.len() >= MAX_ENTRIES {
                    return Err(invalid(
                        &path,
                        "Migration exceeds the bounded traversal entry limit.",
                    ));
                }
                pending.push((child.path(), depth + 1));
            }
            plan.directories.push(PlannedDirectory {
                source: path,
                target,
                identity,
            });
        } else if metadata.is_file() {
            let mut node = checked_file(&path, false, None, None, cancellation)?;
            let identity = node.identity().map_err(|error| read_error(&path, error))?;
            let digest = hash_reader(node.reader(), &path, cancellation, |count| {
                status.completed_bytes += count;
                progress(status);
            })?;
            plan.total_bytes = plan
                .total_bytes
                .checked_add(digest.size)
                .ok_or_else(|| invalid(&path, "Total migration size overflow."))?;
            plan.files.push(PlannedFile {
                source: path,
                target,
                identity,
                digest,
                reused: false,
            });
            status.completed_files += 1;
            progress(status);
        } else {
            return Err(invalid(
                &path,
                "Migration only accepts regular files and plain directories.",
            ));
        }
    }
    plan.directories
        .sort_by_key(|entry| PathKey(entry.source.clone()));
    plan.files
        .sort_by_key(|entry| PathKey(entry.source.clone()));
    Ok(plan)
}

#[derive(Default)]
struct Conflicts {
    paths: BTreeSet<PathKey>,
    details: Vec<FileConflict>,
    sources: BTreeMap<PathKey, PathBuf>,
    reused: BTreeSet<PathKey>,
}

impl Conflicts {
    fn add(&mut self, path: &Path, message: &str) {
        if self.paths.insert(PathKey(path.to_owned())) && self.details.len() < MAX_CONFLICT_DETAILS
        {
            self.details.push(FileConflict {
                source: self
                    .sources
                    .get(&PathKey(path.to_owned()))
                    .cloned()
                    .unwrap_or_default(),
                target: path.to_owned(),
                message: message.into(),
            });
        }
    }
}

fn inspect_targets(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
    inspect_existing: bool,
) -> Result<Conflicts, StorageError> {
    let mut desired = BTreeMap::<PathKey, Option<&FileDigest>>::new();
    let mut conflicts = Conflicts::default();
    for (source, target) in plan
        .roots
        .iter()
        .map(|root| (&root.source, &root.target))
        .chain(
            plan.directories
                .iter()
                .map(|entry| (&entry.source, &entry.target)),
        )
        .chain(
            plan.files
                .iter()
                .map(|entry| (&entry.source, &entry.target)),
        )
    {
        conflicts
            .sources
            .entry(PathKey(target.clone()))
            .or_insert_with(|| source.clone());
    }
    for path in plan
        .roots
        .iter()
        .map(|root| &root.target)
        .chain(plan.directories.iter().map(|entry| &entry.target))
    {
        desired.insert(PathKey(path.clone()), None);
    }
    for entry in &plan.files {
        let key = PathKey(entry.target.clone());
        match desired.get(&key) {
            Some(Some(digest)) if *digest == &entry.digest => (),
            Some(_) => conflicts.add(
                &entry.target,
                "Multiple source entries map to incompatible destination contents or types.",
            ),
            None => {
                desired.insert(key, Some(&entry.digest));
            }
        }
    }
    for (key, wanted) in &desired {
        cancelled(cancellation)?;
        let path = &key.0;
        let mut blocked = false;
        for ancestor in path.ancestors().skip(1) {
            if matches!(desired.get(&PathKey(ancestor.to_owned())), Some(Some(_))) {
                conflicts.add(path, "A destination ancestor is also mapped as a file.");
                blocked = true;
            }
            if !inspect_existing
                && plan
                    .takeovers
                    .iter()
                    .any(|root| contains(&root.target, ancestor))
            {
                continue;
            }
            match fs::symlink_metadata(ancestor) {
                Ok(metadata) if !metadata.is_dir() => {
                    conflicts.add(path, "A destination ancestor is not a directory.");
                    blocked = true;
                }
                Ok(_) => (),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) => {}
                Err(error) => return Err(read_error(ancestor, error)),
            }
        }
        if blocked {
            continue;
        }
        if !inspect_existing {
            continue;
        }
        reject_links(path).map_err(|error| read_error(path, error))?;
        match fs::symlink_metadata(path) {
            Ok(metadata) if wanted.is_none() && metadata.is_dir() => (),
            Ok(metadata) if wanted.is_some() && metadata.is_file() => {
                let mut file = checked_file(path, false, None, None, cancellation)?;
                let actual = hash_reader(file.reader(), path, cancellation, |_| {})?;
                if wanted.is_some_and(|expected| expected != &actual) {
                    conflicts.add(
                        path,
                        "A destination file already exists with different contents.",
                    );
                } else {
                    conflicts.reused.insert(key.clone());
                }
            }
            Ok(_) => conflicts.add(
                path,
                "A destination file/directory type conflicts with the source.",
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                conflicts.add(path, "A destination ancestor is a file.")
            }
            Err(error) => return Err(read_error(path, error)),
        }
    }
    Ok(conflicts)
}

pub(crate) fn plan(
    roots: &[RootMove],
    excluded_paths: &[PathBuf],
    cancellation: Option<&AtomicBool>,
    progress: &mut dyn FnMut(FileProgress),
) -> Result<FilePlan, StorageError> {
    cancelled(cancellation)?;
    let roots = normalized_roots(roots)?;
    let excluded_paths = excluded_paths
        .iter()
        .map(|path| normalize_resource_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut plan = scan_sources(roots, excluded_paths, cancellation, progress)?;
    plan.takeovers = takeover::plan_targets(&plan.roots)?;
    let conflicts = inspect_targets(&plan, cancellation, false)?;
    plan.conflict_count = conflicts.paths.len() as u64;
    plan.conflicts = conflicts.details;
    progress(FileProgress {
        phase: "planned",
        total_bytes: plan.total_bytes,
        total_files: plan.files.len() as u64,
        completed_bytes: plan.total_bytes,
        completed_files: plan.files.len() as u64,
    });
    cancelled(cancellation)?;
    Ok(plan)
}

fn validate_plan(plan: &FilePlan, overrides: &DestinationHashes) -> Result<(), StorageError> {
    let roots = normalized_roots(&plan.roots)?;
    if plan.whole_directory {
        takeover::validate(plan)?;
    } else if !plan.takeovers.is_empty() {
        return Err(invalid(
            Path::new(""),
            "A merge-style migration cannot contain destination takeovers.",
        ));
    }
    if roots.len() > MAX_ENTRIES || plan.files.len() + plan.directories.len() > MAX_ENTRIES {
        return Err(invalid(
            Path::new(""),
            "Migration plan exceeds its entry limit.",
        ));
    }
    if plan.conflict_count != 0 || !plan.conflicts.is_empty() {
        return Err(invalid(
            Path::new(""),
            format!(
                "Migration has {} conflicting destination paths; no files were copied.",
                plan.conflict_count
            ),
        ));
    }
    let mut sources = BTreeSet::new();
    for (source, target) in plan
        .directories
        .iter()
        .map(|entry| (&entry.source, &entry.target))
        .chain(
            plan.files
                .iter()
                .map(|entry| (&entry.source, &entry.target)),
        )
    {
        if !source.is_absolute()
            || !target.is_absolute()
            || !roots.iter().any(|root| contains(&root.source, source))
            || excluded(source, &plan.excluded_paths)
            || PathKey(map_normalized(source, &roots)) != PathKey(target.clone())
            || PathKey(normalize_resource_path(source)?) != PathKey(source.clone())
            || (!plan.whole_directory
                && PathKey(normalize_resource_path(target)?) != PathKey(target.clone()))
            || !sources.insert(PathKey(source.clone()))
        {
            return Err(invalid(
                source,
                "The saved migration plan has an invalid or duplicate file mapping.",
            ));
        }
    }
    let mut total = 0_u64;
    for file in &plan.files {
        validate_digest(&file.source, &file.digest)?;
        total = total
            .checked_add(file.digest.size)
            .ok_or_else(|| invalid(&file.source, "Migration plan size overflow."))?;
    }
    if total != plan.total_bytes {
        return Err(invalid(
            Path::new(""),
            "Migration plan byte total is inconsistent.",
        ));
    }
    for (path, digest) in overrides {
        if !plan.files.iter().any(|file| &file.target == path) {
            return Err(invalid(
                path,
                "Destination hash override is not a planned migration file.",
            ));
        }
        validate_digest(path, digest)?;
    }
    Ok(())
}

fn validate_digest(path: &Path, digest: &FileDigest) -> Result<(), StorageError> {
    if digest.sha256.len() != 64
        || !digest
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            path,
            "Migration plan contains an invalid SHA-256 digest.",
        ));
    }
    Ok(())
}

fn verify_sources(plan: &FilePlan, cancellation: Option<&AtomicBool>) -> Result<(), StorageError> {
    let observed = scan_sources(
        plan.roots.clone(),
        plan.excluded_paths.clone(),
        cancellation,
        &mut |_| {},
    )?;
    if observed.files.len() != plan.files.len()
        || observed.directories.len() != plan.directories.len()
    {
        return Err(invalid(
            Path::new(""),
            "Source directory contents changed after migration planning.",
        ));
    }
    let mut expected_files = plan.files.iter().collect::<Vec<_>>();
    expected_files.sort_by_key(|file| PathKey(file.source.clone()));
    for (actual, expected) in observed.files.iter().zip(expected_files) {
        if PathKey(actual.source.clone()) != PathKey(expected.source.clone())
            || actual.identity != expected.identity
            || actual.digest != expected.digest
        {
            return Err(invalid(
                &expected.source,
                "Source file identity or contents changed after migration planning.",
            ));
        }
    }
    let mut expected_dirs = plan.directories.iter().collect::<Vec<_>>();
    expected_dirs.sort_by_key(|entry| PathKey(entry.source.clone()));
    for (actual, expected) in observed.directories.iter().zip(expected_dirs) {
        if PathKey(actual.source.clone()) != PathKey(expected.source.clone())
            || actual.identity != expected.identity
        {
            return Err(invalid(
                &expected.source,
                "Source directory identity changed after migration planning.",
            ));
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> Result<(), StorageError> {
    let mut missing = Vec::new();
    let mut cursor = path.to_owned();
    loop {
        match fs::symlink_metadata(&cursor) {
            Ok(_) => {
                directory_guard(&cursor)?;
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(cursor.clone());
                if !cursor.pop() {
                    return Err(invalid(
                        path,
                        "Destination has no existing parent directory.",
                    ));
                }
            }
            Err(error) => return Err(read_error(&cursor, error)),
        }
    }
    for directory in missing.into_iter().rev() {
        let parent = directory
            .parent()
            .ok_or_else(|| invalid(&directory, "Destination directory has no parent."))?;
        let _parent = directory_guard(parent)?;
        match fs::create_dir(&directory) {
            Ok(()) => (),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
            Err(source) => {
                return Err(StorageError::CreatePath {
                    path: directory,
                    source,
                });
            }
        }
        native::open(&directory, true, false).map_err(|error| read_error(&directory, error))?;
    }
    Ok(())
}

struct PendingFile {
    path: PathBuf,
    identity: FileIdentity,
    output: Option<File>,
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        drop(self.output.take());
        // Only this unique, identity-bound temporary file is ours to remove.
        if let Ok(node) = native::open_verified_file(&self.path, true)
            && node.identity().ok().as_ref() == Some(&self.identity)
        {
            let _ = node.remove();
        }
    }
}

fn copy_one(
    entry: &PlannedFile,
    cancellation: Option<&AtomicBool>,
    status: &mut FileProgress,
    progress: &mut dyn FnMut(FileProgress),
) -> Result<(), StorageError> {
    cancelled(cancellation)?;
    let parent = entry
        .target
        .parent()
        .ok_or_else(|| invalid(&entry.target, "Destination file has no parent."))?;
    let _parent = native::open(parent, true, false).map_err(|error| read_error(parent, error))?;
    match fs::symlink_metadata(&entry.target) {
        Ok(_) => {
            checked_file(
                &entry.source,
                false,
                Some(&entry.identity),
                Some(&entry.digest),
                cancellation,
            )?;
            checked_file(
                &entry.target,
                false,
                None,
                Some(&entry.digest),
                cancellation,
            )?;
            status.completed_bytes += entry.digest.size;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(read_error(&entry.target, error)),
    }
    let mut source = checked_file(
        &entry.source,
        false,
        Some(&entry.identity),
        None,
        cancellation,
    )?;
    let permissions = source
        .reader()
        .metadata()
        .map_err(|error| read_error(&entry.source, error))?
        .permissions();
    let temporary = parent.join(format!(".langame-migration-{}.part", uuid::Uuid::new_v4()));
    let output = File::options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| StorageError::CreatePath {
            path: temporary.clone(),
            source,
        })?;
    let temporary_identity = identity(&output).map_err(|error| read_error(&temporary, error))?;
    let mut pending = PendingFile {
        path: temporary.clone(),
        identity: temporary_identity,
        output: Some(output),
    };
    let mut hasher = Sha256::new();
    let mut copied = 0_u64;
    let mut buffer = vec![0; BUFFER_BYTES];
    if let Some(output) = pending.output.as_mut() {
        loop {
            cancelled(cancellation)?;
            let count = source
                .reader()
                .read(&mut buffer)
                .map_err(|error| read_error(&entry.source, error))?;
            if count == 0 {
                break;
            }
            output
                .write_all(&buffer[..count])
                .map_err(|source| StorageError::CopyPath {
                    from: entry.source.clone(),
                    to: entry.target.clone(),
                    source,
                })?;
            hasher.update(&buffer[..count]);
            copied += count as u64;
            status.completed_bytes += count as u64;
            progress(*status);
        }
        output
            .set_permissions(permissions)
            .map_err(|error| read_error(&temporary, error))?;
        output.sync_all().map_err(|source| StorageError::SyncFile {
            path: temporary.clone(),
            source,
        })?;
    }
    let digest = FileDigest {
        size: copied,
        sha256: hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    };
    if digest != entry.digest {
        return Err(invalid(
            &entry.source,
            "Source changed during copying; it was preserved.",
        ));
    }
    drop(pending.output.take());
    let verified = checked_file(
        &temporary,
        true,
        Some(&pending.identity),
        Some(&entry.digest),
        cancellation,
    )?;
    cancelled(cancellation)?;
    #[cfg(windows)]
    let published = verified.rename(&entry.target);
    #[cfg(not(windows))]
    let published = {
        // The private copy is already on the target volume. Creating its second
        // name atomically refuses replacement, unlike std::fs::rename on Unix.
        fs::hard_link(&temporary, &entry.target).and_then(|()| verified.remove())
    };
    // A racing same-content destination is reusable; different data remains
    // untouched and prevents the metadata/settings commit.
    if let Err(error) = published
        && checked_file(
            &entry.target,
            false,
            None,
            Some(&entry.digest),
            cancellation,
        )
        .is_err()
    {
        return Err(StorageError::MovePath {
            from: temporary,
            to: entry.target.clone(),
            source: error,
        });
    }
    checked_file(
        &entry.target,
        false,
        None,
        Some(&entry.digest),
        cancellation,
    )?;
    Ok(())
}

/// Revalidate a persisted plan without allocating new recovery paths or writing.
pub(crate) fn recheck(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
    progress: &mut dyn FnMut(FileProgress),
) -> Result<(), StorageError> {
    cancelled(cancellation)?;
    if !plan.whole_directory {
        return Err(invalid(
            Path::new(""),
            "This migration preview uses the previous merge behavior; prepare a new preview before moving directories.",
        ));
    }
    validate_plan(plan, &DestinationHashes::new())?;
    verify_sources(plan, cancellation)?;
    let conflicts = inspect_targets(plan, cancellation, false)?;
    if !conflicts.paths.is_empty() {
        return Err(invalid(
            &conflicts.details[0].target,
            "Migration source mappings or destination ancestors conflict.",
        ));
    }
    takeover::recheck(plan, cancellation)?;
    progress(FileProgress {
        phase: "checked",
        total_bytes: plan.total_bytes,
        total_files: plan.files.len() as u64,
        completed_bytes: plan.total_bytes,
        completed_files: plan.files.len() as u64,
    });
    cancelled(cancellation)
}

pub(crate) fn preserved_destinations(
    plan: &FilePlan,
) -> Result<Vec<(PathBuf, PathBuf)>, StorageError> {
    takeover::preserved_destinations(plan)
}

/// Requires the complete plan to have been durably saved by the caller first.
/// Existing outer destination roots are preserved whole before any file copy.
pub(crate) fn copy_verified(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
    progress: &mut dyn FnMut(FileProgress),
) -> Result<(), StorageError> {
    recheck(plan, cancellation, &mut |_| {})?;
    let _destinations = takeover::acquire(plan, cancellation)?;
    verify_destination_layout(plan, false, cancellation)?;
    let conflicts = inspect_targets(plan, cancellation, true)?;
    if !conflicts.paths.is_empty() {
        return Err(invalid(
            &conflicts.details[0].target,
            format!(
                "{} destination paths changed or conflict; no files were copied.",
                conflicts.paths.len()
            ),
        ));
    }
    let mut status = FileProgress {
        phase: "copying",
        total_bytes: plan.total_bytes,
        total_files: plan.files.len() as u64,
        ..FileProgress::default()
    };
    progress(status);
    cancelled(cancellation)?;
    for path in plan
        .roots
        .iter()
        .map(|root| &root.target)
        .chain(plan.directories.iter().map(|entry| &entry.target))
    {
        cancelled(cancellation)?;
        ensure_directory(path)?;
    }
    for file in &plan.files {
        copy_one(file, cancellation, &mut status, progress)?;
        status.completed_files += 1;
        progress(status);
    }
    cancelled(cancellation)?;
    verify_sources(plan, cancellation)?;
    verify_destination_layout(plan, true, cancellation)?;
    verify_directories(plan)?;
    for file in &plan.files {
        checked_file(&file.target, false, None, Some(&file.digest), cancellation)?;
    }
    Ok(())
}

pub(crate) fn verify_source_and_destination(
    plan: &FilePlan,
    overrides: &DestinationHashes,
) -> Result<(), StorageError> {
    validate_plan(plan, overrides)?;
    let _destinations = if plan.whole_directory {
        takeover::owned_destinations(plan)?
    } else {
        Vec::new()
    };
    verify_sources(plan, None)?;
    if plan.whole_directory {
        verify_destination_layout(plan, true, None)?;
    }
    verify_directories(plan)?;
    for file in &plan.files {
        checked_file(
            &file.target,
            false,
            None,
            Some(overrides.get(&file.target).unwrap_or(&file.digest)),
            None,
        )?;
    }
    Ok(())
}

fn verify_directories(plan: &FilePlan) -> Result<(), StorageError> {
    for path in plan
        .roots
        .iter()
        .map(|root| &root.target)
        .chain(plan.directories.iter().map(|directory| &directory.target))
    {
        native::open(path, true, false).map_err(|error| read_error(path, error))?;
    }
    Ok(())
}

/// Membership must match the source projection, including empty directories.
/// Unexpected destination entries are preserved and block source cleanup.
fn verify_destination_layout(
    plan: &FilePlan,
    complete: bool,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let mut expected = BTreeMap::<PathKey, bool>::new();
    for path in plan
        .roots
        .iter()
        .map(|root| &root.target)
        .chain(plan.directories.iter().map(|directory| &directory.target))
    {
        expected.insert(PathKey(path.clone()), true);
    }
    for file in &plan.files {
        expected.insert(PathKey(file.target.clone()), false);
    }
    for key in expected.keys().cloned().collect::<Vec<_>>() {
        for ancestor in key.0.ancestors().skip(1) {
            if plan
                .takeovers
                .iter()
                .any(|root| contains(&root.target, ancestor))
            {
                expected.entry(PathKey(ancestor.to_owned())).or_insert(true);
            }
        }
    }
    let mut pending = plan
        .takeovers
        .iter()
        .map(|root| (root.target.clone(), 0_usize))
        .collect::<Vec<_>>();
    let mut visited = 0_usize;
    while let Some((path, depth)) = pending.pop() {
        cancelled(cancellation)?;
        if depth > MAX_DEPTH || visited >= MAX_ENTRIES {
            return Err(invalid(
                &path,
                "Destination layout exceeds the migration traversal limit.",
            ));
        }
        let wanted = expected.remove(&PathKey(path.clone())).ok_or_else(|| {
            invalid(
                &path,
                "An unexpected destination entry was preserved; migration cannot be completed.",
            )
        })?;
        let node = native::open(&path, wanted, false).map_err(|error| read_error(&path, error))?;
        visited += 1;
        if wanted {
            for child in fs::read_dir(&path).map_err(|error| read_error(&path, error))? {
                let child = child.map_err(|error| read_error(&path, error))?;
                if pending.len() + visited >= MAX_ENTRIES {
                    return Err(invalid(
                        &path,
                        "Destination layout exceeds the migration entry limit.",
                    ));
                }
                pending.push((child.path(), depth + 1));
            }
        }
        drop(node);
    }
    if complete && let Some((path, _)) = expected.first_key_value() {
        return Err(invalid(&path.0, "A planned destination entry is missing."));
    }
    Ok(())
}

fn source_parent_guards(
    path: &Path,
    directories: &BTreeMap<PathKey, &PlannedDirectory>,
) -> Result<Vec<OwnedNode>, StorageError> {
    let mut guards = Vec::new();
    for ancestor in path.ancestors().skip(1) {
        if let Some(expected) = directories.get(&PathKey(ancestor.to_owned())) {
            let guard =
                native::open(ancestor, true, false).map_err(|error| read_error(ancestor, error))?;
            if guard
                .identity()
                .map_err(|error| read_error(ancestor, error))?
                != expected.identity
            {
                return Err(invalid(
                    ancestor,
                    "Source parent identity changed; remaining source files were preserved.",
                ));
            }
            guards.push(guard);
        }
    }
    Ok(guards)
}

/// Call only after durable metadata/settings commit. Missing source entries are
/// accepted on recovery. Changed entries and every unplanned entry are retained.
/// No recursive removal is used, including for excluded or newly added files.
pub(crate) fn cleanup_sources(plan: &FilePlan, overrides: &DestinationHashes) -> Vec<String> {
    let mut warnings = Vec::new();
    if let Err(error) = validate_plan(plan, overrides) {
        return vec![error.to_string()];
    }
    let _destinations = if plan.whole_directory {
        match takeover::owned_destinations(plan).and_then(|guards| {
            verify_destination_layout(plan, true, None)?;
            Ok(guards)
        }) {
            Ok(guards) => guards,
            Err(error) => return vec![error.to_string()],
        }
    } else {
        Vec::new()
    };
    let directories = plan
        .directories
        .iter()
        .map(|entry| (PathKey(entry.source.clone()), entry))
        .collect::<BTreeMap<_, _>>();
    for file in &plan.files {
        let cleanup = || -> Result<(), StorageError> {
            match fs::symlink_metadata(&file.source) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(read_error(&file.source, error)),
                Ok(_) => (),
            }
            let _parents = source_parent_guards(&file.source, &directories)?;
            let source = checked_file(
                &file.source,
                true,
                Some(&file.identity),
                Some(&file.digest),
                None,
            )?;
            let _destination = checked_file(
                &file.target,
                false,
                None,
                Some(overrides.get(&file.target).unwrap_or(&file.digest)),
                None,
            )?;
            source.remove().map_err(|source| StorageError::DeletePath {
                path: file.source.clone(),
                source,
            })
        };
        if let Err(error) = cleanup() {
            warnings.push(error.to_string());
        }
    }
    let mut ordered = plan.directories.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|entry| std::cmp::Reverse(entry.source.components().count()));
    // Locks and stationary child roots are intentionally outside the move.
    // Retain their ancestors without reporting successful cleanup as a failure.
    let mut retained = plan
        .excluded_paths
        .iter()
        .cloned()
        .map(PathKey)
        .collect::<BTreeSet<_>>();
    for directory in ordered {
        let mut cleanup = || -> Result<(), StorageError> {
            match fs::symlink_metadata(&directory.source) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(read_error(&directory.source, error)),
                Ok(_) => (),
            }
            let _parents = source_parent_guards(&directory.source, &directories)?;
            let node = native::open(&directory.source, true, true)
                .map_err(|error| read_error(&directory.source, error))?;
            if node
                .identity()
                .map_err(|error| read_error(&directory.source, error))?
                != directory.identity
            {
                return Err(invalid(
                    &directory.source,
                    "Source directory identity changed; it was preserved.",
                ));
            }
            let _destination = native::open(&directory.target, true, false)
                .map_err(|error| read_error(&directory.target, error))?;
            match node.remove() {
                Ok(()) => Ok(()),
                Err(source) => {
                    if source.kind() == io::ErrorKind::DirectoryNotEmpty
                        && only_retained_children(&directory.source, &retained)?
                    {
                        retained.insert(PathKey(directory.source.clone()));
                        return Ok(());
                    }
                    Err(StorageError::DeletePath {
                        path: directory.source.clone(),
                        source,
                    })
                }
            }
        };
        if let Err(error) = cleanup() {
            warnings.push(error.to_string());
        }
    }
    if plan.whole_directory
        && let Err(error) = verify_destination_layout(plan, true, None)
    {
        warnings.push(error.to_string());
    }
    warnings
}

fn only_retained_children(path: &Path, retained: &BTreeSet<PathKey>) -> Result<bool, StorageError> {
    let mut found = false;
    for child in fs::read_dir(path).map_err(|error| read_error(path, error))? {
        let child = child.map_err(|error| read_error(path, error))?;
        if !retained.contains(&PathKey(child.path())) {
            return Ok(false);
        }
        found = true;
    }
    Ok(found)
}

#[cfg(test)]
#[path = "storage_migration_files_tests.rs"]
mod tests;
