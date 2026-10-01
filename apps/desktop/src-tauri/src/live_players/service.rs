use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use app_core::{
    InstanceDetails, InstanceProcessState, ModulePlayerListSource, RuntimeLivePlayerIssue,
    RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
};
use app_modules::ModuleDescriptor;

use crate::runtime_log_stream::RuntimeLogTailState;

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCacheKey, LivePlayerCollectionResult};
use super::contract::build_live_player_security_contract_fingerprint;
pub(crate) use super::dispatch_deadline::{
    DstStructuredLogCollectionBudget, RuntimeActionDispatchDeadlineError,
    RuntimeActionDispatchDeadlineOutcome, await_runtime_action_dispatch_until,
};
use super::dst_client_table::{
    CollectedLivePlayers, DstCaptureOutcome, MAX_CAPTURE_BYTES, PartialLivePlayers,
    parse_dst_client_table,
};
use super::log_capture::{log_baseline, read_checked_delta};

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const READ_CHUNK_BYTES: usize = 8 * 1024;

pub(crate) fn build_cache_key(
    details: &InstanceDetails,
    descriptor: &ModuleDescriptor,
) -> Option<LivePlayerCacheKey> {
    let run = details.active_run.as_ref()?;
    let player_list = descriptor.runtime.player_list.as_ref()?;
    let process_key = player_list
        .action_id
        .as_ref()
        .and_then(|id| {
            descriptor
                .runtime
                .player_actions
                .iter()
                .find(|action| &action.id == id)
        })
        .and_then(|action| action.process_key.as_deref());
    let process_identity = run
        .processes
        .iter()
        .find(|process| process_key.map_or(process.is_primary, |key| process.process_key == key))
        .map(|process| (&process.process_key, process.pid, &process.process_identity));
    Some(LivePlayerCacheKey {
        instance_id: details.summary.id.clone(),
        run_id: run.run_id.to_string(),
        security_contract_fingerprint: format!(
            "{}:{process_identity:?}",
            build_live_player_security_contract_fingerprint(
                &descriptor.summary.id,
                player_list,
                &descriptor.runtime.player_actions,
            ),
        ),
        refresh_interval_ms: player_list.refresh_interval_ms,
    })
}

pub(crate) fn unsupported_snapshot(
    instance_id: &str,
    snapshot_id: String,
) -> RuntimeLivePlayerSnapshot {
    capability_snapshot(
        instance_id,
        snapshot_id,
        RuntimeLivePlayerStatus::Unsupported,
        None,
        Some(RuntimeLivePlayerIssue {
            code: RuntimeLivePlayerIssueCode::AdapterUnavailable,
            setting_keys: Vec::new(),
            summary: String::from("This module has no connected online-player adapter."),
        }),
    )
}

pub(crate) fn stopped_snapshot(
    instance_id: &str,
    snapshot_id: String,
    source: ModulePlayerListSource,
) -> RuntimeLivePlayerSnapshot {
    capability_snapshot(
        instance_id,
        snapshot_id,
        RuntimeLivePlayerStatus::Stopped,
        Some(source),
        None,
    )
}

pub(crate) fn refreshing_snapshot(
    instance_id: &str,
    snapshot_id: String,
    source: ModulePlayerListSource,
) -> RuntimeLivePlayerSnapshot {
    capability_snapshot(
        instance_id,
        snapshot_id,
        RuntimeLivePlayerStatus::Refreshing,
        Some(source),
        None,
    )
}

pub(crate) fn misconfigured_snapshot(
    instance_id: &str,
    snapshot_id: String,
    source: ModulePlayerListSource,
    code: RuntimeLivePlayerIssueCode,
    summary: impl Into<String>,
    setting_keys: Vec<String>,
) -> RuntimeLivePlayerSnapshot {
    capability_snapshot(
        instance_id,
        snapshot_id,
        RuntimeLivePlayerStatus::Misconfigured,
        Some(source),
        Some(RuntimeLivePlayerIssue {
            code,
            setting_keys,
            summary: summary.into(),
        }),
    )
}

pub(crate) fn failed_snapshot(
    instance_id: &str,
    snapshot_id: String,
    source: ModulePlayerListSource,
    code: RuntimeLivePlayerIssueCode,
    summary: impl Into<String>,
    truncated: bool,
) -> RuntimeLivePlayerSnapshot {
    let mut snapshot = capability_snapshot(
        instance_id,
        snapshot_id,
        RuntimeLivePlayerStatus::Failed,
        Some(source),
        Some(RuntimeLivePlayerIssue {
            code,
            setting_keys: Vec::new(),
            summary: summary.into(),
        }),
    );
    snapshot.truncated = truncated;
    snapshot
}

