use std::path::Path;
use std::time::{Duration, Instant};

use app_core::{
    ModulePlayerListCodec, ModulePlayerListSource, RuntimeLivePlayerIssueCode,
    RuntimeLivePlayerSnapshot, RuntimeLivePlayerStatus,
};

use super::cache::{CachedLivePlayerSnapshot, LivePlayerCollectionResult};
use super::log_capture::{log_baseline, read_checked_delta};
use super::service::{
    RuntimeActionDispatchDeadlineError, await_runtime_action_dispatch_until, failed_snapshot,
};
use crate::runtime_log_stream::RuntimeLogTailState;

#[cfg(test)]
#[path = "console_log_tests.rs"]
mod tests;

pub(crate) struct ConsoleCollection<'a> {
    pub instance_id: &'a str,
    pub request_id: &'a str,
    pub codec: ModulePlayerListCodec,
    pub log_path: &'a Path,
    pub observed_at: u64,
}

pub(crate) async fn collect_console_players<Dispatch, DispatchFuture>(
    context: ConsoleCollection<'_>,
    dispatch: Dispatch,
) -> LivePlayerCollectionResult
where
    Dispatch: FnOnce(app_runtime::RuntimeCommandSubmissionTracker, Instant) -> DispatchFuture,
    DispatchFuture: std::future::Future<Output = Result<(), String>>,
{
    collect_console_players_with_budget(context, Duration::from_secs(5), dispatch).await
}

async fn collect_console_players_with_budget<Dispatch, DispatchFuture>(
    context: ConsoleCollection<'_>,
    total_budget: Duration,
    dispatch: Dispatch,
) -> LivePlayerCollectionResult
where
    Dispatch: FnOnce(app_runtime::RuntimeCommandSubmissionTracker, Instant) -> DispatchFuture,
    DispatchFuture: std::future::Future<Output = Result<(), String>>,
{
    let source = ModulePlayerListSource::ConsoleLog;
    let deadline = Instant::now() + total_budget;
    let failure = |code, message: &str, truncated| {
        Box::new(failed_snapshot(
            context.instance_id,
            context.request_id.to_owned(),
            source,
            code,
            message,
            truncated,
        ))
    };
    let path = context.log_path.to_path_buf();
    let baseline = tokio::time::timeout_at(
        tokio::time::Instant::from_std(deadline),
        tokio::task::spawn_blocking(move || log_baseline(&path)),
    )
    .await
    .map_err(|_| {
        failure(
            RuntimeLivePlayerIssueCode::CollectionTimeout,
            "Preparing the player console timed out.",
            false,
        )
    })?
    .map_err(|_| {
        failure(
            RuntimeLivePlayerIssueCode::IoFailed,
            "The player console could not be opened.",
            false,
        )
    })?
    .map_err(|_| {
        failure(
            RuntimeLivePlayerIssueCode::LogUnavailable,
            "The player console is unavailable.",
            false,
        )
    })?;
    await_runtime_action_dispatch_until(
        (Instant::now() + Duration::from_secs(2)).min(deadline),
        dispatch,
    )
    .await
    .map_err(|error| match error {
        RuntimeActionDispatchDeadlineError::DeadlineBeforeSubmission => failure(
            RuntimeLivePlayerIssueCode::CollectionTimeout,
            "The player-list command could not be submitted before the deadline.",
            false,
        ),
        RuntimeActionDispatchDeadlineError::DispatchFailed => failure(
            RuntimeLivePlayerIssueCode::RuntimeActionUnavailable,
            "The player-list command could not be submitted.",
            false,
        ),
    })?;
    let mut tail = RuntimeLogTailState::default();
    tail.byte_offset = baseline.byte_offset;
    let mut captured = String::new();
    let mut total_bytes = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(failure(
                RuntimeLivePlayerIssueCode::ProtocolIncomplete,
                "The console did not return a complete player list before the deadline.",
                false,
            ));
        }
        let remaining = (64 * 1024_usize).saturating_sub(total_bytes);
        if remaining == 0 {
            return Err(failure(
                RuntimeLivePlayerIssueCode::CaptureLimit,
                "The console player response exceeded its capture limit.",
                true,
            ));
        }
        let path = context.log_path.to_path_buf();
        let baseline = baseline.clone();
        let (next_tail, delta) = tokio::time::timeout_at(
            tokio::time::Instant::from_std(deadline),
            tokio::task::spawn_blocking(move || {
                let mut tail = tail;
                let delta = read_checked_delta(&path, &baseline, &mut tail, remaining);
                (tail, delta)
            }),
        )
        .await
        .map_err(|_| {
            failure(
                RuntimeLivePlayerIssueCode::CollectionTimeout,
                "Reading the player console timed out.",
                false,
            )
        })?
        .map_err(|_| {
            failure(
                RuntimeLivePlayerIssueCode::IoFailed,
                "The player console read failed.",
                false,
            )
        })?;
        tail = next_tail;
        let delta = delta.map_err(|_| {
            failure(
                RuntimeLivePlayerIssueCode::LogUnavailable,
                "The console changed while reading players.",
                false,
            )
        })?;
        total_bytes += delta.bytes_read;
        if delta.limit_exhausted {
            return Err(failure(
                RuntimeLivePlayerIssueCode::CaptureLimit,
                "The console response contains an overlong or truncated line.",
                true,
            ));
        }
        for line in delta.lines {
            captured.push_str(&line);
            captured.push('\n');
        }
        if let Ok(mut parsed) = super::console_codecs::parse(
            context.codec,
            &captured,
            context.request_id,
            context.observed_at,
        ) {
            // Console output without a request token is a read-only observation, never action authority.
            for entry in &mut parsed.entries {
                entry.available_action_ids.clear();
            }
            return Ok(CachedLivePlayerSnapshot {
                public_snapshot: RuntimeLivePlayerSnapshot {
                    snapshot_id: context.request_id.to_owned(),
                    instance_id: context.instance_id.to_owned(),
                    status: RuntimeLivePlayerStatus::Ready,
                    source: Some(source),
                    observed_at_unix_ms: Some(context.observed_at),
                    expires_at_unix_ms: None,
                    complete: parsed.complete,
                    truncated: parsed.truncated,
                    stale: false,
                    current_players: parsed.current_players,
                    max_players: parsed.max_players,
                    entries: parsed.entries,
                    issue: None,
                },
                private_action_bindings: std::collections::HashMap::new(),
                collected_at: context.observed_at,
            });
        }
        if delta.bytes_read == 0 {
            tokio::time::sleep(
                Duration::from_millis(100).min(deadline.saturating_duration_since(Instant::now())),
            )
            .await;
        }
    }
}
