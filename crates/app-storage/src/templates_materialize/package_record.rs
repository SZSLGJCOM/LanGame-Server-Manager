use super::*;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};

#[cfg(test)]
static FAIL_NEXT_PUBLICATION: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
pub(crate) fn fail_next_publication_for_test(target: &Path) {
    *FAIL_NEXT_PUBLICATION.lock().unwrap() = Some(target.to_owned());
}

pub(crate) fn file_sha256(path: &Path) -> io::Result<String> {
    let mut source = File::open(path)?;
    let expected_length = source.metadata()?.len();
    let limit = expected_length
        .checked_add(1)
        .ok_or_else(|| io::Error::other("PAK length exceeds the hash inspection limit."))?;
    let mut bounded = Read::by_ref(&mut source).take(limit);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut read_length = 0_u64;
    loop {
        let count = bounded.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read_length += count as u64;
        hash.update(&buffer[..count]);
    }
    if read_length != expected_length || source.metadata()?.len() != expected_length {
        return Err(io::Error::other("PAK size changed during hash inspection."));
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// An absent target can be created. An existing target must match bytes that
/// LGSM previously published, including an explicitly pending publication.
pub(crate) fn validate_owned_target(
    path: &Path,
    expected: &[String],
) -> io::Result<Option<String>> {
    reject_link_ancestors(path)?;
    if optional_snapshot(path, PackageKind::File)?.is_none() {
        return Ok(None);
    }
    if expected.is_empty() {
        return Err(io::Error::other(
            "An untracked native PAK already exists; it will not be overwritten.",
        ));
    }
    let digest = file_sha256(path)?;
    if !expected.contains(&digest) {
        return Err(io::Error::other(
            "The native PAK was modified outside LGSM; it will not be overwritten.",
        ));
    }
    Ok(Some(digest))
}

/// Publish one PAK and its ownership record as an independent package operation.
/// The pending record is durable before the PAK changes. It describes both old
/// and prepared hashes, so an interruption never leaves an unowned native copy.
/// The final record narrows ownership back to the newly published bytes.
pub(crate) fn copy_package_file_with_record(
    module_id: &str,
    source: &Path,
    target: &Path,
    record: &Path,
    previous_record: Option<&[u8]>,
    expected_hashes: &[String],
    build_record: impl FnOnce(&str, Option<&str>) -> io::Result<(Vec<u8>, Vec<u8>)>,
) -> Result<Vec<u8>, StorageError> {
    let result = (|| {
        reject_link_ancestors(source)?;
        reject_link_ancestors(target)?;
        reject_link_ancestors(record)?;
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("Package target has no parent."))?;
        let record_parent = record
            .parent()
            .ok_or_else(|| io::Error::other("Package record has no parent."))?;
        fs::create_dir_all(parent)?;
        fs::create_dir_all(record_parent)?;
        let parent = fs::canonicalize(parent)?;
        let source_stamp = snapshot(source, PackageKind::File)?;
        let original = optional_snapshot(target, PackageKind::File)?;
        let original_digest = validate_owned_target(target, expected_hashes)?;
        verify_record(record, previous_record)?;
        let work = create_work_directory(&parent)?;
        // Metadata must be staged on its own volume for rename publication.
        let record_parent = fs::canonicalize(record_parent)?;
        let record_work = match create_work_directory(&record_parent) {
            Ok(path) => path,
            Err(error) => {
                let _ = remove_work_directory(&work, &parent);
                return Err(error);
            }
        };
        let prepared = work.join("prepared");
        let retained = work.join("retained");
        let pending = record_work.join("pending");
        let finalized = record_work.join("final");
        let retained_record = record_work.join("retained");
        let mut target_moved = false;
        let mut payload_published = false;
        let mut record_published = false;
        let mut published_record = Vec::new();
        let mut prepared_digest = String::new();
        let operation = (|| {
            copy_snapshot(source, &prepared, &source_stamp, &mut copy_bytes)?;
            prepared_digest = file_sha256(&prepared)?;
            let (pending_bytes, final_bytes) =
                build_record(&prepared_digest, original_digest.as_deref())?;
            write_synced(&pending, &pending_bytes)?;
            write_synced(&finalized, &final_bytes)?;
            if let Some(bytes) = previous_record {
                write_synced(&retained_record, bytes)?;
            }
            if snapshot(source, PackageKind::File)? != source_stamp
                || optional_snapshot(target, PackageKind::File)? != original
                || validate_owned_target(target, expected_hashes)? != original_digest
            {
                return Err(io::Error::other(
                    "Package source or target changed during staging.",
                ));
            }
            verify_record(record, previous_record)?;
            replace_record(&pending, record)?;
            record_published = true;
            published_record = pending_bytes;
            #[cfg(test)]
            {
                let mut failure = FAIL_NEXT_PUBLICATION.lock().unwrap();
                if failure.as_deref() == Some(target) {
                    *failure = None;
                    return Err(io::Error::other("injected native PAK publication failure"));
                }
            }
            if original.is_some() {
                move_exclusive(target, &retained)?;
                target_moved = true;
            }
            move_exclusive(&prepared, target)?;
            payload_published = true;
            verify_record(record, Some(&published_record))?;
            replace_record(&finalized, record)?;
            published_record = final_bytes.clone();
            Ok(final_bytes)
        })();
        match operation {
            Ok(bytes) => {
                remove_work_directory(&work, &parent)?;
                remove_work_directory(&record_work, &record_parent)?;
                Ok(bytes)
            }
            Err(error) => {
                let rollback = (|| {
                    if payload_published {
                        reject_link_ancestors(target)?;
                        if file_sha256(target)? != prepared_digest {
                            return Err(io::Error::other(
                                "Published PAK changed before rollback; preserving it.",
                            ));
                        }
                        fs::remove_file(target)?;
                    }
                    if target_moved {
                        move_exclusive(&retained, target)?;
                    }
                    if record_published {
                        verify_record(record, Some(&published_record))?;
                        if previous_record.is_some() {
                            replace_record(&retained_record, record)?;
                        } else {
                            fs::remove_file(record)?;
                        }
                    }
                    Ok::<(), io::Error>(())
                })();
                if let Err(rollback) = rollback {
                    return Err(io::Error::other(format!(
                        "{error}; rollback failed: {rollback}; retained package: {}; retained record: {}",
                        work.display(),
                        record_work.display()
                    )));
                }
                remove_work_directory(&work, &parent)?;
                remove_work_directory(&record_work, &record_parent)?;
                Err(error)
            }
        }
    })();
    result.map_err(|error| StorageError::ModuleSupportMaterialization {
        module_id: module_id.to_owned(),
        path: target.to_owned(),
        message: error.to_string(),
    })
}

fn verify_record(path: &Path, expected: Option<&[u8]>) -> io::Result<()> {
    reject_link_ancestors(path)?;
    match (optional_snapshot(path, PackageKind::File)?, expected) {
        (None, None) => Ok(()),
        (Some(_), Some(expected)) => {
            let file = File::open(path)?;
            if file.metadata()?.len() != expected.len() as u64 {
                return Err(io::Error::other(
                    "Package ownership record changed during staging.",
                ));
            }
            let mut bytes = Vec::new();
            file.take(expected.len() as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes == expected {
                Ok(())
            } else {
                Err(io::Error::other(
                    "Package ownership record changed during staging.",
                ))
            }
        }
        _ => Err(io::Error::other(
            "Package ownership record changed during staging.",
        )),
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::options().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(windows))]
fn replace_record(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_record(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both paths live on the record's volume; replacement never leaves an
    // interval in which a previously published ownership record is absent.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
