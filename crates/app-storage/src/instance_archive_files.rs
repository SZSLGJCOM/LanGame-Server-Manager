use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::instance_archive_store::{MAX_ARCHIVES, digest, invalid};
use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};
use crate::managed_console_log::owned_fs::FileIdentity;
use crate::{StorageError, StoragePaths};

#[path = "instance_archive_native.rs"]
pub(crate) mod native;

const MAX_TREE_ENTRIES: usize = 200_000;
const MAX_TREE_DEPTH: usize = 128;

pub(crate) struct InventoryEntry {
    pub leaf: String,
    pub identity: Option<String>,
    pub problem: Option<String>,
    pub instances_identity: String,
    pub parent_identity: String,
}

pub(crate) struct ParentGuards {
    _instances: native::OwnedNode,
    _trash: native::OwnedNode,
}

pub(crate) fn parent_identities(paths: &StoragePaths) -> Result<(String, String), StorageError> {
    let instances = identity(&paths.instances_root)?
        .ok_or_else(|| invalid(&paths.instances_root, "Instances directory is missing."))?;
    let trash = paths.archives_root.clone();
    let parent =
        identity(&trash)?.ok_or_else(|| invalid(&trash, "Archive container is missing."))?;
    Ok((instances, parent))
}

pub(crate) fn guard_parents(
    paths: &StoragePaths,
    instances: Option<&str>,
    parent: Option<&str>,
) -> Result<ParentGuards, StorageError> {
    fn guard(path: &Path, expected: Option<&str>) -> Result<native::OwnedNode, StorageError> {
        let node = native::open(path, true, false).map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
        let observed =
            serde_json::to_string(&node.identity().map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?)?;
        if expected.is_some_and(|expected| expected != observed) {
            return Err(invalid(
                path,
                "Archive parent identity changed; files and recovery metadata were preserved.",
            ));
        }
        Ok(node)
    }
    Ok(ParentGuards {
        _instances: guard(&paths.instances_root, instances)?,
        _trash: guard(&paths.archives_root, parent)?,
    })
}

pub(crate) fn archive_path(paths: &StoragePaths, leaf: &str) -> Result<PathBuf, StorageError> {
    let mut components = Path::new(leaf).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || leaf.contains(['/', '\\', '\0'])
        || leaf.ends_with(['.', ' '])
    {
        return Err(invalid(
            &paths.instances_root,
            "Archive directory name is invalid.",
        ));
    }
    let root = paths.archives_root.clone();
    plain_directory(&paths.instances_root)?;
    plain_directory(&root)?;
    normalize_path(&root)?;
    Ok(root.join(leaf))
}

pub(crate) fn plain_directory(path: &Path) -> Result<bool, StorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
    };
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || crate::private_runtime::is_reparse_point(path)?
    {
        return Err(StorageError::UnsafeManagedPath {
            path: path.to_owned(),
            root: path.parent().unwrap_or(path).to_owned(),
        });
    }
    normalize_path(path)?;
    Ok(true)
}

pub(crate) fn identity(path: &Path) -> Result<Option<String>, StorageError> {
    if !plain_directory(path)? {
        return Ok(None);
    }
    let file = native::open(path, true, false).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    Ok(Some(serde_json::to_string(&file.identity().map_err(
        |source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        },
    )?)?))
}

pub(crate) fn verify_identity(path: &Path, expected: Option<&str>) -> Result<bool, StorageError> {
    let observed = identity(path)?;
    if observed.is_none() {
        return Ok(false);
    }
    if observed.as_deref() != expected {
        return Err(invalid(
            path,
            "Archive directory identity changed; no files were modified.",
        ));
    }
    Ok(true)
}

pub(crate) fn guard_identity(
    path: &Path,
    expected: &str,
) -> Result<native::OwnedNode, StorageError> {
    let node = native::open(path, true, false).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let actual =
        serde_json::to_string(&node.identity().map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?)?;
    if actual != expected {
        return Err(invalid(
            path,
            "Owned directory identity changed; existing files were preserved.",
        ));
    }
    Ok(node)
}

pub(crate) fn config_hash(root: &Path) -> Result<String, StorageError> {
    let path = root.join("config/instance.json");
    normalize_resource_path(&path)?;
    let file = fs::File::open(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid(
            &path,
            "Archived instance configuration exceeds 4 MiB.",
        ));
    }
    let _: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(digest(&bytes))
}

