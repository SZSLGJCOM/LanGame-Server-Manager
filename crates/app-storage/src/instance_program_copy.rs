use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::StorageError;
use crate::instance_creation_io::{
    check_creation_cancelled, copy_independent_program_file, copy_verified_creation_file,
    publish_creation_directory,
};
use crate::private_runtime::{PRIVATE_RUNTIME_MARKER, is_reparse_point, path_starts_with};
use crate::private_runtime_refresh::{
    PackageTree, excluded_package_path, relative_exclusions, relative_key,
};
pub(crate) fn prepare_private_runtime_root(
    shared_root: &Path,
    instance_root: &Path,
    excluded_paths: &[PathBuf],
    source_generation: Option<&str>,
    exclude_dst_workshop_mods: bool,
    selection: crate::program_runtime::ProgramFileSelection<'_>,
    cancellation: Option<&AtomicBool>,
) -> Result<PathBuf, StorageError> {
    check_creation_cancelled(cancellation)?;
    let mut copy_plan = crate::program_adoption::clean_copy_exclusions(
        shared_root,
        excluded_paths,
        selection,
        cancellation,
    )?;
    let excluded_paths = copy_plan.exclusions.as_slice();
    if excluded_paths
        .iter()
        .any(|path| path_starts_with(shared_root, path))
    {
        return Err(StorageError::InvalidPrivateRuntimeProjection {
            path: shared_root.to_owned(),
            message: String::from(
                "retained instance saves cover the whole shared package; a clean private runtime cannot be prepared",
            ),
        });
    }
    let runtime_root = instance_root.join("runtime");
    let staging_root = instance_root.join("runtime.staging");
    if staging_root.exists() {
        fs::remove_dir_all(&staging_root).map_err(|source| StorageError::DeletePath {
            path: staging_root.clone(),
            source,
        })?;
    }
    let expected =
        copy_plan
            .inventory
            .as_mut()
            .map(|inventory| {
                let excluded = relative_exclusions(shared_root, excluded_paths)?;
                inventory.package.files.retain(|key, _| {
                    !excluded_package_path(key, &excluded, exclude_dst_workshop_mods)
                });
                inventory.package.directories.retain(|key| {
                    !excluded_package_path(key, &excluded, exclude_dst_workshop_mods)
                });
                Ok::<_, StorageError>(case_normalized_package(&inventory.package))
            })
            .transpose()?;
    let mut context = RuntimeCopyContext {
        excluded_paths,
        expected: expected.as_ref(),
        package: expected.as_ref().map(|_| PackageTree::default()),
        copied_paths: Vec::new(),
        cancellation,
    };
    copy_runtime_directory(shared_root, &staging_root, Path::new(""), &mut context)?;
    let RuntimeCopyContext {
        package,
        copied_paths,
        ..
    } = context;
    if let (Some(package), Some(expected), Some(mut inventory)) =
        (package, expected, copy_plan.inventory)
    {
        let copied = case_normalized_package(&package);
        if copied.files != expected.files || copied.directories != expected.directories {
            return Err(StorageError::CleanLibraryProgramRequired {
                path: shared_root.to_owned(),
            });
        }
        inventory.package = package.clone();
        crate::private_runtime_refresh::record_copied_package_baseline(
            shared_root,
            &staging_root,
            excluded_paths,
            source_generation,
            exclude_dst_workshop_mods,
            package,
            cancellation,
        )?;
        crate::program_seed::write_clean_package_copy_inventory(&staging_root, &inventory)?;
    } else {
        #[cfg(windows)]
        crate::private_runtime_refresh::validate_case_unique_path_keys(
            copied_paths.iter(),
            &staging_root,
        )?;
    }
    check_creation_cancelled(cancellation)?;
    fs::write(staging_root.join(PRIVATE_RUNTIME_MARKER), b"managed\n").map_err(|source| {
        StorageError::WriteConfig {
            path: staging_root.join(PRIVATE_RUNTIME_MARKER),
            source,
        }
    })?;
    publish_creation_directory(&staging_root, &runtime_root, cancellation)?;
    Ok(runtime_root)
}

