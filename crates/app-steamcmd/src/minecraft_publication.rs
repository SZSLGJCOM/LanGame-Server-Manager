use super::*;
use crate::install_publication_metadata::InstallPublicationMetadata;
use crate::install_transaction::{prepared_install_publish_script, recover_install_publication};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) struct MinecraftPublication {
    root: PathBuf,
    pub(super) stage: PathBuf,
    rollback: PathBuf,
    rejected: PathBuf,
    phase: PathBuf,
    recovery: PathBuf,
    metadata: InstallPublicationMetadata,
}

impl MinecraftPublication {
    pub(super) fn new(root: &Path, module_id: &str) -> Result<Self, SteamCmdError> {
        let parent = root
            .parent()
            .ok_or_else(|| invalid(root, "installation has no parent"))?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let id = format!(
            "{:x}-{stamp:x}-{:x}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let sibling = |suffix| parent.join(format!(".lg-mc-{id}.{suffix}"));
        let stage = sibling("stage");
        let metadata = InstallPublicationMetadata::read(root, module_id, false)?;
        fs::create_dir(&stage).map_err(|source| SteamCmdError::CreatePath {
            path: stage.clone(),
            source,
        })?;
        Ok(Self {
            root: root.to_owned(),
            stage,
            rollback: sibling("rollback"),
            rejected: sibling("rejected"),
            phase: sibling("publish.state"),
            recovery: PathBuf::from(".langame/minecraft-recovery").join(id),
            metadata,
        })
    }

    pub(super) async fn merge_existing(
        &self,
        deadline: InstallDeadline,
    ) -> Result<(), SteamCmdError> {
        let source = self.root.clone();
        let stage = self.stage.clone();
        let recovery = self.recovery.clone();
        let cancellation = InstallCancellation::current();
        tokio::task::spawn_blocking(move || {
            merge_existing(&source, &stage, &recovery, deadline, cancellation.as_ref())
        })
        .await
        .map_err(|error| invalid(&self.root, &format!("preservation worker failed: {error}")))?
    }

    pub(super) async fn publish(
        &self,
        verification: &Path,
        deadline: InstallDeadline,
    ) -> Result<(), SteamCmdError> {
        deadline.check_cancelled()?;
        let script = prepared_install_publish_script(
            &self.root,
            &self.stage,
            &self.rollback,
            &self.phase,
            &self.stage.join(verification),
            &self.metadata,
        );
        let output = run_powershell(&script, None, deadline).await?;
        if !output.status.success() {
            return Err(SteamCmdError::DirectDownloadFailed {
                output_excerpt: output_excerpt(&output.stdout, &output.stderr),
            });
        }
        Ok(())
    }

    pub(super) async fn recover(&self, published: bool) -> Result<(), SteamCmdError> {
        recover_install_publication(
            &self.root,
            &self.stage,
            &self.rollback,
            &self.rejected,
            &self.phase,
            published,
        )
        .await
    }

    pub(super) fn finish(&self) {
        schedule_verified_directory_cleanup(vec![self.rollback.clone(), self.stage.clone()]);
        let _ = fs::remove_file(&self.phase);
    }
}

// The callback has already inventoried only newly verified Mojang/Temurin files.
// Merge operator files afterwards, without ever adding them to that inventory.
// A replacement keeps displaced bytes in the same atomic publication transaction.
fn merge_existing(
    source: &Path,
    stage: &Path,
    recovery: &Path,
    deadline: InstallDeadline,
    cancellation: Option<&InstallCancellation>,
) -> Result<(), SteamCmdError> {
    check(deadline, cancellation)?;
    let entries = tree(source, deadline, cancellation)?;
    tree(stage, deadline, cancellation)?;
    if source.join(recovery).exists() || stage.join(recovery).exists() {
        return Err(invalid(
            &source.join(recovery),
            "recovery destination already exists",
        ));
    }
    let mut displaced_directories = Vec::<PathBuf>::new();
    let mut observed = BTreeMap::new();
    for (relative, directory) in &entries {
        check(deadline, cancellation)?;
        if manager_replaced(relative) {
            continue;
        }
        let old = source.join(relative);
        let mut target = stage.join(relative);
        let inherited_conflict = displaced_directories
            .iter()
            .any(|prefix| relative.starts_with(prefix));
        if inherited_conflict {
            target = stage.join(recovery).join(relative);
        } else if let Ok(metadata) = fs::symlink_metadata(&target) {
            if *directory && metadata.is_dir() {
                continue;
            }
            if !*directory && metadata.is_file() {
                let old_hash = digest(&old, deadline, cancellation)?;
                observed.insert(relative.clone(), old_hash.clone());
                if old_hash == digest(&target, deadline, cancellation)? {
                    continue;
                }
            }
            if *directory {
                displaced_directories.push(relative.clone());
            }
            target = stage.join(recovery).join(relative);
        }
        if *directory {
            fs::create_dir_all(&target).map_err(|source| SteamCmdError::CreatePath {
                path: target,
                source,
            })?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|source| SteamCmdError::CreatePath {
                    path: parent.to_owned(),
                    source,
                })?;
            }
            let hash = copy_file(&old, &target, deadline, cancellation)?;
            observed.insert(relative.clone(), hash);
        }
    }
    // The game operation lease stops managed writers. Detect external changes
    // during the copy too, before the old root can enter the publication step.
    if tree(source, deadline, cancellation)? != entries {
        return Err(invalid(
            source,
            "installation contents changed during preservation",
        ));
    }
    for (relative, expected) in observed {
        if digest(&source.join(&relative), deadline, cancellation)? != expected {
            return Err(invalid(
                &source.join(relative),
                "file changed during preservation",
            ));
        }
    }
    check(deadline, cancellation)
}

