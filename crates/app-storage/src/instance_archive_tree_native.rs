use std::ffi::{OsStr, OsString};
use std::io;
use std::marker::PhantomData;
#[cfg(not(windows))]
use std::path::PathBuf;
use std::path::{Component, Path};

use super::OwnedNode;
use crate::managed_console_log::owned_fs::FileIdentity;

/// A direct child borrows its parent guard, so every directory in the active
/// branch remains pinned until all child handles and mutations have finished.
pub(crate) struct TreeNode<'parent> {
    node: OwnedNode,
    #[cfg(not(windows))]
    path: PathBuf,
    _parent: PhantomData<&'parent OwnedNode>,
}

impl TreeNode<'static> {
    pub(crate) fn open_root(path: &Path, mutate: bool) -> io::Result<Self> {
        let path = crate::instance_isolation::paths::normalize_resource_path(path)
            .map_err(io::Error::other)?;
        let node = super::open(&path, true, mutate)?;
        Ok(Self {
            node,
            #[cfg(not(windows))]
            path,
            _parent: PhantomData,
        })
    }
}

impl TreeNode<'_> {
    pub(crate) fn identity(&self) -> io::Result<FileIdentity> {
        self.node.identity()
    }

    pub(crate) fn children(&self, limit: usize) -> io::Result<Vec<(OsString, bool)>> {
        self.check_directory()?;
        #[cfg(windows)]
        {
            windows::children(&self.node, limit)
        }
        #[cfg(not(windows))]
        {
            super::reject_links(&self.path)?;
            let mut children = Vec::new();
            for entry in std::fs::read_dir(&self.path)? {
                let entry = entry?;
                let name = entry.file_name();
                validate_name(&name)?;
                let metadata = std::fs::symlink_metadata(entry.path())?;
                if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file())
                {
                    return Err(io::Error::other("Archive contains an unsupported node."));
                }
                push_child(&mut children, (name, metadata.is_dir()), limit)?;
            }
            super::reject_links(&self.path)?;
            Ok(children)
        }
    }

    pub(crate) fn child<'parent>(
        &'parent self,
        name: &OsStr,
        directory: bool,
        mutate: bool,
    ) -> io::Result<TreeNode<'parent>> {
        validate_name(name)?;
        self.check_directory()?;
        #[cfg(windows)]
        let node = windows::open_child(&self.node, name, directory, mutate)?;
        #[cfg(not(windows))]
        let path = self.path.join(name);
        #[cfg(not(windows))]
        let node = super::open(&path, directory, mutate)?;
        Ok(TreeNode {
            node,
            #[cfg(not(windows))]
            path,
            _parent: PhantomData,
        })
    }

    pub(crate) fn remove(self) -> io::Result<()> {
        self.node.remove()
    }

    fn check_directory(&self) -> io::Result<()> {
        let metadata = self.node.file.metadata()?;
        if !metadata.is_dir() {
            return Err(io::Error::other("Archive parent is not a directory."));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other("Archive parent became a reparse point."));
            }
        }
        Ok(())
    }
}

fn validate_name(name: &OsStr) -> io::Result<()> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(value)) if value == name)
        || components.next().is_some()
    {
        return Err(io::Error::other(
            "Archive child must have one ordinary name.",
        ));
    }
    #[cfg(windows)]
    {
        let name = name.to_string_lossy();
        if name.ends_with(['.', ' '])
            || name
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '|' | '?' | '*'))
        {
            return Err(io::Error::other(
                "Archive child has an ambiguous Windows name.",
            ));
        }
    }
    Ok(())
}

