use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LogFileIdentity {
    canonical_path: PathBuf,
    #[cfg(windows)]
    volume_serial_number: u32,
    #[cfg(windows)]
    file_index: u64,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

pub(crate) fn log_identity(path: &Path, file: &File) -> io::Result<LogFileIdentity> {
    let canonical_path = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };

        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: File owns a valid handle throughout the call; information is
        // writable and has the exact layout required by the Windows API.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // NTFS tunneling can preserve a deleted file's creation time for its
        // replacement. Volume + file index identifies the actual file object.
        Ok(LogFileIdentity {
            canonical_path,
            volume_serial_number: information.dwVolumeSerialNumber,
            file_index: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata()?;
        Ok(LogFileIdentity {
            canonical_path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Ok(LogFileIdentity { canonical_path })
    }
}
