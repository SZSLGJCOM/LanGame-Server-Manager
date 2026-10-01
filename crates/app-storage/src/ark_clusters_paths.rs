use std::path::{Path, PathBuf};

/// Cluster transfers are explicitly shared resources, so resolve filesystem aliases.
/// Missing tails are allowed before the first launch; no directories are created.
pub(super) fn canonical_cluster_directory(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path.to_owned();
    let mut missing = Vec::new();
    loop {
        match std::fs::metadata(&existing) {
            Ok(metadata) if metadata.is_dir() => break,
            Ok(_) => {
                return Err(format!(
                    "Cluster path is not a directory: {}",
                    existing.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    existing
                        .file_name()
                        .ok_or_else(|| {
                            format!(
                                "Cluster path has no accessible ancestor: {}",
                                path.display()
                            )
                        })?
                        .to_owned(),
                );
                if !existing.pop() {
                    return Err(format!(
                        "Cluster path has no accessible ancestor: {}",
                        path.display()
                    ));
                }
            }
            Err(error) => {
                return Err(format!(
                    "Cannot inspect cluster directory {}: {error}",
                    existing.display()
                ));
            }
        }
    }
    let mut canonical = std::fs::canonicalize(&existing).map_err(|error| {
        format!(
            "Cannot resolve cluster directory {}: {error}",
            existing.display()
        )
    })?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

pub(super) fn directory_key(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    #[cfg(windows)]
    {
        value.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        value
    }
}
