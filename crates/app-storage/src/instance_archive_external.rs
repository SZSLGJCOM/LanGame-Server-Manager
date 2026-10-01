use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::instance_archive_files as files;
use crate::instance_archive_store::invalid;
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::private_runtime_refresh::validated_relative_path;
use crate::{StorageError, StoragePaths};

#[path = "instance_archive_external_io.rs"]
mod io;
pub(super) use io::verify;
pub(crate) use io::{
    capture_files, cleanup_owned, finish_restore, inspect, prepare_payload, prepare_restore,
    restore_files,
};
#[path = "instance_archive_dependencies.rs"]
mod dependencies;
pub use dependencies::ensure_program_archive_dependencies;
pub(crate) use dependencies::program_has_archive_reservation;

pub(crate) const PAYLOAD: &str = ".langame-archived-program";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalFile {
    pub sha256: String,
    pub bytes: u64,
    pub stored: bool,
    pub owned: bool,
}

/// A durable allowlist, not a claim that the installation is pristine. Only
/// declared instance data is removed from the retained installation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalProgramPlan {
    pub root: PathBuf,
    pub identity: String,
    pub parent_identity: String,
    pub files: BTreeMap<String, ExternalFile>,
    pub directories: BTreeSet<String>,
    pub fingerprint: String,
    pub payload_identity: Option<String>,
    pub exclusive: bool,
    pub complete_inventory: bool,
    pub restore_token: String,
    pub restore_identity: Option<String>,
    pub binding_sha256: String,
    pub binding_bytes: u64,
}

impl ExternalProgramPlan {
    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        if !self.root.is_absolute()
            || self.files.len() + self.directories.len() > 200_000
            || uuid::Uuid::parse_str(&self.restore_token).is_err()
        {
            return Err(invalid(
                &self.root,
                "Invalid external archive program inventory.",
            ));
        }
        let mut folded = BTreeSet::new();
        let mut bytes = 0u64;
        for (relative, file) in &self.files {
            validated_relative_path(relative)?;
            if !folded.insert(relative.to_ascii_lowercase())
                || file.sha256.len() != 64
                || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || (file.owned && !file.stored)
            {
                return Err(invalid(
                    &self.root,
                    "Invalid external archive file identity.",
                ));
            }
            bytes = bytes
                .checked_add(file.bytes)
                .ok_or_else(|| invalid(&self.root, "Archive size overflow."))?;
        }
        for relative in &self.directories {
            validated_relative_path(relative)?;
        }
        if self.fingerprint
            != crate::instance_archive_store::digest(&serde_json::to_vec(&(
                &self.files,
                &self.directories,
            ))?)
        {
            return Err(invalid(
                &self.root,
                "External archive inventory fingerprint is invalid.",
            ));
        }
        Ok(())
    }

    pub(crate) fn requires_source(&self) -> bool {
        self.files.values().any(|file| !file.stored)
    }
    pub(crate) fn omitted_bytes(&self) -> u64 {
        self.files
            .values()
            .filter(|file| !file.stored)
            .map(|file| file.bytes)
            .sum()
    }
    pub(crate) fn omitted_files(&self) -> usize {
        self.files.values().filter(|file| !file.stored).count()
    }
}

pub(crate) fn owned_paths(
    module: &str,
    id: &str,
    root: &Path,
    saves: &Path,
) -> Result<Vec<PathBuf>, StorageError> {
    let mut candidates = crate::instance_isolation::native::configuration_paths(module, root, id);
    let root = normalize_path(root)?;
    let saves = normalize_path(saves)?;
    if contains(&root, &saves) && saves != root {
        candidates.push(saves);
    }
    let mut owned = Vec::new();
    for candidate in candidates {
        let candidate = normalize_resource_path(&candidate)?;
        if candidate == root || !contains(&root, &candidate) {
            return Err(invalid(
                &candidate,
                "Declared instance data escapes the installation.",
            ));
        }
        owned.push(candidate);
    }
    owned.sort();
    owned.dedup();
    Ok(owned)
}

pub(crate) enum CaptureMode {
    Archive { source_idle: bool },
    Delete,
}

