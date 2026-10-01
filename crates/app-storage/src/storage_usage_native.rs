use std::fs::{File, Metadata};
use std::io;
use std::path::Path;

pub(super) struct FileUsage {
    pub logical: u64,
    pub allocated: Option<u64>,
    pub identity: Option<[u64; 3]>,
}

pub(super) fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 || metadata.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(windows)]
fn open_metadata(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::*;
    let file = std::fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(
            FILE_SHARE_READ | FILE_SHARE_WRITE | if directory { 0 } else { FILE_SHARE_DELETE },
        )
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .open(path)?;
    let metadata = file.metadata()?;
    if is_link(&metadata) || metadata.is_dir() != directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "filesystem entry changed during inspection",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
pub(super) fn pin_directory(path: &Path) -> io::Result<Option<File>> {
    open_metadata(path, true).map(Some)
}

pub(super) fn pin_ancestors(path: &Path) -> io::Result<Vec<File>> {
    let mut ancestors = path
        .parent()
        .into_iter()
        .flat_map(Path::ancestors)
        .filter(|ancestor| ancestor.has_root())
        .collect::<Vec<_>>();
    ancestors.reverse();
    let mut pins = Vec::with_capacity(ancestors.len());
    for ancestor in ancestors {
        if let Some(pin) = pin_directory(ancestor)? {
            pins.push(pin);
        }
    }
    Ok(pins)
}

#[cfg(not(windows))]
pub(super) fn pin_directory(path: &Path) -> io::Result<Option<File>> {
    let metadata = std::fs::symlink_metadata(path)?;
    if is_link(&metadata) || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory changed during inspection",
        ));
    }
    Ok(None)
}

#[cfg(windows)]
pub(super) fn file_usage(path: &Path, _: &Metadata) -> io::Result<FileUsage> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;
    let file = open_metadata(path, false)?;
    let handle = file.as_raw_handle();
    let mut standard: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
    let mut identity: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileStandardInfo,
            (&mut standard as *mut FILE_STANDARD_INFO).cast(),
            std::mem::size_of_val(&standard) as u32,
        )
    } == 0
        || unsafe { GetFileInformationByHandle(handle, &mut identity) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let allocated = u64::try_from(standard.AllocationSize)
        .map_err(|_| io::Error::other("filesystem returned a negative allocation size"))?;
    Ok(FileUsage {
        logical: file.metadata()?.len(),
        allocated: Some(allocated),
        identity: Some([
            u64::from(identity.dwVolumeSerialNumber),
            u64::from(identity.nFileIndexHigh),
            u64::from(identity.nFileIndexLow),
        ]),
    })
}

#[cfg(unix)]
pub(super) fn file_usage(_: &Path, metadata: &Metadata) -> io::Result<FileUsage> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileUsage {
        logical: metadata.len(),
        allocated: Some(metadata.blocks().saturating_mul(512)),
        identity: Some([metadata.dev(), metadata.ino(), 0]),
    })
}

#[cfg(not(any(windows, unix)))]
pub(super) fn file_usage(_: &Path, metadata: &Metadata) -> io::Result<FileUsage> {
    Ok(FileUsage {
        logical: metadata.len(),
        allocated: None,
        identity: None,
    })
}
