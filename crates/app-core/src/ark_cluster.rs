use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArkClusterError {
    pub field: &'static str,
    pub message: &'static str,
}

impl std::fmt::Display for ArkClusterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ArkClusterError {}

/// Resolve an explicitly shared cluster path without migrating existing uploads.
/// An empty directory keeps the instance-owned path used by existing settings.
pub fn resolve_cluster_directory(
    cluster_id: &str,
    cluster_directory: &str,
    saves_dir: &Path,
) -> Result<Option<PathBuf>, ArkClusterError> {
    if cluster_id.chars().count() > 128 || cluster_id.chars().any(unsafe_argument_character) {
        return Err(ArkClusterError {
            field: "cluster_id",
            message: "Use at most 128 characters without quotes, control characters or line breaks.",
        });
    }
    if cluster_directory.chars().count() > 4096
        || cluster_directory.chars().any(unsafe_argument_character)
    {
        return Err(ArkClusterError {
            field: "cluster_directory",
            message: "Use an unquoted directory path without control characters or line breaks (at most 4096 characters).",
        });
    }
    let id = cluster_id.trim();
    let directory = cluster_directory.trim();
    if id.is_empty() {
        return if directory.is_empty() {
            Ok(None)
        } else {
            Err(ArkClusterError {
                field: "cluster_directory",
                message: "Set a Cluster ID before choosing a shared cluster directory.",
            })
        };
    }
    if directory.is_empty() {
        return Ok(Some(saves_dir.join("cluster")));
    }
    let path = {
        let normalized = directory.replace('\\', "/");
        let drive_path = normalized.as_bytes().get(1) == Some(&b':')
            && normalized.as_bytes()[0].is_ascii_alphabetic()
            && normalized.as_bytes().get(2) == Some(&b'/');
        let unc_path = normalized.starts_with("//")
            && normalized[2..]
                .split('/')
                .filter(|part| !part.is_empty())
                .count()
                >= 2;
        let native_path = Path::new(&normalized).is_absolute() && !normalized.starts_with("//");
        let components = if drive_path {
            &normalized[3..]
        } else {
            &normalized
        };
        if !(drive_path || unc_path || native_path)
            || components
                .chars()
                .any(|character| matches!(character, ':' | '<' | '>' | '|' | '?' | '*'))
            || components.split('/').any(|part| {
                part == "." || part == ".." || part.ends_with(' ') || part.ends_with('.')
            })
        {
            return Err(ArkClusterError {
                field: "cluster_directory",
                message: "Choose an absolute directory (drive path or UNC share) without parent traversal, device prefixes or invalid path characters.",
            });
        }
        PathBuf::from(normalized)
    };
    match std::fs::metadata(&path) {
        Ok(metadata) if !metadata.is_dir() => Err(ArkClusterError {
            field: "cluster_directory",
            message: "The cluster directory points to an existing file. Choose a directory.",
        }),
        Ok(_) => Ok(Some(path)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(path)),
        Err(_) => Err(ArkClusterError {
            field: "cluster_directory",
            message: "The cluster directory cannot be inspected. Check the path and access permissions.",
        }),
    }
}

fn unsafe_argument_character(character: char) -> bool {
    character.is_control() || matches!(character, '"' | '\u{2028}' | '\u{2029}')
}
