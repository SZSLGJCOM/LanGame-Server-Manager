use std::collections::BTreeMap;
use std::io::{Read, Write};

use sha2::{Digest, Sha256};

use super::*;
use crate::instance_archive_files::native;
use crate::instance_isolation::paths::{contains, normalize_path};

const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 64;
const MAX_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

struct Destination {
    _parent: native::OwnedNode,
    file: fs::File,
}

fn new_destination(path: &Path) -> Result<Destination, StorageError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "Save destination has no parent."))?;
    let guard =
        native::open(parent, true, false).map_err(|error| invalid(parent, error.to_string()))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|error| invalid(path, error.to_string()))?;
    Ok(Destination {
        _parent: guard,
        file,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Inventory {
    pub digest: String,
    pub files: u64,
    pub bytes: u64,
    pub entries: BTreeMap<PathBuf, Option<(u64, String)>>,
}

impl Inventory {
    pub(super) fn with_preserved_files(
        &self,
        current: Option<&Self>,
        paths: &[&str],
    ) -> Result<Self, StorageError> {
        let mut entries = self.entries.clone();
        for preserved in paths {
            entries.retain(|path, _| !same_relative_path(path, Path::new(preserved)));
            if let Some((path, value)) = current.and_then(|tree| {
                tree.entries
                    .iter()
                    .find(|(path, _)| same_relative_path(path, Path::new(preserved)))
            }) {
                if value.is_none() {
                    return Err(invalid(
                        path,
                        "A preserved world configuration must be a regular file.",
                    ));
                }
                entries.insert(path.clone(), value.clone());
            }
        }
        let files = entries.values().filter(|entry| entry.is_some()).count() as u64;
        let bytes = entries.values().flatten().map(|(bytes, _)| bytes).sum();
        Ok(Self {
            digest: digest(serde_json::to_vec(&entries)?),
            files,
            bytes,
            entries,
        })
    }
}

pub(super) fn same_relative_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        left.to_string_lossy().replace('\\', "/").to_lowercase()
            == right.to_string_lossy().replace('\\', "/").to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

pub(super) fn digest(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One bounded walk hashes the exact bytes read through verified handles. A
/// second walk after staging rejects changes between preview, copy and commit.
pub(super) fn inventory(root: &Path, copy_to: Option<&Path>) -> Result<Inventory, StorageError> {
    inventory_with_preserved(root, copy_to, &[])
}

pub(super) fn inventory_with_preserved(
    root: &Path,
    copy_to: Option<&Path>,
    preserved: &[&str],
) -> Result<Inventory, StorageError> {
    normalize_path(root)?;
    let _root_guard =
        native::open(root, true, false).map_err(|error| invalid(root, error.to_string()))?;
    let _destination_guard = copy_to
        .map(|target| {
            native::open(target, true, false).map_err(|error| invalid(target, error.to_string()))
        })
        .transpose()?;
    let mut entries = BTreeMap::new();
    let mut pending = vec![(root.to_owned(), 0)];
    let mut files = 0;
    let mut bytes = 0_u64;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DEPTH {
            return Err(invalid(root, "Save import exceeds 64 directory levels."));
        }
        let _directory_guard = native::open(&directory, true, false)
            .map_err(|error| invalid(&directory, error.to_string()))?;
        for entry in
            fs::read_dir(&directory).map_err(|error| invalid(&directory, error.to_string()))?
        {
            let path = entry
                .map_err(|error| invalid(&directory, error.to_string()))?
                .path();
            if entries.len() >= MAX_ENTRIES {
                return Err(invalid(root, "Save import exceeds 200000 entries."));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| invalid(&path, "Save entry escaped its root."))?
                .to_owned();
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| invalid(&path, error.to_string()))?;
            if is_link_or_reparse(&metadata) || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(invalid(
                    &path,
                    "Saves cannot contain links, junctions or special files.",
                ));
            }
            if metadata.is_dir() {
                // The open pins all existing ancestors on Windows, including a
                // child that was replaced after the initial metadata read.
                let _guard = native::open(&path, true, false)
                    .map_err(|error| invalid(&path, error.to_string()))?;
                if let Some(destination) = copy_to {
                    let target = destination.join(&relative);
                    let parent = target
                        .parent()
                        .ok_or_else(|| invalid(&target, "Save entry has no parent."))?;
                    let _guard = native::open(parent, true, false)
                        .map_err(|error| invalid(parent, error.to_string()))?;
                    fs::create_dir(&target).map_err(|error| invalid(&target, error.to_string()))?;
                }
                entries.insert(relative, None);
                pending.push((path, depth + 1));
                continue;
            }
            let mut source = native::open_verified_file(&path, false)
                .map_err(|error| invalid(&path, error.to_string()))?;
            let mut target = copy_to
                .filter(|_| {
                    !preserved
                        .iter()
                        .any(|name| same_relative_path(&relative, Path::new(name)))
                })
                .map(|destination| {
                    let target = destination.join(&relative);
                    new_destination(&target)
                })
                .transpose()?;
            let mut hash = Sha256::new();
            let mut length = 0_u64;
            let mut buffer = [0_u8; 65536];
            loop {
                let count = source
                    .reader()
                    .read(&mut buffer)
                    .map_err(|error| invalid(&path, error.to_string()))?;
                if count == 0 {
                    break;
                }
                length += count as u64;
                bytes = bytes
                    .checked_add(count as u64)
                    .filter(|size| *size <= MAX_BYTES)
                    .ok_or_else(|| {
                        invalid(root, "Save import exceeds the 1 TiB verification limit.")
                    })?;
                hash.update(&buffer[..count]);
                if let Some(target) = target.as_mut() {
                    target
                        .file
                        .write_all(&buffer[..count])
                        .map_err(|error| invalid(&path, error.to_string()))?;
                }
            }
            if length != metadata.len() {
                return Err(invalid(
                    &path,
                    "A save file changed while reading. Close the source game and retry.",
                ));
            }
            if let Some(target) = target {
                target
                    .file
                    .sync_all()
                    .map_err(|error| invalid(&path, error.to_string()))?;
            }
            let hash = hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            entries.insert(relative, Some((length, hash)));
            files += 1;
        }
    }
    Ok(Inventory {
        digest: digest(serde_json::to_vec(&entries)?),
        files,
        bytes,
        entries,
    })
}

