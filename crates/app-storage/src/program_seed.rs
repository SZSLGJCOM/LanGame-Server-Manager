use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use app_modules::ModuleDescriptor;

use crate::instance_creation_io::{
    check_creation_cancelled, copy_creation_file, publish_creation_directory,
};
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::{StorageError, StoragePaths};

#[path = "program_seed_sources.rs"]
mod sources;

#[path = "program_package.rs"]
mod package;
#[cfg(test)]
use package::{CLEAN_PACKAGE, MAX_MANIFEST_BYTES};
pub(crate) use package::{
    CleanCopyInventory, read_clean_package_copy_inventory, read_clean_package_inventory,
    read_package_inventory, require_initial_package_tree, retired_library_is_clean,
    write_clean_package_copy_inventory,
};
use package::{
    CleanPackage, checked_relative, excluded, package_exclusions, read_manifest, validate_manifest,
    write_manifest,
};
pub use package::{
    library_program_is_pristine, record_library_program_baseline,
    retain_published_library_program_baseline, retain_verified_library_program_baseline,
};
pub(crate) use package::{read_clean_package_tree, require_clean_package_tree};

const STAGE_OWNER: &str = ".langame-seed-staging";
#[path = "program_acquisition.rs"]
mod acquisition;
#[cfg(test)]
use acquisition::{ACQUISITION, MAX_ACQUISITION_BYTES};
pub use acquisition::{
    LibraryProgramAcquisition, library_program_acquisition_is_trusted,
    read_library_program_acquisition, restore_library_program_acquisition,
};
use acquisition::{clear_completed_acquisition, write_acquisition};

#[derive(Debug)]
pub struct CleanLibrarySeed {
    pub install_root: PathBuf,
    pub requires_validation: bool,
    pub current_version: Option<String>,
}

#[derive(Clone)]
struct SeedSource {
    root: PathBuf,
    current_version: Option<String>,
}

struct SelectedSeed {
    stage: Option<SeedStage>,
    complete: bool,
    current_version: Option<String>,
    // The blocking copier owns this lease, including after its waiter is dropped.
    _archive_lease: Option<crate::instance_settings_lock::InstanceSettingsLock>,
}

pub async fn prepare_clean_library_seed(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<CleanLibrarySeed, StorageError> {
    let target = library_target(paths, descriptor)?;
    prepare_clean_library_seed_at(paths, descriptor, &target, cancellation).await
}

/// The caller selects the registered library location and holds its lifecycle
/// lease through seed publication and installation. Custom locations are valid,
/// but an instance-owned directory can never become a library seed target.
pub async fn prepare_clean_library_seed_at(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    target: &Path,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<CleanLibrarySeed, StorageError> {
    let target = validate_seed_target(paths, target)?;
    let target =
        crate::program_library_retention::validate_retained_library_target(paths, &target)?;
    prepare_seed_target(paths, descriptor, target, cancellation, true).await
}

/// Seed a program acquired exclusively for a new independent instance. Its
/// managed location allows a recoverable same-volume ownership transfer.
pub async fn prepare_instance_program_seed_at(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    target: &Path,
    cancellation: Option<Arc<AtomicBool>>,
) -> Result<CleanLibrarySeed, StorageError> {
    if !crate::is_instance_program_acquisition(paths, &descriptor.summary.id, target)? {
        return Err(invalid(
            target,
            "invalid instance program acquisition target",
        ));
    }
    let target = normalize_path(target)?;
    require_absent(&target)?;
    prepare_seed_target(paths, descriptor, target, cancellation, false).await
}

async fn prepare_seed_target(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    target: PathBuf,
    cancellation: Option<Arc<AtomicBool>>,
    retain_source: bool,
) -> Result<CleanLibrarySeed, StorageError> {
    let token = cancellation.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
    let mut cancel_on_drop = CancelOnDrop(Some(Arc::clone(&token)));
    check_creation_cancelled(Some(&token))?;
    let pool = crate::storage_db::connect_pool(paths).await?;
    let ownership = async {
        let mut connection = pool.acquire().await?;
        crate::program_install_records::ensure_library_install_root_available(
            &mut connection,
            &target,
        )
        .await
    }
    .await;
    pool.close().await;
    ownership?;
    let selected = sources::select(paths, descriptor, &target, &token).await?;
    let descriptor = descriptor.clone();
    let worker_target = target.clone();
    let result = tokio::task::spawn_blocking(move || {
        publish_seed_at(
            &worker_target,
            &descriptor,
            selected,
            Some(&token),
            retain_source,
        )
    })
    .await
    .map_err(|error| invalid(&target, format!("clean seed worker failed: {error}")))?;
    cancel_on_drop.0 = None;
    result
}

struct CancelOnDrop(Option<Arc<AtomicBool>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(true, Ordering::Release);
        }
    }
}

