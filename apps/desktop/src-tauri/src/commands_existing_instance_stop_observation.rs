//! Bounded, read-only diagnostics after an unconfirmed normal stop. A finalized
//! exit may make batch cleanup safe, but never changes failed stop acceptance.
use super::super::{ExistingClient, InstanceDetails, InstanceRuntimeOverview, read, selection};
use super::{IdentityOutcome, StartedScope, identity_outcome};
use crate::commands::runtime_exit::windows_crash_exit_reason;
use crate::runtime_service::{
    STOP_ERROR_CATEGORIES as CATEGORIES, STOP_ERROR_PREFIX as ERROR_PREFIX,
    redact_backend_stop_error,
};
use app_core::{InstanceRunRecord, ProcessIdentity};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

#[derive(Serialize)]
pub(super) struct ApiFailure {
    outcome: &'static str,
    categories: Vec<&'static str>,
}

impl ApiFailure {
    pub(super) fn from_reason(reason: &str) -> Self {
        if let Some(encoded) = reason.strip_prefix(ERROR_PREFIX) {
            let categories = CATEGORIES
                .iter()
                .map(|(_, category)| *category)
                .chain(["unclassified_backend_error"])
                .filter(|known| encoded.split(',').any(|part| part == *known))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            return Self {
                outcome: "backend_rejected",
                categories,
            };
        }
        let (outcome, category) = match reason {
            "existing backend connection failed; no request sent" => {
                ("not_sent", "backend_connection_failed")
            }
            "backend response timed out; operation may still be running" => {
                ("uncertain", "backend_response_timeout")
            }
            "backend response unavailable; operation state is uncertain" => {
                ("uncertain", "backend_response_unavailable")
            }
            "request transmission uncertain; inspect before retrying"
            | "request transmission failed; operation state is uncertain" => {
                ("uncertain", "request_transmission_uncertain")
            }
            "existing backend exited" => ("uncertain", "backend_exited"),
            "existing backend generation or executable changed; request refused"
            | "existing backend pipe owner changed; request refused" => {
                ("uncertain", "backend_identity_changed")
            }
            _ => ("unknown", "unclassified_client_error"),
        };
        Self {
            outcome,
            categories: vec![category],
        }
    }
}

#[derive(Serialize)]
pub(super) struct PostErrorObservation {
    budget_ms: u64,
    pub(super) elapsed_ms: u128,
    pub(super) budget_expired: bool,
    required_backend_contract: &'static str,
    pub(super) safe_to_continue: bool,
    samples: Vec<Sample>,
}

impl Default for PostErrorObservation {
    fn default() -> Self {
        Self {
            budget_ms: 30_000,
            elapsed_ms: 0,
            budget_expired: false,
            required_backend_contract: "backend_finalizes_run_only_after_full_owned_tree_exits",
            safe_to_continue: false,
            samples: Vec::new(),
        }
    }
}

#[derive(Default, Serialize)]
struct Sample {
    elapsed_ms: u128,
    tracked_exited: usize,
    tracked_running: usize,
    identity_inspection_failed: usize,
    storage_inactive: Option<bool>,
    same_run_finalized: Option<bool>,
    run_exits: Vec<RunExit>,
    read_failures: Vec<&'static str>,
}

#[derive(Serialize)]
struct RunExit {
    run_id: i64,
    process_key: String,
    exit_code: Option<i32>,
    stopped_at: Option<String>,
    status: String,
    crash_flag: bool,
    native_crash_reason: Option<&'static str>,
}

impl Sample {
    fn safe(&self, expected_count: usize) -> bool {
        expected_count > 0
            && self.tracked_exited == expected_count
            && self.tracked_running == 0
            && self.identity_inspection_failed == 0
            && self.storage_inactive == Some(true)
            && self.same_run_finalized == Some(true)
            && self.read_failures.is_empty()
    }
}

fn expected_identity_count(
    scope: Option<&StartedScope>,
    identities: &[(u32, ProcessIdentity)],
) -> usize {
    let Some(scope) = scope.filter(|scope| scope.valid) else {
        return 0;
    };
    let expected: BTreeSet<_> = scope.processes.iter().map(|(_, _, pid)| *pid).collect();
    let observed: BTreeSet<_> = identities.iter().map(|(pid, _)| Some(*pid)).collect();
    if identities.len() != scope.processes.len()
        || observed.len() != identities.len()
        || observed != expected
        || identities
            .iter()
            .any(|(_, identity)| identity.creation_time == 0 || identity.image_path.is_empty())
    {
        return 0;
    }
    identities.len()
}

fn inactive_instance(scope: &StartedScope, actual: &InstanceDetails) -> bool {
    scope.valid
        && actual.summary.id == scope.instance_id
        && actual.summary.module_id == scope.module_id
        && selection::is_inactive(&actual.summary)
        && actual.active_run.is_none()
}

