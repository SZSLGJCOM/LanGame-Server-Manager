use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::StorageError;
use crate::atomic_file::write_file_atomically;
use crate::instance_creation_io::{check_creation_cancelled, publish_creation_directory};
use crate::instance_isolation::paths::{contains, normalize_path, normalize_resource_path};
use crate::private_runtime::{PRIVATE_RUNTIME_MARKER, PROJECTION_RUNTIME_MARKER};
use crate::private_runtime_refresh::{
    PackageTree, validate_package_paths, validated_relative_path,
};
use crate::program_runtime::ProgramFileSelection;
use crate::program_runtime::invalid;

pub(crate) const ADOPTION_JOURNAL: &str = ".langame-program-adoption.json";
const BASELINE: &str = ".langame-package-baseline.json";
const RETAINED: &str = "installation-retained";

/// Durable transfer of an instance acquisition; original library installations
/// remain at their existing paths and are never implicitly transferred.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ProgramAdoption {
    version: u32,
    source: PathBuf,
    instance_root: PathBuf,
    exclusions: Vec<String>,
}

impl ProgramAdoption {
    pub(crate) fn begin(
        source: &Path,
        instance_root: &Path,
        exclusions: &[PathBuf],
        exclude_dst_mods: bool,
        selection: ProgramFileSelection<'_>,
        cancellation: Option<&AtomicBool>,
    ) -> Result<Option<Self>, StorageError> {
        check_creation_cancelled(cancellation)?;
        let source = normalize_path(source)?;
        let instance_root = normalize_path(instance_root)?;
        if matches!(selection, ProgramFileSelection::Verified(_))
            && !source
                .try_exists()
                .map_err(|source_error| StorageError::ReadPath {
                    path: source.clone(),
                    source: source_error,
                })?
        {
            return Err(StorageError::CleanLibraryProgramRequired { path: source });
        }
        if !source.is_dir()
            || contains(&source, &instance_root)
            || contains(&instance_root, &source)
        {
            return Err(invalid(
                &source,
                "an independent installation requires a separate existing program directory",
            ));
        }
        for reserved in [PRIVATE_RUNTIME_MARKER, PROJECTION_RUNTIME_MARKER, BASELINE] {
            if fs::symlink_metadata(source.join(reserved)).is_ok() {
                return Err(invalid(
                    &source,
                    "the program directory is already managed by an instance",
                ));
            }
        }
        let mut relative = BTreeSet::new();
        for excluded in exclusions {
            let normalized = normalize_resource_path(excluded)?;
            if !contains(&source, &normalized) || normalized == source {
                return Err(invalid(
                    excluded,
                    "private data exclusion does not belong to the downloaded program",
                ));
            }
            let path = normalized
                .strip_prefix(&source)
                .map_err(|_| invalid(excluded, "invalid private data boundary"))?;
            let key = path.to_string_lossy().replace('\\', "/");
            validated_relative_path(&key)?;
            relative.insert(key);
        }
        if matches!(selection, ProgramFileSelection::Local) {
            relative.insert(String::from(".langame-clean-package.json"));
        }
        if let ProgramFileSelection::Verified(module_id) = selection {
            let package =
                crate::program_seed::require_clean_package_tree(&source, module_id, cancellation)?;
            retain_unlisted_paths(&source, &package, &mut relative, cancellation)?;
        } else if !matches!(selection, ProgramFileSelection::Local)
            && fs::symlink_metadata(source.join(".langame-clean-package.json")).is_ok()
        {
            let package = crate::program_seed::read_clean_package_tree(&source, cancellation)?
                .ok_or_else(|| invalid(&source, "original program files changed; obtain a clean official package before creating an instance"))?;
            retain_unlisted_paths(&source, &package, &mut relative, cancellation)?;
        } else {
            validate_package_paths(&source, &relative, exclude_dst_mods, cancellation)?;
        }
        // A directory owns its descendants; move it once and preserve every byte.
        let exclusions = relative
            .iter()
            .filter(|candidate| {
                let mut parent = candidate.as_str();
                while let Some((ancestor, _)) = parent.rsplit_once('/') {
                    if relative.contains(ancestor) {
                        return false;
                    }
                    parent = ancestor;
                }
                true
            })
            .cloned()
            .collect::<Vec<_>>();
        let journal = Self {
            version: 1,
            source,
            instance_root,
            exclusions,
        };
        journal.save()?;
        let runtime = journal.instance_root.join("runtime");
        if let Err(error) = publish_creation_directory(&journal.source, &runtime, cancellation) {
            let cross_volume = matches!(&error, StorageError::PublishInstanceRuntime { source, .. }
                if source.raw_os_error() == Some(if cfg!(windows) { 17 } else { 18 }));
            journal.clear()?;
            return if cross_volume { Ok(None) } else { Err(error) };
        }
        let prepared = (|| {
            for relative in &journal.exclusions {
                check_creation_cancelled(cancellation)?;
                let path = validated_relative_path(relative)?;
                let original = runtime.join(&path);
                match fs::symlink_metadata(&original) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(source) => {
                        return Err(StorageError::ReadPath {
                            path: original,
                            source,
                        });
                    }
                    Ok(_) => {}
                }
                let retained = journal.instance_root.join(RETAINED).join(path);
                if let Some(parent) = retained.parent() {
                    fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                        path: parent.to_owned(),
                        source,
                    })?;
                }
                fs::rename(&original, &retained).map_err(|source| StorageError::MovePath {
                    from: original,
                    to: retained,
                    source,
                })?;
            }
            fs::write(runtime.join(PRIVATE_RUNTIME_MARKER), b"managed\n").map_err(|source| {
                StorageError::WriteConfig {
                    path: runtime.join(PRIVATE_RUNTIME_MARKER),
                    source,
                }
            })?;
            Ok::<_, StorageError>(())
        })();
        if let Err(error) = prepared {
            if let Err(rollback) = journal.rollback() {
                return Err(StorageError::InstanceCreationRollback {
                    path: journal.instance_root.clone(),
                    creation_error: error.to_string(),
                    cleanup_error: rollback.to_string(),
                });
            }
            return Err(error);
        }
        Ok(Some(journal))
    }

    pub(crate) fn runtime_root(&self) -> PathBuf {
        self.instance_root.join("runtime")
    }

    pub(crate) fn source_root(&self) -> &Path {
        &self.source
    }

    pub(crate) fn commit(&self) -> Result<(), StorageError> {
        self.clear()
    }

    pub(crate) fn rollback(&self) -> Result<(), StorageError> {
        if self.version != 1 {
            return Err(invalid(&self.instance_root, "unsupported adoption journal"));
        }
        let runtime = self.instance_root.join("runtime");
        if self
            .source
            .try_exists()
            .map_err(|source| StorageError::ReadPath {
                path: self.source.clone(),
                source,
            })?
        {
            if runtime
                .try_exists()
                .map_err(|source| StorageError::ReadPath {
                    path: runtime.clone(),
                    source,
                })?
            {
                return Err(invalid(
                    &runtime,
                    "both installation paths exist; preserving them for recovery",
                ));
            }
            return self.clear();
        }
        normalize_path(&runtime)?;
        for relative in &self.exclusions {
            let relative = validated_relative_path(relative)?;
            let retained = self.instance_root.join(RETAINED).join(&relative);
            match fs::symlink_metadata(&retained) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(StorageError::ReadPath {
                        path: retained,
                        source,
                    });
                }
                Ok(_) => {}
            }
            normalize_resource_path(&retained)?;
            let destination = runtime.join(&relative);
            normalize_resource_path(&destination)?;
            if fs::symlink_metadata(&destination).is_ok() {
                // Native config materialization may recreate an excluded directory
                // before the transaction fails. Preserve those new files outside
                // the active data paths, then put the original bytes back.
                let recovery = runtime
                    .join(".langame-creation-recovery")
                    .join(uuid::Uuid::new_v4().to_string());
                normalize_resource_path(&recovery)?;
                fs::create_dir_all(&recovery).map_err(|source| StorageError::CreatePath {
                    path: recovery.clone(),
                    source,
                })?;
                let preserved = recovery.join(&relative);
                if let Some(parent) = preserved.parent() {
                    fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                        path: parent.to_owned(),
                        source,
                    })?;
                }
                fs::rename(&destination, &preserved).map_err(|source| StorageError::MovePath {
                    from: destination.clone(),
                    to: preserved,
                    source,
                })?;
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|source| StorageError::CreatePath {
                    path: parent.to_owned(),
                    source,
                })?;
            }
            fs::rename(&retained, &destination).map_err(|source| StorageError::MovePath {
                from: retained,
                to: destination,
                source,
            })?;
        }
        for name in [PRIVATE_RUNTIME_MARKER, BASELINE] {
            let path = runtime.join(name);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => return Err(StorageError::DeletePath { path, source }),
            }
        }
        publish_creation_directory(&runtime, &self.source, None)?;
        self.clear()
    }

    fn save(&self) -> Result<(), StorageError> {
        let path = self.instance_root.join(ADOPTION_JOURNAL);
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > 65_536 || self.exclusions.len() > 4_096 {
            return Err(invalid(
                &path,
                "program adoption journal exceeds its recovery limit",
            ));
        }
        write_file_atomically(&path, &bytes)
            .map_err(|source| StorageError::WriteConfig { path, source })
    }

    fn clear(&self) -> Result<(), StorageError> {
        let path = self.instance_root.join(ADOPTION_JOURNAL);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(StorageError::DeletePath { path, source }),
        }
    }
}

