use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub(crate) enum FileStamp {
    Windows {
        volume: u64,
        file_id: [u8; 16],
        size: u64,
        modified: i64,
        changed: i64,
    },
}

impl FileStamp {
    /// Read from the content handle; the caller must exclude path links and concurrent writes.
    pub(crate) fn read(file: &File) -> io::Result<Option<Self>> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "program verification requires a regular file",
            ));
        }
        #[cfg(windows)]
        {
            windows::read(file, &metadata)
        }
        #[cfg(not(windows))]
        {
            // Other platforms lack a verified filesystem capability check; always hash content.
            Ok(None)
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::fs::Metadata;
    use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Storage::FileSystem::*;

    pub(super) fn read(file: &File, metadata: &Metadata) -> io::Result<Option<FileStamp>> {
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "program verification cannot reuse reparse-point metadata",
            ));
        }
        let handle = file.as_raw_handle();
        if !local_volume(handle)? || !supported_filesystem(handle)? {
            return Ok(None);
        }

        let mut identity = FILE_ID_INFO::default();
        let mut basic = FILE_BASIC_INFO::default();
        // Both buffers have the exact type and size required by their information class.
        let identified = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut identity as *mut FILE_ID_INFO).cast(),
                std::mem::size_of_val(&identity) as u32,
            )
        };
        if identified == 0 {
            return unsupported_or_error(io::Error::last_os_error());
        }
        let queried = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileBasicInfo,
                (&mut basic as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&basic) as u32,
            )
        };
        if queried == 0 {
            return unsupported_or_error(io::Error::last_os_error());
        }
        if basic.FileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "program file changed type during verification",
            ));
        }
        if identity.FileId.Identifier == [0; 16]
            || basic.ChangeTime <= 0
            || basic.LastWriteTime <= 0
        {
            return Ok(None);
        }
        Ok(Some(FileStamp::Windows {
            volume: identity.VolumeSerialNumber,
            file_id: identity.FileId.Identifier,
            size: metadata.len(),
            modified: basic.LastWriteTime,
            changed: basic.ChangeTime,
        }))
    }

    fn local_volume(handle: HANDLE) -> io::Result<bool> {
        // Network shares have no volume GUID; avoid trusting redirector timestamp semantics.
        let mut path = vec![0u16; 512];
        for _ in 0..2 {
            let length = unsafe {
                GetFinalPathNameByHandleW(
                    handle,
                    path.as_mut_ptr(),
                    path.len() as u32,
                    VOLUME_NAME_GUID | FILE_NAME_OPENED,
                )
            } as usize;
            if length == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_PATH_NOT_FOUND as i32) {
                    return Ok(false);
                }
                return unsupported_or_error(error).map(|_: Option<()>| false);
            }
            if length < path.len() {
                return Ok(path[..length]
                    .iter()
                    .copied()
                    .take(11)
                    .eq(r"\\?\Volume{".encode_utf16()));
            }
            if length > 32_768 {
                return Ok(false);
            }
            path.resize(length + 1, 0);
        }
        Ok(false)
    }

    fn supported_filesystem(handle: HANDLE) -> io::Result<bool> {
        let mut name = [0u16; 32];
        let success = unsafe {
            GetVolumeInformationByHandleW(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                name.as_mut_ptr(),
                name.len() as u32,
            )
        };
        if success == 0 {
            return unsupported_or_error(io::Error::last_os_error()).map(|_: Option<()>| false);
        }
        let end = name
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(name.len());
        Ok(reliable_filesystem(&name[..end]))
    }

    pub(super) fn reliable_filesystem(name: &[u16]) -> bool {
        // FAT and exFAT cannot reliably detect a same-size rewrite with restored mtime.
        name.iter().copied().eq("NTFS".encode_utf16())
            || name.iter().copied().eq("ReFS".encode_utf16())
    }

    pub(super) fn unsupported_or_error<T>(error: io::Error) -> io::Result<Option<T>> {
        match error.raw_os_error().map(|code| code as u32) {
            Some(
                ERROR_INVALID_FUNCTION
                | ERROR_NOT_SUPPORTED
                | ERROR_INVALID_PARAMETER
                | ERROR_CALL_NOT_IMPLEMENTED
                | ERROR_INVALID_LEVEL,
            ) => Ok(None),
            _ => Err(error),
        }
    }
}

#[cfg(test)]
#[path = "program_file_stamp_tests.rs"]
mod tests;
