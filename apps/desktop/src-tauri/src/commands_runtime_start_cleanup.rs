//! Startup cleanup keeps the attempt alive until its native workers have settled.
use super::*;

pub(super) async fn cleanup(
    spawned: &mut Vec<(ProcessLaunchPlan, app_runtime::SpawnedProcess)>,
    reservation: &RuntimeStartReservationLease,
) -> Result<SpawnedProcessStopReport, String> {
    let mut owned = std::mem::take(spawned);
    let cancel_startup = reservation.cancelled_by_user();
    let result = spawn_blocking_storage_context_task(reservation, move || {
        let report = if cancel_startup {
            super::super::commands_runtime_supervision::force_stop_spawned_processes(&mut owned)
        } else {
            stop_spawned_processes(&mut owned)
        };
        (owned, report)
    })
    .await;
    let (owned, report) = result.map_err(|error| {
        let message =
            format!("Startup cleanup worker failed before confirming process-tree exit: {error}");
        reservation.record_cleanup_failure(&message);
        message
    })?;
    *spawned = owned;
    if report.has_survivors() {
        reservation.record_cleanup_failure(&report.issue_summary().unwrap_or_else(|| {
            "Startup cleanup could not confirm complete process-tree exit".into()
        }));
    }
    Ok(report)
}

pub(super) fn retain_survivors(
    spawned: &mut Vec<(ProcessLaunchPlan, app_runtime::SpawnedProcess)>,
    cleanup: &SpawnedProcessStopReport,
) {
    *spawned = std::mem::take(spawned)
        .into_iter()
        .zip(&cleanup.statuses)
        .filter_map(|(process, status)| status.still_running.then_some(process))
        .collect();
}

#[cfg(all(test, windows))]
#[path = "commands_runtime_start_cleanup_tests.rs"]
mod tests;
