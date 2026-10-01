use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::instance_settings_lock::acquire_instance_settings_mutation_lock;
use crate::storage_db::{connect_pool, fetch_instance_record};
use crate::{InstanceStatus, StorageError, StoragePaths};

#[path = "instance_file_patches.rs"]
mod batch;
#[path = "instance_file_patch_io.rs"]
pub(crate) mod io;
pub use batch::{
    InstanceFileEdits, InstanceFileEditsPreview, InstanceFilePatchOutcome, InstanceFilePatchState,
    InstanceFilePatchesResult, InstanceFilePatchesStatus, InstanceTextEdit,
    PreparedInstanceFilePatches, apply_instance_file_patches, prepare_instance_file_patches,
    validate_instance_file_edits,
};

pub(crate) const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_PATCH_BYTES: usize = 8 * 1024;
const MAX_LIST_FILES: usize = 64;
const MAX_SCAN_ENTRIES: usize = 512;
const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceTextPatch {
    pub file: String,
    pub source_sha256: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTextPatchPreview {
    pub file: String,
    pub source_sha256: String,
    pub result_sha256: String,
    pub before: String,
    pub after: String,
}

/// The full source stays in the native confirmation store, never in a model plan.
#[derive(Clone)]
pub struct PreparedInstanceTextPatch {
    instance_id: String,
    root: PathBuf,
    original: Vec<u8>,
    replacement: Vec<u8>,
    preview: InstanceTextPatchPreview,
}

impl std::fmt::Debug for PreparedInstanceTextPatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedInstanceTextPatch")
            .field("instance_id", &self.instance_id)
            .field("file", &self.preview.file)
            .field("source_sha256", &self.preview.source_sha256)
            .field("result_sha256", &self.preview.result_sha256)
            .finish_non_exhaustive()
    }
}

impl PreparedInstanceTextPatch {
    pub fn preview(&self) -> &InstanceTextPatchPreview {
        &self.preview
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatchFile {
    pub file: String,
    pub source_sha256: String,
    /// The caller must redact this complete document before model pagination.
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstancePatchFileList {
    pub files: Vec<String>,
    pub truncated: bool,
    pub scanned_entries: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFilePatchResult {
    pub file: String,
    pub source_sha256: String,
    pub result_sha256: String,
    pub backup_id: String,
    pub read_back_verified: bool,
}

pub(crate) fn invalid(path: &Path, message: impl Into<String>) -> StorageError {
    StorageError::WriteConfig {
        path: path.to_path_buf(),
        source: std::io::Error::other(message.into()),
    }
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

pub(crate) async fn instance_root(
    paths: &StoragePaths,
    instance_id: &str,
    require_stopped: bool,
) -> Result<PathBuf, StorageError> {
    let pool = connect_pool(paths).await?;
    let record = fetch_instance_record(&pool, instance_id).await?;
    pool.close().await;
    if require_stopped
        && (!matches!(record.summary.status, InstanceStatus::Stopped)
            || record.summary.active_process_count != 0)
    {
        return Err(invalid(
            &record.config_dir,
            "Stop the instance before modifying private extension files.",
        ));
    }
    let root = record.config_dir.parent().ok_or_else(|| {
        invalid(
            &record.config_dir,
            "Instance configuration has no managed parent.",
        )
    })?;
    let relative = root
        .strip_prefix(&paths.instances_root)
        .map_err(|_| invalid(root, "Instance root is outside managed storage."))?;
    if !root.is_absolute()
        || relative.components().count() != 1
        || !matches!(
            relative.components().next(),
            Some(std::path::Component::Normal(_))
        )
        || relative == Path::new(".trash")
    {
        return Err(invalid(
            root,
            "Instance root must be a direct managed instance directory.",
        ));
    }
    Ok(root.to_path_buf())
}

pub async fn list_instance_patch_files(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstancePatchFileList, StorageError> {
    let root = instance_root(paths, instance_id, false).await?;
    blocking(move || list_files(&root)).await
}

pub async fn read_instance_patch_file(
    paths: &StoragePaths,
    instance_id: &str,
    file: &str,
) -> Result<InstancePatchFile, StorageError> {
    let root = instance_root(paths, instance_id, false).await?;
    let file = file.to_owned();
    blocking(move || read_file(&root, &file)).await
}

pub async fn prepare_instance_file_patch(
    paths: &StoragePaths,
    instance_id: &str,
    patch: InstanceTextPatch,
) -> Result<PreparedInstanceTextPatch, StorageError> {
    let root = instance_root(paths, instance_id, true).await?;
    let instance_id = instance_id.to_owned();
    blocking(move || prepare_patch(&root, &instance_id, patch)).await
}

/// The desktop additionally holds its instance lifecycle permit to exclude starts.
/// This storage lease survives cancellation until the filesystem worker finishes.
pub async fn apply_instance_file_patch(
    paths: &StoragePaths,
    instance_id: &str,
    prepared: PreparedInstanceTextPatch,
) -> Result<InstanceFilePatchResult, StorageError> {
    let lease = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let root = instance_root(paths, instance_id, true).await?;
    if prepared.instance_id != instance_id || prepared.root != root {
        return Err(invalid(
            &root,
            "The prepared patch belongs to a different instance root.",
        ));
    }
    lease
        .spawn_blocking(move || io::apply_patch(&prepared))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "applying instance file patch",
            message: error.to_string(),
        })?
}

async fn blocking<T, F>(operation: F) -> Result<T, StorageError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, StorageError> + Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "reading instance patch files",
            message: error.to_string(),
        })?
}

fn validate_relative_file(file: &str) -> Result<(), StorageError> {
    crate::instance_workspace::validate_relative_path(file)?;
    if let Some(reason) = crate::instance_workspace::edit_protection(file) {
        return Err(invalid(
            Path::new(file),
            format!(
                "Only private extension text can be patched: {reason}. Use declared settings for managed configuration."
            ),
        ));
    }
    Ok(())
}

fn validate_runtime_marker(root: &Path, file: &str) -> Result<(), StorageError> {
    if file
        .split('/')
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("runtime"))
        && io::read_bytes(&root.join("runtime/.langame-private-runtime"))? != b"managed\n"
    {
        return Err(invalid(root, "The private runtime marker is invalid."));
    }
    Ok(())
}

