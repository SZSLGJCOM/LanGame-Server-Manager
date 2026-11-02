use super::*;
use crate::managed_console_log::owned_fs::FileIdentity;

pub(super) fn check(plan: &FilePlan) -> Result<(), StorageError> {
    let mut volumes = Vec::<(FileIdentity, PathBuf, u64)>::new();
    for file in plan.files.iter().filter(|file| !file.reused) {
        // Old destination children are about to be preserved as a whole tree.
        // A junction or a file inside that tree must not select the copy volume.
        let destination_root = plan
            .roots
            .iter()
            .filter(|root| contains(&root.target, &file.target))
            .min_by_key(|root| root.target.components().count())
            .ok_or_else(|| invalid(&file.target, "No destination root owns this file."))?;
        let parent = destination_root
            .target
            .ancestors()
            .skip(1)
            .find(|path| path.is_dir())
            .ok_or_else(|| invalid(&file.target, "No existing destination volume is available."))?;
        let volume = if parent.parent().is_none() {
            files::volume_root_identity(parent)
        } else {
            crate::instance_archive_files::native::open(parent, true, false)
                .and_then(|node| node.identity())
        }
        .map_err(|source| StorageError::ReadPath {
            path: parent.to_owned(),
            source,
        })?;
        let index = volumes
            .iter()
            .position(|(known, _, _)| known.same_volume(&volume));
        let entry = if let Some(index) = index {
            &mut volumes[index]
        } else {
            volumes.push((volume, parent.to_owned(), 0));
            volumes
                .last_mut()
                .ok_or_else(|| invalid(parent, "Missing destination volume."))?
        };
        entry.2 = entry
            .2
            .checked_add(file.digest.size)
            .ok_or_else(|| invalid(parent, "Migration byte count overflow."))?;
    }
    for (_, path, required) in &volumes {
        if available(path)? < *required {
            return Err(invalid(
                path,
                format!(
                    "Not enough destination space; {required} bytes are required for verified copies."
                ),
            ));
        }
    }
    Ok(())
}

#[cfg(windows)]
fn available(path: &Path) -> Result<u64, StorageError> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide =
        crate::atomic_file::wide_verbatim_path(path).map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
    let mut bytes = 0;
    // Caller-available space respects filesystem quotas. All buffers outlive the call.
    if unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut bytes,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(StorageError::ReadPath {
            path: path.to_owned(),
            source: std::io::Error::last_os_error(),
        });
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn available(path: &Path) -> Result<u64, StorageError> {
    Err(invalid(
        path,
        "Storage migration capacity checks are supported only on Windows.",
    ))
}
