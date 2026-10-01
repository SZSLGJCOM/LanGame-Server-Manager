use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn local_directory_to_open(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(String::from("path is required"));
    }

    #[cfg(windows)]
    let target = PathBuf::from(trimmed.replace('/', "\\"));
    #[cfg(not(windows))]
    let target = PathBuf::from(trimmed);

    let metadata = fs::metadata(&target)
        .map_err(|error| format!("failed to inspect path {}: {error}", target.display()))?;
    let directory = if metadata.is_file() {
        target
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    } else if metadata.is_dir() {
        &target
    } else {
        return Err(format!(
            "path is not a file or directory: {}",
            target.display()
        ));
    };
    explorer_directory_argument(directory)
}

pub(super) fn explorer_directory_argument(directory: &Path) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(directory)
        .map_err(|error| format!("failed to resolve path {}: {error}", directory.display()))?;
    // Explorer can interpret forward slashes as switches. Absolute native paths
    // also prevent relative names from being treated as shell destinations.
    // Keep verbatim paths when shortening them would change their meaning.
    #[cfg(windows)]
    {
        Ok(dunce::simplified(&absolute).to_path_buf())
    }
    #[cfg(not(windows))]
    {
        Ok(absolute)
    }
}