fn capability_snapshot(
    instance_id: &str,
    snapshot_id: String,
    status: RuntimeLivePlayerStatus,
    source: Option<ModulePlayerListSource>,
    issue: Option<RuntimeLivePlayerIssue>,
) -> RuntimeLivePlayerSnapshot {
    RuntimeLivePlayerSnapshot {
        snapshot_id,
        instance_id: instance_id.to_owned(),
        status,
        source,
        observed_at_unix_ms: None,
        expires_at_unix_ms: None,
        complete: false,
        truncated: false,
        stale: false,
        current_players: None,
        max_players: None,
        entries: Vec::new(),
        issue,
    }
}

pub(crate) fn select_running_process<'a>(
    details: &'a InstanceDetails,
    process_key: &str,
) -> Option<&'a InstanceProcessState> {
    details
        .active_run
        .as_ref()?
        .processes
        .iter()
        .find(|process| {
            process.process_key.eq_ignore_ascii_case(process_key)
                && process.status.eq_ignore_ascii_case("running")
                && process.pid.is_some()
        })
}

pub(crate) fn new_request_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

pub(crate) async fn collect_dst_structured_log<Dispatch, DispatchFuture>(
    instance_id: &str,
    source: ModulePlayerListSource,
    request_id: &str,
    log_path: &Path,
    validated_action_ids: &[String],
    collected_at: u64,
    dispatch: Dispatch,
) -> LivePlayerCollectionResult
where
    Dispatch: FnOnce(app_runtime::RuntimeCommandSubmissionTracker, Instant) -> DispatchFuture,
    DispatchFuture: std::future::Future<Output = Result<(), String>>,
{
    collect_dst_structured_log_with_budget(
        instance_id,
        source,
        request_id,
        log_path,
        validated_action_ids,
        collected_at,
        DstStructuredLogCollectionBudget::standard(),
        dispatch,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn collect_dst_structured_log_with_budget<Dispatch, DispatchFuture>(
    instance_id: &str,
    source: ModulePlayerListSource,
    request_id: &str,
    log_path: &Path,
    validated_action_ids: &[String],
    collected_at: u64,
    budget: DstStructuredLogCollectionBudget,
    dispatch: Dispatch,
) -> LivePlayerCollectionResult
where
    Dispatch: FnOnce(app_runtime::RuntimeCommandSubmissionTracker, Instant) -> DispatchFuture,
    DispatchFuture: std::future::Future<Output = Result<(), String>>,
{
    let started_at = Instant::now();
    let deadline = started_at + budget.total;
    let dispatch_deadline = started_at + budget.dispatch_phase.min(budget.total);
    let baseline_path = log_path.to_path_buf();
    let baseline_budget = dispatch_deadline.saturating_duration_since(Instant::now());
    let baseline = tokio::time::timeout(
        baseline_budget,
        tauri::async_runtime::spawn_blocking(move || log_baseline(&baseline_path)),
    )
    .await
    .map_err(|_| {
        failed_snapshot(
            instance_id,
            request_id.to_owned(),
            source,
            RuntimeLivePlayerIssueCode::CollectionTimeout,
            "Timed out while preparing the server player log.",
            false,
        )
    })?
    .map_err(|_| {
        failed_snapshot(
            instance_id,
            request_id.to_owned(),
            source,
            RuntimeLivePlayerIssueCode::IoFailed,
            "Unable to prepare the server player log.",
            false,
        )
    })?
    .map_err(|_| {
        failed_snapshot(
            instance_id,
            request_id.to_owned(),
            source,
            RuntimeLivePlayerIssueCode::LogUnavailable,
            "The server player log is unavailable.",
            false,
        )
    })?;

    let dispatch_confirmation_pending =
        match await_runtime_action_dispatch_until(dispatch_deadline, dispatch).await {
            Ok(RuntimeActionDispatchDeadlineOutcome::Completed) => false,
            Ok(RuntimeActionDispatchDeadlineOutcome::SubmittedPending) => true,
            Err(RuntimeActionDispatchDeadlineError::DeadlineBeforeSubmission) => {
                return Err(failed_snapshot(
                    instance_id,
                    request_id.to_owned(),
                    source,
                    RuntimeLivePlayerIssueCode::CollectionTimeout,
                    "Player collection timed out before the list action could be submitted.",
                    false,
                )
                .into());
            }
            Err(RuntimeActionDispatchDeadlineError::DispatchFailed) => {
                return Err(failed_snapshot(
                    instance_id,
                    request_id.to_owned(),
                    source,
                    RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
                    "The server could not run its player-list action.",
                    false,
                )
                .into());
            }
        };

    let mut tail = RuntimeLogTailState::default();
    tail.byte_offset = baseline.byte_offset;
    let mut lines = Vec::new();
    let mut total_bytes = 0_usize;
    let mut last_partial = None;

    loop {
        if Instant::now() >= deadline {
            return Err(last_partial
                .map(|partial| partial_failure(instance_id, request_id, source, partial))
                .unwrap_or_else(|| {
                    failed_snapshot(
                        instance_id,
                        request_id.to_owned(),
                        source,
                        RuntimeLivePlayerIssueCode::CollectionTimeout,
                        if dispatch_confirmation_pending {
                            "The player-list action was accepted, but its response did not arrive in time."
                        } else {
                            "Timed out while waiting for the server player list."
                        },
                        false,
                    )
                })
                .into());
        }

        let remaining = MAX_CAPTURE_BYTES.saturating_sub(total_bytes);
        if remaining == 0 {
            return Err(failed_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::CaptureLimit,
                "The server player response exceeded the safe capture limit.",
                true,
            )
            .into());
        }
        let read_budget = remaining.min(READ_CHUNK_BYTES);
        let read_path = log_path.to_path_buf();
        let expected_baseline = baseline.clone();
        let io_budget = deadline.saturating_duration_since(Instant::now());
        let (next_tail, delta) = tokio::time::timeout(
            io_budget,
            tauri::async_runtime::spawn_blocking(move || {
                let mut tail = tail;
                let result =
                    read_checked_delta(&read_path, &expected_baseline, &mut tail, read_budget);
                (tail, result)
            }),
        )
        .await
        .map_err(|_| {
            failed_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::CollectionTimeout,
                if dispatch_confirmation_pending {
                    "The player-list action was accepted, but reading its response timed out."
                } else {
                    "Timed out while reading the server player response."
                },
                false,
            )
        })?
        .map_err(|_| {
            failed_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::IoFailed,
                "Unable to read the server player response.",
                false,
            )
        })?;
        tail = next_tail;
        let delta = delta.map_err(|_| {
            failed_snapshot(
                instance_id,
                request_id.to_owned(),
                source,
                RuntimeLivePlayerIssueCode::IoFailed,
                "The server player response became unavailable during refresh.",
                false,
            )
        })?;
        total_bytes = total_bytes.saturating_add(delta.bytes_read);
        lines.extend(delta.lines);

        match parse_dst_client_table(&lines, request_id, validated_action_ids) {
            DstCaptureOutcome::Complete(collected) => {
                return Ok(cached_complete_snapshot(
                    instance_id,
                    request_id,
                    source,
                    collected_at,
                    collected,
                ));
            }
            DstCaptureOutcome::Incomplete(partial) => {
                if partial.truncated {
                    return Err(partial_failure(instance_id, request_id, source, partial).into());
                }
                last_partial = Some(partial);
            }
            DstCaptureOutcome::NoCapture => {}
        }

        if delta.bytes_read == 0 {
            tokio::time::sleep(
                POLL_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
    }
}

fn cached_complete_snapshot(
    instance_id: &str,
    request_id: &str,
    source: ModulePlayerListSource,
    collected_at: u64,
    collected: CollectedLivePlayers,
) -> CachedLivePlayerSnapshot {
    let mut private_action_bindings = HashMap::new();
    let entries = collected
        .players
        .into_iter()
        .map(|player| {
            for (action_id, target) in player.action_bindings {
                private_action_bindings
                    .insert((player.entry.player_key.clone(), action_id), target);
            }
            player.entry
        })
        .collect();
    CachedLivePlayerSnapshot {
        public_snapshot: RuntimeLivePlayerSnapshot {
            snapshot_id: request_id.to_owned(),
            instance_id: instance_id.to_owned(),
            status: collected.status,
            source: Some(source),
            observed_at_unix_ms: Some(collected_at),
            expires_at_unix_ms: None,
            complete: collected.complete,
            truncated: collected.truncated,
            stale: false,
            current_players: collected.current_players,
            max_players: None,
            entries,
            issue: collected.truncated.then(|| RuntimeLivePlayerIssue {
                code: RuntimeLivePlayerIssueCode::CaptureLimit,
                setting_keys: Vec::new(),
                summary: String::from("Only part of the player list could be returned safely."),
            }),
        },
        private_action_bindings,
        collected_at,
    }
}

fn partial_failure(
    instance_id: &str,
    request_id: &str,
    source: ModulePlayerListSource,
    partial: PartialLivePlayers,
) -> RuntimeLivePlayerSnapshot {
    let truncated = partial.truncated;
    let entries = partial
        .players
        .into_iter()
        .map(|mut player| {
            player.entry.available_action_ids.clear();
            player.entry
        })
        .collect::<Vec<_>>();
    let mut snapshot = failed_snapshot(
        instance_id,
        request_id.to_owned(),
        source,
        if truncated {
            RuntimeLivePlayerIssueCode::CaptureLimit
        } else {
            RuntimeLivePlayerIssueCode::ProtocolIncomplete
        },
        if truncated {
            "The server player response exceeded the safe capture limit."
        } else {
            "The server returned an incomplete player list."
        },
        truncated,
    );
    snapshot.entries = entries;
    snapshot
}
