//! Stop API success is only one part of acceptance. This external observer can
//! verify declared process identities, not the backend's private Windows Jobs.
//! Full-tree graceful exit relies on the tested normal-stop backend contract;
//! neither an exit code of zero nor disappearing tracked PIDs proves it alone.
use super::{ExistingClient, InstanceDetails, ProcessIdentity, Receipt, StartInstanceResult};
use super::{StopInstanceResult, events, read, selection};
use crate::commands::runtime_exit::windows_crash_exit_reason;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

#[path = "commands_existing_instance_stop_observation.rs"]
mod observation;

type ProcessKey = (i64, String, Option<u32>);

pub(super) fn process_identities(instance: &InstanceDetails) -> Vec<(u32, ProcessIdentity)> {
    instance
        .active_run
        .as_ref()
        .into_iter()
        .flat_map(|run| &run.processes)
        .filter_map(|process| Some((process.pid?, process.process_identity.clone()?)))
        .collect()
}

pub(super) struct StartedScope {
    instance_id: String,
    module_id: String,
    run_id: i64,
    session_id: Option<String>,
    processes: BTreeSet<ProcessKey>,
    valid: bool,
}

impl StartedScope {
    pub(super) fn from_result(id: &str, module: &str, result: &StartInstanceResult) -> Self {
        let processes: BTreeSet<_> = result
            .processes
            .iter()
            .map(|process| {
                (
                    process.run_id,
                    process.process_key.clone(),
                    Some(process.pid),
                )
            })
            .collect();
        let valid = result.summary.id == id
            && result.summary.module_id == module
            && result.process_count == result.processes.len()
            && valid_processes(&processes, result.process_count)
            && processes
                .iter()
                .any(|(run, _, pid)| *run == result.run_id && *pid == Some(result.pid));
        Self {
            instance_id: id.into(),
            module_id: module.into(),
            run_id: result.run_id,
            session_id: result.session_id.clone(),
            processes,
            valid,
        }
    }

    pub(super) fn matches_instance(&self, instance: &InstanceDetails) -> bool {
        let Some(active) = &instance.active_run else {
            return false;
        };
        let processes: BTreeSet<_> = active
            .processes
            .iter()
            .map(|process| (process.run_id, process.process_key.clone(), process.pid))
            .collect();
        self.valid
            && instance.summary.id == self.instance_id
            && instance.summary.module_id == self.module_id
            && active.run_id == self.run_id
            && active.session_id == self.session_id
            && active.process_count == self.processes.len()
            && active.processes.len() == self.processes.len()
            && processes == self.processes
            && active
                .processes
                .iter()
                .all(|process| process.session_id == self.session_id)
    }

    fn matches_stop(&self, result: &StopInstanceResult) -> bool {
        let processes: BTreeSet<_> = result
            .processes
            .iter()
            .map(|process| (process.run_id, process.process_key.clone(), process.pid))
            .collect();
        self.valid
            && result.summary.id == self.instance_id
            && result.summary.module_id == self.module_id
            && selection::is_stopped(&result.summary)
            && result.run_id == self.run_id
            && result.session_id == self.session_id
            && self
                .processes
                .iter()
                .any(|(run, _, pid)| *run == result.run_id && *pid == result.pid)
            && result.process_count == self.processes.len()
            && result.processes.len() == self.processes.len()
            && processes == self.processes
    }
}

fn valid_processes(processes: &BTreeSet<ProcessKey>, count: usize) -> bool {
    !processes.is_empty()
        && processes.len() == count
        && processes
            .iter()
            .all(|(run, key, pid)| *run > 0 && !key.is_empty() && pid.is_some_and(|pid| pid > 0))
        && processes
            .iter()
            .map(|(run, _, _)| run)
            .collect::<BTreeSet<_>>()
            .len()
            == count
        && processes
            .iter()
            .map(|(_, key, _)| key)
            .collect::<BTreeSet<_>>()
            .len()
            == count
        && processes
            .iter()
            .map(|(_, _, pid)| pid)
            .collect::<BTreeSet<_>>()
            .len()
            == count
}