pub(crate) fn inventory(paths: &StoragePaths) -> Result<Vec<InventoryEntry>, StorageError> {
    let root = paths.archives_root.clone();
    if !plain_directory(&root)? {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let (instances_identity, parent_identity) = parent_identities(paths)?;
    for entry in fs::read_dir(&root).map_err(|source| StorageError::ReadDirectory {
        path: root.clone(),
        source,
    })? {
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: root.clone(),
            source,
        })?;
        if result.len() == MAX_ARCHIVES {
            return Err(invalid(
                &root,
                "Archive directory contains more than 4096 entries; inventory was not truncated.",
            ));
        }
        let leaf = entry.file_name().into_string().map_err(|_| {
            invalid(
                &root,
                "An archive name cannot be represented safely as text.",
            )
        })?;
        let (identity, problem) = match identity(&entry.path()) {
            Ok(Some(identity)) => (Some(identity), None),
            Ok(None) => (
                None,
                Some(String::from("Archive disappeared during inventory.")),
            ),
            Err(error) => (None, Some(error.to_string())),
        };
        result.push(InventoryEntry {
            leaf,
            identity,
            problem,
            instances_identity: instances_identity.clone(),
            parent_identity: parent_identity.clone(),
        });
    }
    Ok(result)
}

pub(crate) fn move_directory(from: &Path, to: &Path, expected: &str) -> Result<(), StorageError> {
    normalize_path(from)?;
    normalize_path(to)?;
    match fs::symlink_metadata(to) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: to.to_owned(),
                source,
            });
        }
        Ok(_) => return Err(invalid(to, "Archive move destination already exists.")),
    }
    let expected: FileIdentity = serde_json::from_str(expected)?;
    let source = native::open(from, true, true).map_err(|source| StorageError::ReadPath {
        path: from.to_owned(),
        source,
    })?;
    if source.identity().map_err(|source| StorageError::ReadPath {
        path: from.to_owned(),
        source,
    })? != expected
    {
        return Err(invalid(
            from,
            "Instance directory identity changed before archival or restoration.",
        ));
    }
    source.rename(to).map_err(|source| StorageError::MovePath {
        from: from.to_owned(),
        to: to.to_owned(),
        source,
    })
}

struct TreeEntry {
    path: PathBuf,
    identity: FileIdentity,
    directory: bool,
}

fn tree_entries(root: &Path, expected: &str) -> Result<Vec<TreeEntry>, StorageError> {
    if !verify_identity(root, Some(expected))? {
        return Err(invalid(
            root,
            "Owned directory is missing before tree preflight.",
        ));
    }
    // Finish the complete bounded link/identity preflight before removing any data.
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut entries = Vec::new();
    while let Some((path, depth)) = pending.pop() {
        if entries.len() >= MAX_TREE_ENTRIES || depth > MAX_TREE_DEPTH {
            return Err(invalid(
                root,
                "Archive exceeds the safe cleanup traversal limit; no files were removed.",
            ));
        }
        normalize_resource_path(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink()
            || crate::private_runtime::is_reparse_point(&path)?
            || (!metadata.is_dir() && !metadata.is_file())
        {
            return Err(invalid(
                &path,
                "Archive contains an external link or unsupported node; cleanup was refused.",
            ));
        }
        let directory = metadata.is_dir();
        let handle =
            native::open(&path, directory, false).map_err(|source| StorageError::ReadPath {
                path: path.clone(),
                source,
            })?;
        let identity = handle.identity().map_err(|source| StorageError::ReadPath {
            path: path.clone(),
            source,
        })?;
        if directory {
            for child in fs::read_dir(&path).map_err(|source| StorageError::ReadDirectory {
                path: path.clone(),
                source,
            })? {
                let child = child.map_err(|source| StorageError::ReadDirectory {
                    path: path.clone(),
                    source,
                })?;
                if pending.len() + entries.len() >= MAX_TREE_ENTRIES {
                    return Err(invalid(
                        root,
                        "Archive exceeds the safe cleanup entry limit; no files were removed.",
                    ));
                }
                pending.push((child.path(), depth + 1));
            }
        }
        entries.push(TreeEntry {
            path,
            identity,
            directory,
        });
    }
    if !verify_identity(root, Some(expected))? {
        return Err(invalid(root, "Archive vanished after cleanup preflight."));
    }
    Ok(entries)
}

pub(crate) fn preflight_tree(root: &Path, expected: &str) -> Result<(), StorageError> {
    let _root = guard_identity(root, expected)?;
    tree_entries(root, expected).map(|_| ())
}

pub(crate) fn purge_tree(root: &Path, expected: &str) -> Result<(), StorageError> {
    let mut root_guard = Some(guard_identity(root, expected)?);
    let entries = tree_entries(root, expected)?;
    for entry in entries.into_iter().rev() {
        if entry.path == root {
            drop(root_guard.take());
        }
        let handle = native::open(&entry.path, entry.directory, true).map_err(|source| {
            StorageError::DeletePath {
                path: entry.path.clone(),
                source,
            }
        })?;
        if handle.identity().map_err(|source| StorageError::ReadPath {
            path: entry.path.clone(),
            source,
        })? != entry.identity
        {
            return Err(invalid(
                &entry.path,
                "Archive node changed after preflight; cleanup remains incomplete.",
            ));
        }
        handle.remove().map_err(|source| StorageError::DeletePath {
            path: entry.path,
            source,
        })?;
    }
    Ok(())
}
