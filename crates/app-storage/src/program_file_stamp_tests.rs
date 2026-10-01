use super::*;
#[cfg(windows)]
use std::fs::FileTimes;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("lgstamp-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, b"original payload").unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.0.starts_with(std::env::temp_dir()));
        fs::remove_dir_all(&self.0).expect("remove only this file-stamp fixture");
    }
}

#[cfg(windows)]
#[test]
fn file_stamp_is_stable_across_reopening_and_serialization() {
    let fixture = Fixture::new();
    let path = fixture.file("payload.bin");
    let stamp = FileStamp::read(&File::open(&path).unwrap()).unwrap();
    assert!(
        stamp.is_some(),
        "this fixture requires a reliable local filesystem"
    );
    assert_eq!(stamp, FileStamp::read(&File::open(&path).unwrap()).unwrap());
    let encoded = serde_json::to_vec(&stamp).unwrap();
    assert_eq!(
        stamp,
        serde_json::from_slice::<Option<FileStamp>>(&encoded).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn file_stamp_distinguishes_a_copy_with_the_same_size_and_mtime() {
    let fixture = Fixture::new();
    let source = fixture.file("source.bin");
    let modified = fs::metadata(&source).unwrap().modified().unwrap();
    let copied = fixture.0.join("copy.bin");
    fs::copy(&source, &copied).unwrap();
    OpenOptions::new()
        .write(true)
        .open(&copied)
        .unwrap()
        .set_times(FileTimes::new().set_modified(modified))
        .unwrap();
    let original = FileStamp::read(&File::open(source).unwrap())
        .unwrap()
        .unwrap();
    let replacement = FileStamp::read(&File::open(copied).unwrap())
        .unwrap()
        .unwrap();
    assert_ne!(
        original, replacement,
        "content metadata must include file identity"
    );
}

#[cfg(windows)]
#[test]
fn file_stamp_detects_same_size_rewrite_with_restored_mtime() {
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;

    let fixture = Fixture::new();
    let path = fixture.file("payload.bin");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    {
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        // Establish a known old ChangeTime so the test never depends on clock-tick timing.
        let old = FILE_BASIC_INFO {
            ChangeTime: 132_000_000_000_000_000,
            ..Default::default()
        };
        let changed = unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileBasicInfo,
                (&old as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of_val(&old) as u32,
            )
        };
        assert_ne!(changed, 0, "{}", io::Error::last_os_error());
    }
    let original = FileStamp::read(&File::open(&path).unwrap())
        .unwrap()
        .unwrap();
    {
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all(b"modified payload").unwrap();
        file.sync_all().unwrap();
        file.set_times(FileTimes::new().set_modified(modified))
            .unwrap();
    }
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::metadata(&path).unwrap().len(), 16);
    let rewritten = FileStamp::read(&File::open(path).unwrap())
        .unwrap()
        .unwrap();
    let (
        FileStamp::Windows {
            file_id: before_id,
            modified: before_mtime,
            changed: before_ctime,
            ..
        },
        FileStamp::Windows {
            file_id: after_id,
            modified: after_mtime,
            changed: after_ctime,
            ..
        },
    ) = (&original, &rewritten);
    assert_eq!(before_id, after_id);
    assert_eq!(before_mtime, after_mtime);
    assert_ne!(before_ctime, after_ctime);
    assert_ne!(original, rewritten);
}

#[test]
fn file_stamp_rejects_directory_handles() {
    let fixture = Fixture::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    let directory = options.open(&fixture.0).unwrap();
    assert_eq!(
        FileStamp::read(&directory).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[cfg(windows)]
#[test]
fn file_stamp_falls_back_for_coarse_filesystems_and_preserves_io_errors() {
    use windows_sys::Win32::Foundation::*;
    for name in ["FAT", "FAT32", "exFAT", "", "unknown"] {
        assert!(!windows::reliable_filesystem(
            &name.encode_utf16().collect::<Vec<_>>()
        ));
    }
    for name in ["NTFS", "ReFS"] {
        assert!(windows::reliable_filesystem(
            &name.encode_utf16().collect::<Vec<_>>()
        ));
    }
    for code in [
        ERROR_NOT_SUPPORTED,
        ERROR_INVALID_FUNCTION,
        ERROR_INVALID_PARAMETER,
    ] {
        assert!(
            windows::unsupported_or_error::<()>(io::Error::from_raw_os_error(code as i32))
                .unwrap()
                .is_none()
        );
    }
    for code in [
        ERROR_ACCESS_DENIED,
        ERROR_CRC,
        ERROR_READ_FAULT,
        ERROR_INVALID_HANDLE,
    ] {
        assert_eq!(
            windows::unsupported_or_error::<()>(io::Error::from_raw_os_error(code as i32))
                .unwrap_err()
                .raw_os_error(),
            Some(code as i32)
        );
    }
}

#[cfg(unix)]
#[test]
fn file_stamp_rejects_special_files() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let (socket, _peer) = UnixStream::pair().unwrap();
    let file = File::from(OwnedFd::from(socket));
    assert_eq!(
        FileStamp::read(&file).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[cfg(not(windows))]
#[test]
fn file_stamp_requires_content_verification_without_a_filesystem_capability_check() {
    let fixture = Fixture::new();
    let path = fixture.file("payload.bin");
    assert!(
        FileStamp::read(&File::open(path).unwrap())
            .unwrap()
            .is_none()
    );
}