fn library_target(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
) -> Result<PathBuf, StorageError> {
    let install = descriptor
        .install
        .as_ref()
        .ok_or_else(|| invalid(&paths.games_root, "module has no install specification"))?;
    let relative = checked_relative(&install.shared_game_dir.replace('\\', "/"))?;
    let root = normalize_path(&paths.games_root)?;
    let target = normalize_path(&root.join(relative))?;
    if !contains(&root, &target) || root == target {
        return Err(invalid(&target, "invalid library target"));
    }
    Ok(target)
}

fn validate_seed_target(paths: &StoragePaths, target: &Path) -> Result<PathBuf, StorageError> {
    let target = normalize_path(target)?;
    let instances = normalize_path(&paths.instances_root)?;
    if contains(&instances, &target) || contains(&target, &instances) {
        return Err(invalid(
            &target,
            "library seed target overlaps the instance directory",
        ));
    }
    require_absent(&target)?;
    Ok(target)
}

#[cfg(test)]
fn prepare_seed_at(
    target: &Path,
    descriptor: &ModuleDescriptor,
    sources: &[SeedSource],
    cancellation: Option<&AtomicBool>,
) -> Result<CleanLibrarySeed, StorageError> {
    let selected = sources::select_candidates(target, descriptor, sources, cancellation)?;
    publish_seed_at(target, descriptor, selected, cancellation, false)
}

fn publish_seed_at(
    target: &Path,
    descriptor: &ModuleDescriptor,
    selected: Option<SelectedSeed>,
    cancellation: Option<&AtomicBool>,
    retain_source: bool,
) -> Result<CleanLibrarySeed, StorageError> {
    let admission = (|| {
        check_creation_cancelled(cancellation)?;
        normalize_path(target)?;
        require_absent(target)
    })();
    if let Err(error) = admission {
        return Err(sources::cleanup_after_error(selected, error));
    }
    let (mut stage, complete, current_version, _archive_lease) = match selected {
        Some(mut selected) => (
            selected
                .stage
                .take()
                .ok_or_else(|| invalid(target, "selected seed staging ownership is missing"))?,
            selected.complete,
            selected.current_version.take(),
            selected._archive_lease.take(),
        ),
        None => (SeedStage::for_target(target)?, false, None, None),
    };
    let result = (|| {
        if retain_source {
            crate::program_library_retention::write_retained_library_source(
                &stage.path,
                target,
                &descriptor.summary.id,
            )?;
        }
        if !complete {
            write_acquisition(&stage.path, target, &descriptor.summary.id)?;
        }
        check_creation_cancelled(cancellation)?;
        publish_creation_directory(&stage.path, target, cancellation)?;
        stage.settled = true;
        fs::remove_file(target.join(STAGE_OWNER)).map_err(|source| StorageError::DeletePath {
            path: target.join(STAGE_OWNER),
            source,
        })?;
        Ok(CleanLibrarySeed {
            install_root: target.to_owned(),
            requires_validation: !complete,
            current_version,
        })
    })();
    if let Err(error) = result {
        if let Err(cleanup) = stage.cleanup() {
            return Err(invalid(
                &stage.path,
                format!("{error}; owned seed cleanup failed: {cleanup}"),
            ));
        }
        return Err(error);
    }
    result
}

