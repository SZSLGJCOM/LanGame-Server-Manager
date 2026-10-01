use std::collections::HashSet;

use super::*;

const MAX_FILES: usize = 8;
const MAX_EDITS: usize = 16;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceTextEdit {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceFileEdits {
    pub file: String,
    pub source_sha256: String,
    pub edits: Vec<InstanceTextEdit>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFileEditsPreview {
    pub file: String,
    pub source_sha256: String,
    pub result_sha256: String,
    pub edits: Vec<InstanceTextEdit>,
}

/// Complete sources remain in native confirmation state, never in the model plan.
#[derive(Clone)]
pub struct PreparedInstanceFilePatches {
    patches: Vec<PreparedInstanceTextPatch>,
    previews: Vec<InstanceFileEditsPreview>,
}

impl std::fmt::Debug for PreparedInstanceFilePatches {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedInstanceFilePatches")
            .field("files", &self.patches)
            .finish_non_exhaustive()
    }
}

impl PreparedInstanceFilePatches {
    pub fn previews(&self) -> &[InstanceFileEditsPreview] {
        &self.previews
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceFilePatchState {
    NotApplied,
    Applied,
    RolledBack,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceFilePatchesStatus {
    Applied,
    NotApplied,
    RolledBack,
    Partial,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFilePatchOutcome {
    pub file: String,
    pub source_sha256: String,
    pub result_sha256: String,
    pub backup_id: Option<String>,
    pub state: InstanceFilePatchState,
    pub read_back_verified: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceFilePatchesResult {
    pub status: InstanceFilePatchesStatus,
    pub files: Vec<InstanceFilePatchOutcome>,
    pub error: Option<String>,
}

pub fn validate_instance_file_edits(files: &[InstanceFileEdits]) -> Result<(), StorageError> {
    if files.is_empty() || files.len() > MAX_FILES {
        return Err(invalid(
            Path::new(""),
            "Supply between 1 and 8 existing instance files.",
        ));
    }
    let mut seen = HashSet::new();
    for file in files {
        let path = Path::new(&file.file);
        validate_relative_file(&file.file)?;
        if !seen.insert(file.file.to_lowercase()) {
            return Err(invalid(
                path,
                "Each file may occur only once, including case aliases.",
            ));
        }
        if file.source_sha256.len() != 64
            || !file
                .source_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(invalid(
                path,
                "Supply the exact lowercase source SHA256 from a file read.",
            ));
        }
        if file.edits.is_empty() || file.edits.len() > MAX_EDITS {
            return Err(invalid(
                path,
                "Each file requires 1 to 16 exact text edits.",
            ));
        }
        let mut bytes = 0_usize;
        for edit in &file.edits {
            bytes = bytes
                .saturating_add(edit.before.len())
                .saturating_add(edit.after.len());
            if edit.before.is_empty()
                || edit.before == edit.after
                || bytes > MAX_PATCH_BYTES
                || edit.before.contains('\0')
                || edit.after.contains('\0')
            {
                return Err(invalid(
                    path,
                    "Edits need nonempty changed matches, no NUL, and at most 8 KiB of patch text per file.",
                ));
            }
        }
    }
    Ok(())
}

pub async fn prepare_instance_file_patches(
    paths: &StoragePaths,
    instance_id: &str,
    files: Vec<InstanceFileEdits>,
) -> Result<PreparedInstanceFilePatches, StorageError> {
    validate_instance_file_edits(&files)?;
    let root = instance_root(paths, instance_id, true).await?;
    let instance_id = instance_id.to_owned();
    blocking(move || prepare_patches(&root, &instance_id, files)).await
}

fn prepare_patches(
    root: &Path,
    instance_id: &str,
    files: Vec<InstanceFileEdits>,
) -> Result<PreparedInstanceFilePatches, StorageError> {
    validate_instance_file_edits(&files)?;
    let mut prepared = PreparedInstanceFilePatches {
        patches: Vec::new(),
        previews: Vec::new(),
    };
    for file in files {
        let source = read_file(root, &file.file)?;
        let path = root.join(&file.file);
        if source.source_sha256 != file.source_sha256 {
            return Err(invalid(
                &path,
                "A source changed; read every affected file again before proposing edits.",
            ));
        }
        // Every edit addresses the same original document, not an intermediate result.
        let mut ranges = Vec::new();
        for edit in &file.edits {
            let start = source
                .content
                .find(&edit.before)
                .ok_or_else(|| invalid(&path, "Each before segment must match exactly once."))?;
            let next = start + edit.before.chars().next().map(char::len_utf8).unwrap_or(0);
            if source.content[next..].contains(&edit.before) {
                return Err(invalid(
                    &path,
                    "Each before segment must match exactly once, including overlapping matches.",
                ));
            }
            ranges.push((start, start + edit.before.len(), edit.after.as_str()));
        }
        ranges.sort_by_key(|range| range.0);
        if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
            return Err(invalid(&path, "Text edits overlap in the original file."));
        }
        let mut replacement = String::new();
        let mut offset = 0;
        for (start, end, after) in ranges {
            replacement.push_str(&source.content[offset..start]);
            replacement.push_str(after);
            offset = end;
        }
        replacement.push_str(&source.content[offset..]);
        if replacement.is_empty()
            || replacement.len() > MAX_FILE_BYTES
            || replacement == source.content
        {
            return Err(invalid(
                &path,
                "The edited document must change and remain nonempty within 256 KiB.",
            ));
        }
        let result_sha256 = sha256(replacement.as_bytes());
        prepared.previews.push(InstanceFileEditsPreview {
            file: file.file.clone(),
            source_sha256: source.source_sha256.clone(),
            result_sha256: result_sha256.clone(),
            edits: file.edits,
        });
        prepared.patches.push(PreparedInstanceTextPatch {
            instance_id: instance_id.to_owned(),
            root: root.to_path_buf(),
            original: source.content.into_bytes(),
            replacement: replacement.into_bytes(),
            preview: InstanceTextPatchPreview {
                file: file.file,
                source_sha256: source.source_sha256,
                result_sha256,
                before: String::new(),
                after: String::new(),
            },
        });
    }
    Ok(prepared)
}

/// The caller holds the instance lifecycle permit. The storage lease remains
/// owned by the worker through cancellation, all writes and any rollback.
pub async fn apply_instance_file_patches(
    paths: &StoragePaths,
    instance_id: &str,
    prepared: PreparedInstanceFilePatches,
) -> Result<InstanceFilePatchesResult, StorageError> {
    let lease = acquire_instance_settings_mutation_lock(paths, instance_id)?;
    let root = instance_root(paths, instance_id, true).await?;
    if prepared.patches.is_empty()
        || prepared
            .patches
            .iter()
            .any(|patch| patch.instance_id != instance_id || patch.root != root)
    {
        return Err(invalid(
            &root,
            "The prepared edits belong to a different instance root.",
        ));
    }
    lease
        .spawn_blocking(move || apply_patches(&prepared))
        .await
        .map_err(|error| StorageError::BlockingTaskFailed {
            operation: "applying instance file edits",
            message: error.to_string(),
        })?
}

#[path = "instance_file_patches_io.rs"]
mod writes;
use writes::apply_patches;

#[cfg(test)]
#[path = "instance_file_patches_tests.rs"]
mod tests;
