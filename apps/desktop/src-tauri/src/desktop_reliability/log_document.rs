use std::path::Path;

use app_core::LogTailSnapshot;

use super::{config, server::INSTANCE_ID};

pub(super) fn read(
    root: &Path,
    instance_id: &str,
    max_lines: Option<usize>,
    run_id: Option<i64>,
    source: Option<&str>,
) -> Result<LogTailSnapshot, String> {
    let max_lines = max_lines.unwrap_or(200);
    if instance_id != INSTANCE_ID
        || !(1..=400).contains(&max_lines)
        || run_id.is_some_and(|value| value != 1)
        || source.is_some_and(|value| value != "console")
        || (source.is_some() && run_id.is_none())
    {
        return Err("Log reads must target this fixture's bounded console run".into());
    }
    let path = root.join("logs/managed-console/run-1.log");
    config::checked_path(&path)?;
    Ok(app_storage::read_log_path_snapshot(
        path.to_string_lossy().into_owned(),
        max_lines,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    struct FixtureRoot(PathBuf);

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fixture_log_document_reads_retained_native_output_with_a_bounded_tail() {
        let root = std::env::temp_dir().join(format!(
            "lgsm-reliability-log-document-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root).unwrap();
        let _cleanup = FixtureRoot(root.clone());
        let path = root.join("logs/managed-console/run-1.log");
        let mut writer = app_storage::managed_console_log::ManagedConsoleLog::open(&path).unwrap();
        for index in 0..450 {
            writeln!(writer, "fixture output 1:{index}").unwrap();
        }
        writeln!(writer, "LGSM_RELIABILITY_COMMAND_1").unwrap();
        writer.flush().unwrap();

        let first = read(&root, INSTANCE_ID, Some(400), None, None).unwrap();
        assert_eq!(first.source_path.as_deref(), path.to_str());
        assert_eq!(first.lines.len(), 400);
        assert_eq!(first.total_lines, 451);
        assert_eq!(first.lines.first().unwrap(), "fixture output 1:51");
        assert_eq!(first.lines.last().unwrap(), "LGSM_RELIABILITY_COMMAND_1");
        assert!(first.truncated);
        assert!(first.read_error.is_none());

        writeln!(writer, "LGSM_RELIABILITY_COMMAND_2").unwrap();
        writer.flush().unwrap();
        let refreshed = read(&root, INSTANCE_ID, Some(400), Some(1), Some("console")).unwrap();
        assert_eq!(refreshed.lines.len(), 400);
        assert_eq!(
            refreshed.lines.last().unwrap(),
            "LGSM_RELIABILITY_COMMAND_2"
        );
        assert!(refreshed.read_error.is_none());
    }

    #[test]
    fn fixture_log_document_rejects_other_instances_runs_sources_and_unbounded_reads() {
        let unused_root = Path::new(r"D:\fixture-log-request-must-not-read");
        for (instance, max_lines, run_id, source) in [
            ("other-instance", Some(400), None, None),
            (INSTANCE_ID, Some(0), None, None),
            (INSTANCE_ID, Some(401), None, None),
            (INSTANCE_ID, Some(400), Some(2), None),
            (INSTANCE_ID, Some(400), Some(1), Some("game")),
            (INSTANCE_ID, Some(400), Some(1), Some("other")),
            (INSTANCE_ID, Some(400), None, Some("console")),
        ] {
            assert!(read(unused_root, instance, max_lines, run_id, source).is_err());
        }
    }
}
