//! Admission for future copies in registered, one-shot native smoke roots only.
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

const MAX_POLICY_BYTES: u64 = 64 * 1024;
const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 64;

#[derive(Clone)]
pub(super) struct SmokeCopyPreflight {
    target: PathBuf,
    policy: PathBuf,
}

impl SmokeCopyPreflight {
    // The caller constructs this only after the manifest accepts the managed root.
    pub(super) fn new(target: &Path, policy: &Path) -> Self {
        Self {
            target: target.to_path_buf(),
            policy: policy.to_path_buf(),
        }
    }

    pub(super) fn require_capacity(&self, sources: &[PathBuf]) -> io::Result<()> {
        self.require_with(sources, |path| {
            crate::commands::commands_mod_capacity::available_bytes(path).map_err(io::Error::other)
        })
    }

    fn require_with(
        &self,
        sources: &[PathBuf],
        mut available: impl FnMut(&Path) -> io::Result<u64>,
    ) -> io::Result<()> {
        let floor = read_floor(&self.policy)?;
        // Refuse low-space starts without walking a potentially large source tree.
        check_capacity(available(&self.target)?, floor, 0)?;
        let payload = source_bytes(sources, MAX_ENTRIES, MAX_DEPTH)?;
        // Inventory can take time: never authorize from the pre-inventory reading.
        check_capacity(available(&self.target)?, floor, payload)
    }
}

fn check_capacity(available: u64, floor: u64, payload: u64) -> io::Result<()> {
    let required = floor
        .checked_add(payload)
        .ok_or_else(|| io::Error::other("Native smoke copy capacity accounting overflow"))?;
    if available < required {
        return Err(io::Error::other(format!(
            "Native smoke copy requires {required} free bytes ({payload} payload plus policy floor {floor}); {available} available. Free space or choose another managed output volume."
        )));
    }
    Ok(())
}

fn plain(metadata: &fs::Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other(
                "Native smoke preflight refuses reparse points",
            ));
        }
    }
    if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
        return Err(io::Error::other(
            "Native smoke preflight requires plain entries",
        ));
    }
    Ok(())
}

