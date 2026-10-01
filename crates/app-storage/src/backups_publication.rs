//! Short database fences for backup publication. Copying, hashing and disposal
//! run on the worker; only revalidated metadata and directory moves hold a writer.
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::mpsc;
use std::time::SystemTime;

use sqlx::{SqliteConnection, SqlitePool};
use tokio::sync::{mpsc as async_mpsc, oneshot};

use crate::StorageError;

pub(crate) type Validation<'a> =
    Pin<Box<dyn Future<Output = Result<(), StorageError>> + Send + 'a>>;

struct Request {
    admitted: mpsc::Sender<Result<(), StorageError>>,
    finished: oneshot::Receiver<()>,
    released: mpsc::Sender<Result<(), StorageError>>,
}

pub(crate) struct Publication {
    requests: Option<async_mpsc::Sender<Request>>,
}

impl Publication {
    #[cfg(test)]
    pub(crate) fn uncoordinated() -> Self {
        Self { requests: None }
    }

    pub(crate) fn publish<T>(
        &self,
        stamp: &Stamp,
        operation: impl FnOnce() -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        stamp.validate()?;
        let Some(requests) = &self.requests else {
            return operation();
        };
        let (admitted, admission) = mpsc::channel();
        let (finished, completion) = oneshot::channel();
        let (released, release) = mpsc::channel();
        requests
            .blocking_send(Request {
                admitted,
                finished: completion,
                released,
            })
            .map_err(|_| failure("backup publication coordinator stopped"))?;
        admission
            .recv()
            .map_err(|_| failure("backup admission was interrupted"))??;
        #[cfg(test)]
        if let Some(root) = stamp.roots.first() {
            crate::instance_archive::test_gate::pause(
                root,
                crate::instance_archive::test_gate::Point::BackupPublishing,
            );
        }
        // Dropping this sender also releases the fence after a worker panic.
        // Instance leases exclude supported content writers. Keep the final
        // writer fence bounded by scope count rather than descendant count.
        let result = stamp.validate_roots().and_then(|()| operation());
        let _ = finished.send(());
        let released = release
            .recv()
            .map_err(|_| failure("backup publication release was interrupted"))?;
        #[cfg(test)]
        let released = if stamp.roots.first().is_some_and(take_release_failure) {
            Err(failure("injected publication release failure"))
        } else {
            released
        };
        match (result, released) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(release)) => Err(failure(format!(
                "{error}; releasing publication failed: {release}"
            ))),
        }
    }
}

