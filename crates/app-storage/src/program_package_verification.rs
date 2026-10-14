use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use app_core::{InstallVerificationProgress, InstallVerificationStage};
use serde::{Deserialize, Serialize};

use super::file_stamp::FileStamp;
use super::{CleanPackage, checked_relative, invalid, read_named_manifest, write_named_manifest};
use crate::StorageError;
use crate::instance_creation_io::check_creation_cancelled;
use crate::instance_isolation::paths::normalize_resource_path;
use crate::private_runtime_refresh::{
    PackageTree, hash_open_file_with_progress, scan_package_with,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CachedVerification {
    digest: String,
    stamp: FileStamp,
}

/// Content evidence captured by this installation while its verified file
/// handle still excludes writers. It cannot be restored from a stored manifest.
#[derive(Debug, Default)]
pub struct VerifiedProgramFiles {
    files: BTreeMap<String, CachedVerification>,
}

impl VerifiedProgramFiles {
    /// Minimum content size for capturing full filesystem identity evidence;
    /// smaller files are rehashed without the filesystem capability queries.
    pub const MIN_LARGE_FILE_BYTES: u64 = 4 * 1024 * 1024;

    /// Capture only after hashing this same handle while denying writes and
    /// replacements. Unsupported filesystems provide no reusable evidence.
    pub fn capture_verified_file(
        &mut self,
        relative: &str,
        file: &File,
        digest: &str,
    ) -> Result<(), StorageError> {
        self.capture_verified_file_with_minimum(relative, file, digest, None)
    }

    /// Capture evidence only for large, already verified protected handles.
    /// Smaller files still receive full content verification at publication.
    pub fn capture_large_verified_file(
        &mut self,
        relative: &str,
        file: &File,
        digest: &str,
    ) -> Result<(), StorageError> {
        self.capture_verified_file_with_minimum(
            relative,
            file,
            digest,
            Some(Self::MIN_LARGE_FILE_BYTES),
        )
    }

    fn capture_verified_file_with_minimum(
        &mut self,
        relative: &str,
        file: &File,
        digest: &str,
        minimum_size: Option<u64>,
    ) -> Result<(), StorageError> {
        let path = checked_relative(relative)?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(invalid(&path, "invalid verified program SHA-256"));
        }
        self.files.remove(relative);
        if let Some(minimum_size) = minimum_size {
            let metadata = file.metadata().map_err(|source| StorageError::ReadPath {
                path: path.clone(),
                source,
            })?;
            if metadata.len() < minimum_size {
                return Ok(());
            }
        }
        if let Some(stamp) = stamp(file, &path)? {
            self.files.insert(
                relative.to_owned(),
                CachedVerification {
                    digest: digest.to_owned(),
                    stamp,
                },
            );
        }
        Ok(())
    }
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

pub(super) fn scan_verified_package_with_progress(
    root: &Path,
    cancellation: Option<&AtomicBool>,
    on_progress: &mut dyn FnMut(InstallVerificationProgress),
) -> Result<(PackageTree, BTreeMap<String, CachedVerification>), StorageError> {
    let mut verified = BTreeMap::new();
    let mut total_bytes = 0_u64;
    let mut package = scan_package_with(root, &BTreeSet::new(), false, cancellation, |_, path| {
        let metadata = fs::metadata(path).map_err(|source| StorageError::ReadPath {
            path: path.to_owned(),
            source,
        })?;
        total_bytes = total_bytes
            .checked_add(metadata.len())
            .ok_or_else(|| invalid(root, "program verification byte count overflow"))?;
        Ok(String::new())
    })?;
    let mut progress = VerificationReporter::new(total_bytes, package.files.len(), on_progress);
    for (key, expected) in &mut package.files {
        check_creation_cancelled(cancellation)?;
        let path = root.join(checked_relative(key)?);
        normalize_resource_path(&path)?;
        let mut file = open_package_file(&path)?;
        let before = stamp(&file, &path)?;
        let digest = hash_open_file_with_progress(&mut file, &path, cancellation, &mut |bytes| {
            progress.advance_bytes(bytes);
        })?;
        let after = stamp(&file, &path)?;
        if before != after {
            return Err(invalid(
                &path,
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
        *expected = digest;
        progress.finish_file();
    }
    check_creation_cancelled(cancellation)?;
    progress.finish();
    Ok((package, verified))
}

struct VerificationReporter<'a> {
    progress: InstallVerificationProgress,
    last_report: Instant,
    on_progress: &'a mut dyn FnMut(InstallVerificationProgress),
}

impl<'a> VerificationReporter<'a> {
    fn new(
        total_bytes: u64,
        total_files: usize,
        on_progress: &'a mut dyn FnMut(InstallVerificationProgress),
    ) -> Self {
        let progress = InstallVerificationProgress {
            stage: InstallVerificationStage::PackageFiles,
            completed_bytes: 0,
            total_bytes,
            completed_files: 0,
            total_files: total_files as u64,
        };
        Self::resume(progress, on_progress)
    }

    fn resume(
        progress: InstallVerificationProgress,
        on_progress: &'a mut dyn FnMut(InstallVerificationProgress),
    ) -> Self {
        on_progress(progress.clone());
        Self {
            progress,
            last_report: Instant::now(),
            on_progress,
        }
    }

    fn advance_bytes(&mut self, bytes: u64) {
        self.progress.completed_bytes = self.progress.completed_bytes.saturating_add(bytes);
        self.report_if_due();
    }

    fn finish_file(&mut self) {
        self.progress.completed_files += 1;
        self.report_if_due();
    }

    fn report_if_due(&mut self) {
        if self.last_report.elapsed() >= Duration::from_millis(250) {
            (self.on_progress)(self.progress.clone());
            self.last_report = Instant::now();
        }
    }

    fn finish(&mut self) {
        (self.on_progress)(self.progress.clone());
    }
}

pub(super) struct PackageVerifier<'a> {
    root: &'a Path,
    allow_cached: bool,
    cancellation: Option<&'a AtomicBool>,
    observed: BTreeMap<String, CachedVerification>,
    progress_state: Option<(InstallVerificationProgress, BTreeSet<String>)>,
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
            progress_state: None,
        }
    }

    pub(super) fn with_verified_files(mut self, verified: VerifiedProgramFiles) -> Self {
        self.observed = verified.files;
        self
    }

    /// Two inventories can describe the same files. Count their union once,
    /// just as observed content evidence avoids hashing their overlap twice.
    pub(super) fn set_progress_inventory(
        &mut self,
        clean: &CleanPackage,
        initial: Option<&CleanPackage>,
    ) -> Result<(), StorageError> {
        let paths: BTreeSet<_> = clean
            .files
            .keys()
            .chain(initial.into_iter().flat_map(|package| package.files.keys()))
            .collect();
        let mut total_bytes = 0_u64;
        for relative in &paths {
            check_creation_cancelled(self.cancellation)?;
            let path = self.root.join(checked_relative(relative)?);
            normalize_resource_path(&path)?;
            let size = match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => metadata.len(),
                Ok(_) => 0,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                Err(source) => return Err(StorageError::ReadPath { path, source }),
            };
            total_bytes = total_bytes
                .checked_add(size)
                .ok_or_else(|| invalid(self.root, "program verification byte count overflow"))?;
        }
        self.progress_state = Some((
            InstallVerificationProgress {
                stage: InstallVerificationStage::PackageFiles,
                completed_bytes: 0,
                total_bytes,
                completed_files: 0,
                total_files: paths.len() as u64,
            },
            BTreeSet::new(),
        ));
        Ok(())
    }

    pub(super) fn verify(
        &mut self,
        module_id: Option<&str>,
        manifest: Option<CleanPackage>,
    ) -> Result<Option<CleanPackage>, StorageError> {
        self.verify_with_progress(module_id, manifest, &mut |_| {})
    }

    pub(super) fn verify_with_progress(
        &mut self,
        module_id: Option<&str>,
        manifest: Option<CleanPackage>,
        on_progress: &mut dyn FnMut(InstallVerificationProgress),
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
        let mut total_bytes = 0_u64;
        for key in manifest.files.keys() {
            check_creation_cancelled(self.cancellation)?;
            let path = self.root.join(checked_relative(key)?);
            normalize_resource_path(&path)?;
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => {
                    total_bytes = total_bytes.checked_add(metadata.len()).ok_or_else(|| {
                        invalid(self.root, "program verification byte count overflow")
                    })?;
                }
                Ok(metadata) if metadata.is_dir() => return Ok(None),
                Ok(_) => return Err(invalid(&path, "unsupported clean package entry type")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(source) => return Err(StorageError::ReadPath { path, source }),
            }
        }
        let shared_progress = self.progress_state.take();
        let aggregate = shared_progress.is_some();
        let (mut progress, mut reported_files) = match shared_progress {
            Some((progress, files)) => (VerificationReporter::resume(progress, on_progress), files),
            None => (
                VerificationReporter::new(total_bytes, manifest.files.len(), on_progress),
                BTreeSet::new(),
            ),
        };
        let mut verified = BTreeMap::new();
        for (key, expected) in &manifest.files {
            check_creation_cancelled(self.cancellation)?;
            let path = self.root.join(checked_relative(key)?);
            normalize_resource_path(&path)?;
            let report_file = reported_files.insert(key.clone());
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
                Some(cached) => {
                    let metadata = file.metadata().map_err(|source| StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    })?;
                    if report_file {
                        progress.advance_bytes(metadata.len());
                    }
                    cached.digest.clone()
                }
                None => hash_open_file_with_progress(
                    &mut file,
                    &path,
                    self.cancellation,
                    &mut |bytes| {
                        if report_file {
                            progress.advance_bytes(bytes);
                        }
                    },
                )?,
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
            if report_file {
                progress.finish_file();
            }
        }
        check_creation_cancelled(self.cancellation)?;
        manifest.verified_files = verified;
        progress.finish();
        if aggregate {
            self.progress_state = Some((progress.progress.clone(), reported_files));
        }
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