fn manager_replaced(relative: &Path) -> bool {
    matches!(
        relative
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
            .as_str(),
        ".langame-clean-package.json"
            | ".langame-initial-package.json"
            | ".langame-uninstalled"
            | ".langame-retained-library.json"
    )
}

fn tree(
    root: &Path,
    deadline: InstallDeadline,
    cancellation: Option<&InstallCancellation>,
) -> Result<BTreeMap<PathBuf, bool>, SteamCmdError> {
    for ancestor in root.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)
            .map_err(|error| invalid(ancestor, &error.to_string()))?;
        plain(ancestor, &metadata)?;
    }
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in
            fs::read_dir(&directory).map_err(|error| invalid(&directory, &error.to_string()))?
        {
            check(deadline, cancellation)?;
            let entry = entry.map_err(|error| invalid(&directory, &error.to_string()))?;
            let path = entry.path();
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| invalid(&path, &error.to_string()))?;
            plain(&path, &metadata)?;
            entries.insert(
                path.strip_prefix(root)
                    .expect("enumerated child")
                    .to_owned(),
                metadata.is_dir(),
            );
            if entries.len() > 1_000_000 {
                return Err(invalid(
                    root,
                    "installation exceeds preservation entry limit",
                ));
            }
            if metadata.is_dir() {
                pending.push(path);
            }
        }
    }
    Ok(entries)
}

fn plain(path: &Path, metadata: &fs::Metadata) -> Result<(), SteamCmdError> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse || (!metadata.is_dir() && !metadata.is_file()) {
        return Err(invalid(
            path,
            "installation preservation requires plain files and directories",
        ));
    }
    Ok(())
}

fn copy_file(
    source: &Path,
    target: &Path,
    deadline: InstallDeadline,
    cancellation: Option<&InstallCancellation>,
) -> Result<Vec<u8>, SteamCmdError> {
    let mut input = fs::File::open(source).map_err(|error| invalid(source, &error.to_string()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|error| invalid(target, &error.to_string()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        check(deadline, cancellation)?;
        let count = input
            .read(&mut buffer)
            .map_err(|error| invalid(source, &error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|error| invalid(target, &error.to_string()))?;
    }
    output
        .sync_all()
        .map_err(|error| invalid(target, &error.to_string()))?;
    Ok(hasher.finalize().to_vec())
}

fn digest(
    path: &Path,
    deadline: InstallDeadline,
    cancellation: Option<&InstallCancellation>,
) -> Result<Vec<u8>, SteamCmdError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| invalid(path, &error.to_string()))?;
    plain(path, &metadata)?;
    let mut input = fs::File::open(path).map_err(|error| invalid(path, &error.to_string()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0u8; 65_536];
    loop {
        check(deadline, cancellation)?;
        let count = input
            .read(&mut buffer)
            .map_err(|error| invalid(path, &error.to_string()))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hasher.finalize().to_vec())
}

fn check(
    deadline: InstallDeadline,
    cancellation: Option<&InstallCancellation>,
) -> Result<(), SteamCmdError> {
    if cancellation.is_some_and(InstallCancellation::is_cancelled) {
        return Err(SteamCmdError::InstallCancelled {
            operation: deadline.operation().into(),
        });
    }
    deadline.check_cancelled()
}

fn invalid(path: &Path, detail: &str) -> SteamCmdError {
    SteamCmdError::WriteMinecraftServerFile {
        path: path.to_owned(),
        source: std::io::Error::other(detail),
    }
}

#[cfg(test)]
#[path = "minecraft_publication_tests.rs"]
mod tests;
