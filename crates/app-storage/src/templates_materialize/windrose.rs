use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use thiserror::Error;

use crate::StorageError;
#[cfg(test)]
use crate::atomic_file::compare_and_swap_file_atomically;

use super::super::{ModuleSupportMaterializationContext, WINDROSE_SERVER_DESCRIPTION_FILE};

#[path = "windrose_document.rs"]
mod windrose_document;
use windrose_document::*;
#[path = "windrose_plan.rs"]
mod windrose_plan;
use windrose_plan::*;
#[path = "windrose_updater.rs"]
mod windrose_updater;
#[cfg(test)]
use windrose_updater::*;

const WORLD_DESCRIPTION_FILE: &str = "WorldDescription.json";
const WORLD_DATABASE_RELATIVE_ROOT: &[&str] =
    &["R5", "Saved", "SaveProfiles", "Default", "RocksDB_v2"];

pub(crate) async fn apply_pending_world_update(
    install_root: &Path,
    config_dir: &Path,
    settings: &Map<String, Value>,
) -> Result<(), WindroseWorldTargetError> {
    windrose_updater::apply_pending_world_update(install_root, config_dir, settings).await
}

#[derive(Debug, Error)]
pub(crate) enum WindroseWorldTargetError {
    #[error("select an existing Windrose world before editing world parameters")]
    EmptySelection,
    #[error("Windrose world selection is not a single safe folder name: {selection:?}")]
    UnsafeSelection { selection: String },
    #[error("no exact Windrose world matches islandId {selection:?}")]
    NoMatch { selection: String },
    #[error("Windrose world islandId {selection:?} is ambiguous across {matches} version folders")]
    Ambiguous { selection: String, matches: usize },
    #[error("failed to inspect Windrose path {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Windrose JSON at {path} is malformed: {message}")]
    MalformedJson { path: PathBuf, message: String },
    #[error("Windrose JSON at {path} has an invalid {field} shape")]
    InvalidShape { path: PathBuf, field: String },
    #[error(
        "Windrose world identity mismatch for {evidence}: expected {expected:?}, got {actual:?}"
    )]
    IdentityMismatch {
        evidence: &'static str,
        expected: String,
        actual: String,
    },
    #[error("Windrose target {path} resolves outside approved install root {root}")]
    OutsideInstallRoot { path: PathBuf, root: PathBuf },
    #[error("Windrose setting {key} is missing or has the wrong type")]
    InvalidSetting { key: &'static str },
    #[error("stop the Windrose instance before changing per-world parameters")]
    RunningWorldMutation,
    #[error("Windrose pending world update plan at {path} is invalid: {message}")]
    InvalidPendingPlan { path: PathBuf, message: String },
    #[error("Windrose world updater is missing at {path}")]
    MissingUpdater { path: PathBuf },
    #[error("failed to launch Windrose world updater at {path}: {source}")]
    UpdaterLaunch {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Windrose world updater exited unsuccessfully with code {code:?}")]
    UpdaterFailed { code: Option<i32> },
    #[error("Windrose world updater timed out after {timeout:?}")]
    UpdaterTimeout { timeout: std::time::Duration },
    #[error("Windrose world updater termination could not be confirmed at {path}: {message}")]
    UpdaterTermination { path: PathBuf, message: String },
    #[error("Windrose target changed while it was being materialized: {path}")]
    ConcurrentModification { path: PathBuf },
    #[error("failed to atomically replace Windrose target {path}: {source}")]
    Replacement {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to roll back Windrose native target {path} after updater failure")]
    RollbackFailed { path: PathBuf },
}

#[derive(Debug)]
pub(crate) struct ResolvedWindroseWorld {
    pub(crate) path: PathBuf,
    pub(super) original: Vec<u8>,
    pub(super) document: Value,
}

pub(crate) fn resolve_world_description(
    install_root: &Path,
    selected_world_island_id: &str,
) -> Result<ResolvedWindroseWorld, WindroseWorldTargetError> {
    let selection = validate_selection(selected_world_island_id)?;
    let canonical_install = canonicalize(install_root)?;
    let database_root = WORLD_DATABASE_RELATIVE_ROOT
        .iter()
        .fold(install_root.to_path_buf(), |path, segment| {
            path.join(segment)
        });
    if !database_root.is_dir() {
        return Err(WindroseWorldTargetError::NoMatch { selection });
    }
    let canonical_database = canonicalize(&database_root)?;
    ensure_within_root(&canonical_install, &canonical_database)?;

    let mut candidates = Vec::new();
    for entry in read_directory(&database_root)? {
        let version_path = entry
            .map_err(|source| WindroseWorldTargetError::Io {
                path: database_root.clone(),
                source,
            })?
            .path();
        if !version_path.is_dir() {
            continue;
        }
        let canonical_version = canonicalize(&version_path)?;
        ensure_within_root(&canonical_database, &canonical_version)?;
        let worlds_root = version_path.join("Worlds");
        if !worlds_root.is_dir() {
            continue;
        }
        let canonical_worlds = canonicalize(&worlds_root)?;
        ensure_within_root(&canonical_database, &canonical_worlds)?;
        let candidate = worlds_root.join(&selection).join(WORLD_DESCRIPTION_FILE);
        if !candidate.is_file() {
            continue;
        }
        let canonical_candidate = canonicalize(&candidate)?;
        ensure_within_root(&canonical_install, &canonical_candidate)?;
        ensure_within_root(&canonical_worlds, &canonical_candidate)?;
        let folder_id = canonical_candidate
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if folder_id != selection {
            return Err(WindroseWorldTargetError::IdentityMismatch {
                evidence: "world folder name",
                expected: selection,
                actual: String::from(folder_id),
            });
        }
        candidates.push(canonical_candidate);
    }

    candidates.sort();
    candidates.dedup();
    if candidates.is_empty() {
        return Err(WindroseWorldTargetError::NoMatch { selection });
    }
    if candidates.len() != 1 {
        return Err(WindroseWorldTargetError::Ambiguous {
            selection,
            matches: candidates.len(),
        });
    }

    let path = candidates.remove(0);
    let original = read_bytes(&path)?;
    let document = parse_json(&path, &original)?;
    let world_id = required_string_at(&document, &["WorldDescription", "islandId"], &path)?;
    require_identity("WorldDescription islandId", &selection, world_id)?;

    let server_candidate = canonical_install
        .join("R5")
        .join(WINDROSE_SERVER_DESCRIPTION_FILE);
    match fs::canonicalize(&server_candidate) {
        Ok(server_path) => {
            ensure_within_root(&canonical_install, &server_path)?;
            let server_original = read_bytes(&server_path)?;
            let server_document = parse_json(&server_path, &server_original)?;
            let server_world_id = required_string_at(
                &server_document,
                &["ServerDescription_Persistent", "WorldIslandId"],
                &server_path,
            )?;
            require_identity(
                "ServerDescription WorldIslandId",
                &selection,
                server_world_id,
            )?;
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(WindroseWorldTargetError::Io {
                path: server_candidate,
                source,
            });
        }
    }

    Ok(ResolvedWindroseWorld {
        path,
        original,
        document,
    })
}

#[cfg(test)]
fn patch_world_description(
    resolved_world: ResolvedWindroseWorld,
    settings: &Map<String, Value>,
) -> Result<(), WindroseWorldTargetError> {
    patch_world_description_with(resolved_world, settings, |path, expected, replacement| {
        compare_and_swap_file_atomically(path, expected, replacement)
    })
}

#[cfg(test)]
fn patch_world_description_with<F>(
    resolved_world: ResolvedWindroseWorld,
    settings: &Map<String, Value>,
    replace: F,
) -> Result<(), WindroseWorldTargetError>
where
    F: FnOnce(&Path, &[u8], &[u8]) -> io::Result<bool>,
{
    let replacement = render_world_description(&resolved_world, settings)?;
    replace(&resolved_world.path, &resolved_world.original, &replacement)
        .map_err(|source| WindroseWorldTargetError::Replacement {
            path: resolved_world.path.clone(),
            source,
        })?
        .then_some(())
        .ok_or(WindroseWorldTargetError::ConcurrentModification {
            path: resolved_world.path,
        })
}

pub(super) fn materialize_windrose_support_files(
    context: &ModuleSupportMaterializationContext<'_>,
    files: &mut super::ManagedConfigMutation,
) -> Result<(), StorageError> {
    if !context.install_root.exists() {
        return Ok(());
    }
    materialize_windrose_documents_pending(
        context.install_root,
        context.config_dir,
        context.settings,
        context.instance_running,
        files,
    )
    .map_err(|error| StorageError::ModuleSupportMaterialization {
        module_id: "windrose".to_owned(),
        path: error_path(&error, context.install_root),
        message: error.to_string(),
    })?;
    super::ue4ss_player_query::materialize(
        super::ue4ss_player_query::Game::Windrose,
        context.install_root,
        context.instance_running,
        files,
    )
}

fn validate_selection(value: &str) -> Result<String, WindroseWorldTargetError> {
    if value.trim().is_empty() {
        return Err(WindroseWorldTargetError::EmptySelection);
    }
    if value != value.trim() || value.contains(['/', '\\', '\0']) {
        return Err(WindroseWorldTargetError::UnsafeSelection {
            selection: String::from(value),
        });
    }
    let components = Path::new(value).components().collect::<Vec<_>>();
    if components.len() != 1 || !matches!(components[0], Component::Normal(_)) {
        return Err(WindroseWorldTargetError::UnsafeSelection {
            selection: String::from(value),
        });
    }
    Ok(String::from(value))
}

fn read_directory(path: &Path) -> Result<fs::ReadDir, WindroseWorldTargetError> {
    fs::read_dir(path).map_err(|source| WindroseWorldTargetError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn canonicalize(path: &Path) -> Result<PathBuf, WindroseWorldTargetError> {
    fs::canonicalize(path).map_err(|source| WindroseWorldTargetError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn ensure_within_root(root: &Path, path: &Path) -> Result<(), WindroseWorldTargetError> {
    if path == root || path.starts_with(root) {
        Ok(())
    } else {
        Err(WindroseWorldTargetError::OutsideInstallRoot {
            path: path.to_path_buf(),
            root: root.to_path_buf(),
        })
    }
}

fn validate_destination(install_root: &Path, path: &Path) -> Result<(), WindroseWorldTargetError> {
    let canonical_install = canonicalize(install_root)?;
    match fs::canonicalize(path) {
        Ok(canonical_path) => {
            return ensure_within_root(&canonical_install, &canonical_path);
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(WindroseWorldTargetError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| WindroseWorldTargetError::InvalidShape {
            path: path.to_path_buf(),
            field: String::from("destination parent"),
        })?;
    if parent.exists() {
        ensure_within_root(&canonical_install, &canonicalize(parent)?)
    } else if path.starts_with(install_root) {
        Ok(())
    } else {
        Err(WindroseWorldTargetError::OutsideInstallRoot {
            path: path.to_path_buf(),
            root: canonical_install,
        })
    }
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, WindroseWorldTargetError> {
    fs::read(path).map_err(|source| WindroseWorldTargetError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
#[path = "windrose_bootstrap_tests.rs"]
mod windrose_bootstrap_tests;
#[cfg(test)]
#[path = "windrose_tests.rs"]
mod windrose_tests;
#[cfg(test)]
#[path = "windrose_updater_tests.rs"]
mod windrose_updater_tests;
