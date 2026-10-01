use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub(super) struct NativePackage {
    pub root: PathBuf,
    pub install_root: PathBuf,
    pub copied_bytes: u64,
    pub cleanup_allowed: Arc<AtomicBool>,
}

impl Drop for NativePackage {
    fn drop(&mut self) {
        if !self.cleanup_allowed.load(Ordering::SeqCst) {
            eprintln!(
                "native lifecycle cleanup retained its package because process shutdown is unconfirmed"
            );
            return;
        }
        let started = Instant::now();
        if let Err(error) = remove_package_with(
            &self.root,
            |root| {
                #[cfg(windows)]
                remove_profile_cache_junction(root)?;
                fs::remove_dir_all(root)
            },
            || std::thread::sleep(Duration::from_millis(50)),
        ) {
            eprintln!(
                "native lifecycle cleanup failed: disposable package remains; root={} attempts=5 elapsed_ms={} error_kind={:?} raw_os_error={:?} error={error}",
                self.root.display(),
                started.elapsed().as_millis(),
                error.kind(),
                error.raw_os_error(),
            );
        }
    }
}

#[cfg(windows)]
fn remove_profile_cache_junction(root: &Path) -> std::io::Result<()> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::*;

    let refused = || {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native profile cache junction did not match the owned fixture layout",
        )
    };
    let pin = |path: &Path| -> std::io::Result<File> {
        let file = File::options()
            .access_mode(0)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(refused());
        }
        Ok(file)
    };
    if !root.is_absolute()
        || root.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(refused());
    }
    let cache = root.join("profile/AppData/Local/Microsoft/Windows/INetCache");
    let mut ancestors: Vec<_> = cache.ancestors().collect();
    ancestors.reverse();
    // Keep every ancestor pinned against replacement until the non-recursive
    // unlink completes. This runs only after NativePackage's shutdown guard.
    let mut pins = Vec::new();
    for ancestor in ancestors {
        match pin(ancestor) {
            Ok(file) => pins.push(file),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        }
    }
    let link = cache.join("Content.IE5");
    let metadata = match fs::symlink_metadata(&link) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return Ok(());
    }
    let target = cache.join("IE");
    // A missing or redirected target is a validation failure, not an absent
    // package: remove_package_with must retain that fixture for inspection.
    pins.push(pin(&target).map_err(|_| refused())?);
    let local_path = |path: &Path| {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        if let Some(Component::Prefix(prefix)) = parts.next()
            && let Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) = prefix.kind()
        {
            let mut local = PathBuf::from(format!("{}:", char::from(drive).to_ascii_uppercase()));
            local.extend(parts);
            return local;
        }
        path.to_path_buf()
    };
    // The junction may retain the owned root's short name or its canonical
    // long name. Normalize only the local drive namespace; never resolve an
    // untrusted link target or accept an extra junction on the way to pinned IE.
    let link_target = local_path(&fs::read_link(&link)?);
    let canonical_target = local_path(&fs::canonicalize(&target)?);
    if link_target != local_path(&target) && link_target != canonical_target {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "native profile cache junction target differs from the pinned IE directory",
        ));
    }
    let file = File::options()
        .access_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(&link)?;
    let mut tag = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    // SAFETY: file owns a live handle and tag is the correctly sized output
    // structure. Query-only access also works with WinINet's deny-list ACL.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&mut tag as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of_val(&tag) as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    const MOUNT_POINT_TAG: u32 = 0xA000_0003;
    if tag.ReparseTag != MOUNT_POINT_TAG || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(refused());
    }
    drop(file);
    // WinINet creates this compatibility junction with Deny ListDirectory,
    // which makes recursive traversal fail. RemoveDirectory unlinks only the
    // junction; it neither follows nor changes the target or its permissions.
    fs::remove_dir(link)
}

#[cfg(windows)]
#[path = "commands_native_package_cleanup_tests.rs"]
mod cleanup_tests;

fn remove_package_with(
    root: &Path,
    mut remove: impl FnMut(&Path) -> std::io::Result<()>,
    mut pause: impl FnMut(),
) -> std::io::Result<()> {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match remove(root) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                pause();
                if attempts == 5 {
                    return Err(error);
                }
            }
        }
    }
}

