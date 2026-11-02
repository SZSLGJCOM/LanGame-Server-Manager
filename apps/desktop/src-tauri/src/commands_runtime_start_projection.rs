use std::collections::HashSet;

use app_core::{InstanceRuntimeOverview, InstanceStatus, InstanceSummary, LogTailSnapshot};

use crate::state::DesktopState;

/// Startup reservations outlive their requesting page. Project them only onto
/// response copies so releasing a reservation immediately reveals stored state.
pub(in crate::commands) fn project_pending_runtime_start_summaries(
    state: &DesktopState,
    instances: &mut [InstanceSummary],
) -> Result<(), String> {
    let pending = state
        .pending_runtime_start_instance_ids()?
        .into_iter()
        .collect::<HashSet<_>>();
    for instance in instances {
        if pending.contains(&instance.id) {
            instance.status = InstanceStatus::Starting;
        }
    }
    Ok(())
}

pub(super) async fn project_pending_runtime_start_overview(
    state: &DesktopState,
    instance_id: &str,
    overview: &mut InstanceRuntimeOverview,
) -> Result<(), String> {
    let (pending, log_path) = {
        let reservations = state
            .runtime_start_reservations
            .lock()
            .map_err(|_| String::from("runtime start reservation lock poisoned"))?;
        (
            reservations
                .pending_instance_ids()
                .iter()
                .any(|id| id == instance_id),
            reservations.pending_console_log_path(instance_id),
        )
    };
    if !pending {
        return Ok(());
    }

    // Before this attempt publishes its log, the database still points at the
    // previous run. Keep that run in history without presenting its output or
    // readiness/failure signals as evidence about the current startup.
    overview.log_tail = if let Some(path) = log_path {
        tokio::task::spawn_blocking(move || app_storage::read_log_path_snapshot(path, 32))
            .await
            .map_err(|error| format!("Pending startup log reader failed: {error}"))?
    } else {
        LogTailSnapshot {
            source_path: None,
            lines: Vec::new(),
            total_lines: 0,
            truncated: false,
            read_error: None,
        }
    };
    overview.health = app_core::RuntimeHealth {
        status: String::from("starting"),
        summary: String::from(
            "Server startup is in progress. Preparing the server and waiting for startup checks to complete.",
        ),
        reason: app_core::RuntimeHealthReason {
            code: String::from("startup_pending"),
            params: Default::default(),
        },
        matched_line: None,
    };
    overview.diagnostics.clear();
    overview.performance.status = String::from("pending");
    overview.performance.summary =
        String::from("Runtime performance is pending while server startup is in progress.");
    Ok(())
}

#[cfg(test)]
#[path = "commands_runtime_start_projection_tests.rs"]
mod tests;