pub(super) struct RuntimeCopyContext<'a> {
    pub(super) excluded_paths: &'a [PathBuf],
    pub(super) expected: Option<&'a PackageTree>,
    pub(super) package: Option<PackageTree>,
    pub(super) copied_paths: Vec<String>,
    pub(super) cancellation: Option<&'a AtomicBool>,
}

pub(super) fn copy_runtime_directory(
    source: &Path,
    destination: &Path,
    relative: &Path,
    context: &mut RuntimeCopyContext<'_>,
) -> Result<(), StorageError> {
    check_creation_cancelled(context.cancellation)?;
    fs::create_dir_all(destination).map_err(|source_error| StorageError::CreatePath {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    for entry in fs::read_dir(source).map_err(|source_error| StorageError::ReadDirectory {
        path: source.to_path_buf(),
        source: source_error,
    })? {
        check_creation_cancelled(context.cancellation)?;
        let entry = entry.map_err(|source_error| StorageError::ReadDirectory {
            path: source.to_path_buf(),
            source: source_error,
        })?;
        let source_path = entry.path();
        // A new instance copies the installed program, never an existing
        // instance's world, including saves nested under its own instance ID.
        if context
            .excluded_paths
            .iter()
            .any(|path| path_starts_with(&source_path, path))
        {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        let child_relative = relative.join(entry.file_name());
        let key = copy_path_key(&relative_key(&child_relative)?);
        if let Some(expected) = context.expected
            && !expected.files.contains_key(&key)
            && !expected.directories.contains(&key)
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|source_error| StorageError::ReadPath {
                path: source_path.clone(),
                source: source_error,
            })?;
        if file_type.is_symlink() || is_reparse_point(&source_path)? {
            return Err(StorageError::UnsafeManagedPath {
                path: source_path,
                root: source.to_path_buf(),
            });
        }
        if file_type.is_dir() {
            if context
                .expected
                .is_some_and(|package| !package.directories.contains(&key))
            {
                return Err(StorageError::CleanLibraryProgramRequired { path: source_path });
            }
            copy_runtime_directory(&source_path, &destination_path, &child_relative, context)?;
            if let Some(package) = &mut context.package {
                package.copied_directory(&child_relative)?;
            } else {
                context
                    .copied_paths
                    .push(crate::private_runtime_refresh::relative_key(
                        &child_relative,
                    )?);
            }
        } else if file_type.is_file() {
            if let Some(package) = &mut context.package {
                if context
                    .expected
                    .is_some_and(|expected| !expected.files.contains_key(&key))
                {
                    return Err(StorageError::CleanLibraryProgramRequired { path: source_path });
                }
                let digest = copy_verified_creation_file(
                    &source_path,
                    &destination_path,
                    context.cancellation,
                )?;
                if context
                    .expected
                    .is_some_and(|expected| expected.files.get(&key) != Some(&digest))
                {
                    return Err(StorageError::CleanLibraryProgramRequired { path: source_path });
                }
                package.copied_file(&child_relative, digest)?;
            } else {
                copy_independent_program_file(
                    &source_path,
                    &destination_path,
                    context.cancellation,
                )?;
                context
                    .copied_paths
                    .push(crate::private_runtime_refresh::relative_key(
                        &child_relative,
                    )?);
            }
        }
    }
    Ok(())
}

fn copy_path_key(key: &str) -> String {
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key.to_owned()
    }
}

fn case_normalized_package(package: &PackageTree) -> PackageTree {
    PackageTree {
        files: package
            .files
            .iter()
            .map(|(key, digest)| (copy_path_key(key), digest.clone()))
            .collect(),
        directories: package
            .directories
            .iter()
            .map(|key| copy_path_key(key))
            .collect(),
    }
}