fn read_file(root: &Path, file: &str) -> Result<InstancePatchFile, StorageError> {
    validate_relative_file(file)?;
    validate_runtime_marker(root, file)?;
    let path = root.join(file);
    let bytes = io::read_bytes(&path)?;
    let content = text(&path, &bytes)?.to_owned();
    Ok(InstancePatchFile {
        file: file.to_owned(),
        source_sha256: sha256(&bytes),
        content,
    })
}

fn text<'a>(path: &Path, bytes: &'a [u8]) -> Result<&'a str, StorageError> {
    let text = std::str::from_utf8(bytes).map_err(|_| {
        invalid(
            path,
            "Mod text must be valid UTF-8; encoding conversion is not supported.",
        )
    })?;
    if text.contains('\0') {
        return Err(invalid(path, "Mod text cannot contain NUL characters."));
    }
    Ok(text)
}

fn prepare_patch(
    root: &Path,
    instance_id: &str,
    patch: InstanceTextPatch,
) -> Result<PreparedInstanceTextPatch, StorageError> {
    let path = root.join(&patch.file);
    if patch.before.is_empty()
        || patch.before == patch.after
        || patch.before.len().saturating_add(patch.after.len()) > MAX_PATCH_BYTES
        || patch.before.contains('\0')
        || patch.after.contains('\0')
    {
        return Err(invalid(
            &path,
            "Provide one nonempty exact replacement with different content and at most 8 KiB total patch text.",
        ));
    }
    let source = read_file(root, &patch.file)?;
    if source.source_sha256 != patch.source_sha256 {
        return Err(invalid(
            &path,
            "The Mod file changed; read it again before preparing a patch.",
        ));
    }
    let first_match = source.content.find(&patch.before);
    let unique_match = first_match.is_some_and(|start| {
        let next_boundary = start + patch.before.chars().next().map(char::len_utf8).unwrap_or(0);
        !source.content[next_boundary..].contains(&patch.before)
    });
    if !unique_match {
        return Err(invalid(
            &path,
            "The before text must match exactly once in the complete source file.",
        ));
    }
    let replacement = source
        .content
        .replacen(&patch.before, &patch.after, 1)
        .into_bytes();
    if replacement.len() > MAX_FILE_BYTES || replacement.is_empty() {
        return Err(invalid(
            &path,
            "Patched file must remain nonempty and within 256 KiB.",
        ));
    }
    let preview = InstanceTextPatchPreview {
        file: patch.file,
        source_sha256: source.source_sha256,
        result_sha256: sha256(&replacement),
        before: patch.before,
        after: patch.after,
    };
    Ok(PreparedInstanceTextPatch {
        instance_id: instance_id.to_owned(),
        root: root.to_path_buf(),
        original: source.content.into_bytes(),
        replacement,
        preview,
    })
}

fn list_files(root: &Path) -> Result<InstancePatchFileList, StorageError> {
    let _guards = io::guard_directories(root)?;
    let mut result = InstancePatchFileList {
        files: Vec::new(),
        truncated: false,
        scanned_entries: 0,
    };
    let mut directories = vec![
        root.join("data/ugc/Master/content/322330"),
        root.join("data/ugc/Caves/content/322330"),
        root.join("data/mods"),
        root.join("data/plugins"),
        root.join("data/scripts"),
    ];
    if root
        .join("runtime/.langame-private-runtime")
        .try_exists()
        .map_err(|error| invalid(root, error.to_string()))?
    {
        validate_runtime_marker(root, "runtime/mods")?;
        directories.push(root.join("runtime"));
    }
    for directory in directories {
        if directory
            .try_exists()
            .map_err(|error| invalid(&directory, error.to_string()))?
        {
            scan_files(root, &directory, 0, &mut result)?;
        }
    }
    result.files.sort();
    Ok(result)
}

fn scan_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    result: &mut InstancePatchFileList,
) -> Result<(), StorageError> {
    if result.files.len() >= MAX_LIST_FILES
        || result.scanned_entries >= MAX_SCAN_ENTRIES
        || depth >= MAX_DEPTH
    {
        result.truncated = true;
        return Ok(());
    }
    let _guards = io::guard_directories(directory)?;
    for entry in fs::read_dir(directory).map_err(|error| invalid(directory, error.to_string()))? {
        if result.files.len() >= MAX_LIST_FILES || result.scanned_entries >= MAX_SCAN_ENTRIES {
            result.truncated = true;
            break;
        }
        result.scanned_entries += 1;
        let entry = entry.map_err(|error| invalid(directory, error.to_string()))?;
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| invalid(&entry.path(), error.to_string()))?;
        if io::is_link(&metadata) {
            continue;
        }
        if metadata.is_dir() {
            scan_files(root, &entry.path(), depth + 1, result)?;
        } else if metadata.is_file() && metadata.len() <= MAX_FILE_BYTES as u64 {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| invalid(directory, error.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if validate_relative_file(&relative).is_ok() {
                result.files.push(relative);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "instance_file_patch_tests.rs"]
mod tests;
