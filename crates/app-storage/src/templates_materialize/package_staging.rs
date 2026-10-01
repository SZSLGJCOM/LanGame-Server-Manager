use std::fs::{self, File, FileTimes, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::StorageError;

#[path = "package_record.rs"]
mod package_record;
pub(crate) use package_record::{copy_package_file_with_record, validate_owned_target};
#[cfg(test)]
pub(crate) use package_record::{fail_next_publication_for_test, file_sha256};

const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 24;

#[derive(Clone, Copy)]
enum PackageKind {
    File,
    Directory,
}

#[derive(PartialEq, Eq)]
struct EntryStamp {
    relative: PathBuf,
    directory: bool,
    length: u64,
    modified: SystemTime,
}

/// Installs one package from a sibling staging directory. Other packages and
/// instance settings have separate owners; this is not a runtime-wide rollback.
pub(crate) fn copy_package_file(
    module_id: &str,
    source: &Path,
    target: &Path,
) -> Result<(), StorageError> {
    deploy(
        module_id,
        source,
        target,
        PackageKind::File,
        copy_bytes,
        move_exclusive,
    )
}

pub(crate) fn replace_package_directory(
    module_id: &str,
    source: &Path,
    target: &Path,
) -> Result<(), StorageError> {
    deploy(
        module_id,
        source,
        target,
        PackageKind::Directory,
        copy_bytes,
        move_exclusive,
    )
}

fn deploy(
    module_id: &str,
    source: &Path,
    target: &Path,
    kind: PackageKind,
    mut copy: impl FnMut(&Path, &Path) -> io::Result<u64>,
    publish: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<(), StorageError> {
    let result = (|| {
        reject_link_ancestors(source)?;
        reject_link_ancestors(target)?;
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("Package target has no parent."))?;
        fs::create_dir_all(parent)?;
        let parent = fs::canonicalize(parent)?;
        let target = parent.join(
            target
                .file_name()
                .ok_or_else(|| io::Error::other("Package target has no name."))?,
        );
        let source = fs::canonicalize(source)?;
        if target.starts_with(&source) || source.starts_with(&target) {
            return Err(io::Error::other(
                "Package source and target must be separate.",
            ));
        }
        let source_stamp = snapshot(&source, kind)?;
        let original = optional_snapshot(&target, kind)?;
        let work = create_work_directory(&parent)?;
        let prepared = work.join("prepared");
        let retained = work.join("retained");
        let operation = (|| {
            copy_snapshot(&source, &prepared, &source_stamp, &mut copy)?;
            if snapshot(&source, kind)? != source_stamp {
                return Err(io::Error::other("Package source changed during staging."));
            }
            if optional_snapshot(&target, kind)? != original {
                return Err(io::Error::other("Package target changed during staging."));
            }
            if original.is_some() {
                move_exclusive(&target, &retained)?;
                if optional_snapshot(&retained, kind)? != original {
                    return Err(io::Error::other(
                        "Package target changed before publication.",
                    ));
                }
            }
            publish(&prepared, &target)
        })();
        match operation {
            Ok(()) => remove_work_directory(&work, &parent),
            Err(error) => {
                if retained.exists() {
                    // Never replace a target created by another writer while the
                    // original was retained. Keep that original for recovery.
                    if let Err(restore) = move_exclusive(&retained, &target) {
                        return Err(io::Error::other(format!(
                            "{error}; original package retained at {} because restoration failed: {restore}",
                            retained.display()
                        )));
                    }
                }
                match remove_work_directory(&work, &parent) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(io::Error::other(format!(
                        "{error}; staging cleanup failed: {cleanup}"
                    ))),
                }
            }
        }
    })();
    result.map_err(|error| StorageError::ModuleSupportMaterialization {
        module_id: module_id.to_owned(),
        path: target.to_owned(),
        message: error.to_string(),
    })
}

