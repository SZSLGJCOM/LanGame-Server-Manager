use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Snapshot the tiny synthetic file trees used by storage tests, including empty directories.
pub(crate) fn tree_snapshot(root: &Path) -> std::io::Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
    let mut snapshot = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(path) = pending.pop() {
        let relative = path
            .strip_prefix(root)
            .map_err(std::io::Error::other)?
            .to_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink()
            || crate::private_runtime::is_reparse_point(&path).map_err(std::io::Error::other)?
        {
            return Err(std::io::Error::other(
                "snapshot fixture must not contain links",
            ));
        }
        if metadata.is_dir() {
            snapshot.insert(relative, None);
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else {
            snapshot.insert(relative, Some(fs::read(&path)?));
        }
    }
    Ok(snapshot)
}
