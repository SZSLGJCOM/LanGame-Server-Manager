use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::instance_file_patch::{MAX_FILE_BYTES, instance_root, invalid, io, sha256};
use crate::{StorageError, StoragePaths};

const MAX_SCAN_ENTRIES: usize = 2048;
const MAX_DEPTH: usize = 12;
const PAGE_SIZE: usize = 64;
const PAGE_ENTRY_BYTES: usize = 8192;

/// Constructed from a stored instance, never from a model-supplied filesystem root.
#[derive(Clone)]
pub struct InstanceWorkspace {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceWorkspaceEntry {
    pub file: String,
    pub byte_length: u64,
    pub editable: bool,
    pub protection_reason: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceWorkspacePage {
    pub files: Vec<InstanceWorkspaceEntry>,
    pub directories: Vec<String>,
    pub next_offset: Option<usize>,
    pub next_file_offset: Option<usize>,
    pub next_directory_offset: Option<usize>,
    pub listing_sha256: String,
    pub scan_truncated: bool,
    pub scanned_entries: usize,
}

pub struct InstanceWorkspaceDocument {
    pub entry: InstanceWorkspaceEntry,
    pub source_sha256: String,
    /// Consumers must redact the entire document before search or pagination.
    pub content: String,
}

pub async fn open_instance_workspace(
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<InstanceWorkspace, StorageError> {
    Ok(InstanceWorkspace {
        root: instance_root(paths, instance_id, false).await?,
    })
}

impl InstanceWorkspace {
    pub fn list_files(
        &self,
        directory: &str,
        offset: usize,
    ) -> Result<InstanceWorkspacePage, StorageError> {
        if !directory.is_empty() {
            validate_relative_path(directory)?;
            ensure_readable_path(directory)?;
        }
        if offset > MAX_SCAN_ENTRIES {
            return Err(invalid(
                Path::new(directory),
                "Workspace page offset is too large.",
            ));
        }
        let start = self.root.join(directory);
        let mut result = InstanceWorkspacePage {
            files: Vec::new(),
            directories: Vec::new(),
            next_offset: None,
            next_file_offset: None,
            next_directory_offset: None,
            listing_sha256: String::new(),
            scan_truncated: false,
            scanned_entries: 0,
        };
        self.scan(&start, 0, self.private_runtime(), &mut result)?;
        result
            .files
            .sort_by(|left, right| left.file.cmp(&right.file));
        result.directories.sort();
        let catalog = serde_json::to_vec(&(
            &self.root,
            directory,
            &result.files,
            &result.directories,
            result.scan_truncated,
            result.scanned_entries,
        ))
        .map_err(|error| invalid(Path::new(directory), error.to_string()))?;
        result.listing_sha256 = sha256(&catalog);
        // Reserve room for the first directory so a long file page cannot
        // starve directory discovery. Both streams advance independently.
        let directory_reserve = result
            .directories
            .get(offset)
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|error| invalid(Path::new(directory), error.to_string()))?
            .map_or(0, |bytes| bytes.len() + 1);
        let mut remaining = PAGE_ENTRY_BYTES - directory_reserve;
        result.next_file_offset = paginate_entries(&mut result.files, offset, &mut remaining)?;
        remaining += directory_reserve;
        result.next_directory_offset =
            paginate_entries(&mut result.directories, offset, &mut remaining)?;
        result.next_offset = [result.next_file_offset, result.next_directory_offset]
            .into_iter()
            .flatten()
            .min();
        Ok(result)
    }

    pub fn read_file(&self, file: &str) -> Result<InstanceWorkspaceDocument, StorageError> {
        self.read_file_with_limit(file, MAX_FILE_BYTES)
    }

    /// Reads no more than the limit plus one overflow-detection byte, including
    /// when the file grows or decoding fails after its directory entry was read.
    pub fn read_file_with_limit(
        &self,
        file: &str,
        maximum: usize,
    ) -> Result<InstanceWorkspaceDocument, StorageError> {
        validate_relative_path(file)?;
        ensure_readable_path(file)?;
        if !supported_text(Path::new(file)) {
            return Err(invalid(
                Path::new(file),
                "Only supported instance text files can be read.",
            ));
        }
        let bytes = io::read_bytes_with_limit(&self.root.join(file), maximum)
            .map_err(|error| self.relative_error(file, error))?;
        let content = std::str::from_utf8(&bytes)
            .map_err(|_| invalid(Path::new(file), "Workspace text must be UTF-8; use configuration tools for BOM-encoded configuration."))?;
        if content.contains('\0') {
            return Err(invalid(
                Path::new(file),
                "Workspace text cannot contain NUL characters.",
            ));
        }
        let reason = edit_protection(file).or_else(|| {
            (runtime_path(file) && !self.private_runtime())
                .then_some("shared_or_unverified_runtime")
        });
        Ok(InstanceWorkspaceDocument {
            entry: InstanceWorkspaceEntry {
                file: file.into(),
                byte_length: bytes.len() as u64,
                editable: reason.is_none(),
                protection_reason: reason,
            },
            source_sha256: sha256(&bytes),
            content: content.to_owned(),
        })
    }

    fn private_runtime(&self) -> bool {
        io::read_bytes_with_limit(
            &self.root.join("runtime/.langame-private-runtime"),
            b"managed\n".len(),
        )
        .is_ok_and(|bytes| bytes == b"managed\n")
    }

    fn relative_error(&self, file: &str, error: StorageError) -> StorageError {
        invalid(
            Path::new(file),
            error
                .to_string()
                .replace(self.root.to_string_lossy().as_ref(), "<instance>"),
        )
    }

    fn scan(
        &self,
        directory: &Path,
        depth: usize,
        private_runtime: bool,
        result: &mut InstanceWorkspacePage,
    ) -> Result<(), StorageError> {
        if depth >= MAX_DEPTH || result.scanned_entries >= MAX_SCAN_ENTRIES {
            result.scan_truncated = true;
            return Ok(());
        }
        let relative_directory = directory
            .strip_prefix(&self.root)
            .map_err(|_| invalid(directory, "Directory is outside the bound instance."))?
            .to_string_lossy()
            .replace('\\', "/");
        let _guards = io::guard_directories(directory)
            .map_err(|error| self.relative_error(&relative_directory, error))?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(directory)
            .map_err(|error| invalid(Path::new(&relative_directory), error.to_string()))?
        {
            if result.scanned_entries >= MAX_SCAN_ENTRIES {
                result.scan_truncated = true;
                break;
            }
            result.scanned_entries += 1;
            entries.push(
                entry
                    .map_err(|error| invalid(Path::new(&relative_directory), error.to_string()))?,
            );
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let relative = path
                .strip_prefix(&self.root)
                .map_err(|_| invalid(&path, "File escaped instance root."))?
                .to_string_lossy()
                .replace('\\', "/");
            if validate_relative_path(&relative).is_err()
                || ensure_readable_path(&relative).is_err()
            {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .map_err(|error| invalid(Path::new(&relative), error.to_string()))?;
            if io::is_link(&metadata) {
                result.scan_truncated = true;
                continue;
            }
            if metadata.is_dir() {
                result.directories.push(relative);
                self.scan(&path, depth + 1, private_runtime, result)?;
            } else if metadata.is_file() && supported_text(&path) {
                let reason = edit_protection(&relative).or_else(|| {
                    (runtime_path(&relative) && !private_runtime)
                        .then_some("shared_or_unverified_runtime")
                });
                result.files.push(InstanceWorkspaceEntry {
                    file: relative,
                    byte_length: metadata.len(),
                    editable: reason.is_none() && metadata.len() <= MAX_FILE_BYTES as u64,
                    protection_reason: if metadata.len() > MAX_FILE_BYTES as u64 {
                        Some("file_size_limit")
                    } else {
                        reason
                    },
                });
            }
        }
        Ok(())
    }
}

fn paginate_entries<T: Serialize>(
    entries: &mut Vec<T>,
    offset: usize,
    remaining: &mut usize,
) -> Result<Option<usize>, StorageError> {
    let count = entries.len();
    let mut selected = Vec::new();
    for entry in entries.drain(..).skip(offset).take(PAGE_SIZE) {
        let bytes = serde_json::to_vec(&entry)
            .map_err(|error| invalid(Path::new(""), error.to_string()))?
            .len()
            + 1;
        if bytes > *remaining {
            break;
        }
        *remaining -= bytes;
        selected.push(entry);
    }
    let end = offset.saturating_add(selected.len());
    *entries = selected;
    Ok((end < count).then_some(end))
}

pub(crate) fn validate_relative_path(file: &str) -> Result<(), StorageError> {
    if file.is_empty()
        || file.len() > 1024
        || file.contains('\\')
        || file.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.ends_with(['.', ' '])
                || device_component(part)
                || part.chars().any(|ch| {
                    ch.is_control() || matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|')
                })
        })
    {
        return Err(invalid(
            Path::new(file),
            "Use an instance-relative path without traversal, streams, wildcards or aliases.",
        ));
    }
    Ok(())
}

fn device_component(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || stem
            .strip_prefix("com")
            .or_else(|| stem.strip_prefix("lpt"))
            .is_some_and(|suffix| suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9'))
}

fn ensure_readable_path(file: &str) -> Result<(), StorageError> {
    let denied = file.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        lower.starts_with('.')
            || matches!(lower.as_str(), "backup" | "backups" | "credentials")
            || lower.split(['_', '-', '.']).any(|word| {
                matches!(
                    word,
                    "password"
                        | "passwords"
                        | "token"
                        | "tokens"
                        | "secret"
                        | "secrets"
                        | "credential"
                        | "credentials"
                        | "keyring"
                        | "keystore"
                )
            })
    });
    if denied {
        Err(invalid(
            Path::new(file),
            "Credential, backup and manager-private paths are excluded from workspace tools.",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn supported_text(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "lua"
                    | "json"
                    | "toml"
                    | "ini"
                    | "cfg"
                    | "conf"
                    | "txt"
                    | "properties"
                    | "xml"
                    | "yaml"
                    | "yml"
                    | "log"
                    | "md"
                    | "js"
                    | "ts"
                    | "cs"
                    | "py"
                    | "sh"
                    | "ps1"
                    | "bat"
                    | "cmd"
            )
        })
}

fn runtime_path(file: &str) -> bool {
    file.split('/')
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("runtime"))
}

