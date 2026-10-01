#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AssistantVerificationStatus {
    Verified,
    Failed,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantOperationVerification {
    pub status: AssistantVerificationStatus,
    pub summary: String,
    pub run_id: Option<i64>,
    pub evidence: Value,
    pub can_continue: bool,
}

const ASSISTANT_VERIFICATION_TIMEOUT: Duration = Duration::from_secs(15);
const ASSISTANT_VERIFICATION_OBSERVATIONS: usize = 3;
const ASSISTANT_VERIFICATION_MAX_PROCESSES: usize = 16;

struct AssistantVerificationStart {
    instance_id: String,
    module_id: String,
    run_id: i64,
    session_id: Option<String>,
    processes: Vec<(i64, String, u32, String)>,
}

struct AssistantVerificationSample {
    details: InstanceDetails,
    health: app_core::RuntimeHealth,
    log: LogTailSnapshot,
    diagnostics: Vec<RuntimeDiagnosticSignal>,
    process_identities: Vec<(String, Result<Option<ProcessIdentity>, String>)>,
    launch_ready: Option<bool>,
    launch_issues: Value,
    log_failures: Vec<String>,
    log_read_errors: Vec<String>,
}

fn assess_assistant_verification(
    action: AssistantOperationAction,
    before: &InstanceDetails,
    after: Option<&InstanceDetails>,
    started: Option<&AssistantVerificationStart>,
    sample: &AssistantVerificationSample,
    operation_error: Option<&str>,
) -> AssistantOperationVerification {
    use AssistantVerificationStatus::{Failed, Inconclusive, Verified};
    let current = &sample.details;
    let result = |status, summary: &str, can_continue| AssistantOperationVerification {
        status,
        summary: String::from(summary),
        run_id: current
            .active_run
            .as_ref()
            .map(|run| run.run_id)
            .or_else(|| started.map(|receipt| receipt.run_id)),
        evidence: assistant_verification_evidence(sample, operation_error),
        can_continue,
    };
    if current.summary.id != before.summary.id
        || current.summary.module_id != before.summary.module_id
    {
        return result(
            Inconclusive,
            "The verification target changed; request a new preview.",
            false,
        );
    }
    if let Some(after) = after
        && (after.summary.id != current.summary.id
            || after.summary.module_id != current.summary.module_id
            || after.config_file_path != current.config_file_path
            || after.saves_path != current.saves_path
            || after.settings_json != current.settings_json
            || after.summary.bind_ip != current.summary.bind_ip
            || assistant_port_snapshot(&after.ports) != assistant_port_snapshot(&current.ports))
    {
        return result(
            Inconclusive,
            "The saved configuration changed during verification; request a new preview.",
            false,
        );
    }
    if let Some(run) = current.active_run.as_ref() {
        let expected = started
            .map(|receipt| (receipt.run_id, receipt.session_id.as_ref()))
            .or_else(|| {
                after
                    .and_then(|details| details.active_run.as_ref())
                    .map(|run| (run.run_id, run.session_id.as_ref()))
            })
            .or_else(|| {
                before
                    .active_run
                    .as_ref()
                    .map(|run| (run.run_id, run.session_id.as_ref()))
            });
        if expected.is_some_and(|(id, session)| id != run.run_id || session != run.session_id.as_ref())
            || run.processes.iter().any(|process| {
                sample.process_identities.iter().any(|(key, observed)| {
                    key == &process.process_key && matches!((process.process_identity.as_ref(), observed),
                        (Some(expected), Ok(Some(actual))) if !process_identities_match(expected, actual))
                })
            })
        {
            return result(Inconclusive, "A run or process identity changed during verification; request a new preview.", false);
        }
    }
    if operation_error.is_some() {
        return result(
            Failed,
            "The operation failed; healthy logs cannot verify an unsuccessful change.",
            true,
        );
    }
    if after.is_none() {
        return result(
            Inconclusive,
            "No saved operation result is available to verify.",
            false,
        );
    }
    if action != AssistantOperationAction::StartServer {
        if before.active_run.is_some() || current.active_run.is_some() {
            return result(
                Inconclusive,
                "The change was saved, but an existing run cannot prove it took effect. A restart needs a separate confirmation.",
                false,
            );
        }
        return match sample.launch_ready {
            Some(false) => result(
                Failed,
                "The saved configuration failed the launch preflight; the server has not been started.",
                true,
            ),
            Some(true) => result(
                Inconclusive,
                "The saved configuration passed launch preflight. Request a start preview to verify runtime health.",
                true,
            ),
            None => result(
                Inconclusive,
                "Launch preflight is unavailable; runtime recovery is unverified.",
                true,
            ),
        };
    }
    let Some(started) = started else {
        return result(
            Inconclusive,
            "No start receipt is available; existing ready logs do not prove a new successful run.",
            false,
        );
    };
    if started.instance_id != current.summary.id
        || started.module_id != current.summary.module_id
        || before
            .active_run
            .as_ref()
            .is_some_and(|run| run.run_id == started.run_id)
    {
        return result(
            Inconclusive,
            "The start receipt does not identify a new run for this target.",
            false,
        );
    }
    let Some(run) = current.active_run.as_ref() else {
        return result(
            Failed,
            "The newly started process is no longer recorded as running.",
            true,
        );
    };
    if run.run_id != started.run_id || run.session_id != started.session_id {
        return result(
            Inconclusive,
            "Another run replaced the confirmed start; verification has stopped.",
            false,
        );
    }
    if started.processes.is_empty()
        || started.processes.len() > ASSISTANT_VERIFICATION_MAX_PROCESSES
    {
        return result(
            Inconclusive,
            "The start receipt has no complete bounded process set.",
            false,
        );
    }
    if run.processes.len() > started.processes.len() {
        return result(
            Inconclusive,
            "An unexpected process joined the run during verification.",
            false,
        );
    }
    let mut identity_unknown = false;
    for (run_id, key, pid, log_path) in &started.processes {
        let Some(process) = run
            .processes
            .iter()
            .find(|process| process.process_key == *key)
        else {
            return result(
                Failed,
                "A required process disappeared from the newly started run.",
                true,
            );
        };
        if process.run_id != *run_id
            || process.pid != Some(*pid)
            || process.session_id != started.session_id
            || process.log_path.as_ref() != Some(log_path)
        {
            return result(
                Inconclusive,
                "A process was replaced during verification; request a new preview.",
                false,
            );
        }
        match sample
            .process_identities
            .iter()
            .find(|(process_key, _)| process_key == key)
            .map(|(_, identity)| identity)
        {
            Some(Ok(None)) => {
                return result(
                    Failed,
                    "A newly started process exited during verification.",
                    true,
                );
            }
            Some(Ok(Some(observed))) => match &process.process_identity {
                Some(expected) if !process_identities_match(expected, observed) => {
                    return result(
                        Inconclusive,
                        "The process identity changed during verification; the PID may have been reused.",
                        false,
                    );
                }
                None => identity_unknown = true,
                _ => {}
            },
            _ => identity_unknown = true,
        }
        if process.crash_flag || process.exit_code.is_some() || process.status != "running" {
            return result(
                Failed,
                "A newly started process reported an exit or crash.",
                true,
            );
        }
    }
    let fresh_log = sample
        .log
        .source_path
        .as_ref()
        .is_some_and(|path| started.processes.iter().any(|process| &process.3 == path));
    if !sample.log_failures.is_empty()
        || matches!(current.summary.status, InstanceStatus::Error)
        || (fresh_log
            && (sample.health.status == "error"
                || sample.health.reason.code == "dst_lua_config_failed"
                || sample
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == "error")))
    {
        return result(
            Failed,
            "Fresh runtime evidence reports a startup or configuration failure.",
            true,
        );
    }
    if !matches!(current.summary.status, InstanceStatus::Running) {
        return result(
            Inconclusive,
            "The new run has not reached a stable running state.",
            !matches!(current.summary.status, InstanceStatus::Stopping),
        );
    }
    if identity_unknown || sample.log.read_error.is_some() || !sample.log_read_errors.is_empty() {
        return result(
            Inconclusive,
            "Process identity or fresh logs could not be read completely; recovery remains unverified.",
            true,
        );
    }
    // DST returns its receipt only after each enabled shard acknowledges the
    // nonce-bound native readiness probe. Other modules need a current-run log.
    if started.module_id == "dontstarve" || (fresh_log && sample.health.status == "ready") {
        return result(
            Verified,
            "The new run remained alive with verified process identities and startup readiness across consecutive observations.",
            false,
        );
    }
    result(
        Inconclusive,
        "The new run is alive but fresh readiness evidence is still missing.",
        true,
    )
}

