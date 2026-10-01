use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use app_core::InstallState;
use app_modules::ModuleDescriptor;
use sqlx::Row;

use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::private_runtime_refresh::{hash_file, validated_relative_path};
use crate::{ProgramInstallRecord, ProgramInstallScope, StorageError, StoragePaths};

const MAX_INSTALLATIONS: usize = 4_096;
const MAX_TREE_ENTRIES: usize = 200_000;
const MAX_TREE_DEPTH: usize = 64;
const MAX_METADATA_BYTES: u64 = 65_536;

#[derive(Debug)]
pub struct LibraryCleanupPlan {
    pub installations: Vec<ProgramInstallRecord>,
    pub keep_install_id: Option<i64>,
}

/// Read-only selection, not deletion authority. The caller holds the module's
/// lifecycle lease and rechecks instance/archive dependencies before removal.
pub async fn plan_module_library_cleanup(
    paths: &StoragePaths,
    descriptor: &ModuleDescriptor,
    keep_base: bool,
) -> Result<LibraryCleanupPlan, StorageError> {
    let pool = crate::storage_db::connect_pool(paths).await?;
    let result = async {
        let rows = sqlx::query(
            "SELECT id,module_id,install_root,install_state,current_version,owner_instance_id
             FROM game_installs WHERE module_id=?1 AND scope='library' ORDER BY id LIMIT 4097",
        )
        .bind(&descriptor.summary.id)
        .fetch_all(&pool)
        .await?;
        if rows.len() > MAX_INSTALLATIONS {
            return Err(invalid(
                &paths.games_root,
                "library cleanup inventory exceeds 4096 installations",
            ));
        }
        rows.into_iter()
            .map(|row| {
                let root = PathBuf::from(row.get::<String, _>("install_root"));
                if row.get::<Option<String>, _>("owner_instance_id").is_some() {
                    return Err(invalid(
                        &root,
                        "library installation unexpectedly has an instance owner",
                    ));
                }
                Ok(ProgramInstallRecord {
                    id: row.get("id"),
                    module_id: row.get("module_id"),
                    install_root: root,
                    install_state: crate::storage_db::install_state_from_db_value(Some(
                        &row.get::<String, _>("install_state"),
                    )),
                    current_version: row.get("current_version"),
                    scope: ProgramInstallScope::Library,
                    owner_instance_id: None,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()
    }
    .await;
    pool.close().await;
    let installations = result?;
    if !keep_base || installations.is_empty() {
        return Ok(LibraryCleanupPlan {
            installations,
            keep_install_id: None,
        });
    }
    let relative = descriptor
        .install
        .as_ref()
        .map(|install| install.shared_game_dir.as_str())
        .unwrap_or(&descriptor.summary.id);
    let default_root = paths
        .games_root
        .join(validated_relative_path(&relative.replace('\\', "/"))?);
    let descriptor = descriptor.clone();
    tokio::task::spawn_blocking(move || {
        let default_root = normalize_resource_path(&default_root)?;
        let mut candidates = Vec::new();
        let mut fallback = None;
        let mut default_id = None;
        for installation in &installations {
            if installation.install_state != InstallState::Installed {
                continue;
            }
            let root = normalize_resource_path(&installation.install_root)?;
            match fs::symlink_metadata(&root) {
                Ok(metadata) if metadata.is_dir() => {
                    fallback.get_or_insert(installation.id);
                    if contains(&root, &default_root) && contains(&default_root, &root) {
                        default_id = Some(installation.id);
                    }
                    let retained =
                        crate::program_library_retention::retained_library_program_source(
                            &root,
                            &descriptor.summary.id,
                        )?;
                    candidates.push((installation.id, root, retained));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(StorageError::ReadPath { path: root, source }),
            }
        }
        // The sole installed candidate is retained even when its payload is
        // modified. Reading every program byte cannot change that decision.
        if let [(id, _, _)] = candidates.as_slice() {
            return Ok(LibraryCleanupPlan {
                installations,
                keep_install_id: Some(*id),
            });
        }
        // Retention expresses a role, not health. Never discard the remaining
        // good package merely because a newer retained source has been modified.
        candidates.sort_by(|left, right| {
            let priority = |candidate: &(i64, PathBuf, bool)| {
                if candidate.2 {
                    0
                } else if Some(candidate.0) == default_id {
                    1
                } else {
                    2
                }
            };
            priority(left).cmp(&priority(right)).then_with(|| {
                if left.2 {
                    right.0.cmp(&left.0)
                } else {
                    left.0.cmp(&right.0)
                }
            })
        });
        let mut keep_install_id = None;
        for (id, root, _) in candidates {
            if crate::library_program_is_pristine(&root, &descriptor, None)? {
                keep_install_id = Some(id);
                break;
            }
        }
        let keep_install_id = keep_install_id.or(default_id).or(fallback);
        Ok(LibraryCleanupPlan {
            installations,
            keep_install_id,
        })
    })
    .await
    .map_err(|error| StorageError::BlockingTaskFailed {
        operation: "selecting a retained base library",
        message: error.to_string(),
    })?
}

/// Protect every byte not proven to be original program content. None means
/// there is no trustworthy allowlist and automatic cleanup must leave the root.
/// Call in a blocking worker under the lifecycle lease; this performs no writes.
pub fn library_cleanup_retained_paths(
    root: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<Option<Vec<PathBuf>>, StorageError> {
    let root = normalize_path(root)?;
    let Some(package) = crate::program_seed::read_clean_package_inventory(&root, descriptor)?
    else {
        return Ok(None);
    };
    // Inspect all subtrees first, including unknown and excluded data. A link
    // hidden in a retained directory must not bypass the removal preflight.
    let entries = inspect_tree(&root, MAX_TREE_ENTRIES)?;
    let mut retained_keys = BTreeSet::new();
    let mut retained = Vec::new();
    for entry in entries {
        let key = entry.key.to_string_lossy().replace('\\', "/");
        if has_retained_ancestor(&entry.key, &retained_keys) {
            continue;
        }
        let original = if entry.directory {
            package.directories.contains(&key)
        } else if let Some(expected) = package.files.get(&key) {
            normalize_resource_path(&entry.path)?;
            hash_file(&entry.path, None)? == *expected
        } else {
            removable_metadata(&root, &entry.key, descriptor)?
        };
        if !original {
            retained_keys.insert(entry.key);
            retained.push(entry.path);
        }
    }
    Ok(Some(retained))
}

struct TreeEntry {
    path: PathBuf,
    key: PathBuf,
    directory: bool,
}

fn inspect_tree(root: &Path, limit: usize) -> Result<Vec<TreeEntry>, StorageError> {
    let mut entries = Vec::new();
    let mut pending = vec![(root.to_owned(), 0usize)];
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_TREE_DEPTH {
            return Err(invalid(
                &directory,
                "library cleanup tree exceeds 64 directory levels",
            ));
        }
        normalize_path(&directory)?;
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadDirectory {
            path: directory.clone(),
            source,
        })? {
            let entry = entry.map_err(|source| StorageError::ReadDirectory {
                path: directory.clone(),
                source,
            })?;
            visited += 1;
            if visited > limit {
                return Err(invalid(
                    root,
                    "library cleanup tree exceeds its entry limit",
                ));
            }
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
            if metadata.file_type().is_symlink() || crate::private_runtime::is_reparse_point(&path)?
            {
                return Err(StorageError::UnsafeManagedPath {
                    path,
                    root: root.to_owned(),
                });
            }
            if !metadata.is_file() && !metadata.is_dir() {
                return Err(invalid(
                    &path,
                    "unsupported file type in library cleanup tree",
                ));
            }
            let key = path
                .strip_prefix(root)
                .map_err(|_| invalid(&path, "cleanup entry escapes its installation"))?
                .to_owned();
            if metadata.is_dir() {
                pending.push((path.clone(), depth + 1));
            }
            entries.push(TreeEntry {
                path,
                key,
                directory: metadata.is_dir(),
            });
        }
    }
    // Parents must be classified before children so a protected subtree is
    // represented by one path, without hashing its personal files.
    entries.sort_by_key(|entry| entry.key.components().count());
    Ok(entries)
}

fn has_retained_ancestor(path: &Path, retained: &BTreeSet<PathBuf>) -> bool {
    path.ancestors()
        .skip(1)
        .any(|parent| retained.contains(parent))
}

fn removable_metadata(
    root: &Path,
    relative: &Path,
    descriptor: &ModuleDescriptor,
) -> Result<bool, StorageError> {
    if relative.components().count() != 1 {
        return Ok(false);
    }
    let Some(name) = relative.to_str() else {
        return Ok(false);
    };
    match name {
        ".langame-clean-package.json" => return Ok(true),
        ".langame-initial-package.json" => {
            return Ok(
                crate::program_seed::read_package_inventory(root, &descriptor.summary.id)?
                    .is_some(),
            );
        }
        ".langame-program-acquisition.json" => {
            return crate::program_seed::library_program_acquisition_is_trusted(root, descriptor);
        }
        crate::program_library_retention::RETAINED_LIBRARY => {
            return crate::program_library_retention::retained_library_program_source(
                root,
                &descriptor.summary.id,
            );
        }
        ".langame-program-identity.json"
        | ".langame-program-usage.json"
        | ".langame-exclusive-program.json" => {}
        _ => return Ok(false),
    }
    let path = normalize_resource_path(&root.join(relative))?;
    let file = fs::File::open(&path).map_err(|source| StorageError::ReadPath {
        path: path.clone(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| StorageError::ReadPath { path, source })?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Ok(false);
    }
    let Ok(serde_json::Value::Object(record)) = serde_json::from_slice(&bytes) else {
        return Ok(false);
    };
    if record.get("version").and_then(serde_json::Value::as_u64) != Some(1)
        || record.get("module_id").and_then(serde_json::Value::as_str)
            != Some(descriptor.summary.id.as_str())
    {
        return Ok(false);
    }
    let fields: &[&str] = match name {
        ".langame-program-identity.json" => &["id"],
        ".langame-program-usage.json" => &["program_id", "instance_id"],
        _ => &["program_id", "program_root"],
    };
    Ok(record.len() == fields.len() + 2
        && fields.iter().all(|field| {
            record
                .get(*field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
        }))
}

fn invalid(path: &Path, message: &str) -> StorageError {
    StorageError::InvalidInstancePath {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "program_library_cleanup_tests.rs"]
mod tests;
