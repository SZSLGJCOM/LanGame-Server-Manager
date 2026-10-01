use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use super::file_stamp::FileStamp;
use super::{CleanPackage, checked_relative, invalid, read_named_manifest, write_named_manifest};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::normalize_resource_path;
use crate::private_runtime_refresh::{PackageTree, hash_open_file, scan_package_with};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedVerification {
    digest: String,
    stamp: FileStamp,
}

// A damaged or unsupported optional cache cannot invalidate an otherwise valid
// official allowlist. Its files still have to pass content verification.
pub(super) fn read_cached_verifications<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, CachedVerification>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

pub(super) fn scan_verified_package(
    root: &Path,
    cancellation: Option<&AtomicBool>,
) -> Result<(PackageTree, BTreeMap<String, CachedVerification>), StorageError> {
    let mut verified = BTreeMap::new();
    let package = scan_package_with(root, &BTreeSet::new(), false, cancellation, |key, path| {
        let mut file = open_package_file(path)?;
        let before = stamp(&file, path)?;
        let digest = hash_open_file(&mut file, path, cancellation)?;
        let after = stamp(&file, path)?;
        if before != after {
            return Err(invalid(
                path,
                "program file changed while recording its baseline",
            ));
        }
        if let Some(stamp) = after {
            verified.insert(
                key.to_owned(),
                CachedVerification {
                    digest: digest.clone(),
                    stamp,
                },
            );
        }
        Ok(digest)
    })?;
    Ok((package, verified))
}

pub(super) struct PackageVerifier<'a> {
    root: &'a Path,
    allow_cached: bool,
    cancellation: Option<&'a AtomicBool>,
    observed: BTreeMap<String, CachedVerification>,
}

impl<'a> PackageVerifier<'a> {
    pub(super) fn new(
        root: &'a Path,
        allow_cached: bool,
        cancellation: Option<&'a AtomicBool>,
    ) -> Self {
        Self {
            root,
            allow_cached,
            cancellation,
            observed: BTreeMap::new(),
        }
    }

    pub(super) fn verify(
        &mut self,
        module_id: Option<&str>,
        manifest: Option<CleanPackage>,
    ) -> Result<Option<CleanPackage>, StorageError> {
        check_creation_cancelled(self.cancellation)?;
        let Some(mut manifest) = manifest else {
            return Ok(None);
        };
        if module_id.is_some_and(|id| id != manifest.module_id) {
            return Ok(None);
        }
        // Ancestors must remain directories before accessing their children.
        for key in &manifest.directories {
            check_creation_cancelled(self.cancellation)?;
            let path = self.root.join(checked_relative(key)?);
            normalize_resource_path(&path)?;
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(metadata) if metadata.is_file() => return Ok(None),
                Ok(_) => return Err(invalid(&path, "unsupported clean package entry type")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => return Err(StorageError::ReadPath { path, source }),
            }
        }
        let mut verified = BTreeMap::new();
        for (key, expected) in &manifest.files {
            check_creation_cancelled(self.cancellation)?;
            let path = self.root.join(checked_relative(key)?);
            normalize_resource_path(&path)?;
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(metadata) if metadata.is_dir() => return Ok(None),
                Ok(_) => return Err(invalid(&path, "unsupported clean package entry type")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => return Err(StorageError::ReadPath { path, source }),
            }
            let mut file = match open_package_file(&path) {
                Ok(file) => file,
                Err(StorageError::ReadPath { source, .. })
                    if source.kind() == std::io::ErrorKind::NotFound =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error),
            };
            let before = stamp(&file, &path)?;
            let matches = |cached: &&CachedVerification| {
                cached.digest == *expected && Some(&cached.stamp) == before.as_ref()
            };
            let cached = self.observed.get(key).filter(matches).or_else(|| {
                self.allow_cached
                    .then(|| manifest.verified_files.get(key).filter(matches))
                    .flatten()
            });
            let actual = match cached {
                Some(cached) => cached.digest.clone(),
                None => hash_open_file(&mut file, &path, self.cancellation)?,
            };
            let after = stamp(&file, &path)?;
            if before != after || actual != *expected {
                return Ok(None);
            }
            if let Some(stamp) = after {
                let cached = CachedVerification {
                    digest: actual,
                    stamp,
                };
                self.observed.insert(key.clone(), cached.clone());
                verified.insert(key.clone(), cached);
            }
        }
        check_creation_cancelled(self.cancellation)?;
        manifest.verified_files = verified;
        Ok(Some(manifest))
    }

    pub(super) fn persist_verification(
        &self,
        manifest: &CleanPackage,
        name: &str,
    ) -> Result<(), StorageError> {
        check_creation_cancelled(self.cancellation)?;
        let current = read_named_manifest(self.root, name)?.ok_or_else(|| {
            invalid(
                self.root,
                "program inventory disappeared during verification",
            )
        })?;
        if current.module_id != manifest.module_id
            || current.files != manifest.files
            || current.directories != manifest.directories
        {
            return Err(invalid(
                self.root,
                "program inventory changed during verification",
            ));
        }
        if current.verified_files != manifest.verified_files {
            write_named_manifest(self.root, manifest, name)?;
        }
        Ok(())
    }
}

fn stamp(file: &File, path: &Path) -> Result<Option<FileStamp>, StorageError> {
    FileStamp::read(file).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })
}

fn open_package_file(path: &Path) -> Result<File, StorageError> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        // Windows timestamps can lag while another write handle is open.
        // Deny writers and replacements for the lifetime of this sample/read.
        options
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path).map_err(|source| StorageError::ReadPath {
        path: path.to_owned(),
        source,
    })
}
