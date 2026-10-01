use app_runtime::ExitedManagedProcess;
use app_storage::{StorageBootstrap, list_active_instance_runs, mark_instance_process_stopped};

/// The caller holds the instance mutation lock across this read and write.
/// Replayed exits must not rewrite settled history or a replacement session's state.
pub(super) async fn persist_current_exit(
    storage: &StorageBootstrap,
    exited: &ExitedManagedProcess,
    crash_flag: bool,
) -> Result<bool, String> {
    let active = list_active_instance_runs(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let same_session = |run: &app_storage::ActiveInstanceRunEntry| match (
        run.session_id.as_deref(),
        exited.session_id.as_deref(),
    ) {
        (Some(current), Some(expected)) => current == expected,
        (None, None) => run.run_id == exited.run_id,
        _ => false,
    };
    let exact_run_is_active = active.iter().any(|run| {
        run.instance_id == exited.summary.id && run.run_id == exited.run_id && same_session(run)
    });
    let another_session_is_active = active
        .iter()
        .any(|run| run.instance_id == exited.summary.id && !same_session(run));
    if !exact_run_is_active || another_session_is_active {
        return Ok(false);
    }
    mark_instance_process_stopped(
        &storage.paths,
        &exited.summary.id,
        exited.run_id,
        exited.exit_code,
        crash_flag,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(true)
}

#[cfg(test)]
#[path = "commands_runtime_reconciliation_tests.rs"]
mod tests;