/// Call from a complete_mutation owner: cancellation must not detach the worker
/// from its instance lease or from the coordinator needed for compensation.
pub(crate) async fn run<T, V, W>(
    pool: &SqlitePool,
    mut validate: V,
    worker: W,
) -> Result<T, StorageError>
where
    T: Send + 'static,
    V: for<'a> FnMut(&'a mut SqliteConnection) -> Validation<'a>,
    W: FnOnce(Publication) -> Result<T, StorageError> + Send + 'static,
{
    let (requests, mut receiver) = async_mpsc::channel::<Request>(1);
    let mut task = tokio::task::spawn_blocking(move || {
        worker(Publication {
            requests: Some(requests),
        })
    });
    loop {
        tokio::select! {
            result = &mut task => return result.map_err(|error| failure(format!("backup worker failed: {error}")))?,
            request = receiver.recv() => {
                let Some(request) = request else {
                    return task.await.map_err(|error| failure(format!("backup worker failed: {error}")))?;
                };
                let mut tx = match pool.begin_with("BEGIN IMMEDIATE").await {
                    Ok(tx) => tx,
                    Err(error) => { let _ = request.admitted.send(Err(error.into())); continue; }
                };
                if let Err(error) = validate(&mut tx).await {
                    let rollback = tx.rollback().await;
                    let error = match rollback {
                        Ok(()) => error,
                        Err(rollback) => failure(format!("{error}; admission rollback failed: {rollback}")),
                    };
                    let _ = request.admitted.send(Err(error));
                    continue;
                }
                if request.admitted.send(Ok(())).is_ok() {
                    let _ = request.finished.await;
                }
                // These transactions reserve ownership but never modify DB rows.
                let result = tx.rollback().await.map_err(StorageError::from);
                let _ = request.released.send(result);
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    identity: String,
    directory: bool,
    bytes: u64,
    modified: SystemTime,
}

pub(crate) struct Stamp {
    roots: Vec<PathBuf>,
    entries: Vec<Entry>,
    root_entries: Vec<Entry>,
    recursive: bool,
}

impl Stamp {
    pub(crate) fn capture(roots: &[PathBuf]) -> Result<Self, StorageError> {
        Self::capture_inner(roots, true)
    }

    pub(crate) fn directories(roots: &[PathBuf]) -> Result<Self, StorageError> {
        Self::capture_inner(roots, false)
    }

    fn capture_inner(roots: &[PathBuf], recursive: bool) -> Result<Self, StorageError> {
        Self::capture_limited(roots, recursive, 200_000, 64)
    }

    fn capture_limited(
        roots: &[PathBuf],
        recursive: bool,
        entry_limit: usize,
        depth_limit: usize,
    ) -> Result<Self, StorageError> {
        let mut entries = Vec::new();
        let mut root_stamps = Vec::new();
        for root in roots {
            let mut pending = vec![(root.clone(), 0)];
            let mut root_entries = 0;
            while let Some((path, depth)) = pending.pop() {
                // Match the existing per-tree inventory limit, including its
                // root and leaf files inside a directory at depth 64. Copies
                // and distinct cluster scopes do not share an aggregate quota.
                if depth > depth_limit + 1 || root_entries > entry_limit {
                    return Err(failure("backup publication exceeds its metadata limit"));
                }
                let metadata = match std::fs::symlink_metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound && path == *root => {
                        continue;
                    }
                    Err(source) => return Err(StorageError::ReadPath { path, source }),
                };
                if super::is_link_or_reparse(&metadata)
                    || (!metadata.is_dir() && !metadata.is_file())
                {
                    return Err(failure(format!(
                        "unsafe backup publication entry: {}",
                        path.display()
                    )));
                }
                let directory = metadata.is_dir();
                if directory && depth > depth_limit {
                    return Err(failure(
                        "backup publication exceeds its directory depth limit",
                    ));
                }
                let node = crate::instance_archive_files::native::open(&path, directory, false)
                    .map_err(|source| StorageError::ReadPath {
                        path: path.clone(),
                        source,
                    })?;
                let identity = node.identity().map_err(|source| StorageError::ReadPath {
                    path: path.clone(),
                    source,
                })?;
                let entry = Entry {
                    path: path.clone(),
                    identity: serde_json::to_string(&identity)?,
                    directory,
                    bytes: if directory { 0 } else { metadata.len() },
                    modified: metadata
                        .modified()
                        .map_err(|source| StorageError::ReadPath {
                            path: path.clone(),
                            source,
                        })?,
                };
                if path == *root {
                    root_stamps.push(entry.clone());
                }
                entries.push(entry);
                root_entries += 1;
                if directory && recursive {
                    for child in
                        std::fs::read_dir(&path).map_err(|source| StorageError::ReadDirectory {
                            path: path.clone(),
                            source,
                        })?
                    {
                        let child = child.map_err(|source| StorageError::ReadDirectory {
                            path: path.clone(),
                            source,
                        })?;
                        if root_entries + pending.len() > entry_limit {
                            return Err(failure("backup publication exceeds its metadata limit"));
                        }
                        pending.push((child.path(), depth + 1));
                    }
                }
            }
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        root_stamps.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self {
            roots: roots.to_vec(),
            entries,
            root_entries: root_stamps,
            recursive,
        })
    }

    pub(crate) fn validate(&self) -> Result<(), StorageError> {
        if Self::capture_inner(&self.roots, self.recursive)?.entries != self.entries {
            return Err(failure(
                "backup files changed after content verification; retained every recovery copy",
            ));
        }
        Ok(())
    }

    fn validate_roots(&self) -> Result<(), StorageError> {
        let current = Self::capture_inner(&self.roots, false)?;
        if current.entries != self.root_entries {
            return Err(failure(
                "backup publication roots changed after verification",
            ));
        }
        Ok(())
    }
}

fn failure(message: impl Into<String>) -> StorageError {
    StorageError::ReadPath {
        path: Path::new("backup publication").to_owned(),
        source: std::io::Error::other(message.into()),
    }
}

#[cfg(test)]
fn release_failures() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static FAILURES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    FAILURES.get_or_init(Default::default)
}

#[cfg(test)]
pub(crate) fn fail_next_release(root: PathBuf) {
    assert!(release_failures().lock().unwrap().insert(root));
}

#[cfg(test)]
fn take_release_failure(root: &PathBuf) -> bool {
    release_failures().lock().unwrap().remove(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_limits_apply_per_original_scope_and_not_to_wrapper_directories() {
        let root = std::env::temp_dir().join(format!("backup-stamp-{}", uuid::Uuid::new_v4()));
        let live = root.join("live");
        let prepared = root.join("workspace/new");
        for path in [&live, &prepared] {
            std::fs::create_dir_all(path.join("nested")).unwrap();
            std::fs::write(path.join("nested/leaf"), b"fixture").unwrap();
        }
        let scopes = [live.clone(), prepared];
        // Each scope has two descendants, and a file may be one level below
        // the deepest supported directory. Two copies keep that same budget.
        assert!(Stamp::capture_limited(&scopes, true, 2, 1).is_ok());
        std::fs::write(live.join("extra"), b"over budget").unwrap();
        assert!(Stamp::capture_limited(&scopes, true, 2, 1).is_err());
        std::fs::remove_file(live.join("extra")).unwrap();
        std::fs::create_dir(live.join("nested/deeper")).unwrap();
        assert!(Stamp::capture_limited(&scopes, true, 4, 1).is_err());
        assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(root).unwrap();
    }
}