pub(crate) fn plan(
    instance: &Path,
    root: &Path,
    saves: &Path,
    module: &str,
    id: &str,
    exclusive: bool,
    mode: CaptureMode,
) -> Result<ExternalProgramPlan, StorageError> {
    if std::fs::symlink_metadata(instance.join(PAYLOAD)).is_ok() {
        return Err(invalid(
            instance,
            "An unrecognized archive payload directory already exists; its files were retained.",
        ));
    }
    let root = normalize_path(root)?;
    let identity = files::identity(&root)?
        .ok_or_else(|| invalid(&root, "Program installation is missing."))?;
    let parent = root
        .parent()
        .ok_or_else(|| invalid(&root, "Program directory has no parent."))?;
    let parent_identity =
        files::identity(parent)?.ok_or_else(|| invalid(parent, "Program parent is missing."))?;
    let ownership = if exclusive {
        owned_paths(module, id, &root, saves)?
    } else {
        Vec::new()
    };
    // Inventory is only used to choose individually verified omissions. A
    // missing or invalid manifest falls back to capturing the actual bytes.
    let baseline = if matches!(mode, CaptureMode::Archive { source_idle: true }) {
        crate::program_seed::read_package_inventory(&root, module)
            .ok()
            .flatten()
    } else {
        None
    };
    let archival = matches!(mode, CaptureMode::Archive { .. });
    let (actual, directories) = if archival {
        io::inventory(&root)?
    } else {
        io::owned_inventory(&root, &ownership)?
    };
    let mut entries = BTreeMap::new();
    for (relative, (sha256, bytes)) in actual {
        let path = root.join(&relative);
        let owned = ownership.iter().any(|owner| contains(owner, &path));
        let official =
            baseline.as_ref().and_then(|tree| tree.files.get(&relative)) == Some(&sha256);
        entries.insert(
            relative,
            ExternalFile {
                sha256,
                bytes,
                stored: owned || !official,
                owned,
            },
        );
    }
    let fingerprint =
        crate::instance_archive_store::digest(&serde_json::to_vec(&(&entries, &directories))?);
    let binding = instance.join("runtime").join(if exclusive {
        ".langame-exclusive-program.json"
    } else {
        ".langame-shared-program.json"
    });
    let (binding_sha256, binding_bytes) = io::digest_path(&binding)?;
    let plan = ExternalProgramPlan {
        root,
        identity,
        parent_identity,
        files: entries,
        directories,
        fingerprint,
        payload_identity: None,
        exclusive,
        complete_inventory: archival,
        restore_token: uuid::Uuid::new_v4().to_string(),
        restore_identity: None,
        binding_sha256,
        binding_bytes,
    };
    plan.validate()?;
    Ok(plan)
}

#[derive(Clone, Debug, Serialize)]
pub struct InstanceRemovalPlan {
    pub program_path: String,
    pub data_path: String,
    pub remove_program: bool,
    pub preserved_program_path: Option<String>,
    pub owned_data_paths: Vec<String>,
    pub preserved_external_saves_path: Option<String>,
}

pub async fn inspect_instance_removal(
    paths: &StoragePaths,
    id: &str,
) -> Result<InstanceRemovalPlan, StorageError> {
    let pool = crate::storage_db::connect_pool(paths).await?;
    let result = async {
        let mut connection = pool.acquire().await?;
        let record = crate::storage_db::fetch_instance_record(&mut *connection, id).await?;
        let root = record
            .config_dir
            .parent()
            .ok_or_else(|| invalid(&record.config_dir, "Instance root is missing."))?;
        let retirement = crate::instance_retirement_paths::resolve(paths, &record).await?;
        let program = retirement.program;
        let saves = retirement.saves;
        let library = retirement.library;
        let exclusive = retirement.exclusive;
        let owned = if exclusive {
            owned_paths(&record.summary.module_id, id, &program, &saves)?
        } else {
            Vec::new()
        };
        let normalized_saves = normalize_path(&saves)?;
        let external = !contains(&normalize_path(root)?, &normalized_saves)
            && !owned.iter().any(|path| contains(path, &normalized_saves));
        Ok(InstanceRemovalPlan {
            program_path: program.to_string_lossy().into_owned(),
            data_path: root.to_string_lossy().into_owned(),
            remove_program: !library,
            preserved_program_path: library.then(|| program.to_string_lossy().into_owned()),
            owned_data_paths: owned
                .into_iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
            preserved_external_saves_path: external.then(|| saves.to_string_lossy().into_owned()),
        })
    }
    .await;
    pool.close().await;
    result
}