pub(super) fn copy_package(
    source: &Path,
    module_id: &str,
    excluded: Option<&Path>,
    max_bytes: u64,
    timeout: Duration,
) -> Result<NativePackage, String> {
    if is_link(source)? || !source.is_dir() {
        return Err("native package source must be a plain directory".into());
    }
    let source = source
        .canonicalize()
        .map_err(|_| "cannot resolve native package source")?;
    let root = create_native_root()?;
    let mut package = NativePackage {
        // Native DLL loaders may reject long absolute paths even when Rust's
        // filesystem API accepts them. Keep this owned copy below the wrapper
        // scratch root and publish its exact root through the install record.
        install_root: root.join(module_id),
        root,
        copied_bytes: 0,
        cleanup_allowed: Arc::new(AtomicBool::new(true)),
    };
    let canonical_root = package
        .root
        .canonicalize()
        .map_err(|_| "cannot resolve disposable directory")?;
    if canonical_root.starts_with(&source) || source.starts_with(&canonical_root) {
        return Err("native package source and disposable copy must not overlap".into());
    }
    let excluded = excluded.map(|path| path.to_path_buf());
    let deadline = Instant::now() + timeout;
    let mut pending = vec![(source.clone(), package.install_root.clone(), 0_u16)];
    let mut entries = 0_usize;
    let mut buffer = vec![0_u8; 1024 * 1024];
    while let Some((from, to, depth)) = pending.pop() {
        if depth > 64 || Instant::now() >= deadline {
            return Err("native package copy exceeded depth or time limit".into());
        }
        fs::create_dir_all(&to).map_err(|_| "cannot create disposable package subdirectory")?;
        for entry in fs::read_dir(&from).map_err(|_| "cannot enumerate native package source")? {
            let entry = entry.map_err(|_| "cannot inspect native package entry")?;
            let path = entry.path();
            let relative = path
                .strip_prefix(&source)
                .map_err(|_| "package entry escaped source")?;
            if excluded
                .as_ref()
                .is_some_and(|excluded| is_excluded(relative, excluded))
            {
                continue;
            }
            entries += 1;
            if entries > 1_000_000 || Instant::now() >= deadline {
                return Err("native package copy exceeded entry or time limit".into());
            }
            if is_link(&path)? {
                return Err(
                    "native package contains a link or reparse point; isolated copy refused".into(),
                );
            }
            let target = to.join(entry.file_name());
            let metadata = entry
                .metadata()
                .map_err(|_| "cannot read native package metadata")?;
            if metadata.is_dir() {
                pending.push((path, target, depth + 1));
            } else if metadata.is_file() {
                if package.copied_bytes.saturating_add(metadata.len()) > max_bytes {
                    return Err("native package exceeds configured copy byte limit".into());
                }
                // New files own their contents and permissions. No hard links,
                // junctions, or source ACL/readonly mutations are used.
                let mut reader =
                    File::open(&path).map_err(|_| "cannot read native package file")?;
                let mut writer =
                    File::create(&target).map_err(|_| "cannot create package copy file")?;
                loop {
                    if Instant::now() >= deadline {
                        return Err("native package copy timed out".into());
                    }
                    let count = reader
                        .read(&mut buffer)
                        .map_err(|_| "native package file read failed")?;
                    if count == 0 {
                        break;
                    }
                    package.copied_bytes = package.copied_bytes.saturating_add(count as u64);
                    if package.copied_bytes > max_bytes {
                        return Err("native package grew beyond copy byte limit".into());
                    }
                    writer
                        .write_all(&buffer[..count])
                        .map_err(|_| "native package copy write failed")?;
                }
            } else {
                return Err("native package contains an unsupported filesystem entry".into());
            }
        }
    }
    Ok(package)
}

fn create_native_root() -> Result<PathBuf, String> {
    let start = (uuid::Uuid::new_v4().as_u128() % NATIVE_ROOT_NAMES.len() as u128) as usize;
    create_native_root_at(&std::env::temp_dir(), start)
}

// Lowercase names give 36 distinct candidates on case-insensitive Windows.
// Keep the wrapper-owned parent unchanged while leaving room for deep DLLs.
const NATIVE_ROOT_NAMES: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn create_native_root_at(parent: &Path, start: usize) -> Result<PathBuf, String> {
    for offset in 0..NATIVE_ROOT_NAMES.len() {
        let index = (start % NATIVE_ROOT_NAMES.len() + offset) % NATIVE_ROOT_NAMES.len();
        let root = parent.join(char::from(NATIVE_ROOT_NAMES[index]).to_string());
        // Only a successful exclusive creation grants cleanup ownership. A
        // collision never inspects, reuses, renames, or removes that entry.
        match fs::create_dir(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("cannot create disposable native package directory".into()),
        }
    }
    Err("all 36 short disposable native package directories are occupied".into())
}

fn is_link(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "cannot inspect package path")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        Ok(metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(metadata.file_type().is_symlink())
    }
}

fn is_excluded(relative: &Path, excluded: &Path) -> bool {
    #[cfg(windows)]
    {
        let mut actual = relative.components();
        excluded.components().all(|expected| {
            actual.next().is_some_and(|component| {
                component
                    .as_os_str()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
            })
        })
    }
    #[cfg(not(windows))]
    {
        relative.starts_with(excluded)
    }
}

#[test]
fn native_package_cleanup_preserves_the_last_error_after_five_attempts() {
    let root = Path::new("owned-fixture");
    let mut attempts = 0;
    let mut pauses = 0;
    let expected_kind = std::io::Error::from_raw_os_error(5).kind();
    let error = remove_package_with(
        root,
        |path| {
            assert_eq!(path, root);
            attempts += 1;
            Err(std::io::Error::from_raw_os_error(if attempts == 5 {
                5
            } else {
                32
            }))
        },
        || pauses += 1,
    )
    .unwrap_err();
    assert_eq!(attempts, 5);
    assert_eq!(pauses, 5);
    assert_eq!(error.kind(), expected_kind);
    assert_eq!(error.raw_os_error(), Some(5));
}