fn push_child(
    children: &mut Vec<(OsString, bool)>,
    child: (OsString, bool),
    limit: usize,
) -> io::Result<()> {
    if children.len() >= limit {
        return Err(io::Error::other(
            "Archive exceeds the safe cleanup entry limit.",
        ));
    }
    children.push(child);
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::{OwnedNode, push_child, validate_name};
    use std::ffi::{OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::os::windows::fs::MetadataExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
        FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
    };
    use windows_sys::Win32::Foundation::{
        ERROR_NO_MORE_FILES, HANDLE, RtlNtStatusToDosError, UNICODE_STRING,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FULL_DIR_INFO,
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        FileFullDirectoryInfo, FileFullDirectoryRestartInfo, GetFileInformationByHandleEx,
        SYNCHRONIZE,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    pub(super) fn open_child(
        parent: &OwnedNode,
        name: &OsStr,
        directory: bool,
        mutate: bool,
    ) -> io::Result<OwnedNode> {
        let mut encoded = name.encode_wide().collect::<Vec<_>>();
        let bytes = u16::try_from(
            encoded
                .len()
                .checked_mul(2)
                .ok_or_else(|| io::Error::other("Archive child name is too long."))?,
        )
        .map_err(|_| io::Error::other("Archive child name is too long."))?;
        let name = UNICODE_STRING {
            Length: bytes,
            MaximumLength: bytes,
            Buffer: encoded.as_mut_ptr(),
        };
        let attributes = OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.file.as_raw_handle(),
            ObjectName: &name,
            // Enumeration supplied the exact name; preserve distinct entries
            // in Windows directories with case sensitivity enabled.
            Attributes: 0,
            ..Default::default()
        };
        let mut status_block = IO_STATUS_BLOCK::default();
        let mut handle: HANDLE = std::ptr::null_mut();
        // SAFETY: Both input structures and their UTF-16 buffer remain alive
        // until this synchronous call returns; parent owns RootDirectory. The
        // name is one validated component, so it cannot escape that handle.
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                FILE_READ_ATTRIBUTES
                    | SYNCHRONIZE
                    | if directory { FILE_LIST_DIRECTORY } else { 0 }
                    | if mutate { DELETE } else { 0 },
                &attributes,
                &mut status_block,
                std::ptr::null(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                FILE_OPEN,
                FILE_OPEN_REPARSE_POINT
                    | FILE_SYNCHRONOUS_IO_NONALERT
                    | if directory {
                        FILE_DIRECTORY_FILE
                    } else {
                        FILE_NON_DIRECTORY_FILE
                    },
                std::ptr::null(),
                0,
            )
        };
        if status < 0 {
            // SAFETY: This conversion accepts every NTSTATUS value and has no
            // pointer arguments or ownership effects.
            let error = unsafe { RtlNtStatusToDosError(status) };
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        if status != 0 {
            // The synchronous FILE_OPEN contract returns completion here, never
            // STATUS_PENDING. Do not treat an unexpected informational status
            // as a completed acquisition or leak a handle it supplied.
            if !handle.is_null() {
                // SAFETY: A non-error result supplied a unique owned handle.
                drop(unsafe { File::from_raw_handle(handle) });
            }
            return Err(io::Error::other(
                "Archive child open did not complete successfully.",
            ));
        }
        if handle.is_null() {
            return Err(io::Error::other("Archive child open returned no handle."));
        }
        // SAFETY: A successful NtCreateFile returned this uniquely owned handle.
        // File closes it on metadata errors as well as on normal scope exit.
        let file = unsafe { File::from_raw_handle(handle) };
        let metadata = file.metadata()?;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other("Archive node is a reparse point."));
        }
        if metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
            return Err(io::Error::other("Archive node type changed."));
        }
        Ok(OwnedNode {
            file,
            // TreeNode's borrow keeps the already pinned parent chain alive.
            _parents: Vec::new(),
        })
    }

    pub(super) fn children(parent: &OwnedNode, limit: usize) -> io::Result<Vec<(OsString, bool)>> {
        const BUFFER_BYTES: usize = 64 * 1024;
        const NAME_OFFSET: usize = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
        let mut buffer = vec![0_u64; BUFFER_BYTES / std::mem::size_of::<u64>()];
        let mut class = FileFullDirectoryRestartInfo;
        let mut children = Vec::new();
        loop {
            // SAFETY: The live directory handle has LIST_DIRECTORY access. u64
            // storage supplies the documented 8-byte alignment and full size.
            if unsafe {
                GetFileInformationByHandleEx(
                    parent.file.as_raw_handle(),
                    class,
                    buffer.as_mut_ptr().cast(),
                    BUFFER_BYTES as u32,
                )
            } == 0
            {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    return Ok(children);
                }
                return Err(error);
            }
            class = FileFullDirectoryInfo;
            let mut offset = 0_usize;
            loop {
                if !offset.is_multiple_of(8)
                    || offset + std::mem::size_of::<FILE_FULL_DIR_INFO>() > BUFFER_BYTES
                {
                    return Err(io::Error::other(
                        "Invalid archive directory enumeration header.",
                    ));
                }
                // SAFETY: Offset is aligned and the complete fixed header lies
                // in initialized storage populated by the successful call.
                let entry = unsafe {
                    &*buffer
                        .as_ptr()
                        .cast::<u8>()
                        .add(offset)
                        .cast::<FILE_FULL_DIR_INFO>()
                };
                let name_bytes = entry.FileNameLength as usize;
                if name_bytes == 0
                    || !name_bytes.is_multiple_of(2)
                    || name_bytes > BUFFER_BYTES - offset - NAME_OFFSET
                {
                    return Err(io::Error::other("Invalid archive directory entry name."));
                }
                // SAFETY: The byte count above bounds this aligned UTF-16 tail;
                // OsString copies it before the enumeration buffer is reused.
                let name = OsString::from_wide(unsafe {
                    std::slice::from_raw_parts(
                        buffer
                            .as_ptr()
                            .cast::<u8>()
                            .add(offset + NAME_OFFSET)
                            .cast::<u16>(),
                        name_bytes / 2,
                    )
                });
                if name != OsStr::new(".") && name != OsStr::new("..") {
                    validate_name(&name)?;
                    if entry.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                        return Err(io::Error::other("Archive contains a reparse point."));
                    }
                    push_child(
                        &mut children,
                        (name, entry.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0),
                        limit,
                    )?;
                }
                let next = entry.NextEntryOffset as usize;
                if next == 0 {
                    break;
                }
                if !next.is_multiple_of(8)
                    || next < NAME_OFFSET + name_bytes
                    || next > BUFFER_BYTES - offset
                {
                    return Err(io::Error::other("Invalid archive directory entry offset."));
                }
                offset += next;
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "instance_archive_tree_native_tests.rs"]
mod tests;
