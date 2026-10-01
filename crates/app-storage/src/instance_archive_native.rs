use crate::managed_console_log::owned_fs::{FileIdentity, identity, reject_links};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
#[cfg(not(windows))]
use std::path::PathBuf;

pub(crate) struct OwnedNode {
    file: File,
    #[cfg(not(windows))]
    path: PathBuf,
    #[cfg(not(windows))]
    directory: bool,
    #[cfg(windows)]
    _parents: Vec<File>,
}

pub(crate) fn open(path: &Path, directory: bool, mutate: bool) -> io::Result<OwnedNode> {
    reject_links(path)?;
    #[cfg(windows)]
    let parents = pin_parents(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        // Attribute-only handles do not participate in Windows delete sharing.
        // Directory guards need LIST_DIRECTORY to actually prevent replacement.
        options
            .access_mode(
                FILE_READ_ATTRIBUTES
                    | if directory { FILE_LIST_DIRECTORY } else { 0 }
                    | if mutate { DELETE } else { 0 },
            )
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(
                FILE_FLAG_OPEN_REPARSE_POINT
                    | if directory {
                        FILE_FLAG_BACKUP_SEMANTICS
                    } else {
                        0
                    },
            );
    }
    #[cfg(not(windows))]
    let _ = mutate;
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("Archive node is a reparse point."));
        }
    }
    if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err(io::Error::other("Archive node type changed."));
    }
    Ok(OwnedNode {
        file,
        #[cfg(windows)]
        _parents: parents,
        #[cfg(not(windows))]
        path: path.to_owned(),
        #[cfg(not(windows))]
        directory,
    })
}

/// Hash and mutate one unchanged file through the same handle. Windows sharing
/// denies existing or new writers and replacements until this handle closes.
pub(crate) fn open_verified_file(path: &Path, mutate: bool) -> io::Result<OwnedNode> {
    reject_links(path)?;
    #[cfg(windows)]
    let parents = pin_parents(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::*;
        options
            .access_mode(FILE_READ_DATA | FILE_READ_ATTRIBUTES | if mutate { DELETE } else { 0 })
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    let _ = mutate;
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("Archive payload is not a regular file."));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("Archive payload is a reparse point."));
        }
    }
    Ok(OwnedNode {
        file,
        #[cfg(windows)]
        _parents: parents,
        #[cfg(not(windows))]
        path: path.to_owned(),
        #[cfg(not(windows))]
        directory: false,
    })
}

#[cfg(windows)]
fn pin_parents(path: &Path) -> io::Result<Vec<File>> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Archive node has no parent."))?;
    let mut ancestors = parent
        .ancestors()
        .filter(|path| path.has_root())
        .collect::<Vec<_>>();
    ancestors.reverse();
    let mut handles = Vec::new();
    for ancestor in ancestors {
        let file = OpenOptions::new()
            .read(true)
            .access_mode(FILE_READ_ATTRIBUTES | FILE_LIST_DIRECTORY)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(ancestor)?;
        use std::os::windows::fs::MetadataExt;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "Archive ancestor is not a plain directory.",
            ));
        }
        handles.push(file);
    }
    Ok(handles)
}

impl OwnedNode {
    pub(crate) fn identity(&self) -> io::Result<FileIdentity> {
        identity(&self.file)
    }

    pub(crate) fn reader(&mut self) -> &mut File {
        &mut self.file
    }

    pub(crate) fn remove(self) -> io::Result<()> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::*;
            let disposition = FILE_DISPOSITION_INFO_EX {
                Flags: FILE_DISPOSITION_FLAG_DELETE
                    | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                    | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE,
            };
            // The verified handle owns DELETE access and denies replacement until close.
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
        #[cfg(not(windows))]
        {
            reject_links(&self.path)?;
            if self.directory {
                std::fs::remove_dir(&self.path)
            } else {
                std::fs::remove_file(&self.path)
            }
        }
    }

    pub(crate) fn rename(self, to: &Path) -> io::Result<()> {
        reject_links(to)?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::*;
            let to = std::path::absolute(to)?;
            // Unlike std::fs, FILE_RENAME_INFO does not add a verbatim Windows
            // prefix for long DOS paths. Canonicalize the existing parent so
            // both drive and UNC destinations retain extended-path support.
            let parent = to
                .parent()
                .ok_or_else(|| io::Error::other("Archive destination has no parent."))?;
            let leaf = to
                .file_name()
                .ok_or_else(|| io::Error::other("Archive destination has no name."))?;
            let to = std::fs::canonicalize(parent)?.join(leaf);
            let _parents = pin_parents(&to)?;
            let name = to.as_os_str().encode_wide().collect::<Vec<_>>();
            if name.len() > 32767 {
                return Err(io::Error::other("Archive destination path is too long."));
            }
            let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
            let bytes =
                (offset + (name.len() + 1) * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
            let mut storage = vec![0usize; bytes.div_ceil(std::mem::size_of::<usize>())];
            let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
            // The aligned buffer contains the complete header and NUL-terminated name; replacement stays disabled.
            unsafe {
                info.write(FILE_RENAME_INFO::default());
                (*info).FileNameLength = (name.len() * 2) as u32;
                std::ptr::copy_nonoverlapping(
                    name.as_ptr(),
                    storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
                    name.len(),
                );
                if SetFileInformationByHandle(
                    self.file.as_raw_handle(),
                    FileRenameInfo,
                    info.cast(),
                    bytes as u32,
                ) == 0
                {
                    return Err(io::Error::last_os_error());
                }
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            if to.try_exists()? {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "Archive destination exists.",
                ));
            }
            std::fs::rename(&self.path, to)
        }
    }
}
