use app_core::{ActiveInstanceRun, InstanceStatus, LogTailSnapshot};
use std::borrow::Cow;

/// A reused native log is not a run transcript. Readiness requires timestamps
/// after the primary process's creation token. Diagnostic scans may also keep
/// undated lines conservatively; unknown-age output never establishes readiness.
pub(super) fn current_run_lines<'a>(
    module_id: &str,
    status: &InstanceStatus,
    snapshot: &'a LogTailSnapshot,
    active_run: Option<&ActiveInstanceRun>,
    include_undated: bool,
) -> Cow<'a, [String]> {
    if module_id != "runescapedragonwilds"
        || !matches!(status, InstanceStatus::Starting | InstanceStatus::Running)
        || snapshot.source_path.is_none()
        || active_run.is_some_and(|run| run.log_path == snapshot.source_path)
    {
        return Cow::Borrowed(&snapshot.lines);
    }

    let primary = active_run.and_then(|run| {
        run.processes.iter().find(|process| {
            process.is_primary
                && process.run_id == run.run_id
                && process.pid.is_some()
                && process.pid == run.pid
                && process.session_id == run.session_id
                && process.status == "running"
        })
    });
    Cow::Owned(
        snapshot
            .lines
            .iter()
            .filter(|line| {
                (include_undated && super::ark_readiness::native_line_timestamp(line).is_none())
                    || primary.is_some_and(|process| {
                        super::ark_readiness::native_line_is_current(process, line)
                    })
            })
            .cloned()
            .collect(),
    )
}

/// Startup messages include operator-controlled instance names. They remain
/// visible in the transcript and diagnostics, but cannot prove game readiness.
pub(super) fn readiness_lines(lines: &[String]) -> Cow<'_, [String]> {
    let is_game_output = |line: &str| {
        let line = line.trim_start();
        !line.starts_with("[LanGame ") && !line.starts_with("[LanGame]")
    };
    if lines.iter().all(|line| is_game_output(line)) {
        Cow::Borrowed(lines)
    } else {
        Cow::Owned(
            lines
                .iter()
                .filter(|line| is_game_output(line))
                .cloned()
                .collect(),
        )
    }
}