fn copy_manifest(
    source: &Path,
    target: &Path,
    manifest: &CleanPackage,
    cancellation: Option<&AtomicBool>,
) -> Result<bool, StorageError> {
    let mut complete = true;
    let mut unavailable_directories = std::collections::BTreeSet::<PathBuf>::new();
    for key in &manifest.directories {
        check_creation_cancelled(cancellation)?;
        let relative = checked_relative(key)?;
        if !relative
            .ancestors()
            .any(|parent| unavailable_directories.contains(parent))
        {
            let from = source.join(&relative);
            normalize_resource_path(&from)?;
            match fs::symlink_metadata(&from) {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(metadata) if metadata.is_file() => {
                    complete = false;
                    unavailable_directories.insert(relative.clone());
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    complete = false;
                    unavailable_directories.insert(relative.clone());
                }
                Ok(_) => return Err(invalid(&from, "unsupported clean package directory type")),
                Err(source) => return Err(StorageError::ReadPath { path: from, source }),
            }
        }
        let path = target.join(relative);
        fs::create_dir_all(&path).map_err(|source| StorageError::CreatePath { path, source })?;
    }
    for (key, expected) in &manifest.files {
        check_creation_cancelled(cancellation)?;
        let relative = checked_relative(key)?;
        if relative
            .ancestors()
            .any(|parent| unavailable_directories.contains(parent))
        {
            continue;
        }
        let from = source.join(&relative);
        normalize_resource_path(&from)?;
        match fs::symlink_metadata(&from) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                complete = false;
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                complete = false;
                continue;
            }
            Err(source) => return Err(StorageError::ReadPath { path: from, source }),
        }
        let to = target.join(relative);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                path: parent.to_owned(),
                source,
            })?;
        }
        match copy_creation_file(&from, &to, cancellation) {
            Ok(actual) if &actual == expected => {}
            Ok(_) => {
                remove_owned_file(&to)?;
                complete = false;
            }
            Err(StorageError::CopyPath { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                if to.try_exists().map_err(|source| StorageError::ReadPath {
                    path: to.clone(),
                    source,
                })? {
                    remove_owned_file(&to)?;
                }
                complete = false;
            }
            Err(error) => return Err(error),
        }
    }
    Ok(complete)
}

fn require_absent(path: &Path) -> Result<(), StorageError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(invalid(
            path,
            "library seed target already exists; refusing to replace it",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::ReadPath {
            path: path.to_owned(),
            source,
        }),
    }
}

