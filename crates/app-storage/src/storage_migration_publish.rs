//! Metadata publication protected by the coordinator's durable Applying journal.
//! The journal must already contain the complete original and replacement bytes.
//! This is recoverable remove/create publication, not an atomic replacement: a
//! missing name after removal is recovered with an exclusive create on replay.
use std::io::{self, Read};
use std::path::Path;

use crate::StorageError;
use crate::instance_archive_files::native;
use crate::instance_isolation::paths::normalize_resource_path;

const BUFFER_BYTES: usize = 64 * 1024;

/// Publish one entry from an already persisted Applying journal. The caller
/// retains that journal until all metadata, database and settings agree. Never
/// call this using only in-memory original bytes: a crash can leave `path` absent.
pub(crate) fn publish(path: &Path, before: &[u8], after: &[u8]) -> Result<(), StorageError> {
    publish_with(path, before, after, |_, _| Ok(()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublishPoint {
    Verified,
    Removed,
    BeforeCreate,
}

fn publish_with(
    path: &Path,
    before: &[u8],
    after: &[u8],
    mut checkpoint: impl FnMut(PublishPoint, &Path) -> io::Result<()>,
) -> Result<(), StorageError> {
    let path = normalize_resource_path(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| invalid(&path, "Metadata file has no parent."))?;
    // Windows guards pin every ancestor, including the destination parent,
    // throughout the absent-name interval and the final no-replace publication.
    let _parent = native::open(parent, true, false).map_err(|error| read_error(parent, error))?;
    match native::open_verified_file(&path, true) {
        Ok(mut original) => {
            let (matches_before, matches_after) = compare_bytes(original.reader(), before, after)
                .map_err(|error| read_error(&path, error))?;
            if matches_after {
                return Ok(());
            }
            if !matches_before {
                return Err(invalid(
                    &path,
                    "Migration metadata differs from both journal versions; the existing file was preserved.",
                ));
            }
            checkpoint(PublishPoint::Verified, &path).map_err(|error| interrupted(&path, error))?;
            #[cfg(windows)]
            original
                .remove()
                .map_err(|source| StorageError::DeletePath {
                    path: path.clone(),
                    source,
                })?;
            #[cfg(not(windows))]
            return Err(invalid(
                &path,
                "Replacing existing migration metadata requires Windows handle-bound deletion; the original was preserved.",
            ));
            #[cfg(windows)]
            checkpoint(PublishPoint::Removed, &path).map_err(|error| interrupted(&path, error))?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(read_error(&path, error)),
    }
    checkpoint(PublishPoint::BeforeCreate, &path).map_err(|error| interrupted(&path, error))?;
    let publication = crate::atomic_file::create_file_atomically(&path, after);
    match publication {
        Ok(()) => verify_after(&path, after),
        Err(publication_error) => {
            // Another actor may claim the absent name. Its different contents
            // are a conflict, even if the journal still holds our old version.
            // Identical after bytes are an already completed, idempotent result.
            match native::open_verified_file(&path, false) {
                Ok(mut current) => {
                    let (_, matches_after) = compare_bytes(current.reader(), &[], after)
                        .map_err(|error| read_error(&path, error))?;
                    if matches_after {
                        Ok(())
                    } else {
                        Err(invalid(
                            &path,
                            "Another file occupies the metadata destination; it was preserved. The durable migration journal retains the original bytes.",
                        ))
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    Err(StorageError::WriteConfig {
                        path,
                        source: publication_error,
                    })
                }
                Err(error) => Err(read_error(&path, error)),
            }
        }
    }
}

fn verify_after(path: &Path, after: &[u8]) -> Result<(), StorageError> {
    let mut current =
        native::open_verified_file(path, false).map_err(|error| read_error(path, error))?;
    if !compare_bytes(current.reader(), &[], after)
        .map_err(|error| read_error(path, error))?
        .1
    {
        return Err(invalid(
            path,
            "Published migration metadata changed during read-back; the journal and existing file were preserved.",
        ));
    }
    Ok(())
}

fn compare_bytes(reader: &mut impl Read, before: &[u8], after: &[u8]) -> io::Result<(bool, bool)> {
    let mut buffer = [0_u8; BUFFER_BYTES];
    let mut offset = 0_usize;
    let mut matches_before = true;
    let mut matches_after = true;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok((
                matches_before && offset == before.len(),
                matches_after && offset == after.len(),
            ));
        }
        let end = offset
            .checked_add(count)
            .ok_or_else(|| io::Error::other("Metadata size overflow."))?;
        matches_before &= before.get(offset..end) == Some(&buffer[..count]);
        matches_after &= after.get(offset..end) == Some(&buffer[..count]);
        if !matches_before && !matches_after {
            return Ok((false, false));
        }
        offset = end;
    }
}

fn read_error(path: &Path, source: io::Error) -> StorageError {
    StorageError::ReadPath {
        path: path.to_owned(),
        source,
    }
}

fn invalid(path: &Path, message: &str) -> StorageError {
    StorageError::InvalidStorageMigration {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

fn interrupted(path: &Path, error: io::Error) -> StorageError {
    invalid(
        path,
        &format!(
            "Metadata publication was interrupted: {error}. Preserve and replay the durable migration journal."
        ),
    )
}

#[cfg(test)]
#[path = "storage_migration_publish_tests.rs"]
mod tests;
