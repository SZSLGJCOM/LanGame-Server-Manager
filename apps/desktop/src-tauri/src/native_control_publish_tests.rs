use super::*;
#[cfg(windows)]
use std::io::Read;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-control-publish-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn failed_exclusive_create_preserves_foreign_temp_and_abandoned_owned_temp_is_removed() {
    let root = Fixture::new();
    let temporary = root.0.join("request.pending");
    fs::write(&temporary, b"foreign").unwrap();
    assert!(PendingFile::create(&temporary, b"replacement").is_err());
    assert_eq!(fs::read(&temporary).unwrap(), b"foreign");
    fs::remove_file(&temporary).unwrap();
    let pending = PendingFile::create(&temporary, b"owned").unwrap();
    assert!(temporary.is_file());
    drop(pending);
    assert!(!temporary.exists());
}

#[test]
fn absent_destination_is_published_once_and_wrong_directory_is_preserved() {
    let root = Fixture::new();
    let temporary = root.0.join("request.pending");
    let destination = root.0.join("request.txt");
    let mut pending = PendingFile::create(&temporary, b"complete request").unwrap();
    pending.replace(&destination).unwrap();
    assert!(pending.replace(&destination).is_err());
    drop(pending);
    assert_eq!(fs::read(&destination).unwrap(), b"complete request");
    assert!(!temporary.exists());
    fs::remove_file(&destination).unwrap();
    fs::create_dir(&destination).unwrap();
    let mut pending = PendingFile::create(&temporary, b"must not replace directory").unwrap();
    assert!(pending.replace(&destination).is_err());
    drop(pending);
    assert!(destination.is_dir());
    assert!(!temporary.exists());
}

#[cfg(windows)]
#[test]
fn held_reader_keeps_old_bytes_while_fresh_open_sees_complete_new_request() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = Fixture::new();
    let destination = root.0.join("request.txt");
    fs::write(&destination, b"old request").unwrap();
    let mut old = OpenOptions::new()
        .read(true)
        .share_mode(7)
        .open(&destination)
        .unwrap();
    let temporary = root.0.join("request.pending");
    let mut pending = PendingFile::create(&temporary, b"new complete request").unwrap();
    pending.replace(&destination).unwrap();
    drop(pending);
    let mut bytes = Vec::new();
    old.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"old request");
    assert_eq!(fs::read(&destination).unwrap(), b"new complete request");
    // Keep the old object open across a further exchange as well. Zero-link
    // old objects are normal after POSIX replacement, not hard-link aliases.
    let mut pending = PendingFile::create(&temporary, b"third complete request").unwrap();
    pending.replace(&destination).unwrap();
    drop(pending);
    assert_eq!(fs::read(&destination).unwrap(), b"third complete request");
    assert!(!temporary.exists());
}

#[cfg(windows)]
#[test]
fn published_request_is_readable_before_writer_drop_but_rejects_write_and_delete_peers() {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let root = Fixture::new();
    let destination = root.0.join("request.txt");
    fs::write(&destination, b"old request").unwrap();
    let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
    let mut old = OpenOptions::new()
        .read(true)
        .share_mode(sharing)
        .open(&destination)
        .unwrap();
    let temporary = root.0.join("request.pending");
    let mut pending = PendingFile::create(&temporary, b"new complete request").unwrap();
    pending.replace(&destination).unwrap();

    // Keep PendingFile alive throughout: publication is already visible, but
    // its original CREATE_NEW writer handle has deliberately not been dropped.
    let mut fresh = OpenOptions::new()
        .read(true)
        .share_mode(sharing)
        .open(&destination)
        .unwrap();
    let mut bytes = Vec::new();
    fresh.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"new complete request");
    bytes.clear();
    old.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"old request");
    assert_eq!(
        OpenOptions::new()
            .write(true)
            .share_mode(sharing)
            .open(&destination)
            .unwrap_err()
            .raw_os_error(),
        Some(32)
    );
    assert_eq!(
        OpenOptions::new()
            .access_mode(DELETE)
            .share_mode(sharing)
            .open(&destination)
            .unwrap_err()
            .raw_os_error(),
        Some(32)
    );

    drop(fresh);
    drop(pending);
    assert_eq!(fs::read(&destination).unwrap(), b"new complete request");
    assert!(!temporary.exists());
}

#[cfg(windows)]
#[test]
fn readonly_destination_is_not_forced_and_failed_source_is_cleaned() {
    let root = Fixture::new();
    let destination = root.0.join("request.txt");
    fs::write(&destination, b"retained readonly").unwrap();
    let mut permissions = fs::metadata(&destination).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&destination, permissions.clone()).unwrap();
    let temporary = root.0.join("request.pending");
    let mut pending = PendingFile::create(&temporary, b"must fail").unwrap();
    assert!(pending.replace(&destination).is_err());
    drop(pending);
    assert_eq!(fs::read(&destination).unwrap(), b"retained readonly");
    assert!(!temporary.exists());
    permissions.set_readonly(false);
    fs::set_permissions(&destination, permissions).unwrap();
}

#[cfg(windows)]
#[test]
fn long_unicode_destination_names_retain_correct_aligned_utf16_layout() {
    use windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO;
    let root = Fixture::new();
    let mut parent = root.0.clone();
    for _ in 0..4 {
        parent.push("世界目录".repeat(12));
    }
    fs::create_dir_all(&parent).unwrap();
    for leaf in ["奇数名请求.json", "偶数名请求x.json"] {
        let destination = parent.join(leaf);
        let temporary = parent.join("owned.pending");
        let mut pending = PendingFile::create(&temporary, b"long-path request").unwrap();
        pending.replace(&destination).unwrap();
        drop(pending);
        assert_eq!(fs::read(destination).unwrap(), b"long-path request");
    }
    #[cfg(target_pointer_width = "64")]
    {
        assert_eq!(std::mem::offset_of!(FILE_RENAME_INFO, FileName), 20);
        assert_eq!(std::mem::size_of::<FILE_RENAME_INFO>(), 24);
    }
}