#[test]
fn native_package_cleanup_preserves_non_os_failure_details() {
    let error = remove_package_with(
        Path::new("owned-fixture"),
        |_| Err(std::io::Error::other("fixture cleanup diagnostic")),
        || {},
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Other);
    assert_eq!(error.raw_os_error(), None);
    assert_eq!(error.to_string(), "fixture cleanup diagnostic");
}

#[test]
fn native_package_cleanup_finishes_when_removal_succeeds_or_root_is_absent() {
    for absent in [false, true] {
        let mut attempts = 0;
        let mut pauses = 0;
        remove_package_with(
            Path::new("owned-fixture"),
            |_| {
                attempts += 1;
                if attempts == 1 {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "fixture temporary failure",
                    ))
                } else if absent {
                    Err(std::io::Error::from(std::io::ErrorKind::NotFound))
                } else {
                    Ok(())
                }
            },
            || pauses += 1,
        )
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(pauses, 1);
    }
}

#[test]
fn native_package_copy_is_independent_and_excludes_all_worlds() {
    let source =
        std::env::temp_dir().join(format!("lg-copy-source-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(source.join("Saved/Worlds/old-world")).unwrap();
    fs::write(source.join("server.fixture"), b"original").unwrap();
    fs::write(
        source.join("Saved/Worlds/old-world/player.fixture"),
        b"user world",
    )
    .unwrap();
    let package = copy_package(
        &source,
        "fixture",
        Some(Path::new("Saved/Worlds")),
        1024,
        Duration::from_secs(5),
    )
    .unwrap();
    let copied_root = package.root.clone();
    assert!(!package.install_root.join("Saved/Worlds").exists());
    fs::write(package.install_root.join("server.fixture"), b"changed copy").unwrap();
    assert_eq!(
        fs::read(source.join("server.fixture")).unwrap(),
        b"original"
    );
    assert_eq!(
        fs::read(source.join("Saved/Worlds/old-world/player.fixture")).unwrap(),
        b"user world"
    );
    assert!(copy_package(&source, "fixture", None, 1, Duration::from_secs(5)).is_err());
    assert!(copy_package(&source, "fixture", None, 1024, Duration::ZERO).is_err());
    drop(package);
    assert!(!copied_root.exists());
    fs::remove_dir_all(source).unwrap();
}

#[cfg(windows)]
#[test]
fn native_package_world_exclusion_uses_windows_case_insensitive_paths() {
    assert!(is_excluded(
        Path::new("ASTRO/SAVED/savegames/world"),
        Path::new("Astro/Saved/SaveGames")
    ));
    assert!(!is_excluded(
        Path::new("Astro/Saved/Config"),
        Path::new("Astro/Saved/SaveGames")
    ));
}

#[test]
fn native_package_short_root_skips_existing_entries_without_reusing_them() {
    let parent = std::env::temp_dir().join(format!("lg-root-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&parent).unwrap();
    fs::create_dir(parent.join("0")).unwrap();
    fs::write(parent.join("0/owner.fixture"), b"existing directory").unwrap();
    fs::write(parent.join("1"), b"existing file").unwrap();

    let root = create_native_root_at(&parent, 0).unwrap();
    assert_eq!(root, parent.join("2"));
    assert!(root.is_dir());
    assert_eq!(root.file_name().unwrap().to_str().unwrap().len(), 1);
    assert_eq!(
        fs::read(parent.join("0/owner.fixture")).unwrap(),
        b"existing directory"
    );
    assert_eq!(fs::read(parent.join("1")).unwrap(), b"existing file");
    fs::remove_dir(&root).unwrap();
    assert!(parent.join("0/owner.fixture").is_file());
    assert!(parent.join("1").is_file());
    fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn native_package_short_root_exhaustion_is_bounded_and_preserves_every_owner() {
    let parent = std::env::temp_dir().join(format!("lg-root-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&parent).unwrap();
    for name in NATIVE_ROOT_NAMES {
        let root = parent.join(char::from(*name).to_string());
        fs::create_dir(&root).unwrap();
        fs::write(root.join("owner.fixture"), [*name]).unwrap();
    }
    let error = create_native_root_at(&parent, NATIVE_ROOT_NAMES.len() - 1).unwrap_err();
    assert!(error.contains("all 36"));
    assert_eq!(
        fs::read_dir(&parent).unwrap().count(),
        NATIVE_ROOT_NAMES.len()
    );
    for name in NATIVE_ROOT_NAMES {
        let owner = parent
            .join(char::from(*name).to_string())
            .join("owner.fixture");
        assert_eq!(fs::read(owner).unwrap(), [*name]);
    }
    fs::remove_dir_all(&parent).unwrap();
}