/// Permissions depend on file ownership/layout, never on the wording of a request.
pub(crate) fn edit_protection(file: &str) -> Option<&'static str> {
    if validate_relative_path(file).is_err()
        || ensure_readable_path(file).is_err()
        || !supported_text(Path::new(file))
    {
        return Some("protected_path_or_file_type");
    }
    let lower = file.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    if parts.iter().any(|part| {
        matches!(
            *part,
            "config" | "configs" | "saved" | "saves" | "savegames" | "worlds" | "logs"
        )
    }) || matches!(
        parts.last().copied(),
        Some(
            "modoverrides.lua"
                | "leveldataoverride.lua"
                | "worldgenoverride.lua"
                | "dedicated_server_mods_setup.lua"
        )
    ) || matches!(
        Path::new(&lower)
            .extension()
            .and_then(|value| value.to_str()),
        Some("log")
    ) {
        return Some("managed_configuration_save_or_log");
    }
    let runtime_extension = parts.first() == Some(&"runtime")
        && parts.iter().enumerate().skip(1).any(|(index, part)| {
            matches!(*part, "mods" | "plugins" | "scripts") && index + 1 < parts.len()
        });
    let data_extension = parts.len() >= 3
        && parts[0] == "data"
        && matches!(parts[1], "mods" | "plugins" | "scripts");
    let dst_ugc = parts.len() >= 7
        && parts[..2] == ["data", "ugc"]
        && matches!(parts[2], "master" | "caves" | "islands" | "volcano")
        && parts[3..5] == ["content", "322330"]
        && parts[5].bytes().all(|byte| byte.is_ascii_digit());
    if runtime_extension || data_extension || dst_ugc {
        None
    } else {
        Some("not_a_private_extension_file")
    }
}

#[cfg(test)]
#[path = "instance_workspace_tests.rs"]
mod tests;
