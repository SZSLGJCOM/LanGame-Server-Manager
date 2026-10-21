//! Runtime control mailboxes only. Installer/session publication stays separate.
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// A CREATE_NEW sibling stays open from writing through namespace publication.
/// Callers retain their existing ordinary-directory pins and request leases.
pub(crate) struct PendingFile {
    file: File,
    path: PathBuf,
    published: bool,
}

impl PendingFile {
    pub(crate) fn create(path: &Path, body: &[u8]) -> io::Result<Self> {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::{
                DELETE, FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
                FILE_SHARE_READ,
            };
            options
                .access_mode(FILE_GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES)
                // After rename, a reader can open the visible request before
                // this writer is dropped. Write/delete peers remain excluded.
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        // A failed exclusive create never owns or cleans up a foreign name.
        let file = options.open(path)?;
        let mut pending = Self {
            file,
            path: path.to_owned(),
            published: false,
        };
        pending.file.write_all(body)?;
        pending.file.sync_all()?;
        Ok(pending)
    }

    pub(crate) fn replace(&mut self, destination: &Path) -> io::Result<()> {
        if self.published || self.path.parent() != destination.parent() {
            return Err(io::Error::other(
                "control publication requires an unpublished sibling",
            ));
        }
        match fs::symlink_metadata(destination) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(io::Error::other("control destination is a reparse point"));
                    }
                }
                if !metadata.is_file() || metadata.file_type().is_symlink() {
                    return Err(io::Error::other(
                        "control destination is not an ordinary file",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        #[cfg(windows)]
        replace_open_file(&self.file, destination)?;
        #[cfg(not(windows))]
        fs::rename(&self.path, destination)?;
        // No fallible work after namespace success: Drop must never remove the
        // published request, and a caller must never submit it a second time.
        self.published = true;
        Ok(())
    }
}

#[cfg(windows)]
fn replace_open_file(file: &File, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_RENAME_INFO, FileRenameInfoEx, SetFileInformationByHandle,
    };
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("control destination has no parent"))?;
    let leaf = destination
        .file_name()
        .ok_or_else(|| io::Error::other("control destination has no name"))?;
    // Match the existing archive helper: canonicalize the already pinned parent
    // to retain its extended Windows path, including long/Unicode names.
    let destination = fs::canonicalize(parent)?.join(leaf);
    let name = destination.as_os_str().encode_wide().collect::<Vec<_>>();
    if name.len() > 32767 || name.contains(&0) {
        return Err(io::Error::other("invalid control destination path"));
    }
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let bytes = (offset + (name.len() + 1) * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
    let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // REPLACE_IF_EXISTS | POSIX_SEMANTICS, documented FILE_RENAME_INFORMATION
    // flags. windows-sys provides the ABI and FileRenameInfoEx class (22).
    // Do not add IGNORE_READONLY_ATTRIBUTE or a delete/copy fallback.
    const REPLACE_WITH_OPEN_READERS: u32 = 0x1 | 0x2;
    // SAFETY: pointer-aligned storage includes the Windows header and complete
    // NUL-terminated UTF-16 name. The same CREATE_NEW handle owns DELETE access.
    unsafe {
        information.write(FILE_RENAME_INFO::default());
        (*information).Anonymous.Flags = REPLACE_WITH_OPEN_READERS;
        (*information).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfoEx,
            information.cast(),
            bytes as u32,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
            };
            let information = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: our CREATE_NEW handle still excludes peer writes/deletes.
            // Cleanup cannot delete a replacement at its former temporary name.
            let _ = unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfo,
                    (&information as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of_val(&information) as u32,
                )
            };
        }
        #[cfg(not(windows))]
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
#[path = "native_control_publish_tests.rs"]
mod tests;
