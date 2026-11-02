//! One durable, identity-bound takeover for each outermost destination.
//! The caller must persist FilePlan before entering acquire. Recovery directories
//! are permanent: neither cancellation nor a new plan removes an earlier backup.
use super::*;

const MANIFEST_LIMIT: u64 = 4 * 1024 * 1024;
const RECOVERY_PREFIX: &str = ".langame-migration-";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OriginalTarget {
    pub identity: FileIdentity,
    pub directory: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RootTakeover {
    pub target: PathBuf,
    pub recovery_root: PathBuf,
    pub previous: Option<OriginalTarget>,
}

impl RootTakeover {
    fn backup(&self) -> PathBuf {
        self.recovery_root.join("previous")
    }

    fn incoming(&self) -> PathBuf {
        self.recovery_root.join("incoming")
    }

    fn manifest(&self) -> PathBuf {
        self.recovery_root.join("manifest.json")
    }
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    version: u32,
    roots: Vec<RootMove>,
    takeover: RootTakeover,
    recovery_identity: FileIdentity,
    incoming_identity: FileIdentity,
}

struct SavedManifest {
    data: Manifest,
    _directory: OwnedNode,
    _file: OwnedNode,
}

fn outer_targets(roots: &[RootMove]) -> BTreeSet<PathKey> {
    roots
        .iter()
        .filter(|candidate| {
            !roots.iter().any(|other| {
                PathKey(other.target.clone()) != PathKey(candidate.target.clone())
                    && contains(&other.target, &candidate.target)
            })
        })
        .map(|root| PathKey(root.target.clone()))
        .collect()
}

fn validate_target_syntax(path: &Path) -> Result<(), StorageError> {
    use std::path::Component;
    if !path.is_absolute() || path.parent().is_none() || path.file_name().is_none() {
        return Err(invalid(
            path,
            "A migration target must be an absolute non-volume root.",
        ));
    }
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            return Err(invalid(
                path,
                "Parent traversal is not allowed in migration targets.",
            ));
        }
        #[cfg(windows)]
        if let Component::Normal(value) = component {
            let value = value.to_string_lossy();
            if value.ends_with(['.', ' '])
                || value
                    .chars()
                    .any(|ch| ch.is_control() || matches!(ch, ':' | '<' | '>' | '|' | '?' | '*'))
            {
                return Err(invalid(
                    path,
                    "Ambiguous Windows migration target component.",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn normalize_destinations(paths: &[PathBuf]) -> Result<Vec<PathBuf>, StorageError> {
    let roots = paths
        .iter()
        .map(|target| RootMove {
            source: PathBuf::new(),
            target: target.clone(),
        })
        .collect::<Vec<_>>();
    for root in &roots {
        validate_target_syntax(&root.target)?;
    }
    let outer = outer_targets(&roots);
    roots
        .iter()
        .map(|root| {
            let boundary = outer
                .iter()
                .find(|outer| contains(&outer.0, &root.target))
                .ok_or_else(|| invalid(&root.target, "Missing outer migration target."))?;
            // Old contents below this boundary will be moved without traversal.
            // They may be files where the new source tree needs directories.
            let mut result = normalize_resource_path(&boundary.0)?;
            result.extend(
                root.target
                    .components()
                    .skip(boundary.0.components().count()),
            );
            Ok(result)
        })
        .collect()
}

pub(super) fn target_aliases(
    path: &Path,
    roots: &[RootMove],
) -> Result<Vec<DirectoryAlias>, StorageError> {
    let outer = outer_targets(roots);
    let boundary = outer
        .iter()
        .find(|root| contains(&root.0, path))
        .ok_or_else(|| invalid(path, "Missing outer migration target."))?;
    let suffix = path
        .components()
        .skip(boundary.0.components().count())
        .collect::<PathBuf>();
    let mut aliases = directory_aliases(&boundary.0)?;
    if !suffix.as_os_str().is_empty() {
        for alias in &mut aliases {
            alias.tail.push(&suffix);
        }
    }
    Ok(aliases)
}

fn observe(path: &Path) -> Result<Option<OriginalTarget>, StorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(read_error(path, error)),
    };
    let node =
        native::open(path, metadata.is_dir(), false).map_err(|error| read_error(path, error))?;
    Ok(Some(OriginalTarget {
        identity: node.identity().map_err(|error| read_error(path, error))?,
        directory: metadata.is_dir(),
    }))
}

pub(super) fn plan_targets(roots: &[RootMove]) -> Result<Vec<RootTakeover>, StorageError> {
    outer_targets(roots)
        .into_iter()
        .map(|target| {
            let parent = target
                .0
                .parent()
                .ok_or_else(|| invalid(&target.0, "Target has no parent."))?;
            Ok(RootTakeover {
                previous: observe(&target.0)?,
                recovery_root: parent.join(format!("{RECOVERY_PREFIX}{}", uuid::Uuid::new_v4())),
                target: target.0,
            })
        })
        .collect()
}

pub(super) fn validate(plan: &FilePlan) -> Result<(), StorageError> {
    let mut expected = outer_targets(&plan.roots);
    let mut recoveries = BTreeSet::new();
    for root in &plan.takeovers {
        let name = root
            .recovery_root
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|value| value.strip_prefix(RECOVERY_PREFIX));
        if !expected.remove(&PathKey(root.target.clone()))
            || name.is_none_or(|name| uuid::Uuid::parse_str(name).is_err())
            || root.target.parent().map(|path| PathKey(path.to_owned()))
                != root
                    .recovery_root
                    .parent()
                    .map(|path| PathKey(path.to_owned()))
            || !recoveries.insert(PathKey(root.recovery_root.clone()))
            || plan.roots.iter().any(|mapping| {
                contains(&mapping.source, &root.recovery_root)
                    || contains(&root.recovery_root, &mapping.source)
                    || contains(&mapping.target, &root.recovery_root)
                    || contains(&root.recovery_root, &mapping.target)
            })
        {
            return Err(invalid(
                &root.target,
                "Invalid destination takeover/recovery boundary.",
            ));
        }
        if PathKey(normalize_resource_path(&root.recovery_root)?)
            != PathKey(root.recovery_root.clone())
        {
            return Err(invalid(
                &root.recovery_root,
                "Recovery directory namespace changed.",
            ));
        }
    }
    if !expected.is_empty() {
        return Err(invalid(
            Path::new(""),
            "Migration plan does not contain every destination takeover.",
        ));
    }
    Ok(())
}

fn read_manifest(
    plan: &FilePlan,
    root: &RootTakeover,
) -> Result<Option<SavedManifest>, StorageError> {
    let Some(recovery) = observe(&root.recovery_root)? else {
        return Ok(None);
    };
    if !recovery.directory {
        return Err(invalid(
            &root.recovery_root,
            "Recovery path is not a directory; it was preserved.",
        ));
    }
    let directory = native::open(&root.recovery_root, true, false)
        .map_err(|error| read_error(&root.recovery_root, error))?;
    let path = root.manifest();
    let mut file = checked_file(&path, false, None, None, None)?;
    let mut bytes = Vec::new();
    file.reader()
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(&path, error))?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            &path,
            "Migration recovery manifest exceeds its size limit.",
        ));
    }
    let data: Manifest = serde_json::from_slice(&bytes).map_err(|error| {
        invalid(
            &path,
            format!("Invalid migration recovery manifest: {error}"),
        )
    })?;
    if data.version != 1
        || data.roots != plan.roots
        || data.takeover != *root
        || data.recovery_identity != recovery.identity
        || directory
            .identity()
            .map_err(|error| read_error(&root.recovery_root, error))?
            != recovery.identity
    {
        return Err(invalid(
            &path,
            "Migration recovery manifest ownership changed; all files were preserved.",
        ));
    }
    Ok(Some(SavedManifest {
        data,
        _directory: directory,
        _file: file,
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    BeforeBackup,
    BeforePublish,
    Published,
}

fn state(root: &RootTakeover, saved: &SavedManifest) -> Result<State, StorageError> {
    let target = observe(&root.target)?;
    let backup = observe(&root.backup())?;
    let incoming = observe(&root.incoming())?;
    let new = Some(OriginalTarget {
        identity: saved.data.incoming_identity.clone(),
        directory: true,
    });
    let result = if target == new && incoming.is_none() && backup == root.previous {
        Some(State::Published)
    } else if incoming == new && backup.is_none() && target == root.previous && target.is_some() {
        Some(State::BeforeBackup)
    } else if incoming == new && target.is_none() && backup == root.previous {
        Some(State::BeforePublish)
    } else {
        None
    };
    result.ok_or_else(|| invalid(&root.target, format!(
        "Destination or recovery identity changed; source and recovery data at {} were preserved.",
        root.recovery_root.display()
    )))
}

pub(super) fn recheck(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
) -> Result<(), StorageError> {
    for root in &plan.takeovers {
        cancelled(cancellation)?;
        match read_manifest(plan, root)? {
            Some(saved) => {
                state(root, &saved)?;
            }
            None if observe(&root.target)? == root.previous => (),
            None => {
                return Err(invalid(
                    &root.target,
                    "Destination root identity changed after planning.",
                ));
            }
        }
    }
    Ok(())
}

fn prepare_manifest(plan: &FilePlan, root: &RootTakeover) -> Result<SavedManifest, StorageError> {
    if let Some(saved) = read_manifest(plan, root)? {
        return Ok(saved);
    }
    let parent = root
        .target
        .parent()
        .ok_or_else(|| invalid(&root.target, "Target has no parent."))?;
    ensure_directory(parent)?;
    let _parent = directory_guard(parent)?;
    fs::create_dir(&root.recovery_root).map_err(|source| StorageError::CreatePath {
        path: root.recovery_root.clone(),
        source,
    })?;
    let recovery = native::open(&root.recovery_root, true, false)
        .map_err(|error| read_error(&root.recovery_root, error))?;
    let incoming = root.incoming();
    fs::create_dir(&incoming).map_err(|source| StorageError::CreatePath {
        path: incoming.clone(),
        source,
    })?;
    let staged =
        native::open(&incoming, true, false).map_err(|error| read_error(&incoming, error))?;
    let manifest = Manifest {
        version: 1,
        roots: plan.roots.clone(),
        takeover: root.clone(),
        recovery_identity: recovery
            .identity()
            .map_err(|error| read_error(&root.recovery_root, error))?,
        incoming_identity: staged
            .identity()
            .map_err(|error| read_error(&incoming, error))?,
    };
    let bytes = serde_json::to_vec(&manifest)
        .map_err(|error| invalid(&root.manifest(), error.to_string()))?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid(
            &root.manifest(),
            "Migration recovery manifest exceeds its size limit.",
        ));
    }
    crate::atomic_file::create_file_atomically(&root.manifest(), &bytes).map_err(|error| {
        invalid(
            &root.manifest(),
            format!("Could not persist recovery manifest: {error}"),
        )
    })?;
    drop((staged, recovery));
    read_manifest(plan, root)?
        .ok_or_else(|| invalid(&root.manifest(), "Recovery manifest disappeared."))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TakeoverPoint {
    ManifestSaved,
    OriginalPreserved,
    BeforePublish,
    Published,
}

pub(super) fn acquire(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
) -> Result<Vec<OwnedNode>, StorageError> {
    acquire_with(plan, cancellation, &mut |_, _| Ok(()))
}

fn acquire_with(
    plan: &FilePlan,
    cancellation: Option<&AtomicBool>,
    hook: &mut dyn FnMut(TakeoverPoint, &RootTakeover) -> Result<(), StorageError>,
) -> Result<Vec<OwnedNode>, StorageError> {
    if !cfg!(windows) && !plan.takeovers.is_empty() {
        return Err(invalid(
            Path::new(""),
            "Safe whole-directory migration publication requires Windows.",
        ));
    }
    recheck(plan, cancellation)?;
    for root in &plan.takeovers {
        cancelled(cancellation)?;
        let saved = prepare_manifest(plan, root)?;
        hook(TakeoverPoint::ManifestSaved, root)?;
        cancelled(cancellation)?;
        if state(root, &saved)? == State::BeforeBackup {
            let original = root
                .previous
                .as_ref()
                .ok_or_else(|| invalid(&root.target, "Missing original target identity."))?;
            let node = native::open(&root.target, original.directory, true)
                .map_err(|error| read_error(&root.target, error))?;
            if node
                .identity()
                .map_err(|error| read_error(&root.target, error))?
                != original.identity
            {
                return Err(invalid(
                    &root.target,
                    "Destination root changed before preservation.",
                ));
            }
            node.rename(&root.backup()).map_err(|error| {
                invalid(
                    &root.target,
                    format!(
                        "Could not preserve old destination at {}: {error}",
                        root.backup().display()
                    ),
                )
            })?;
            hook(TakeoverPoint::OriginalPreserved, root)?;
        }
        cancelled(cancellation)?;
        if state(root, &saved)? == State::BeforePublish {
            hook(TakeoverPoint::BeforePublish, root)?;
            cancelled(cancellation)?;
            let incoming = root.incoming();
            let node = native::open(&incoming, true, true)
                .map_err(|error| read_error(&incoming, error))?;
            if node
                .identity()
                .map_err(|error| read_error(&incoming, error))?
                != saved.data.incoming_identity
            {
                return Err(invalid(
                    &incoming,
                    "Incoming migration directory identity changed.",
                ));
            }
            node.rename(&root.target).map_err(|error| {
                invalid(
                    &root.target,
                    format!(
                        "Could not publish the new destination; recovery remains at {}: {error}",
                        root.recovery_root.display()
                    ),
                )
            })?;
            hook(TakeoverPoint::Published, root)?;
        }
    }
    owned_destinations(plan)
}

pub(super) fn owned_destinations(plan: &FilePlan) -> Result<Vec<OwnedNode>, StorageError> {
    let mut guards = Vec::new();
    for root in &plan.takeovers {
        let saved = read_manifest(plan, root)?.ok_or_else(|| {
            invalid(
                &root.target,
                "Destination has no durable migration ownership record.",
            )
        })?;
        if state(root, &saved)? != State::Published {
            return Err(invalid(
                &root.target,
                "Destination takeover has not been published.",
            ));
        }
        let node = native::open(&root.target, true, false)
            .map_err(|error| read_error(&root.target, error))?;
        if node
            .identity()
            .map_err(|error| read_error(&root.target, error))?
            != saved.data.incoming_identity
        {
            return Err(invalid(
                &root.target,
                "Published migration destination identity changed.",
            ));
        }
        guards.push(node);
    }
    Ok(guards)
}

pub(super) fn preserved_destinations(
    plan: &FilePlan,
) -> Result<Vec<(PathBuf, PathBuf)>, StorageError> {
    if !plan.whole_directory {
        return if plan.takeovers.is_empty() {
            Ok(Vec::new())
        } else {
            Err(invalid(
                Path::new(""),
                "A merge-style migration cannot contain destination takeovers.",
            ))
        };
    }
    validate(plan)?;
    let mut preserved = Vec::new();
    for root in &plan.takeovers {
        if let Some(actual) = observe(&root.backup())? {
            if root.previous.as_ref() != Some(&actual) {
                return Err(invalid(
                    &root.backup(),
                    "Preserved destination identity changed.",
                ));
            }
            preserved.push((root.target.clone(), root.backup()));
        }
    }
    Ok(preserved)
}

#[cfg(all(test, windows))]
#[path = "storage_migration_takeover_tests.rs"]
mod tests;
