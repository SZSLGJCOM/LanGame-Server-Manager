use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

use crate::instance_file_patch::io::{guard_directories, is_link};

const DOCUMENT_BYTES: usize = 3 * 1024 * 1024;
const DOCUMENTS: usize = 16;
const RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const IO_TIMEOUT: Duration = Duration::from_secs(30);
static ARCHIVE_IO: std::sync::LazyLock<Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(1)));

/// A bounded, application-private journal beside the selected database.
/// Payload schemas and recovery semantics belong to the assistant application.
#[derive(Debug, Clone)]
pub struct AssistantSessionArchive {
    root: PathBuf,
}

pub fn assistant_session_binding_digest(identity: &[u8]) -> String {
    Sha256::digest(identity)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl AssistantSessionArchive {
    pub fn for_database(database: &Path) -> Result<Self, String> {
        if !database.is_absolute()
            || database
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(
                "Assistant archive requires an absolute database path without traversal.".into(),
            );
        }
        let parent = database
            .parent()
            .ok_or("Assistant database has no parent.")?;
        Ok(Self {
            root: parent.join("assistant-sessions"),
        })
    }

    pub async fn load(&self) -> Result<Vec<(String, Vec<u8>)>, String> {
        self.run(|archive| {
            let _guards = archive.prepare()?;
            let mut records = Vec::new();
            for (path, _) in archive.retained()? {
                let id = path.file_stem().and_then(|value| value.to_str()).ok_or("Invalid assistant record name.")?.to_owned();
                let mut options = OpenOptions::new();
                options.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
                    options.share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
                }
                let file = options.open(&path).map_err(io_error)?;
                let metadata = file.metadata().map_err(io_error)?;
                if !metadata.is_file() || is_link(&metadata) || metadata.len() > DOCUMENT_BYTES as u64 {
                    return Err("Assistant archive contains an unsafe or oversized record; no task was restored.".into());
                }
                let mut bytes = Vec::new();
                file.take(DOCUMENT_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(io_error)?;
                if bytes.len() > DOCUMENT_BYTES { return Err("Assistant record exceeds its byte limit.".into()); }
                records.push((id, bytes));
            }
            Ok(records)
        }).await
    }

    pub async fn write(&self, id: String, bytes: Vec<u8>) -> Result<(), String> {
        validate_id(&id)?;
        if bytes.len() > DOCUMENT_BYTES {
            return Err("Assistant record exceeds its byte limit.".into());
        }
        self.run(move |archive| {
            let _guards = archive.prepare()?;
            let mut retained = archive.retained()?;
            let path = archive.root.join(format!("{id}.json"));
            if !retained.iter().any(|(candidate, _)| candidate == &path)
                && retained.len() >= DOCUMENTS
            {
                retained.sort_by_key(|(_, modified)| *modified);
                fs::remove_file(&retained[0].0).map_err(io_error)?;
            }
            ensure_plain_file_or_missing(&path)?;
            let temporary = archive.root.join(format!("{id}.pending"));
            ensure_plain_file_or_missing(&temporary)?;
            // A previous process may have stopped before replacement. Never
            // recover its partial bytes or treat them as committed evidence.
            remove_optional(&temporary)?;
            crate::atomic_file::write_file_atomically_with_fixed_sibling(&path, &bytes, &temporary)
                .map_err(io_error)
        })
        .await
    }

    pub async fn remove(&self, id: String) -> Result<(), String> {
        validate_id(&id)?;
        self.run(move |archive| {
            let _guards = archive.prepare()?;
            for extension in ["json", "pending"] {
                let path = archive.root.join(format!("{id}.{extension}"));
                ensure_plain_file_or_missing(&path)?;
                remove_optional(&path)?;
            }
            Ok(())
        })
        .await
    }

    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(Self) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        self.run_with_deadline(
            ARCHIVE_IO.clone(),
            tokio::time::sleep(IO_TIMEOUT),
            operation,
        )
        .await
    }

    async fn run_with_deadline<T: Send + 'static>(
        &self,
        slots: Arc<tokio::sync::Semaphore>,
        deadline: impl std::future::Future<Output = ()> + Send,
        operation: impl FnOnce(Self) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        // One timer covers both queueing and I/O; admission never renews it.
        tokio::pin!(deadline);
        let permit = tokio::select! {
            biased;
            () = &mut deadline => return Err("Assistant archive timed out waiting for its I/O slot; this operation was not started.".into()),
            permit = slots.acquire_owned() => permit.map_err(|_| "Assistant archive worker is unavailable.")?,
        };
        let archive = self.clone();
        let worker = tokio::task::spawn_blocking(move || {
            // Timing out or cancelling the async caller cannot stop filesystem
            // I/O. The actual worker retains admission until it really exits.
            let _permit = permit;
            operation(archive)
        });
        tokio::select! {
            biased;
            () = &mut deadline => Err("Assistant archive I/O timed out; its outcome is unknown and it may still finish. Inspect the archive before retrying.".into()),
            result = worker => result.map_err(|_| "Assistant archive worker stopped before completing its operation; its outcome is unknown.".to_owned())?,
        }
    }

    fn prepare(&self) -> Result<Vec<File>, String> {
        let parent = self
            .root
            .parent()
            .ok_or("Assistant archive has no parent.")?;
        let mut guards = guard_directories(parent)
            .map_err(|_| "Assistant archive parent must be an existing plain directory.")?;
        match fs::create_dir(&self.root) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                        .map_err(io_error)?;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(io_error(error)),
        }
        guards.extend(
            guard_directories(&self.root)
                .map_err(|_| "Assistant archive must be a plain directory.")?,
        );
        Ok(guards)
    }

    fn retained(&self) -> Result<Vec<(PathBuf, SystemTime)>, String> {
        let mut documents = Vec::new();
        for (index, entry) in fs::read_dir(&self.root).map_err(io_error)?.enumerate() {
            if index > DOCUMENTS * 2 {
                return Err("Assistant archive entry limit exceeded; no task was restored.".into());
            }
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            let Some(id) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_id(id).is_err() {
                continue;
            }
            if !matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("json" | "pending")
            ) {
                continue;
            }
            ensure_plain_file_or_missing(&path)?;
            let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
            let modified = metadata.modified().map_err(io_error)?;
            if path.extension().is_some_and(|value| value == "pending")
                || SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default()
                    >= RETENTION
            {
                fs::remove_file(&path).map_err(io_error)?;
                continue;
            }
            documents.push((path, modified));
        }
        if documents.len() > DOCUMENTS {
            return Err("Assistant archive session limit exceeded.".into());
        }
        Ok(documents)
    }
}

fn validate_id(id: &str) -> Result<(), String> {
    if id.len() == 32 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("Invalid assistant session identifier.".into())
    }
}

fn ensure_plain_file_or_missing(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(()),
        Ok(_) => Err("Assistant archive records cannot be links or directories.".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn remove_optional(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn io_error(error: std::io::Error) -> String {
    format!("Assistant archive I/O failed: {error}")
}

#[cfg(test)]
#[path = "assistant_session_archive_tests.rs"]
mod tests;