pub(super) async fn observe_assistant_operation(
    state: &DesktopState,
    storage: &StorageBootstrap,
    action: AssistantOperationAction,
    before: &InstanceDetails,
    after: Option<&InstanceDetails>,
    started: Option<&StartInstanceResult>,
    operation_error: Option<&str>,
) -> AssistantOperationVerification {
    if let Err(error) =
        ensure_storage_context_snapshot_current(state, storage, "assistant verification")
    {
        return assistant_verification_read_error(&error, operation_error, false);
    }
    let receipt = started.map(|started| AssistantVerificationStart {
        instance_id: started.summary.id.clone(),
        module_id: started.summary.module_id.clone(),
        run_id: started.run_id,
        session_id: started.session_id.clone(),
        processes: started
            .processes
            .iter()
            .map(|process| {
                (
                    process.run_id,
                    process.process_key.clone(),
                    process.pid,
                    process.log_path.clone(),
                )
            })
            .collect(),
    });
    let result = observe_assistant_verification_with(
        action,
        before,
        after,
        receipt.as_ref(),
        operation_error,
        || collect_assistant_verification_sample(storage, before, receipt.as_ref()),
    )
    .await;
    if let Err(error) =
        ensure_storage_context_snapshot_current(state, storage, "assistant verification")
    {
        return assistant_verification_read_error(&error, operation_error, false);
    }
    result
}

