//! DST's native startup barrier already confirms every enabled Lua world and
//! the Master's shard connections. Health must use that same current-run proof.
use super::*;
use crate::runtime_log_diagnostics::{DstGenerationEvent, dst_generation_event};
use console_health_evidence::SessionKind;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GenerationFailure {
    pub(super) line: String,
    retried: bool,
}

impl GenerationFailure {
    pub(super) fn valid(&self, line_limit: usize) -> bool {
        self.line.len() <= line_limit
            && matches!(
                dst_generation_event(&self.line),
                Some(DstGenerationEvent::MissingPrefab)
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event<'a> {
    Probe(&'a str),
    Ready(&'a str),
    Failed(&'a str),
}

pub(super) fn event(line: &str) -> Option<Event<'_>> {
    let body = crate::runtime_log_diagnostics::native_dst_log_body(line)?;
    let (kind, rest) = body.strip_prefix("[LGSM-DST-")?.split_once(':')?;
    let (nonce, suffix) = rest.split_once(']')?;
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    match (kind, suffix) {
        ("PROBE", "") => Some(Event::Probe(nonce)),
        ("READY", "") => Some(Event::Ready(nonce)),
        ("FAILED", message) if message.starts_with(' ') && !message.trim().is_empty() => {
            Some(Event::Failed(nonce))
        }
        _ => None,
    }
}

pub(super) fn consume(
    latest: &mut Option<String>,
    fatal: &mut Option<String>,
    generation_failure: &mut Option<GenerationFailure>,
    line: &str,
) {
    // Preserve every ordinary fatal after it leaves the UI tail. Only DST's
    // known missing-prefab generation retry has a native recovery sequence.
    let generation = dst_generation_event(line);
    if !matches!(generation, Some(DstGenerationEvent::MissingPrefab))
        && !crate::runtime_fatal_log_lines("dontstarve", &[line.into()]).is_empty()
    {
        *fatal = Some(line.into());
    }
    match generation {
        Some(DstGenerationEvent::MissingPrefab) => {
            *generation_failure = Some(GenerationFailure {
                line: line.into(),
                retried: false,
            });
        }
        Some(DstGenerationEvent::Retry) => {
            if let Some(failure) = generation_failure {
                failure.retried = true;
            }
        }
        Some(DstGenerationEvent::Complete)
            if generation_failure
                .as_ref()
                .is_some_and(|failure| failure.retried) =>
        {
            *generation_failure = None;
        }
        Some(DstGenerationEvent::Complete) | None => {}
    }
    let previous = latest.as_deref().and_then(event);
    if matches!(previous, Some(Event::Failed(_))) {
        return;
    }
    let accepted = match event(line) {
        Some(Event::Probe(_)) => true,
        Some(Event::Ready(nonce)) => {
            matches!(previous, Some(Event::Probe(expected)) if expected == nonce)
        }
        // Startup rejects a correlated failure anywhere in the observed batch,
        // including after READY. A stale nonce still cannot revoke this run.
        Some(Event::Failed(nonce)) => {
            matches!(previous, Some(Event::Probe(expected) | Event::Ready(expected)) if expected == nonce)
        }
        None => false,
    };
    if accepted {
        *latest = Some(line.into());
    }
}

pub(super) fn analyze(
    settings_json: &str,
    status: &InstanceStatus,
    active: Option<&ActiveInstanceRun>,
    previous: app_core::RuntimeHealth,
) -> app_core::RuntimeHealth {
    if !matches!(status, InstanceStatus::Running | InstanceStatus::Starting) {
        return previous;
    }
    let settings = match serde_json::from_str(settings_json) {
        Ok(settings) => settings,
        Err(_) => return pending(),
    };
    let shards = match app_core::dst_shards::dst_shards(&settings) {
        Ok(shards) => shards,
        Err(_) => return pending(),
    };
    let Some(active) = active.filter(|run| run.run_id > 0 && run.session_id.is_some()) else {
        return pending();
    };
    let mut observed_nonce = None::<String>;
    let mut run_ids = HashSet::new();
    let mut pids = HashSet::new();
    let mut logs = HashSet::new();
    for shard in shards {
        let matching = active
            .processes
            .iter()
            .filter(|process| process.process_key == shard.process_key)
            .collect::<Vec<_>>();
        let [process] = matching.as_slice() else {
            return pending();
        };
        if process.status != "running"
            || process.crash_flag
            || process.exit_code.is_some()
            || process.run_id <= 0
            || process.session_id != active.session_id
            || !run_ids.insert(process.run_id)
            || !process.process_identity.as_ref().is_some_and(|identity| {
                identity.creation_time > 0 && !identity.image_path.trim().is_empty()
            })
        {
            return pending();
        }
        let Some(pid) = process.pid.filter(|pid| *pid > 0 && pids.insert(*pid)) else {
            return pending();
        };
        let Some(path) = process
            .log_path
            .as_deref()
            .filter(|path| logs.insert(*path))
        else {
            return pending();
        };
        if process.is_primary != (shard.process_key == "master")
            || (process.is_primary
                && (process.run_id != active.run_id
                    || active.pid != Some(pid)
                    || active.log_path.as_deref() != Some(path)))
        {
            return pending();
        }
        // Keep each process's actual database run ID. Siblings share the
        // session and startup nonce, not the Master's run ID or log source.
        let run = ActiveInstanceRun {
            run_id: process.run_id,
            session_id: process.session_id.clone(),
            pid: process.pid,
            log_path: process.log_path.clone(),
            process_count: 1,
            processes: vec![(**process).clone()],
        };
        let tail = read_log_snapshot(Some(path.into()), RUNTIME_HEALTH_SCAN_LINE_LIMIT);
        let diagnostic = analyze_runtime_health("dontstarve", status, &tail, Some(&run), &[]);
        let kind = match shard.process_key {
            "master" => SessionKind::DstMaster,
            "caves" => SessionKind::DstCaves,
            "islands" => SessionKind::DstIslands,
            "volcano" => SessionKind::DstVolcano,
            _ => return pending(),
        };
        let lines = match console_health_evidence::observe(kind, Path::new(path), process.run_id) {
            Ok(lines) => lines,
            Err(error) => {
                return app_core::RuntimeHealth {
                    status: "warning".into(),
                    summary: format!("DST shard readiness could not be read: {error}"),
                    reason: runtime_health_reason(
                        "log_read_failed",
                        &[("error", error.to_string())],
                    ),
                    matched_line: None,
                };
            }
        };
        // Read the observer even when the recent tail reports a failure, so
        // returning that diagnostic cannot leave an older READY checkpoint.
        if !matches!(previous.status.as_str(), "ready" | "starting") {
            return previous;
        }
        if !matches!(diagnostic.status.as_str(), "ready" | "starting") {
            return diagnostic;
        }
        let retained = analyze_runtime_health(
            "dontstarve",
            status,
            &LogTailSnapshot {
                source_path: Some(path.into()),
                total_lines: lines.len(),
                lines: lines.clone(),
                truncated: false,
                read_error: None,
            },
            Some(&run),
            &[],
        );
        if !matches!(retained.status.as_str(), "ready" | "starting") {
            return retained;
        }
        let Some(line) = lines.last() else {
            return pending();
        };
        let nonce = match event(line) {
            Some(Event::Ready(nonce)) => nonce,
            Some(Event::Failed(_)) => {
                return app_core::RuntimeHealth {
                    status: "error".into(),
                    summary: "DST reported a native world or Mod initialization failure.".into(),
                    reason: runtime_health_reason("fatal_log_pattern", &[]),
                    matched_line: Some(line.clone()),
                };
            }
            _ => return pending(),
        };
        if observed_nonce
            .as_deref()
            .is_some_and(|expected| expected != nonce)
        {
            return pending();
        }
        observed_nonce = Some(nonce.into());
    }
    app_core::RuntimeHealth {
        status: "ready".into(),
        summary: "Every enabled DST shard confirmed its native world and shard connections.".into(),
        reason: runtime_health_reason("ready_signal", &[]),
        matched_line: None,
    }
}

fn pending() -> app_core::RuntimeHealth {
    app_core::RuntimeHealth {
        status: "starting".into(),
        summary: "Waiting for current-run native readiness from every enabled DST shard.".into(),
        reason: runtime_health_reason("starting_tasks", &[]),
        matched_line: None,
    }
}

#[cfg(test)]
#[path = "runtime_dst_health_tests.rs"]
mod tests;
