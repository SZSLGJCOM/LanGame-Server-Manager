use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use uuid::Uuid;

static CONFIG_FILE_ACCESS: RwLock<()> = RwLock::new(());

#[cfg(test)]
static FAIL_NEXT_ATOMIC_WRITES: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashSet<PathBuf>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashSet::new()));

#[cfg(test)]
pub(crate) fn fail_next_atomic_write_for_test(path: &Path) {
    FAIL_NEXT_ATOMIC_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(path.to_path_buf());
}

pub(crate) fn read_file_to_string(path: &Path) -> io::Result<String> {
    let _read_guard = CONFIG_FILE_ACCESS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    fs::read_to_string(path).map(strip_utf8_bom)
}

fn strip_utf8_bom(mut content: String) -> String {
    if content.starts_with('\u{feff}') {
        content.drain(..'\u{feff}'.len_utf8());
    }
    content
}

pub(crate) fn read_optional_file_to_string(path: &Path) -> io::Result<Option<String>> {
    match read_file_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub(crate) fn write_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let _write_guard = CONFIG_FILE_ACCESS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_file_atomically_locked(path, content, None, replace_file)
}

/// Publish a complete, synced file only if the destination is still absent.
/// Unlike replacement/CAS, publication never overwrites an external creator.
pub(crate) fn create_file_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let _write_guard = CONFIG_FILE_ACCESS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_file_atomically_locked(path, content, None, publish_new_file)
}

/// A fixed sibling bounds abandoned updates after a crash. An existing sibling
/// is never adopted or removed: its ownership is unknown to this write attempt.
pub(crate) fn write_file_atomically_with_fixed_sibling(
    path: &Path,
    content: &[u8],
    temporary: &Path,
) -> io::Result<()> {
    let _write_guard = CONFIG_FILE_ACCESS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    write_file_atomically_locked(path, content, Some(temporary), replace_file)
}

pub(crate) fn compare_and_swap_file_atomically(
    path: &Path,
    expected: &[u8],
    replacement: &[u8],
) -> io::Result<bool> {
    compare_and_swap_optional_file_atomically(path, Some(expected), Some(replacement))
}

pub(crate) fn compare_and_swap_optional_file_atomically(
    path: &Path,
    expected: Option<&[u8]>,
    replacement: Option<&[u8]>,
) -> io::Result<bool> {
    let _write_guard = CONFIG_FILE_ACCESS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let current = match fs::File::open(path) {
        Ok(file) => {
            // A source longer than the expected value is already a conflict.
            // Bound this second read even if an external writer grew the file.
            let limit = expected.map_or(0, <[u8]>::len).saturating_add(1) as u64;
            let mut content = Vec::new();
            file.take(limit).read_to_end(&mut content)?;
            Some(content)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current.as_deref() != expected {
        return Ok(false);
    }
    match replacement {
        Some(content) => write_file_atomically_locked(path, content, None, replace_file)?,
        None if current.is_some() => fs::remove_file(path)?,
        None => {}
    }
    Ok(true)
}

fn write_file_atomically_locked(
    path: &Path,
    content: &[u8],
    fixed_sibling: Option<&Path>,
    publish: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<()> {
    #[cfg(test)]
    if FAIL_NEXT_ATOMIC_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(path)
    {
        return Err(io::Error::other(
            "injected atomic configuration write failure",
        ));
    }
    let (temporary_path, mut temporary_file) = create_temporary_sibling(path, fixed_sibling)?;
    let write_result = (|| {
        temporary_file.write_all(content)?;
        temporary_file.sync_all()
    })();
    drop(temporary_file);

    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    if let Err(error) = publish(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error);
    }

    Ok(())
}

fn create_temporary_sibling(
    path: &Path,
    fixed_sibling: Option<&Path>,
) -> io::Result<(PathBuf, fs::File)> {
    if let Some(temporary) = fixed_sibling {
        if temporary == path
            || temporary.parent() != path.parent()
            || temporary.file_name().is_none()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "atomic-write temporary must be a distinct sibling",
            ));
        }
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(temporary)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    io::Error::new(
                        error.kind(),
                        "atomic-write sibling already exists; the existing update was preserved",
                    )
                } else {
                    error
                }
            })?;
        return Ok((temporary.to_path_buf(), file));
    }
    for _ in 0..16 {
        let temporary_path = temporary_sibling_path(path);
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
        {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique atomic-write sibling",
    ))
}

