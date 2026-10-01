use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::StorageError;
use crate::instance_creation_io::{
    check_creation_cancelled, copy_creation_file, publish_creation_directory,
};
use crate::private_runtime_refresh::PackageTree;

pub(crate) const PRIVATE_RUNTIME_MARKER: &str = ".langame-private-runtime";
pub(crate) const PROJECTION_RUNTIME_MARKER: &str = ".langame-runtime-projection.json";

#[path = "private_runtime/projection_identity.rs"]
mod projection_identity;
pub(crate) use projection_identity::{
    record_projection_identity, validate_projection_refresh_roots,
};
/// Every instance owns a managed runtime. Missing or invalid runtime trees
/// must never redirect saves or configuration into the shared package.
pub fn resolve_instance_private_runtime_root(
    instance_root: &Path,
) -> Result<PathBuf, StorageError> {
    let runtime_root = instance_root.join("runtime");
    let metadata = match fs::symlink_metadata(&runtime_root) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(StorageError::PrivateRuntimeRefresh {
                path: runtime_root,
                message: String::from(
                    "required private runtime is missing; restore or repair this instance before use; refusing to use the shared installation",
                ),
            });
        }
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: runtime_root,
                source,
            });
        }
    };
    let invalid = || StorageError::PrivateRuntimeRefresh {
        path: runtime_root.clone(),
        message: String::from(
            "instance runtime is missing a valid private marker; refusing to use the shared installation",
        ),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() || is_reparse_point(&runtime_root)? {
        return Err(invalid());
    }
    let marker = runtime_root.join(PRIVATE_RUNTIME_MARKER);
    let marker_metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Err(invalid()),
        Err(source) => {
            return Err(StorageError::ReadPath {
                path: marker,
                source,
            });
        }
    };
    if !marker_metadata.is_file()
        || marker_metadata.file_type().is_symlink()
        || is_reparse_point(&marker)?
    {
        return Err(invalid());
    }
    let content = fs::read(&marker).map_err(|source| StorageError::ReadPath {
        path: marker,
        source,
    })?;
    if content != b"managed\n" {
        return Err(invalid());
    }
    Ok(runtime_root)
}

#[derive(Debug, Clone, Default)]
pub struct PrivateRuntimeProjection {
    pub private_directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct InstanceCreationOptions {
    /// Prefer an unused verified library installation without copying its files.
    pub prefer_existing_install: bool,
    /// Transfer a fresh acquisition from the managed instance staging namespace
    /// into this instance. Never applies to the user's downloaded game library.
    pub take_program_ownership: bool,
    pub private_runtime: Option<PrivateRuntimeProjection>,
    /// Override a sharing-capable module when this instance needs its own program.
    pub program_mode: Option<crate::InstanceProgramMode>,
    /// Pin creation to this exact installed library, independent of later scans
    /// changing the ordering of the module's other installation records.
    pub program_install_root: Option<PathBuf>,
    pub source_generation: Option<String>,
    /// Verify this module's official package at the ownership boundary. A missing
    /// or changed package returns CleanLibraryProgramRequired before adoption.
    pub require_clean_program: bool,
    /// Explicitly import the selected local program, including its modifications.
    /// This never certifies those files as an official clean package.
    pub use_local_program: bool,
    /// Explicit cancellation stops preparation and removes its pending directory.
    /// After the database/filesystem transaction begins, it finishes atomically.
    pub cancellation: Option<Arc<AtomicBool>>,
}

pub(crate) fn prepare_private_runtime_projection(
    shared_root: &Path,
    instance_root: &Path,
    projection: &PrivateRuntimeProjection,
    source_generation: Option<&str>,
    cancellation: Option<&AtomicBool>,
) -> Result<PathBuf, StorageError> {
    check_creation_cancelled(cancellation)?;
    let private_directories = validate_private_directories(shared_root, projection)?;
    let runtime_root = instance_root.join("runtime");
    let staging_root = instance_root.join("runtime.staging");
    if runtime_root.exists() {
        return Err(StorageError::InvalidPrivateRuntimeProjection {
            path: runtime_root,
            message: String::from("runtime root already exists"),
        });
    }
    if staging_root.exists() {
        fs::remove_dir_all(&staging_root).map_err(|source| StorageError::DeletePath {
            path: staging_root.clone(),
            source,
        })?;
    }

    let result = (|| {
        let mut package = PackageTree::default();
        project_directory(
            shared_root,
            &staging_root,
            Path::new(""),
            &private_directories,
            &mut package,
            cancellation,
        )?;
        for relative in &private_directories {
            check_creation_cancelled(cancellation)?;
            create_directory(&staging_root.join(relative))?;
            // A missing nested private directory can also create package-owned parents.
            for parent in relative
                .ancestors()
                .skip(1)
                .take_while(|path| !path.as_os_str().is_empty())
            {
                package.created_directory(parent)?;
            }
        }
        let excluded_paths = private_directories
            .iter()
            .map(|relative| shared_root.join(relative))
            .collect::<Vec<_>>();
        crate::private_runtime_refresh::record_copied_package_baseline(
            shared_root,
            &staging_root,
            &excluded_paths,
            source_generation,
            false,
            package,
            cancellation,
        )?;
        record_projection_identity(shared_root, instance_root, &staging_root)?;
        check_creation_cancelled(cancellation)?;
        fs::write(staging_root.join(PRIVATE_RUNTIME_MARKER), b"managed\n").map_err(|source| {
            StorageError::WriteConfig {
                path: staging_root.join(PRIVATE_RUNTIME_MARKER),
                source,
            }
        })?;
        publish_creation_directory(&staging_root, &runtime_root, cancellation)?;
        Ok(runtime_root.clone())
    })();

    if result.is_err() && staging_root.exists() {
        let _ = fs::remove_dir_all(&staging_root);
    }
    result
}

fn validate_private_directories(
    shared_root: &Path,
    projection: &PrivateRuntimeProjection,
) -> Result<BTreeSet<PathBuf>, StorageError> {
    if !shared_root.is_dir() {
        return Err(StorageError::ReadDirectory {
            path: shared_root.to_path_buf(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "shared install root is missing",
            ),
        });
    }
    let mut validated = BTreeSet::new();
    for relative in &projection.private_directories {
        if relative.as_os_str().is_empty()
            || relative.is_absolute()
            || !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)))
        {
            return Err(StorageError::InvalidPrivateRuntimeProjection {
                path: relative.clone(),
                message: String::from("path must contain only relative normal components"),
            });
        }
        validated.insert(relative.clone());
    }
    if validated.is_empty() {
        return Err(StorageError::InvalidPrivateRuntimeProjection {
            path: shared_root.to_path_buf(),
            message: String::from("at least one private directory is required"),
        });
    }
    Ok(validated)
}

