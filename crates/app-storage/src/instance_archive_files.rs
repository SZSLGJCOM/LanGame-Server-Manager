use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::instance_archive_store::{MAX_ARCHIVES, digest, invalid};
use crate::instance_isolation::paths::{normalize_path, normalize_resource_path};
use crate::managed_console_log::owned_fs::FileIdentity;
use crate::{StorageError, StoragePaths};

#[path = "instance_archive_native.rs"]
pub(crate) mod native;

#[path = "instance_archive_tree.rs"]
mod tree;
pub(crate) use tree::{preflight_tree, purge_tree};

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
        || crate::private_runtime::metadata_is_reparse_point(&metadata)
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
