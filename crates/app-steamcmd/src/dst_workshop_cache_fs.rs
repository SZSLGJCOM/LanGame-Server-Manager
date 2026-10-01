use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::dst_workshop_cache::DstWorkshopCacheError;

const MAX_PAYLOAD_FILES: u64 = 200_000;
const MAX_PAYLOAD_DEPTH: usize = 64;
static NEXT_STAGE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Default)]
pub(crate) struct CopyStats {
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

pub(crate) fn inspect_tree(path: &Path, depth: usize) -> Result<CopyStats, DstWorkshopCacheError> {
    if depth > MAX_PAYLOAD_DEPTH {
        return Err(DstWorkshopCacheError::UnsafePath {
            path: path.to_path_buf(),
        });
    }
    require_plain_directory(path)?;
    let mut stats = CopyStats::default();
    for entry in fs::read_dir(path).map_err(|source| io("read directory", path, source))? {
        let entry = entry.map_err(|source| io("read directory entry", path, source))?;
        let child = entry.path();
        let metadata =
            fs::symlink_metadata(&child).map_err(|source| io("read metadata", &child, source))?;
        if is_link_or_reparse(&metadata) {
            return Err(DstWorkshopCacheError::UnsafePath { path: child });
        }
        if metadata.is_dir() {
            let nested = inspect_tree(&child, depth + 1)?;
            stats.files += nested.files;
            stats.bytes += nested.bytes;
        } else if metadata.is_file() {
            stats.files += 1;
            stats.bytes += metadata.len();
        } else {
            return Err(DstWorkshopCacheError::UnsafePath { path: child });
        }
        if stats.files > MAX_PAYLOAD_FILES {
            return Err(DstWorkshopCacheError::UnsafePath {
                path: path.to_path_buf(),
            });
        }
    }
    Ok(stats)
}

pub(crate) fn copy_tree(
    source: &Path,
    target: &Path,
    depth: usize,
) -> Result<CopyStats, DstWorkshopCacheError> {
    if depth > MAX_PAYLOAD_DEPTH {
        return Err(DstWorkshopCacheError::UnsafePath {
            path: source.to_path_buf(),
        });
    }
    require_plain_directory(source)?;
    let mut stats = CopyStats::default();
    for entry in
        fs::read_dir(source).map_err(|source_error| io("read directory", source, source_error))?
    {
        let entry =
            entry.map_err(|source_error| io("read directory entry", source, source_error))?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from)
            .map_err(|source_error| io("read metadata", &from, source_error))?;
        if is_link_or_reparse(&metadata) {
            return Err(DstWorkshopCacheError::UnsafePath { path: from });
        }
        if metadata.is_dir() {
            fs::create_dir(&to)
                .map_err(|source_error| io("create directory", &to, source_error))?;
            let nested = copy_tree(&from, &to, depth + 1)?;
            stats.files += nested.files;
            stats.bytes += nested.bytes;
        } else if metadata.is_file() {
            stats.bytes +=
                fs::copy(&from, &to).map_err(|source_error| io("copy file", &to, source_error))?;
            stats.files += 1;
        } else {
            return Err(DstWorkshopCacheError::UnsafePath { path: from });
        }
        if stats.files > MAX_PAYLOAD_FILES {
            return Err(DstWorkshopCacheError::UnsafePath {
                path: source.to_path_buf(),
            });
        }
    }
    Ok(stats)
}

pub(crate) fn canonical_plain_directory(path: &Path) -> Result<PathBuf, DstWorkshopCacheError> {
    require_plain_directory(path)?;
    fs::canonicalize(path).map_err(|source| io("canonicalize directory", path, source))
}

pub(crate) fn ensure_plain_directory(path: &Path) -> Result<(), DstWorkshopCacheError> {
    if path.exists() {
        return require_plain_directory(path);
    }
    let parent = path
        .parent()
        .ok_or_else(|| DstWorkshopCacheError::UnsafePath {
            path: path.to_path_buf(),
        })?;
    ensure_plain_directory(parent)?;
    fs::create_dir(path).map_err(|source| io("create directory", path, source))
}

fn require_plain_directory(path: &Path) -> Result<(), DstWorkshopCacheError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io("read metadata", path, source))?;
    if !metadata.is_dir() || is_link_or_reparse(&metadata) {
        return Err(DstWorkshopCacheError::UnsafePath {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

pub(crate) fn require_plain_file(path: &Path) -> Result<(), DstWorkshopCacheError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|source| io("read metadata", path, source))?;
    if !metadata.is_file() || is_link_or_reparse(&metadata) {
        return Err(DstWorkshopCacheError::UnsafePath {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

pub(crate) fn create_stage(root: &Path) -> Result<PathBuf, DstWorkshopCacheError> {
    for _ in 0..16 {
        let path = root.join(format!(
            ".lgsm-dst-workshop-{}-{}",
            std::process::id(),
            NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(io("create staging directory", &path, source)),
        }
    }
    Err(DstWorkshopCacheError::UnsafePath {
        path: root.to_path_buf(),
    })
}

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> DstWorkshopCacheError {
    DstWorkshopCacheError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(windows)]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
