use std::path::{Component, Path, PathBuf};

/// Core Keeper emits undated native output. Its logfile must belong to the
/// same launch as the recorded console, outside managed-console rotation.
/// This pure mapping is shared by launch, health and native acceptance readers.
pub fn corekeeper_native_log_path(console_log: &Path) -> Option<PathBuf> {
    if !console_log.is_absolute()
        || console_log
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return None;
    }
    let name = console_log.file_name()?.to_str()?;
    let stamp = name.strip_prefix("run-")?.strip_suffix("-main.log")?;
    if stamp.is_empty()
        || !stamp.bytes().all(|byte| byte.is_ascii_digit())
        || stamp.parse::<u128>().ok()?.to_string() != stamp
    {
        return None;
    }
    let parent = console_log.parent()?;
    let logs = if parent
        .file_name()
        .is_some_and(|name| name == "managed-console")
    {
        parent.parent()?
    } else {
        parent
    };
    Some(logs.join(format!("corekeeper-{name}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corekeeper_native_logs_belong_to_one_run_outside_console_retention() {
        let root = std::env::temp_dir().join("corekeeper-run-path");
        let expected = root.join("logs/corekeeper-run-42-main.log");
        for console in [
            "logs/managed-console/run-42-main.log",
            "logs/run-42-main.log",
        ] {
            assert_eq!(
                corekeeper_native_log_path(&root.join(console)),
                Some(expected.clone())
            );
        }
        assert_ne!(
            corekeeper_native_log_path(&root.join("logs/managed-console/run-43-main.log")),
            Some(expected)
        );
        for invalid in [
            "logs/CoreKeeperServer.log",
            "logs/run-042-main.log",
            "logs/run-current-main.log",
            "logs/run-42-other.log",
            "logs/../run-42-main.log",
        ] {
            assert!(
                corekeeper_native_log_path(&root.join(invalid)).is_none(),
                "{invalid}"
            );
        }
        assert!(corekeeper_native_log_path(Path::new("relative/logs/run-42-main.log")).is_none());
    }
}