fn optional_snapshot(path: &Path, kind: PackageKind) -> io::Result<Option<Vec<EntryStamp>>> {
    match fs::symlink_metadata(path) {
        Ok(_) => snapshot(path, kind).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn snapshot(path: &Path, kind: PackageKind) -> io::Result<Vec<EntryStamp>> {
    let metadata = reject_link(path)?;
    if metadata.is_dir() != matches!(kind, PackageKind::Directory) {
        return Err(io::Error::other(
            "Package target type does not match its source.",
        ));
    }
    let mut entries = Vec::new();
    snapshot_entry(path, path, &mut entries, 0)?;
    Ok(entries)
}

fn snapshot_entry(
    root: &Path,
    path: &Path,
    entries: &mut Vec<EntryStamp>,
    depth: usize,
) -> io::Result<()> {
    if depth >= MAX_DEPTH || entries.len() >= MAX_FILES {
        return Err(io::Error::other(
            "Package size or directory depth exceeds the limit.",
        ));
    }
    let metadata = reject_link(path)?;
    if !metadata.is_dir() && !metadata.is_file() {
        return Err(io::Error::other(
            "Package entry is not a regular file or directory.",
        ));
    }
    entries.push(EntryStamp {
        relative: path
            .strip_prefix(root)
            .map_err(io::Error::other)?
            .to_owned(),
        directory: metadata.is_dir(),
        length: if metadata.is_dir() { 0 } else { metadata.len() },
        modified: metadata.modified()?,
    });
    if metadata.is_dir() {
        let mut children = Vec::new();
        for entry in fs::read_dir(path)? {
            if children.len() >= MAX_FILES {
                return Err(io::Error::other("Package entry count exceeds the limit."));
            }
            children.push(entry?.path());
        }
        children.sort();
        for child in children {
            snapshot_entry(root, &child, entries, depth + 1)?;
        }
    }
    Ok(())
}

fn copy_snapshot(
    source: &Path,
    target: &Path,
    entries: &[EntryStamp],
    copy: &mut impl FnMut(&Path, &Path) -> io::Result<u64>,
) -> io::Result<()> {
    for entry in entries {
        let from = package_entry_path(source, &entry.relative);
        let to = package_entry_path(target, &entry.relative);
        if entry.directory {
            fs::create_dir(&to)?;
        } else {
            if copy(&from, &to)? != entry.length {
                return Err(io::Error::other(
                    "Package file size changed during staging.",
                ));
            }
            let file = File::options().write(true).open(&to)?;
            file.set_times(FileTimes::new().set_modified(entry.modified))?;
            file.set_permissions(fs::metadata(&from)?.permissions())?;
            file.sync_all()?;
        }
    }
    Ok(())
}

fn package_entry_path(root: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        root.to_owned()
    } else {
        root.join(relative)
    }
}

fn copy_bytes(source: &Path, target: &Path) -> io::Result<u64> {
    let mut source = File::open(source)?;
    let mut target = File::options().write(true).create_new(true).open(target)?;
    io::copy(&mut source, &mut target)
}

fn create_work_directory(parent: &Path) -> io::Result<PathBuf> {
    for _ in 0..16 {
        let path = parent.join(format!(
            ".lgsm-package-{}",
            uuid::Uuid::new_v4().as_simple()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(
        "Could not allocate package staging directory.",
    ))
}

fn remove_work_directory(work: &Path, parent: &Path) -> io::Result<()> {
    reject_link(work)?;
    let resolved = fs::canonicalize(work)?;
    if resolved.parent() != Some(parent)
        || !resolved
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".lgsm-package-"))
    {
        return Err(io::Error::other(
            "Package staging cleanup path escaped its parent.",
        ));
    }
    fs::remove_dir_all(resolved)
}

fn reject_link_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match reject_link(ancestor) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn reject_link(path: &Path) -> io::Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    let link = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let link = {
        use std::os::windows::fs::MetadataExt;
        link || metadata.file_attributes() & 0x400 != 0
    };
    if link {
        return Err(io::Error::other(
            "Linked package entries cannot be materialized.",
        ));
    }
    Ok(metadata)
}

#[cfg(windows)]
fn move_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Omitting MOVEFILE_REPLACE_EXISTING also protects restoration from a new
    // destination created after the original package was moved aside.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn move_exclusive(from: &Path, to: &Path) -> io::Result<()> {
    match fs::symlink_metadata(to) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Package destination already exists.",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::rename(from, to)
}

#[cfg(test)]
#[path = "package_staging_tests.rs"]
mod tests;