fn plain_ancestors(path: &Path) -> io::Result<fs::Metadata> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(io::Error::other(
            "Native smoke source/policy path must be absolute without traversal",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    plain(&metadata)?;
    if let Some(parent) = path.parent() {
        for ancestor in parent.ancestors() {
            let metadata = fs::symlink_metadata(ancestor)?;
            plain(&metadata)?;
            if !metadata.is_dir() {
                return Err(io::Error::other("Native smoke ancestor is not a directory"));
            }
        }
    }
    Ok(metadata)
}

fn read_floor(path: &Path) -> io::Result<u64> {
    let value = read_policy(path)?;
    policy_floor(&value)
}

pub(super) fn read_policy(path: &Path) -> io::Result<serde_json::Value> {
    let metadata = plain_ancestors(path)?;
    if !metadata.is_file() || metadata.len() > MAX_POLICY_BYTES {
        return Err(io::Error::other(
            "Native smoke capacity policy is not a bounded plain file",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000); // OPEN_REPARSE_POINT, no writer during read.
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    plain(&metadata)?;
    if !metadata.is_file() || metadata.len() > MAX_POLICY_BYTES {
        return Err(io::Error::other(
            "Native smoke capacity policy changed or exceeds its bound",
        ));
    }
    let mut bytes = Vec::new();
    file.take(MAX_POLICY_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_POLICY_BYTES {
        return Err(io::Error::other(
            "Native smoke capacity policy exceeds its bound",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if value.as_object().is_none_or(|fields| {
        fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "schemaVersion" | "minimumFreeSpaceBytes" | "managedRoots"
            )
        })
    }) {
        return Err(io::Error::other(
            "Native smoke resource policy contains unknown fields",
        ));
    }
    if value.get("schemaVersion").and_then(|value| value.as_u64()) != Some(1) {
        return Err(io::Error::other(
            "Native smoke requires resource policy schema 1",
        ));
    }
    policy_floor(&value)?;
    Ok(value)
}

fn policy_floor(value: &serde_json::Value) -> io::Result<u64> {
    value
        .get("minimumFreeSpaceBytes")
        .and_then(|value| value.as_u64())
        .filter(|floor| *floor > 0)
        .ok_or_else(|| {
            io::Error::other(
                "Native smoke policy requires a positive integer minimumFreeSpaceBytes",
            )
        })
}

fn add_size(total: u64, length: u64) -> io::Result<u64> {
    total
        .checked_add(length)
        .ok_or_else(|| io::Error::other("Native smoke source byte count overflow"))
}

fn source_bytes(sources: &[PathBuf], max_entries: usize, max_depth: usize) -> io::Result<u64> {
    if sources.is_empty() || sources.len() > max_entries {
        return Err(io::Error::other(
            "Native smoke copy requires bounded source roots",
        ));
    }
    let mut queue = Vec::new();
    for source in sources {
        if !plain_ancestors(source)?.is_dir() {
            return Err(io::Error::other(
                "Native smoke source root is not a directory",
            ));
        }
        queue.push((source.to_path_buf(), 0usize));
    }
    let mut count = sources.len();
    let mut total = 0u64;
    while let Some((directory, depth)) = queue.pop() {
        // A fresh no-follow check rejects links introduced since the parent walk.
        let metadata = fs::symlink_metadata(&directory)?;
        plain(&metadata)?;
        if !metadata.is_dir() {
            return Err(io::Error::other("Native smoke source directory changed"));
        }
        for entry in fs::read_dir(&directory)? {
            count = count
                .checked_add(1)
                .ok_or_else(|| io::Error::other("Native smoke entry count overflow"))?;
            if count > max_entries {
                return Err(io::Error::other(
                    "Native smoke source inventory entry bound exceeded",
                ));
            }
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            plain(&metadata)?;
            if metadata.is_dir() {
                if depth >= max_depth {
                    return Err(io::Error::other(
                        "Native smoke source inventory depth bound exceeded",
                    ));
                }
                queue.push((entry.path(), depth + 1));
            } else {
                total = add_size(total, metadata.len())?;
            }
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
        guard: SmokeCopyPreflight,
        source: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::var_os("LANGAME_PREFLIGHT_TEST_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("fixture-data")
                .join(uuid::Uuid::new_v4().simple().to_string());
            let source = root.join("source");
            fs::create_dir_all(&source).unwrap();
            fs::write(source.join("asset.bin"), b"12345678").unwrap();
            let policy = root.join("policy.json");
            fs::write(
                &policy,
                br#"{"schemaVersion":1,"minimumFreeSpaceBytes":100}"#,
            )
            .unwrap();
            Self {
                guard: SmokeCopyPreflight {
                    target: root.clone(),
                    policy,
                },
                root,
                source,
            }
        }
        fn sources(&self) -> Vec<PathBuf> {
            vec![self.source.clone()]
        }
    }

    #[test]
    fn exact_headroom_succeeds_and_inventory_requires_a_fresh_second_query() {
        let fixture = Fixture::new();
        let mut calls = 0;
        fixture
            .guard
            .require_with(&fixture.sources(), |_| {
                calls += 1;
                Ok(108)
            })
            .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            source_bytes(&fixture.sources(), MAX_ENTRIES, MAX_DEPTH).unwrap(),
            8
        );
    }

    #[test]
    fn public_preflight_uses_existing_volume_query_and_rereads_policy_for_next_copy() {
        let fixture = Fixture::new();
        fixture.guard.require_capacity(&fixture.sources()).unwrap();
        fs::write(
            &fixture.guard.policy,
            br#"{"schemaVersion":1,"minimumFreeSpaceBytes":1000}"#,
        )
        .unwrap();
        let error = fixture
            .guard
            .require_with(&fixture.sources(), |_| Ok(108))
            .unwrap_err();
        assert!(error.to_string().contains("policy floor 1000"));
        assert_eq!(
            SmokeCopyPreflight::new(&fixture.root, &fixture.guard.policy).policy,
            fixture.guard.policy
        );
    }

    #[test]
    fn low_floor_rejects_before_source_inventory() {
        let fixture = Fixture::new();
        let mut calls = 0;
        let error = fixture
            .guard
            .require_with(&[fixture.root.join("missing")], |_| {
                calls += 1;
                Ok(99)
            })
            .unwrap_err();
        assert!(error.to_string().contains("policy floor 100"), "{error}");
        assert_eq!(calls, 1);
    }

    #[test]
    fn forecast_and_falling_space_refuse_the_next_copy_without_writing() {
        let fixture = Fixture::new();
        assert!(
            fixture
                .guard
                .require_with(&fixture.sources(), |_| Ok(107))
                .is_err()
        );
        let mut calls = 0;
        assert!(
            fixture
                .guard
                .require_with(&fixture.sources(), |_| {
                    calls += 1;
                    Ok(if calls == 1 { 1000 } else { 99 })
                })
                .is_err()
        );
        assert_eq!(calls, 2);
        assert_eq!(
            fs::read(fixture.source.join("asset.bin")).unwrap(),
            b"12345678"
        );
        assert!(!fixture.root.join("runtime").exists());
    }

    #[test]
    fn malformed_missing_oversize_and_invalid_policy_are_closed_before_disk_query() {
        let fixture = Fixture::new();
        for bytes in [
            b"{".as_slice(),
            br#"{"schemaVersion":2,"minimumFreeSpaceBytes":100}"#,
            br#"{"schemaVersion":1}"#,
            br#"{"schemaVersion":1,"minimumFreeSpaceBytes":0}"#,
            br#"{"schemaVersion":1,"minimumFreeSpaceBytes":-1}"#,
            br#"{"schemaVersion":1,"minimumFreeSpaceBytes":1.5}"#,
            br#"{"schemaVersion":1,"minimumFreeSpaceBytes":18446744073709551616}"#,
        ] {
            fs::write(&fixture.guard.policy, bytes).unwrap();
            assert!(
                fixture
                    .guard
                    .require_with(&fixture.sources(), |_| panic!("bad policy queried disk"))
                    .is_err()
            );
        }
        fs::write(
            &fixture.guard.policy,
            vec![b' '; MAX_POLICY_BYTES as usize + 1],
        )
        .unwrap();
        assert!(
            fixture
                .guard
                .require_with(&fixture.sources(), |_| panic!("oversize queried disk"))
                .is_err()
        );
        fs::remove_file(&fixture.guard.policy).unwrap();
        assert!(
            fixture
                .guard
                .require_with(&fixture.sources(), |_| panic!(
                    "missing policy queried disk"
                ))
                .is_err()
        );
    }

    #[test]
    fn unknown_disk_and_source_fail_closed_and_arithmetic_never_wraps() {
        let fixture = Fixture::new();
        assert!(
            fixture
                .guard
                .require_with(&fixture.sources(), |_| Err(io::Error::other("unavailable")))
                .is_err()
        );
        assert!(
            fixture
                .guard
                .require_with(&[fixture.root.join("missing")], |_| Ok(1000))
                .is_err()
        );
        assert!(add_size(u64::MAX, 1).is_err());
        assert!(check_capacity(u64::MAX, 1, u64::MAX).is_err());
        assert!(check_capacity(u64::MAX, 1, u64::MAX - 1).is_ok());
    }

    #[test]
    fn source_inventory_bounds_and_non_directory_are_closed() {
        let fixture = Fixture::new();
        assert!(source_bytes(&fixture.sources(), 1, 64).is_err());
        fs::create_dir(fixture.source.join("child")).unwrap();
        assert!(source_bytes(&fixture.sources(), 100, 0).is_err());
        assert!(source_bytes(&[fixture.source.join("asset.bin")], 100, 64).is_err());
        assert!(source_bytes(&[], 100, 64).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn real_filesystem_reparse_source_and_policy_ancestors_are_rejected() {
        let fixture = Fixture::new();
        let link = fixture.root.join("source-link");
        create_junction(&link, &fixture.source);
        let metadata = fs::symlink_metadata(&link).unwrap();
        assert!(plain(&metadata).is_err());
        assert!(source_bytes(std::slice::from_ref(&link), 100, 64).is_err());
        assert!(read_floor(&link.join("policy.json")).is_err());
        let parent = link.parent().unwrap();
        assert!(source_bytes(&[parent.to_path_buf()], 100, 64).is_err());
    }

    #[cfg(windows)]
    fn create_junction(link: &Path, target: &Path) {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateFileW(
                name: *const u16,
                access: u32,
                share: u32,
                security: *const c_void,
                creation: u32,
                flags: u32,
                template: *mut c_void,
            ) -> *mut c_void;
            fn DeviceIoControl(
                handle: *mut c_void,
                code: u32,
                input: *const c_void,
                input_len: u32,
                output: *mut c_void,
                output_len: u32,
                returned: *mut u32,
                overlapped: *mut c_void,
            ) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        fs::create_dir(link).unwrap();
        let substitute: Vec<u16> = format!(r"\??\{}", target.display())
            .encode_utf16()
            .collect();
        let print: Vec<u16> = target.as_os_str().encode_wide().collect();
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&0xA0000003u32.to_le_bytes()); // IO_REPARSE_TAG_MOUNT_POINT.
        buffer.extend_from_slice(
            &((8 + (substitute.len() + print.len() + 2) * 2) as u16).to_le_bytes(),
        );
        buffer.extend_from_slice(&0u16.to_le_bytes());
        for number in [
            0,
            substitute.len() * 2,
            (substitute.len() + 1) * 2,
            print.len() * 2,
        ] {
            buffer.extend_from_slice(&(number as u16).to_le_bytes());
        }
        for character in substitute.into_iter().chain([0]).chain(print).chain([0]) {
            buffer.extend_from_slice(&character.to_le_bytes());
        }
        let name: Vec<u16> = link.as_os_str().encode_wide().chain([0]).collect();
        // Only this newly created test directory is converted to a junction.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                0x40000000,
                7,
                std::ptr::null(),
                3,
                0x02200000,
                std::ptr::null_mut(),
            )
        };
        assert_ne!(handle as isize, -1, "{}", io::Error::last_os_error());
        let mut returned = 0;
        let result = unsafe {
            DeviceIoControl(
                handle,
                0x000900A4,
                buffer.as_ptr().cast(),
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        let error = io::Error::last_os_error();
        unsafe {
            CloseHandle(handle);
        }
        assert_ne!(result, 0, "{error}");
    }
}
