use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FileIdentity {
    volume: u64,
    index: u64,
}

impl FileIdentity {
    pub(crate) fn same_volume(&self, other: &Self) -> bool {
        self.volume == other.volume
    }
}

pub(crate) fn identity(file: &File) -> io::Result<FileIdentity> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: File owns the handle and the output buffer has the required layout.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(FileIdentity {
            volume: u64::from(information.dwVolumeSerialNumber),
            index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(FileIdentity {
            volume: metadata.dev(),
            index: metadata.ino(),
        })
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Err(io::Error::other(
            "console retention requires stable file identities",
        ))
    }
}

pub(crate) fn reject_links(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors().filter(|path| !path.as_os_str().is_empty()) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => reject_link_metadata(&metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn reject_link_metadata(metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse {
        return Err(io::Error::other(
            "console log path contains a link or reparse point",
        ));
    }
    Ok(())
}

pub(super) fn optional_identity(path: &Path) -> io::Result<Option<FileIdentity>> {
    reject_links(path)?;
    match File::open(path) {
        Ok(file) if file.metadata()?.is_file() => Ok(Some(identity(&file)?)),
        Ok(_) => Err(io::Error::other("console path is not a file")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn remove_owned(path: &Path, expected: &FileIdentity) -> io::Result<bool> {
    #[cfg(windows)]
    {
        match windows::OwnedMutation::open(path, expected) {
            Ok(file) => {
                file.remove()?;
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }
    #[cfg(not(windows))]
    {
        match optional_identity(path)? {
            None => Ok(false),
            Some(actual) if &actual == expected => {
                fs::remove_file(path)?;
                Ok(true)
            }
            Some(_) => Err(io::Error::other("owned console segment was replaced")),
        }
    }
}

pub(super) fn move_owned(from: &Path, to: &Path, expected: &FileIdentity) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows::OwnedMutation::open(from, expected)?.rename(to)
    }
    #[cfg(not(windows))]
    {
        if optional_identity(from)?.as_ref() != Some(expected) {
            return Err(io::Error::other("owned console segment was replaced"));
        }
        reject_links(to)?;
        // The non-Windows store uses its existing controlled-directory policy.
        // Hard-link creation refuses to overwrite an existing destination.
        fs::hard_link(from, to)?;
        remove_owned(from, expected)?;
        Ok(())
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Component, PathBuf};
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileDispositionInfoEx, FileRenameInfo, SetFileInformationByHandle,
    };

    /// Deny rename/delete sharing on both the file and every parent for the
    /// entire identity check and mutation. Operations use this exact file handle.
    pub(super) struct OwnedMutation {
        file: File,
        _parents: Vec<File>,
    }

    fn absolute_path(path: &Path) -> io::Result<PathBuf> {
        // Reject aliases before Windows absolute-path normalization can erase
        // trailing dots/spaces and accidentally address a different file name.
        if path.components().any(|component| match component {
            Component::ParentDir | Component::CurDir => true,
            Component::Normal(name) => {
                let text = name.to_string_lossy();
                text.contains([':', '\0']) || text.ends_with(['.', ' '])
            }
            _ => false,
        }) {
            return Err(io::Error::other("invalid console mutation path"));
        }
        std::path::absolute(path)
    }

    fn lock_parents(path: &Path) -> io::Result<Vec<File>> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("console directory missing"))?;
        let mut ancestors = parent
            .ancestors()
            .filter(|path| path.has_root())
            .collect::<Vec<_>>();
        ancestors.reverse();
        let mut handles = Vec::with_capacity(ancestors.len());
        for ancestor in ancestors {
            let file = OpenOptions::new()
                .read(true)
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(ancestor)?;
            let metadata = file.metadata()?;
            reject_link_metadata(&metadata)?;
            if !metadata.is_dir() {
                return Err(io::Error::other("console parent is not a directory"));
            }
            handles.push(file);
        }
        Ok(handles)
    }

    impl OwnedMutation {
        pub(super) fn open(path: &Path, expected: &FileIdentity) -> io::Result<Self> {
            let path = absolute_path(path)?;
            let parents = lock_parents(&path)?;
            let file = OpenOptions::new()
                .read(true)
                .access_mode(DELETE | FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let metadata = file.metadata()?;
            reject_link_metadata(&metadata)?;
            if !metadata.is_file() || identity(&file)? != *expected {
                return Err(io::Error::other("owned console segment was replaced"));
            }
            Ok(Self {
                file,
                _parents: parents,
            })
        }

        pub(super) fn remove(self) -> io::Result<()> {
            let disposition = FILE_DISPOSITION_INFO_EX {
                Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
            };
            // SAFETY: The verified owned handle has DELETE access. POSIX unlink
            // removes this name on close while existing readers keep their data.
            if unsafe {
                SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileDispositionInfoEx,
                    (&disposition as *const FILE_DISPOSITION_INFO_EX).cast(),
                    std::mem::size_of_val(&disposition) as u32,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }

        pub(super) fn rename(self, to: &Path) -> io::Result<()> {
            let to = absolute_path(to)?;
            let _destination_parents = lock_parents(&to)?;
            let name = to.as_os_str().encode_wide().collect::<Vec<_>>();
            if name.len() > 32767 {
                return Err(io::Error::other("console destination path is too long"));
            }
            let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
            // FileNameLength excludes the terminator, but SetFileInformationByHandle
            // still requires a NUL-terminated FileName. Alignment padding is not
            // guaranteed to leave a complete UTF-16 terminator after the name.
            let bytes =
                (offset + (name.len() + 1) * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // SAFETY: usize storage supplies HANDLE alignment and enough space
            // for the header plus NUL-terminated UTF-16 name. Replacement stays false.
            unsafe {
                information.write(FILE_RENAME_INFO::default());
                (*information).FileNameLength = (name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                    name.len(),
                );
                if SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileRenameInfo,
                    information.cast(),
                    bytes as u32,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "managed_console_log_fs_tests.rs"]
mod tests;