#[derive(Serialize)]
pub(super) struct StopEvidence {
    verification_scope: &'static str,
    owned_tree_observation: &'static str,
    required_backend_contract: &'static str,
    api_failure: Option<observation::ApiFailure>,
    post_error_observation: Option<observation::PostErrorObservation>,
    response_run_id: Option<i64>,
    response_session_id: Option<String>,
    response_crash_reason: Option<&'static str>,
    response_matches_start: bool,
    processes: Vec<ProcessStopEvidence>,
    tracked_identities: Vec<IdentityExitEvidence>,
    unknown_exit_code_count: usize,
    nonzero_exit_code_count: usize,
    failures: Vec<&'static str>,
}

impl Default for StopEvidence {
    fn default() -> Self {
        Self {
            verification_scope: "api_receipt_tracked_identities_and_storage_readback",
            owned_tree_observation: "not_exposed_by_ipc",
            required_backend_contract: "normal_stop_returns_only_after_full_owned_tree_exits_without_force",
            api_failure: None,
            post_error_observation: None,
            response_run_id: None,
            response_session_id: None,
            response_crash_reason: None,
            response_matches_start: false,
            processes: Vec::new(),
            tracked_identities: Vec::new(),
            unknown_exit_code_count: 0,
            nonzero_exit_code_count: 0,
            failures: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct ProcessStopEvidence {
    run_id: i64,
    process_key: String,
    pid: Option<u32>,
    exit_code: Option<i32>,
    native_crash_reason: Option<&'static str>,
}

#[derive(Serialize)]
struct IdentityExitEvidence {
    pid: u32,
    expected_creation_time: u64,
    outcome: IdentityOutcome,
}

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum IdentityOutcome {
    Exited,
    ExitedPidReused,
    StillRunning,
    InspectionFailed,
}

fn identity_outcome(
    expected: &ProcessIdentity,
    actual: Result<Option<ProcessIdentity>, ()>,
) -> IdentityOutcome {
    match actual {
        Ok(None) => IdentityOutcome::Exited,
        Ok(Some(actual)) if actual != *expected => IdentityOutcome::ExitedPidReused,
        Ok(Some(_)) => IdentityOutcome::StillRunning,
        Err(()) => IdentityOutcome::InspectionFailed,
    }
}

impl StopEvidence {
    pub(super) fn safe_after_late_exit(&self) -> bool {
        self.post_error_observation
            .as_ref()
            .is_some_and(|observation| observation.safe_to_continue)
    }

    pub(super) fn observe_response(
        &mut self,
        scope: Option<&StartedScope>,
        result: &StopInstanceResult,
    ) {
        self.response_run_id = Some(result.run_id);
        self.response_session_id = result.session_id.clone();
        self.response_crash_reason = windows_crash_exit_reason(result.exit_code);
        self.response_matches_start = scope.is_some_and(|scope| scope.matches_stop(result));
        self.processes = result
            .processes
            .iter()
            .map(|process| ProcessStopEvidence {
                run_id: process.run_id,
                process_key: process.process_key.clone(),
                pid: process.pid,
                exit_code: process.exit_code,
                native_crash_reason: windows_crash_exit_reason(process.exit_code),
            })
            .collect();
        self.unknown_exit_code_count = self
            .processes
            .iter()
            .filter(|process| process.exit_code.is_none())
            .count();
        self.nonzero_exit_code_count = self
            .processes
            .iter()
            .filter(|process| process.exit_code.is_some_and(|code| code != 0))
            .count();
        if !self.response_matches_start {
            self.failures
                .push("stop_receipt_does_not_match_started_run");
        }
        if self.response_crash_reason.is_some()
            || self
                .processes
                .iter()
                .any(|process| process.native_crash_reason.is_some())
        {
            self.failures.push("native_shutdown_crash");
        }
    }

    fn tracked_exited(&self) -> bool {
        let tracked: BTreeSet<_> = self
            .tracked_identities
            .iter()
            .map(|identity| Some(identity.pid))
            .collect();
        let returned: BTreeSet<_> = self.processes.iter().map(|process| process.pid).collect();
        !tracked.is_empty()
            && tracked.len() == self.tracked_identities.len()
            && self.tracked_identities.len() == self.processes.len()
            && tracked == returned
            && self.tracked_identities.iter().all(|identity| {
                matches!(
                    identity.outcome,
                    IdentityOutcome::Exited | IdentityOutcome::ExitedPidReused
                )
            })
    }

    fn corroborates(
        &self,
        api_succeeded: bool,
        started_confirmed: bool,
        storage_stopped: bool,
    ) -> bool {
        api_succeeded
            && started_confirmed
            && storage_stopped
            && self.response_matches_start
            && self.tracked_exited()
            && self.failures.is_empty()
    }
}

pub(super) struct StopBaseline<'a> {
    pub before: &'a InstanceDetails,
    pub scope: Option<&'a StartedScope>,
    pub identities: &'a [(u32, ProcessIdentity)],
    pub prior_run_ids: &'a [i64],
}

pub(super) async fn record(
    client: &ExistingClient,
    baseline: StopBaseline<'_>,
    cursor: &mut events::Observer,
    report: &mut Receipt,
    paths: &mut Vec<PathBuf>,
) {
    let StopBaseline {
        before,
        scope,
        identities,
        prior_run_ids,
    } = baseline;
    let id = &before.summary.id;
    report.phase = "stop_requested".into();
    let began = Instant::now();
    let stopped: Result<StopInstanceResult, String> = cursor
        .watch(
            client,
            read(client, "stop_instance_process", json!({"instanceId": id})),
        )
        .await;
    report.stop_elapsed_ms = began.elapsed().as_millis();
    report.stop_api_succeeded = stopped.is_ok();
    if let Ok(stopped) = &stopped {
        report.stop_evidence.observe_response(scope, stopped);
        report.stop_exit_codes = stopped
            .processes
            .iter()
            .map(|process| process.exit_code)
            .collect();
        for process in &stopped.processes {
            if !prior_run_ids.contains(&process.run_id)
                && let Some(path) = &process.log_path
            {
                paths.push(PathBuf::from(path));
            }
        }
    } else {
        report.stop_evidence.api_failure = stopped
            .as_ref()
            .err()
            .map(|reason| observation::ApiFailure::from_reason(reason));
        report.stop_evidence.failures.push("normal_stop_api_failed");
    }
    match read::<InstanceDetails>(
        client,
        "read_instance_details_from_storage",
        json!({"instanceId": id}),
    )
    .await
    {
        Ok(after) => {
            report.stopped_state_confirmed = after.summary.id == *id
                && after.summary.module_id == before.summary.module_id
                && selection::is_stopped(&after.summary)
                && after.active_run.is_none();
            report.settings_unchanged = before.settings_json == after.settings_json;
            report.ports_unchanged =
                serde_json::to_value(&before.ports).ok() == serde_json::to_value(&after.ports).ok();
            if !report.stopped_state_confirmed {
                report
                    .stop_evidence
                    .failures
                    .push("stopped_state_not_confirmed");
            }
        }
        Err(_) => report
            .stop_evidence
            .failures
            .push("stopped_state_readback_failed"),
    }
    report.stop_evidence.tracked_identities = identities
        .iter()
        .map(|(pid, expected)| IdentityExitEvidence {
            pid: *pid,
            expected_creation_time: expected.creation_time,
            outcome: identity_outcome(
                expected,
                app_runtime::inspect_process_identity(*pid).map_err(|_| ()),
            ),
        })
        .collect();
    // This retained field refers only to the declared identities above. The
    // separate scope fields deliberately do not claim independent Job evidence.
    report.owned_processes_exited = report.stop_evidence.tracked_exited();
    if !report.owned_processes_exited {
        report
            .stop_evidence
            .failures
            .push("tracked_process_exit_not_confirmed");
    }
    report.normal_stop_succeeded = report.stop_evidence.corroborates(
        report.stop_api_succeeded,
        report.started_identity_confirmed,
        report.stopped_state_confirmed,
    );
    report.failures.extend(
        report
            .stop_evidence
            .failures
            .iter()
            .map(|reason| (*reason).to_owned()),
    );
    if !report.normal_stop_succeeded {
        report.failures.push("normal_stop_not_confirmed".into());
    }
    // Older backends may return a successful receipt containing a native crash.
    // Finalized cleanup can permit the next test, never make this stop a pass.
    if !report.normal_stop_succeeded {
        let mut observation = observation::PostErrorObservation::default();
        let began = Instant::now();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            cursor.watch(
                client,
                observation::observe(client, before, scope, identities, &mut observation),
            ),
        )
        .await;
        observation.elapsed_ms = began.elapsed().as_millis();
        observation.budget_expired = result.is_err();
        report.stop_evidence.post_error_observation = Some(observation);
    }
}

#[path = "commands_existing_instance_stop_evidence_tests.rs"]
mod tests;
