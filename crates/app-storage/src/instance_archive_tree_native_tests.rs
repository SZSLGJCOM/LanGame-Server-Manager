use super::TreeNode;
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    root: PathBuf,
    temporary_root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temporary_root = fs::canonicalize(std::env::temp_dir()).unwrap();
        let root = temporary_root.join(format!("archive-tree-native-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        Self {
            root,
            temporary_root,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.root.exists() {
            let resolved = fs::canonicalize(&self.root).unwrap();
            assert!(resolved.starts_with(&self.temporary_root) && resolved != self.temporary_root);
            fs::remove_dir_all(&resolved).expect("remove this test's owned directory");
        }
    }
}

#[test]
fn tree_child_rejects_non_component_and_ambiguous_windows_names() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("ordinary.dat"), b"owned").unwrap();
    let root = TreeNode::open_root(&fixture.root, false).unwrap();
    for name in [
        "",
        ".",
        "..",
        "../outside",
        "nested/file",
        "nested\\file",
        "/rooted",
        "C:\\outside",
        "ordinary.dat:stream",
        "ordinary.dat.",
        "ordinary.dat ",
        "a\0b",
        "a?b",
        "a*b",
        "a|b",
        "a<b",
        "a>b",
    ] {
        assert!(
            root.child(OsStr::new(name), false, false).is_err(),
            "accepted {name:?}"
        );
    }
    assert!(root.child(OsStr::new("ordinary.dat"), false, false).is_ok());
    assert!(root.child(OsStr::new("ordinary.dat"), true, false).is_err());
    assert_eq!(
        fs::read(fixture.root.join("ordinary.dat")).unwrap(),
        b"owned"
    );
}

#[test]
fn tree_children_preserve_unicode_across_native_batches_and_enforce_limit() {
    let fixture = Fixture::new();
    let mut expected = BTreeSet::new();
    // This exceeds the native 64 KiB enumeration buffer by several batches.
    for index in 0..1_200 {
        let name = format!("entry-{index:04}-饥荒-🚀-long-name-file.dat");
        fs::write(fixture.root.join(&name), b"owned").unwrap();
        expected.insert((OsString::from(name), false));
    }
    fs::create_dir(fixture.root.join("子目录")).unwrap();
    expected.insert((OsString::from("子目录"), true));
    let root = TreeNode::open_root(&fixture.root, false).unwrap();
    assert!(root.children(expected.len() - 1).is_err());
    let actual = root.children(expected.len()).unwrap();
    assert_eq!(
        actual.len(),
        expected.len(),
        "duplicate or missing native entry"
    );
    assert_eq!(actual.into_iter().collect::<BTreeSet<_>>(), expected);
    assert_eq!(
        root.children(expected.len()).unwrap().len(),
        expected.len(),
        "enumeration must restart"
    );
    let empty = root.child(OsStr::new("子目录"), true, false).unwrap();
    assert!(empty.children(0).unwrap().is_empty());
}

#[test]
fn tree_child_keeps_every_ancestor_pinned_until_released() {
    let fixture = Fixture::new();
    let branch = fixture.root.join("branch");
    let renamed = fixture.root.join("renamed");
    fs::create_dir(&branch).unwrap();
    fs::write(branch.join("leaf.dat"), b"owned").unwrap();
    let root = TreeNode::open_root(&fixture.root, false).unwrap();
    let directory = root.child(OsStr::new("branch"), true, false).unwrap();
    let child = directory
        .child(OsStr::new("leaf.dat"), false, false)
        .unwrap();
    let blocked = fs::rename(&branch, &renamed);
    drop(child);
    drop(directory);
    // If the protection failed, restore the fixture before asserting.
    if blocked.is_ok() {
        fs::rename(&renamed, &branch).unwrap();
    }
    assert!(
        blocked.is_err(),
        "parent moved while a descendant guard was live"
    );
    fs::rename(&branch, &renamed).unwrap();
    fs::rename(&renamed, &branch).unwrap();
    assert_eq!(fs::read(branch.join("leaf.dat")).unwrap(), b"owned");
}

fn make_junction_in_place(directory: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;

    let target = fs::canonicalize(target)?;
    let target = target.to_string_lossy();
    let substitute = format!(r"\??\{}", target.strip_prefix(r"\\?\").unwrap_or(&target));
    let substitute = OsStr::new(&substitute).encode_wide().collect::<Vec<_>>();
    let name_bytes = u16::try_from(substitute.len() * 2).unwrap();
    // REPARSE_DATA_BUFFER header, mount-point offsets, and the UTF-16 substitute
    // name. The optional print name is empty; both names have a trailing NUL.
    let mut payload = Vec::new();
    payload.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    payload.extend((8 + name_bytes + 4).to_le_bytes());
    payload.extend(0_u16.to_le_bytes());
    for value in [0_u16, name_bytes, name_bytes + 2, 0] {
        payload.extend(value.to_le_bytes());
    }
    for word in substitute {
        payload.extend(word.to_le_bytes());
    }
    payload.extend([0_u8; 4]);
    let mut aligned = vec![0_u64; payload.len().div_ceil(8)];
    for (index, bytes) in payload.chunks(8).enumerate() {
        let mut word = [0_u8; 8];
        word[..bytes.len()].copy_from_slice(bytes);
        aligned[index] = u64::from_le_bytes(word);
    }
    let handle = fs::OpenOptions::new()
        .read(true)
        .access_mode(FILE_WRITE_DATA)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(directory)?;
    let mut returned = 0_u32;
    // SAFETY: The file is this fixture's empty directory. The aligned buffer
    // contains the complete mount-point record and lives through the sync call.
    if unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            aligned.as_ptr().cast(),
            payload.len() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[test]
fn tree_operations_refuse_a_directory_changed_to_a_junction_in_place() {
    let fixture = Fixture::new();
    let directory = fixture.root.join("owned-empty");
    let outside = fixture.root.join("outside-饥荒");
    fs::create_dir(&directory).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.dat"), b"external data survives").unwrap();
    let root = TreeNode::open_root(&directory, false).unwrap();
    let before = root.identity().unwrap();
    let changed = make_junction_in_place(&directory, &outside);
    let enumerated = root.children(10);
    let opened = root.child(OsStr::new("sentinel.dat"), false, true);
    let after = root.identity().unwrap();
    let child_refused = opened.is_err();
    drop(opened);
    drop(root);
    // Remove only the fixture-owned junction before any assertion/recursive
    // fixture teardown; removing the link itself never follows its target.
    if changed.is_ok() {
        fs::remove_dir(&directory).unwrap();
    }
    changed.expect("in-place reparse mutation must exercise the guard race");
    assert_eq!(
        before, after,
        "mutation must keep the same directory identity"
    );
    assert!(
        enumerated.is_err(),
        "enumeration followed an in-place junction"
    );
    assert!(child_refused, "child open followed an in-place junction");
    assert_eq!(
        fs::read(outside.join("sentinel.dat")).unwrap(),
        b"external data survives"
    );
}