struct SeedStage {
    path: PathBuf,
    token: String,
    settled: bool,
}
impl SeedStage {
    fn for_target(target: &Path) -> Result<Self, StorageError> {
        let parent = target
            .parent()
            .ok_or_else(|| invalid(target, "library target has no parent"))?;
        fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
            path: parent.to_owned(),
            source,
        })?;
        normalize_path(parent)?;
        Self::create(parent)
    }

    fn create(parent: &Path) -> Result<Self, StorageError> {
        let token = uuid::Uuid::new_v4().to_string();
        let path = parent.join(format!(".langame-seed-{token}"));
        fs::create_dir(&path).map_err(|source| StorageError::CreatePath {
            path: path.clone(),
            source,
        })?;
        let marker = path.join(STAGE_OWNER);
        let mut file = match fs::File::create_new(&marker) {
            Ok(file) => file,
            Err(source) => {
                // remove_dir cannot remove a concurrently populated directory.
                if let Err(cleanup) = fs::remove_dir(&path) {
                    return Err(invalid(
                        &path,
                        format!(
                            "cannot create seed ownership: {source}; empty staging cleanup failed: {cleanup}"
                        ),
                    ));
                }
                return Err(StorageError::WriteConfig {
                    path: marker,
                    source,
                });
            }
        };
        if let Err(source) = file.write_all(token.as_bytes()) {
            drop(file);
            // No payload has been copied yet; only this newly allocated marker
            // and its empty parent can exist from this operation.
            fs::remove_file(&marker).map_err(|source| StorageError::DeletePath {
                path: marker.clone(),
                source,
            })?;
            fs::remove_dir(&path).map_err(|source| StorageError::DeletePath {
                path: path.clone(),
                source,
            })?;
            return Err(StorageError::WriteConfig {
                path: marker,
                source,
            });
        }
        Ok(Self {
            path,
            token,
            settled: false,
        })
    }
    fn cleanup(&mut self) -> Result<(), StorageError> {
        if self.settled {
            return Ok(());
        }
        normalize_path(&self.path)?;
        let marker = self.path.join(STAGE_OWNER);
        normalize_resource_path(&marker)?;
        if fs::read(&marker).map_err(|source| StorageError::ReadPath {
            path: marker,
            source,
        })? != self.token.as_bytes()
        {
            return Err(invalid(
                &self.path,
                "seed staging ownership changed; refusing cleanup",
            ));
        }
        remove_owned_tree(&self.path)?;
        self.settled = true;
        Ok(())
    }
}
impl Drop for SeedStage {
    fn drop(&mut self) {
        if !self.settled
            && let Err(error) = self.cleanup()
        {
            eprintln!(
                "Program seed staging cleanup failed for {}: {error}",
                self.path.display()
            );
        }
    }
}

fn remove_owned_tree(root: &Path) -> Result<(), StorageError> {
    normalize_path(root)?;
    for entry in fs::read_dir(root).map_err(|source| StorageError::ReadDirectory {
        path: root.to_owned(),
        source,
    })? {
        let path = entry
            .map_err(|source| StorageError::ReadDirectory {
                path: root.to_owned(),
                source,
            })?
            .path();
        if path.file_name().is_some_and(|name| name == STAGE_OWNER) {
            continue;
        }
        normalize_resource_path(&path)?;
        if path.is_dir() {
            remove_owned_tree(&path)?;
        } else {
            remove_owned_file(&path)?;
        }
    }
    let marker = root.join(STAGE_OWNER);
    if marker
        .try_exists()
        .map_err(|source| StorageError::ReadPath {
            path: marker.clone(),
            source,
        })?
    {
        normalize_resource_path(&marker)?;
        remove_owned_file(&marker)?;
    }
    fs::remove_dir(root).map_err(|source| StorageError::DeletePath {
        path: root.to_owned(),
        source,
    })
}

fn remove_owned_file(path: &Path) -> Result<(), StorageError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_READONLY, SetFileAttributesW,
        };

        let attributes = fs::metadata(path)
            .map_err(|source| StorageError::ReadPath {
                path: path.to_owned(),
                source,
            })?
            .file_attributes();
        if attributes & FILE_ATTRIBUTE_READONLY != 0 {
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // Clear only the Windows read-only attribute; retain ACLs and other attributes.
            // The owned UTF-16 buffer stays alive and is NUL-terminated for this call.
            if unsafe { SetFileAttributesW(wide.as_ptr(), attributes & !FILE_ATTRIBUTE_READONLY) }
                == 0
            {
                return Err(StorageError::WriteConfig {
                    path: path.to_owned(),
                    source: std::io::Error::last_os_error(),
                });
            }
        }
    }
    fs::remove_file(path).map_err(|source| StorageError::DeletePath {
        path: path.to_owned(),
        source,
    })
}

fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::PrivateRuntimeRefresh {
        path: path.to_owned(),
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "program_seed_tests.rs"]
mod tests;
