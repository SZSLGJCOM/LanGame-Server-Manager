use std::path::{Path, PathBuf};

use super::native::TreeNode;
use crate::StorageError;
use crate::instance_archive_store::invalid;
use crate::managed_console_log::owned_fs::FileIdentity;

const MAX_TREE_ENTRIES: usize = 200_000;
const MAX_TREE_DEPTH: usize = 128;

struct TreeEntry {
    path: PathBuf,
    identity: FileIdentity,
    directory: bool,
    children: Vec<Self>,
}

fn tree_entries(root: &Path, expected: &str) -> Result<TreeEntry, StorageError> {
    // Validate the absolute root once. Each child is opened relative to its held
    // parent on Windows, preserving the same ancestry without reopening it for
    // every sibling. The complete inventory still precedes any deletion.
    let node = TreeNode::open_root(root, false).map_err(|source| StorageError::ReadPath {
        path: root.to_owned(),
        source,
    })?;
    let expected_identity: FileIdentity = serde_json::from_str(expected)?;
    check_identity(root, &node, &expected_identity)?;
    let mut discovered = 1;
    let entries = scan_tree(root, &node, true, 0, &mut discovered)?;
    if !super::verify_identity(root, Some(expected))? {
        return Err(invalid(root, "Archive vanished after cleanup preflight."));
    }
    Ok(entries)
}

fn scan_tree(
    path: &Path,
    node: &TreeNode<'_>,
    directory: bool,
    depth: usize,
    discovered: &mut usize,
) -> Result<TreeEntry, StorageError> {
    if depth > MAX_TREE_DEPTH {
        return Err(invalid(
            path,
            "Archive exceeds the safe cleanup traversal limit; no files were removed.",
        ));
    }
    let identity = node.identity().map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })?;
    let mut children = Vec::new();
    if directory {
        let names = node
            .children(MAX_TREE_ENTRIES - *discovered)
            .map_err(|source| StorageError::ReadDirectory {
                path: path.to_owned(),
                source,
            })?;
        // Count queued siblings too, so a deep, wide tree cannot hold more
        // unvisited names than the existing total inventory limit allows.
        *discovered += names.len();
        for (name, directory) in names {
            let child_path = path.join(&name);
            let child =
                node.child(&name, directory, false)
                    .map_err(|source| StorageError::ReadPath {
                        path: child_path.clone(),
                        source,
                    })?;
            children.push(scan_tree(
                &child_path,
                &child,
                directory,
                depth + 1,
                discovered,
            )?);
        }
    }
    Ok(TreeEntry {
        path: path.to_owned(),
        identity,
        directory,
        children,
    })
}

fn check_identity(
    path: &Path,
    node: &TreeNode<'_>,
    expected: &FileIdentity,
) -> Result<(), StorageError> {
    if &node.identity().map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })? != expected
    {
        return Err(invalid(
            path,
            "Archive node changed after preflight; cleanup remains incomplete.",
        ));
    }
    Ok(())
}

fn purge_entries(entry: &TreeEntry, node: TreeNode<'_>) -> Result<(), StorageError> {
    // Check the directory before touching any of its descendants. DELETE access
    // is acquired at open, so no second handle must compete with our own guard.
    check_identity(&entry.path, &node, &entry.identity)?;
    for child in &entry.children {
        let name = child
            .path
            .file_name()
            .ok_or_else(|| invalid(&child.path, "Archive child has no file name."))?;
        let handle =
            node.child(name, child.directory, true)
                .map_err(|source| StorageError::DeletePath {
                    path: child.path.clone(),
                    source,
                })?;
        purge_entries(child, handle)?;
    }
    node.remove().map_err(|source| StorageError::DeletePath {
        path: entry.path.clone(),
        source,
    })
}

pub(crate) fn preflight_tree(root: &Path, expected: &str) -> Result<(), StorageError> {
    tree_entries(root, expected).map(|_| ())
}

pub(crate) fn purge_tree(root: &Path, expected: &str) -> Result<(), StorageError> {
    let entries = tree_entries(root, expected)?;
    let root = TreeNode::open_root(root, true).map_err(|source| StorageError::DeletePath {
        path: root.to_owned(),
        source,
    })?;
    purge_entries(&entries, root)
}

#[cfg(test)]
#[path = "instance_archive_tree_tests.rs"]
mod tests;