async fn observe_assistant_verification_with<M, MF>(
    action: AssistantOperationAction,
    before: &InstanceDetails,
    after: Option<&InstanceDetails>,
    started: Option<&AssistantVerificationStart>,
    operation_error: Option<&str>,
    mut observe: M,
) -> AssistantOperationVerification
where
    M: FnMut() -> MF,
    MF: std::future::Future<Output = Result<AssistantVerificationSample, String>>,
{
    let deadline = tokio::time::Instant::now() + ASSISTANT_VERIFICATION_TIMEOUT;
    let mut consecutive_ready = 0;
    for observation in 0..ASSISTANT_VERIFICATION_OBSERVATIONS {
        if observation > 0 {
            tokio::time::sleep_until(
                (tokio::time::Instant::now() + Duration::from_secs(1)).min(deadline),
            )
            .await;
        }
        let sample = match tokio::time::timeout_at(deadline, observe()).await {
            Ok(Ok(sample)) => sample,
            Ok(Err(error)) => {
                return assistant_verification_read_error(&error, operation_error, false);
            }
            Err(_) => {
                return assistant_verification_read_error(
                    "Verification reached its 15-second observation budget.",
                    operation_error,
                    true,
                );
            }
        };
        let mut result =
            assess_assistant_verification(action, before, after, started, &sample, operation_error);
        result.evidence["observationCount"] = json!(observation + 1);
        if result.status == AssistantVerificationStatus::Verified {
            consecutive_ready += 1;
            if consecutive_ready >= 2 {
                return result;
            }
            result.status = AssistantVerificationStatus::Inconclusive;
            result.summary =
                String::from("Readiness was observed once; stability has not yet been verified.");
            result.can_continue = true;
        } else {
            consecutive_ready = 0;
        }
        if action != AssistantOperationAction::StartServer
            || result.status == AssistantVerificationStatus::Failed
            || !result.can_continue
            || observation + 1 == ASSISTANT_VERIFICATION_OBSERVATIONS
        {
            return result;
        }
    }
    assistant_verification_read_error(
        "The observation budget was exhausted.",
        operation_error,
        true,
    )
}

include!("verification_evidence.rs");

#[cfg(test)]
#[path = "verification_tests.rs"]
mod verification_tests;