/// A valid allowlist proves its program files, not the absence of added Mods.
/// Exclude unlisted subtrees without reading their content or following links.
pub(crate) fn retain_unlisted_paths(
    root: &Path,
    package: &PackageTree,
    exclusions: &mut BTreeSet<String>,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    let fold = |value: &str| {
        if cfg!(windows) {
            value.to_lowercase()
        } else {
            value.to_owned()
        }
    };
    let files = package
        .files
        .keys()
        .map(|key| fold(key))
        .collect::<BTreeSet<_>>();
    let directories = package
        .directories
        .iter()
        .map(|key| fold(key))
        .collect::<BTreeSet<_>>();
    let mut pending = vec![PathBuf::new()];
    let mut visited = 0_usize;
    while let Some(relative) = pending.pop() {
        let directory = root.join(&relative);
        for entry in fs::read_dir(&directory).map_err(|source| StorageError::ReadDirectory {
            path: directory.clone(),
            source,
        })? {
            check_creation_cancelled(cancellation)?;
            visited += 1;
            if visited > 1_000_000 {
                return Err(invalid(
                    root,
                    "program inventory exceeds its safe entry limit",
                ));
            }
            let entry = entry.map_err(|source| StorageError::ReadDirectory {
                path: directory.clone(),
                source,
            })?;
            let relative = relative.join(entry.file_name());
            let key = relative
                .to_str()
                .ok_or_else(|| invalid(&relative, "program path is not UTF-8"))?
                .replace('\\', "/");
            validated_relative_path(&key)?;
            let normalized = fold(&key);
            if normalized == ".langame-clean-package.json" {
                continue;
            }
            normalize_resource_path(&entry.path())?;
            let metadata = entry.metadata().map_err(|source| StorageError::ReadPath {
                path: entry.path(),
                source,
            })?;
            if metadata.is_dir() && directories.contains(&normalized) {
                pending.push(relative);
            } else if !metadata.is_file() || !files.contains(&normalized) {
                exclusions.insert(key);
            }
        }
    }
    Ok(())
}

