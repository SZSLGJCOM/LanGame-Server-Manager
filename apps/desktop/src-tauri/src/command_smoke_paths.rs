use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const RUNTIME_ROOT_ENV: &str = "LANGAME_SMOKE_RUNTIME_ROOT";
const GAMES_ROOT_ENV: &str = "LANGAME_SMOKE_GAMES_ROOT";
const MAX_RUN_LABEL_BYTES: usize = 12;

pub(super) fn smoke_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("workspace root")
}

pub(super) fn smoke_games_root() -> PathBuf {
    env::var_os(GAMES_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("langame-smoke-cache/games"))
}

pub(super) fn resolve_smoke_games_root(
    module_environment: Option<&str>,
    fallback: &Path,
) -> PathBuf {
    module_environment
        .and_then(env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.to_path_buf())
}

pub(super) fn allocate_smoke_run_root(label: &str) -> io::Result<PathBuf> {
    if label.is_empty()
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "smoke run labels may contain only ASCII letters, digits, '-' and '_'",
        ));
    }

    let runtime_root = env::var_os(RUNTIME_ROOT_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env::temp_dir().join("langame-smoke-runs"));
    // SQLite adds sidecar suffixes such as `-wal` and `-journal`. Keep the
    // human-readable portion bounded so those files remain below the legacy
    // Windows path limit even when the managed test runner supplies a deep
    // process-temporary root. The UUID still provides the uniqueness boundary.
    let bounded_label = &label[..label.len().min(MAX_RUN_LABEL_BYTES)];
    let run_root = runtime_root.join(format!("{bounded_label}-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&run_root)?;
    Ok(run_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_roots_are_unique_and_reject_path_traversal() {
        let first = allocate_smoke_run_root("public-smoke-path").expect("first root");
        let second = allocate_smoke_run_root("public-smoke-path").expect("second root");
        let long_label = allocate_smoke_run_root("module-uninstall-command-tests")
            .expect("bounded long-label root");
        assert_ne!(first, second);
        assert!(first.is_dir());
        assert!(second.is_dir());
        assert!(
            long_label
                .file_name()
                .expect("long-label run directory name")
                .to_string_lossy()
                .starts_with("module-unins-")
        );
        assert!(allocate_smoke_run_root("../outside").is_err());
        fs::remove_dir_all(first).expect("remove first root");
        fs::remove_dir_all(second).expect("remove second root");
        fs::remove_dir_all(long_label).expect("remove long-label root");
    }
}