pub(super) fn preserve_target_files(
    source: &Path,
    destination: &Path,
    current: Option<&Inventory>,
    preserved: &[&str],
) -> Result<(), StorageError> {
    let _destination_guard = native::open(destination, true, false)
        .map_err(|error| invalid(destination, error.to_string()))?;
    for name in preserved {
        let Some((relative, _)) = current.and_then(|tree| {
            tree.entries
                .iter()
                .find(|(path, _)| same_relative_path(path, Path::new(name)))
        }) else {
            continue;
        };
        let path = source.join(relative);
        let mut file = native::open_verified_file(&path, false)
            .map_err(|error| invalid(&path, error.to_string()))?;
        let target = destination.join(relative);
        let mut output = new_destination(&target)?;
        std::io::copy(file.reader(), &mut output.file)
            .map_err(|error| invalid(&target, error.to_string()))?;
        output
            .file
            .sync_all()
            .map_err(|error| invalid(&target, error.to_string()))?;
    }
    Ok(())
}

pub(super) fn current_inventory(root: &Path) -> Result<Option<Inventory>, StorageError> {
    normalize_path(root)?;
    match fs::symlink_metadata(root) {
        Ok(_) => inventory(root, None).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(invalid(root, error.to_string())),
    }
}

pub(super) fn validate_source_path(
    source: &Path,
    target: &Path,
    protected: &[PathBuf],
) -> Result<PathBuf, StorageError> {
    let source = normalize_path(source)?;
    canonical_plain_directory(&source, &source)?;
    let target = normalize_path(target)?;
    for other in std::iter::once(target).chain(
        protected
            .iter()
            .map(|path| normalize_path(path))
            .collect::<Result<Vec<_>, _>>()?,
    ) {
        if contains(&source, &other) || contains(&other, &source) {
            return Err(invalid(
                &source,
                "Import source must be outside LGSM managed instance, program, backup and archive directories, and must not overlap the destination.",
            ));
        }
    }
    Ok(source)
}

pub(super) fn reject_non_save_entries(
    tree: &Inventory,
    source: &Path,
    native_files: &[PathBuf],
) -> Result<(), StorageError> {
    for relative in tree.entries.keys() {
        if tree
            .entries
            .get(relative)
            .is_some_and(|entry| entry.is_some())
            && native_files
                .iter()
                .any(|path| same_relative_path(relative, path))
        {
            continue;
        }
        let Some(name) = relative.file_name().and_then(|name| name.to_str()) else {
            return Err(invalid(source, "Save entry filename is not valid Unicode."));
        };
        let name = name.to_ascii_lowercase();
        let extension = relative
            .extension()
            .and_then(|part| part.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            extension.as_str(),
            "exe" | "dll" | "bat" | "cmd" | "ps1" | "sh" | "ini" | "cfg" | "toml"
        ) || matches!(
            name.as_str(),
            "server.properties"
                | "serverconfig.txt"
                | "enshrouded_server.json"
                | "serverconfig.json"
                | "instance.json"
                | "adminlist.txt"
                | "bannedlist.txt"
                | "permittedlist.txt"
                | "admins.json"
                | "playerbans.json"
        ) {
            return Err(invalid(
                &source.join(relative),
                "The source contains server configuration or executable files. Select only the native world/save directory.",
            ));
        }
    }
    Ok(())
}

pub(super) fn read_prefix(path: &Path, limit: u64) -> Result<Vec<u8>, StorageError> {
    let mut source = native::open_verified_file(path, false)
        .map_err(|error| invalid(path, error.to_string()))?;
    let mut bytes = Vec::new();
    source
        .reader()
        .take(limit)
        .read_to_end(&mut bytes)
        .map_err(|error| invalid(path, error.to_string()))?;
    Ok(bytes)
}

pub(super) fn copy_file(source: &Path, target: &Path, length: u64) -> Result<(), StorageError> {
    let mut input = native::open_verified_file(source, false)
        .map_err(|error| invalid(source, error.to_string()))?;
    let mut output = new_destination(target)?;
    let copied = std::io::copy(&mut input.reader().take(length + 1), &mut output.file)
        .map_err(|error| invalid(target, error.to_string()))?;
    if copied != length {
        return Err(invalid(source, "Save content changed while copying."));
    }
    output
        .file
        .sync_all()
        .map_err(|error| invalid(target, error.to_string()))
}