pub(crate) struct ProgramCopyPlan {
    pub(crate) exclusions: Vec<PathBuf>,
    pub(crate) inventory: Option<crate::program_seed::CleanCopyInventory>,
}

pub(crate) fn validate_library_copy_source(
    source: &Path,
    instance_root: &Path,
    exclusions: &[PathBuf],
    exclude_dst_mods: bool,
    selection: ProgramFileSelection<'_>,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    check_creation_cancelled(cancellation)?;
    let source = normalize_path(source)?;
    let instance_root = normalize_path(instance_root)?;
    if matches!(selection, ProgramFileSelection::Verified(_))
        && !source
            .try_exists()
            .map_err(|source_error| StorageError::ReadPath {
                path: source.clone(),
                source: source_error,
            })?
    {
        return Err(StorageError::CleanLibraryProgramRequired { path: source });
    }
    if !source.is_dir() || contains(&source, &instance_root) || contains(&instance_root, &source) {
        return Err(invalid(
            &source,
            "an independent installation requires a separate existing program directory",
        ));
    }
    for reserved in [PRIVATE_RUNTIME_MARKER, PROJECTION_RUNTIME_MARKER, BASELINE] {
        if fs::symlink_metadata(source.join(reserved)).is_ok() {
            return Err(invalid(
                &source,
                "the program directory is already managed by an instance",
            ));
        }
    }
    let mut relative = BTreeSet::new();
    for excluded in exclusions {
        let normalized = normalize_resource_path(excluded)?;
        if !contains(&source, &normalized) || normalized == source {
            return Err(invalid(
                excluded,
                "private data exclusion does not belong to the downloaded program",
            ));
        }
        let path = normalized
            .strip_prefix(&source)
            .map_err(|_| invalid(excluded, "invalid private data boundary"))?;
        let key = path.to_string_lossy().replace('\\', "/");
        validated_relative_path(&key)?;
        relative.insert(key);
    }
    if matches!(selection, ProgramFileSelection::Local)
        || matches!(selection, ProgramFileSelection::Automatic)
            && fs::symlink_metadata(source.join(".langame-clean-package.json")).is_err()
    {
        validate_package_paths(&source, &relative, exclude_dst_mods, cancellation)?;
    }
    Ok(())
}

