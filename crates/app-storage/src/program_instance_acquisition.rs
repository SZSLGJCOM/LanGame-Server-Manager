use std::path::{Path, PathBuf};

use crate::instance_isolation::paths::{contains, normalize_path};
use crate::program_runtime::invalid;
use crate::{StorageError, StoragePaths};

pub(crate) fn module_root(paths: &StoragePaths, module_id: &str) -> Result<PathBuf, StorageError> {
    if module_id.is_empty()
        || !module_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid(
            &paths.instances_root,
            "invalid acquisition module identifier",
        ));
    }
    normalize_path(
        &paths
            .instances_root
            .join(".langame")
            .join("program-acquisitions")
            .join(module_id),
    )
}

/// Place temporary instance acquisitions on the instance volume so ownership
/// can be published by directory rename without retaining a second full copy.
pub fn new_instance_program_acquisition(
    paths: &StoragePaths,
    module_id: &str,
) -> Result<PathBuf, StorageError> {
    Ok(module_root(paths, module_id)?.join(uuid::Uuid::new_v4().to_string()))
}

pub fn is_instance_program_acquisition(
    paths: &StoragePaths,
    module_id: &str,
    root: &Path,
) -> Result<bool, StorageError> {
    let namespace = module_root(paths, module_id)?;
    let root = normalize_path(root)?;
    Ok(root
        .parent()
        .is_some_and(|parent| contains(parent, &namespace) && contains(&namespace, parent))
        && root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok()))
}
