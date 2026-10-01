//! Identity-checked filesystem operations for durable program removal.
//!
//! Callers must hold the program lifecycle lease and persist the expected node
//! identities before mutation. Paths alone never authorize replacing a node.
use std::fs;
use std::path::{Path, PathBuf};

use crate::StorageError;
use crate::instance_archive_files::{self as archive, native};
use crate::instance_archive_store::invalid;
use crate::managed_console_log::owned_fs::FileIdentity;

pub fn normalize_path(path: &Path) -> Result<PathBuf, StorageError> {
    crate::instance_isolation::paths::normalize_resource_path(path)
}

pub fn identity(path: &Path, directory: bool) -> Result<Option<String>, StorageError> {
    normalize_path(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: path.to_owned(),
                source,
            });
        }
        Ok(_) => {}
    }
    let node = native::open(path, directory, false).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let identity = node.identity().map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    Ok(Some(serde_json::to_string(&identity)?))
}

fn owned_node(
    path: &Path,
    expected: &str,
    directory: bool,
) -> Result<native::OwnedNode, StorageError> {
    normalize_path(path)?;
    let expected: FileIdentity = serde_json::from_str(expected)?;
    let node = native::open(path, directory, true).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    if node.identity().map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })? != expected
    {
        return Err(invalid(
            path,
            "Program removal node identity changed; existing files were preserved.",
        ));
    }
    Ok(node)
}

pub fn move_path(
    from: &Path,
    to: &Path,
    expected: &str,
    directory: bool,
) -> Result<(), StorageError> {
    normalize_path(to)?;
    match fs::symlink_metadata(to) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: to.to_owned(),
                source,
            });
        }
        Ok(_) => return Err(invalid(to, "Program removal destination already exists.")),
    }
    owned_node(from, expected, directory)?
        .rename(to)
        .map_err(|source| StorageError::MovePath {
            from: from.to_owned(),
            to: to.to_owned(),
            source,
        })
}

pub fn remove_empty_directory(path: &Path, expected: &str) -> Result<(), StorageError> {
    remove_node(path, expected, true)
}

pub fn remove_file(path: &Path, expected: &str) -> Result<(), StorageError> {
    remove_node(path, expected, false)
}

fn remove_node(path: &Path, expected: &str, directory: bool) -> Result<(), StorageError> {
    owned_node(path, expected, directory)?
        .remove()
        .map_err(|source| StorageError::DeletePath {
            path: path.to_owned(),
            source,
        })
}

pub fn preflight_directory(path: &Path, expected: &str) -> Result<(), StorageError> {
    archive::preflight_tree(path, expected)
}

pub fn purge_directory(path: &Path, expected: &str) -> Result<(), StorageError> {
    archive::purge_tree(path, expected)
}
