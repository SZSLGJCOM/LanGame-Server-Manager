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
    if module_id == "corekeeper"
        && matches!(status, InstanceStatus::Starting | InstanceStatus::Running)
        && let Some(source) = snapshot.source_path.as_deref()
        && !include_undated
    {
        let current = active_run
            .filter(|run| run.run_id > 0)
            .and_then(|run| run.log_path.as_deref())
            .and_then(|path| {
                app_core::runtime_logs::corekeeper_native_log_path(std::path::Path::new(path))
            });
        if current.as_deref() != Some(std::path::Path::new(source)) {
            return Cow::Borrowed(&[]);
        }
    }
    if !matches!(
        module_id,
        "runescapedragonwilds" | "conanexiles" | "scum" | "satisfactory"
    ) || !matches!(status, InstanceStatus::Starting | InstanceStatus::Running)
        || snapshot.source_path.is_none()
        || active_run.is_some_and(|run| run.log_path == snapshot.source_path)
    {
        return Cow::Borrowed(&snapshot.lines);
    }

    let primary = active_run.and_then(dragonwilds_primary);
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

/// Readiness from a reused native file belongs to one registered OS process.
pub(super) fn dragonwilds_primary(
    run: &ActiveInstanceRun,
) -> Option<&app_core::InstanceProcessState> {
    run.processes.iter().find(|process| {
        process.is_primary
            && process.run_id == run.run_id
            && process.pid.is_some_and(|pid| pid > 0)
            && process.pid == run.pid
            && process.session_id == run.session_id
            && process.status == "running"
            && process.process_identity.as_ref().is_some_and(|identity| {
                identity.creation_time > 0 && !identity.image_path.trim().is_empty()
            })
    })
}

#[cfg(test)]
#[path = "runtime_health_evidence_tests.rs"]
mod tests;