fn project_directory(
    source_root: &Path,
    destination_root: &Path,
    relative: &Path,
    private_directories: &BTreeSet<PathBuf>,
    package: &mut PackageTree,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    let source_directory = source_root.join(relative);
    create_directory(&destination_root.join(relative))?;
    for entry in fs::read_dir(&source_directory).map_err(|source| StorageError::ReadDirectory {
        path: source_directory.clone(),
        source,
    })? {
        check_creation_cancelled(cancellation)?;
        let entry = entry.map_err(|source| StorageError::ReadDirectory {
            path: source_directory.clone(),
            source,
        })?;
        let source_path = entry.path();
        let child_relative = relative.join(entry.file_name());
        let destination_path = destination_root.join(&child_relative);
        let file_type = entry.file_type().map_err(|source| StorageError::ReadPath {
            path: source_path.clone(),
            source,
        })?;
        if file_type.is_symlink() || is_reparse_point(&source_path)? {
            return Err(StorageError::UnsafeManagedPath {
                path: source_path,
                root: source_root.to_path_buf(),
            });
        }
        if is_private_path(&child_relative, private_directories) {
            create_directory(&destination_path)?;
        } else if file_type.is_dir() {
            project_directory(
                source_root,
                destination_root,
                &child_relative,
                private_directories,
                package,
                cancellation,
            )?;
            package.copied_directory(&child_relative)?;
        } else if file_type.is_file() {
            let digest = copy_creation_file(&source_path, &destination_path, cancellation)?;
            package.copied_file(&child_relative, digest)?;
        }
    }
    Ok(())
}

fn is_private_path(relative: &Path, private_directories: &BTreeSet<PathBuf>) -> bool {
    private_directories
        .iter()
        .any(|private| path_starts_with(relative, private))
}

#[cfg(windows)]
pub(crate) fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    let mut path = path.components();
    for expected in prefix.components() {
        let Some(actual) = path.next() else {
            return false;
        };
        if !actual
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&expected.as_os_str().to_string_lossy())
        {
            return false;
        }
    }
    true
}

#[cfg(not(windows))]
pub(crate) fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    path.starts_with(prefix)
}

fn create_directory(path: &Path) -> Result<(), StorageError> {
    fs::create_dir_all(path).map_err(|source| StorageError::CreatePath {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(windows)]
pub(crate) fn is_reparse_point(path: &Path) -> Result<bool, StorageError> {
    use std::os::windows::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path).map_err(|source| StorageError::ReadPath {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(metadata.file_attributes() & 0x400 != 0)
}

#[cfg(not(windows))]
pub(crate) fn is_reparse_point(_path: &Path) -> Result<bool, StorageError> {
    Ok(false)
}

#[cfg(test)]
#[path = "private_runtime/tests.rs"]
mod tests;