fn finalized(
    scope: &StartedScope,
    runs: &[InstanceRunRecord],
    identities: &[(u32, ProcessIdentity)],
    sample: &mut Sample,
) -> bool {
    let Some(run) = runs
        .iter()
        .find(|run| run.run_id == scope.run_id && run.session_id == scope.session_id)
    else {
        return false;
    };
    sample.run_exits = run
        .processes
        .iter()
        .map(|process| RunExit {
            run_id: process.run_id,
            process_key: process.process_key.clone(),
            exit_code: process.exit_code,
            stopped_at: process.stopped_at.clone(),
            status: process.status.clone(),
            crash_flag: process.crash_flag,
            native_crash_reason: windows_crash_exit_reason(process.exit_code),
        })
        .collect();
    let processes: BTreeSet<_> = run
        .processes
        .iter()
        .map(|process| (process.run_id, process.process_key.clone(), process.pid))
        .collect();
    expected_identity_count(Some(scope), identities) > 0
        && matches!(run.status.as_str(), "stopped" | "error")
        && run.stopped_at.is_some()
        && scope
            .processes
            .iter()
            .any(|(run_id, _, pid)| *run_id == run.run_id && *pid == run.pid)
        && run.process_count == scope.processes.len()
        && run.processes.len() == scope.processes.len()
        && processes == scope.processes
        && run.processes.iter().all(|process| {
            matches!(process.status.as_str(), "stopped" | "error")
                && process.stopped_at.is_some()
                && process.session_id == scope.session_id
                && identities.iter().any(|(pid, identity)| {
                    process.pid == Some(*pid) && process.process_identity.as_ref() == Some(identity)
                })
        })
}

pub(super) async fn observe(
    client: &ExistingClient,
    before: &InstanceDetails,
    scope: Option<&StartedScope>,
    identities: &[(u32, ProcessIdentity)],
    evidence: &mut PostErrorObservation,
) -> Result<(), String> {
    let began = Instant::now();
    let expected_count = expected_identity_count(scope, identities);
    if expected_count == 0 {
        return Err("post_stop_started_identities_not_confirmed".into());
    }
    loop {
        let mut sample = Sample::default();
        for (pid, identity) in identities {
            match identity_outcome(
                identity,
                app_runtime::inspect_process_identity(*pid).map_err(|_| ()),
            ) {
                IdentityOutcome::Exited | IdentityOutcome::ExitedPidReused => {
                    sample.tracked_exited += 1
                }
                IdentityOutcome::StillRunning => sample.tracked_running += 1,
                IdentityOutcome::InspectionFailed => sample.identity_inspection_failed += 1,
            }
        }
        let args = json!({"instanceId": before.summary.id});
        match tokio::time::timeout(
            Duration::from_secs(2),
            read::<InstanceDetails>(client, "read_instance_details_from_storage", args.clone()),
        )
        .await
        {
            Ok(Ok(actual)) => {
                sample.storage_inactive =
                    Some(scope.is_some_and(|scope| inactive_instance(scope, &actual)))
            }
            _ => sample
                .read_failures
                .push("post_stop_instance_readback_failed_or_timed_out"),
        }
        match tokio::time::timeout(
            Duration::from_secs(2),
            read::<InstanceRuntimeOverview>(
                client,
                "read_instance_runtime_overview_from_storage",
                args,
            ),
        )
        .await
        {
            Ok(Ok(actual)) => {
                sample.same_run_finalized = Some(scope.is_some_and(|scope| {
                    finalized(scope, &actual.recent_runs, identities, &mut sample)
                }))
            }
            _ => sample
                .read_failures
                .push("post_stop_history_readback_failed_or_timed_out"),
        }
        sample.elapsed_ms = began.elapsed().as_millis();
        evidence.elapsed_ms = sample.elapsed_ms;
        evidence.safe_to_continue = sample.safe(expected_count);
        evidence.samples.push(sample);
        if evidence.safe_to_continue {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[test]
fn existing_stop_diagnostics_never_retain_raw_backend_or_client_errors() {
    let redacted = redact_backend_stop_error(
        "Shutdown command secret-command failed: runtime stdin write failed; password=fixture-admin-password; RCON completion read failed: socket (os error 10054)",
    );
    let evidence = serde_json::to_string(&ApiFailure::from_reason(&redacted)).unwrap();
    assert!(evidence.contains("stdin_write_failed"));
    assert!(evidence.contains("rcon_completion_read_failed"));
    assert!(evidence.contains("connection_reset"));
    for secret in [
        "secret-command",
        "password",
        "fixture-admin-password",
        "socket",
    ] {
        assert!(!redacted.contains(secret));
        assert!(!evidence.contains(secret));
    }
    let unknown = serde_json::to_string(&ApiFailure::from_reason(
        "native password=fixture-admin-password",
    ))
    .unwrap();
    assert!(!unknown.contains("fixture-admin-password"));
    assert!(unknown.contains("unclassified_client_error"));
}

#[test]
fn existing_stop_late_exit_requires_finalized_run_storage_and_all_identities() {
    let mut sample = Sample {
        tracked_exited: 2,
        storage_inactive: Some(true),
        same_run_finalized: Some(true),
        ..Default::default()
    };
    assert!(sample.safe(2));
    assert!(!sample.safe(0));
    assert!(!sample.safe(3));
    sample.same_run_finalized = None;
    assert!(!sample.safe(2));
    sample.same_run_finalized = Some(true);
    sample.storage_inactive = Some(false);
    assert!(!sample.safe(2));
    sample.storage_inactive = Some(true);
    sample.identity_inspection_failed = 1;
    assert!(!sample.safe(2));
    sample.identity_inspection_failed = 0;
    sample.tracked_running = 1;
    assert!(!sample.safe(2));
}

#[cfg(test)]
#[path = "commands_existing_instance_stop_observation_tests.rs"]
mod tests;
