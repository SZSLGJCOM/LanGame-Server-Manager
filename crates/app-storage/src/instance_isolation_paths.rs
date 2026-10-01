use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::StorageError;

pub(super) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.into(),
    }
}

/// Resolve an existing ancestor, then append the uncreated tail. This also
/// normalizes Windows long-path and short-name aliases without creating files.
pub(crate) fn normalize_path(path: &Path) -> Result<PathBuf, StorageError> {
    normalize(path, true)
}

pub(crate) fn normalize_resource_path(path: &Path) -> Result<PathBuf, StorageError> {
    normalize(path, false)
}

fn normalize(path: &Path, directory: bool) -> Result<PathBuf, StorageError> {
    if !path.is_absolute() {
        return Err(invalid(path, "an absolute instance path is required"));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(invalid(
                path,
                "parent traversal is not permitted in instance paths",
            ));
        }
        #[cfg(windows)]
        match component {
            Component::Prefix(prefix) => {
                use std::path::Prefix;
                if !matches!(
                    prefix.kind(),
                    Prefix::Disk(_)
                        | Prefix::UNC(_, _)
                        | Prefix::VerbatimDisk(_)
                        | Prefix::VerbatimUNC(_, _)
                ) {
                    return Err(invalid(path, "device paths are not permitted"));
                }
            }
            Component::Normal(value) => {
                let value = value.to_string_lossy();
                if value.ends_with(['.', ' '])
                    || value.chars().any(|ch| {
                        ch.is_control() || matches!(ch, ':' | '<' | '>' | '|' | '?' | '*')
                    })
                {
                    return Err(invalid(path, "ambiguous or invalid Windows path component"));
                }
            }
            _ => {}
        }
    }
    // Checking ancestors prevents a junction from silently redirecting ownership.
    // The canonical result is still needed for drive/UNC aliases and case spelling.
    let mut cursor = path.to_owned();
    let mut missing = Vec::new();
    let existing = loop {
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) => {
                if !metadata.is_dir() && (directory || cursor != path || !metadata.is_file()) {
                    return Err(invalid(
                        path,
                        "the instance directory path points to a file",
                    ));
                }
                break cursor.clone();
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    cursor
                        .file_name()
                        .ok_or_else(|| invalid(path, "no existing directory ancestor"))?
                        .to_owned(),
                );
                if !cursor.pop() {
                    return Err(invalid(path, "no existing directory ancestor"));
                }
            }
            Err(source) => {
                return Err(StorageError::ReadPath {
                    path: cursor,
                    source,
                });
            }
        }
    };
    for ancestor in existing.ancestors() {
        let metadata = fs::symlink_metadata(ancestor).map_err(|source| StorageError::ReadPath {
            path: ancestor.to_owned(),
            source,
        })?;
        if metadata.file_type().is_symlink() || crate::private_runtime::is_reparse_point(ancestor)?
        {
            return Err(invalid(
                path,
                format!(
                    "symbolic links and junctions are not permitted in instance paths: {}",
                    ancestor.display()
                ),
            ));
        }
    }
    let mut normalized = fs::canonicalize(&existing).map_err(|source| StorageError::ReadPath {
        path: existing,
        source,
    })?;
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    Ok(normalized)
}

pub(crate) fn contains(parent: &Path, child: &Path) -> bool {
    let mut actual = child.components();
    parent.components().all(|expected| {
        actual.next().is_some_and(|component| {
            #[cfg(windows)]
            {
                component.as_os_str().to_string_lossy().to_lowercase()
                    == expected.as_os_str().to_string_lossy().to_lowercase()
            }
            #[cfg(not(windows))]
            {
                component == expected
            }
        })
    })
}

pub(super) fn overlaps(left: &Path, right: &Path) -> bool {
    contains(left, right) || contains(right, left)
}