pub(crate) fn clean_copy_exclusions(
    source: &Path,
    exclusions: &[PathBuf],
    selection: ProgramFileSelection<'_>,
    cancellation: Option<&AtomicBool>,
) -> Result<ProgramCopyPlan, StorageError> {
    check_creation_cancelled(cancellation)?;
    let mut result = exclusions.to_vec();
    result.extend(
        [
            PROJECTION_RUNTIME_MARKER,
            BASELINE,
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ]
        .map(|name| source.join(name)),
    );
    if matches!(selection, ProgramFileSelection::Local) {
        return Ok(ProgramCopyPlan {
            exclusions: result,
            inventory: None,
        });
    }
    let inventory = if let ProgramFileSelection::Verified(module_id) = selection {
        Some(
            crate::program_seed::read_clean_package_copy_inventory(source, Some(module_id))?
                .ok_or_else(|| StorageError::CleanLibraryProgramRequired {
                    path: source.to_owned(),
                })?,
        )
    } else if fs::symlink_metadata(source.join(".langame-clean-package.json")).is_ok() {
        Some(crate::program_seed::read_clean_package_copy_inventory(source, None)?
            .ok_or_else(|| invalid(source, "original program files changed; obtain a clean official package before copying"))?)
    } else {
        None
    };
    Ok(ProgramCopyPlan {
        exclusions: result,
        inventory,
    })
}

impl ProgramAdoption {
    /// Read only a bounded, plain journal for the caller's exact installation.
    /// Other modules may have an interrupted adoption in the same instance root.
    pub(crate) fn load_for_recovery(
        path: &Path,
        expected_instance: &Path,
        expected_source: &Path,
    ) -> Result<Option<Self>, StorageError> {
        let journal = Self::load_validated(path, expected_instance)?;
        let expected_source = normalize_path(expected_source)?;
        Ok((contains(&journal.source, &expected_source)
            && contains(&expected_source, &journal.source))
        .then_some(journal))
    }

    pub(crate) fn load_for_instance_program_recovery(
        path: &Path,
        expected_instance: &Path,
        acquisitions_root: &Path,
    ) -> Result<Option<Self>, StorageError> {
        let journal = Self::load_validated(path, expected_instance)?;
        let namespace = normalize_path(acquisitions_root)?;
        let owned = journal
            .source
            .parent()
            .is_some_and(|parent| contains(parent, &namespace) && contains(&namespace, parent))
            && journal
                .source
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok());
        Ok(owned.then_some(journal))
    }

    fn load_validated(path: &Path, expected_instance: &Path) -> Result<Self, StorageError> {
        use std::io::Read;
        const MAX_JOURNAL_BYTES: u64 = 65_536;
        let instance = normalize_path(expected_instance)?;
        let journal_path = normalize_resource_path(path)?;
        let expected_journal = normalize_resource_path(&instance.join(ADOPTION_JOURNAL))?;
        if journal_path != expected_journal || !instance.is_dir() {
            return Err(invalid(
                path,
                "adoption journal does not belong to this instance directory",
            ));
        }
        let mut bytes = Vec::new();
        fs::File::open(&journal_path)
            .and_then(|file| file.take(MAX_JOURNAL_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| StorageError::ReadPath {
                path: journal_path.clone(),
                source,
            })?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(invalid(path, "adoption journal is too large"));
        }
        let mut journal: Self = serde_json::from_slice(&bytes)
            .map_err(|error| invalid(path, format!("invalid adoption journal: {error}")))?;
        journal.instance_root = normalize_path(&journal.instance_root)?;
        if journal.version != 1 || journal.instance_root != instance {
            return Err(invalid(
                path,
                "adoption journal version or instance directory does not match",
            ));
        }
        journal.source = normalize_path(&journal.source)?;
        if contains(&journal.source, &instance) || contains(&instance, &journal.source) {
            return Err(invalid(
                path,
                "adoption source overlaps the instance directory",
            ));
        }
        if journal.exclusions.len() > 4_096 {
            return Err(invalid(
                path,
                "adoption journal contains too many exclusions",
            ));
        }
        for excluded in &journal.exclusions {
            let relative = validated_relative_path(excluded)?;
            normalize_resource_path(&instance.join(RETAINED).join(&relative))?;
            normalize_resource_path(&instance.join("runtime").join(relative))?;
        }
        Ok(journal)
    }
}