fn temporary_sibling_path(path: &Path) -> PathBuf {
    let random = Uuid::new_v4().as_u128() & 0x0000_ffff_ffff_ffff;
    path.with_file_name(format!(".l{random:012x}"))
}

#[cfg(not(windows))]
fn publish_new_file(from: &Path, to: &Path) -> io::Result<()> {
    // link() fails atomically if the destination exists. Both names are on the
    // same filesystem; unlinking the private sibling cannot alter published data.
    fs::hard_link(from, to)?;
    fs::remove_file(from)
}

#[cfg(windows)]
fn publish_new_file(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let from_wide = wide_verbatim_path(from)?;
    let to_wide = wide_verbatim_path(to)?;
    // Deliberately omit MOVEFILE_REPLACE_EXISTING, including on retries.
    if unsafe { MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_IGNORE_ACL_ERRORS,
        REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
    };

    let from_wide = wide_verbatim_path(from)?;
    let to_wide = wide_verbatim_path(to)?;
    let mut last_error = None;
    for _ in 0..50 {
        let replaced = unsafe {
            if to.exists() {
                ReplaceFileW(
                    to_wide.as_ptr(),
                    from_wide.as_ptr(),
                    std::ptr::null(),
                    REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
                    std::ptr::null(),
                    std::ptr::null(),
                )
            } else {
                MoveFileExW(from_wide.as_ptr(), to_wide.as_ptr(), MOVEFILE_WRITE_THROUGH)
            }
        };
        if replaced != 0 {
            return Ok(());
        }

        let error = io::Error::last_os_error();
        if !matches!(
            error.raw_os_error(),
            Some(5 | 32 | 33 | 80 | 1175 | 1176 | 1177)
        ) {
            return Err(error);
        }
        last_error = Some(error);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    Err(last_error.unwrap_or_else(|| io::Error::other("atomic file replacement failed")))
}

#[cfg(windows)]
pub(crate) fn wide_verbatim_path(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;

    const VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];

    let absolute = std::path::absolute(path)?;
    let raw = absolute.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut wide = if raw.starts_with(VERBATIM_PREFIX) {
        raw
    } else if raw.starts_with(&[b'\\' as u16, b'\\' as u16]) {
        UNC_PREFIX
            .iter()
            .copied()
            .chain(raw[2..].iter().copied())
            .collect()
    } else {
        VERBATIM_PREFIX.iter().copied().chain(raw).collect()
    };
    wide.push(0);
    Ok(wide)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn config_reader_accepts_utf8_bom() {
        let root =
            std::env::temp_dir().join(format!("lsgm-bom-config-{}", Uuid::new_v4().as_simple()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("instance.json");
        fs::write(&path, b"\xef\xbb\xbf{\"settings\":{}}").unwrap();

        let raw = read_file_to_string(&path).unwrap();
        assert_eq!(raw, r#"{"settings":{}}"#);
        serde_json::from_str::<Value>(&raw).unwrap();

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readers_only_observe_complete_json_during_replacement() {
        let root =
            std::env::temp_dir().join(format!("lsgm-atomic-config-{}", Uuid::new_v4().as_simple()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("instance.json");
        write_file_atomically(&path, br#"{"generation":0}"#).unwrap();

        let reader_path = path.clone();
        let reader = std::thread::spawn(move || {
            for _ in 0..2_000 {
                let raw = read_file_to_string(&reader_path).unwrap();
                serde_json::from_str::<Value>(&raw).unwrap();
            }
        });

        for generation in 1..=200 {
            let payload = format!(r#"{{"generation":{generation}}}"#);
            write_file_atomically(&path, payload.as_bytes()).unwrap();
        }
        reader.join().unwrap();

        let final_value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(final_value["generation"], 200);
        assert_eq!(
            fs::read_dir(&root).unwrap().filter_map(Result::ok).count(),
            1
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compare_and_swap_preserves_a_concurrent_writer() {
        let root =
            std::env::temp_dir().join(format!("lsgm-atomic-cas-{}", Uuid::new_v4().as_simple()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("instance.json");
        let original = br#"{"generation":"original"}"#;
        let migrated = br#"{"generation":"migrated"}"#;
        let external = br#"{"generation":"external"}"#;
        write_file_atomically(&path, original).unwrap();

        assert!(compare_and_swap_file_atomically(&path, original, migrated).unwrap());
        write_file_atomically(&path, external).unwrap();
        assert!(!compare_and_swap_file_atomically(&path, migrated, original).unwrap());
        assert_eq!(fs::read(&path).unwrap(), external);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compare_and_swap_rejects_grown_source_and_distinguishes_empty_from_absent() {
        let root = std::env::temp_dir().join(format!("lsgm-bounded-cas-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("source.txt");
        let grown = vec![b'x'; 1024 * 1024];
        fs::write(&path, &grown).unwrap();
        assert!(!compare_and_swap_file_atomically(&path, b"x", b"replacement").unwrap());
        assert_eq!(fs::read(&path).unwrap(), grown);
        fs::write(&path, b"").unwrap();
        assert!(!compare_and_swap_optional_file_atomically(&path, None, Some(b"new")).unwrap());
        assert!(compare_and_swap_file_atomically(&path, b"", b"new").unwrap());
        assert_eq!(fs::read(&path).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_compare_and_swap_creates_only_when_absent() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-atomic-optional-cas-{}",
            Uuid::new_v4().as_simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("native.ini");

        assert!(
            compare_and_swap_optional_file_atomically(&path, None, Some(b"managed=1\n")).unwrap()
        );
        assert!(
            !compare_and_swap_optional_file_atomically(&path, None, Some(b"raced=1\n")).unwrap()
        );
        assert_eq!(fs::read(&path).unwrap(), b"managed=1\n");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_create_never_replaces_an_existing_destination() {
        let root = std::env::temp_dir().join(format!("lg-atomic-create-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("journal.json");
        let original = br#"{"owner":"original"}"#;
        create_file_atomically(&path, original).unwrap();
        let error = create_file_atomically(&path, br#"{"owner":"other"}"#).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_create_preserves_a_destination_created_at_publication() {
        let root = std::env::temp_dir().join(format!("lg-atomic-create-race-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("journal.json");
        let error = write_file_atomically_locked(
            &path,
            br#"{"owner":"managed"}"#,
            None,
            |temporary, destination| {
                assert!(!destination.exists());
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(destination)?
                    .write_all(br#"{"owner":"concurrent"}"#)?;
                publish_new_file(temporary, destination)
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), br#"{"owner":"concurrent"}"#);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn atomic_create_publishes_only_complete_content_and_can_retry_a_failed_publication() {
        let root = std::env::temp_dir().join(format!("lg-atomic-create-retry-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("journal.json");
        let bytes =
            serde_json::to_vec(&serde_json::json!({ "backup": "x".repeat(64 * 1024) })).unwrap();
        let error = write_file_atomically_locked(&path, &bytes, None, |temporary, destination| {
            assert!(!destination.exists());
            let content: Value = serde_json::from_slice(&fs::read(temporary)?).unwrap();
            assert_eq!(content["backup"].as_str().unwrap().len(), 64 * 1024);
            Err(io::Error::other(
                "injected failure before atomic publication",
            ))
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected failure"));
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        create_file_atomically(&path, &bytes).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn optional_compare_and_swap_deletes_only_matching_content() {
        let root = std::env::temp_dir().join(format!(
            "lsgm-atomic-optional-delete-{}",
            Uuid::new_v4().as_simple()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("native.ini");
        fs::write(&path, b"managed=1\n").unwrap();

        assert!(
            !compare_and_swap_optional_file_atomically(&path, Some(b"stale=1\n"), None).unwrap()
        );
        assert_eq!(fs::read(&path).unwrap(), b"managed=1\n");
        assert!(
            compare_and_swap_optional_file_atomically(&path, Some(b"managed=1\n"), None).unwrap()
        );
        assert!(!path.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn temporary_sibling_name_does_not_repeat_a_long_destination_name() {
        let destination =
            Path::new("nested").join(format!("{}-SandboxSettings.ini", "a".repeat(96)));
        let temporary = temporary_sibling_path(&destination);

        assert_eq!(temporary.parent(), destination.parent());
        assert!(
            temporary
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".l")
        );
        assert!(temporary.as_os_str().len() < destination.as_os_str().len());
    }
}
