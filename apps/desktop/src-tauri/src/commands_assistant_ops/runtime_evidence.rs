const ASSISTANT_RUNTIME_EVIDENCE_PROCESSES: usize = 8;
const ASSISTANT_RUNTIME_EVIDENCE_LINES: usize = 400;
const ASSISTANT_RUNTIME_EVIDENCE_BYTES: usize = 10 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssistantRuntimeLogBinding {
    run_id: i64,
    session_id: Option<String>,
    process_key: String,
    pid: Option<u32>,
    process_identity: Option<app_core::ProcessIdentity>,
    status: String,
    exit_code: Option<i32>,
    crash_flag: bool,
    log_path: Option<String>,
}

fn assistant_runtime_log_bindings(
    processes: &[app_core::InstanceProcessState],
) -> Vec<AssistantRuntimeLogBinding> {
    let mut bindings = processes
        .iter()
        .map(|process| AssistantRuntimeLogBinding {
            run_id: process.run_id,
            session_id: process.session_id.clone(),
            process_key: process.process_key.clone(),
            pid: process.pid,
            process_identity: process.process_identity.clone(),
            status: process.status.clone(),
            exit_code: process.exit_code,
            crash_flag: process.crash_flag,
            log_path: process.log_path.clone(),
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| {
        left.process_key
            .cmp(&right.process_key)
            .then(left.run_id.cmp(&right.run_id))
    });
    bindings
}

fn assistant_runtime_observation_is_current(
    before: &InstanceDetails,
    latest_before: Option<&app_core::InstanceRunRecord>,
    after: &InstanceDetails,
    latest_after: Option<&app_core::InstanceRunRecord>,
) -> bool {
    let run_key = |instance: &InstanceDetails| {
        instance.active_run.as_ref().map(|run| {
            (
                run.run_id,
                run.session_id.clone(),
                run.pid,
                run.log_path.clone(),
                run.process_count,
            )
        })
    };
    let processes = |instance: &InstanceDetails| {
        instance
            .active_run
            .as_ref()
            .map(|run| assistant_runtime_log_bindings(&run.processes))
    };
    let latest_key = |run: &app_core::InstanceRunRecord| {
        (
            run.run_id,
            run.session_id.clone(),
            run.pid,
            run.log_path.clone(),
            run.process_count,
            run.status.clone(),
            run.exit_code,
            run.crash_flag,
            assistant_runtime_log_bindings(&run.processes),
        )
    };
    before.summary.id == after.summary.id
        && before.summary.module_id == after.summary.module_id
        && std::mem::discriminant(&before.summary.status)
            == std::mem::discriminant(&after.summary.status)
        && run_key(before) == run_key(after)
        && processes(before) == processes(after)
        && latest_before.map(latest_key) == latest_after.map(latest_key)
}

pub(super) async fn read_assistant_runtime_evidence(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    instance_id: &str,
    lines: usize,
) -> Result<Value, String> {
    ensure_storage_context_snapshot_current(state, storage, "assistant runtime evidence")?;
    let before = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    // This is an observation only. Reconciliation here could dispatch restarts.
    let overview = read_instance_runtime_overview(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let source = before.clone();
    let latest = overview.recent_runs.first().cloned();
    let mut evidence = run_assistant_evidence_read(state, move || {
        Ok(collect_assistant_registered_runtime_logs(&source, latest.as_ref(), lines))
    })
    .await?;

    // A failed launch may not yet have a registered process. Preserve that tail
    // as explicitly unbound evidence, never as proof about a current run.
    if before.active_run.is_none() {
        let fallback = pending_start_console_log_snapshot(state, instance_id, lines.clamp(1, 50))
            .filter(assistant_log_snapshot_has_content)
            .or_else(|| {
                overview
                    .recent_runs
                    .is_empty()
                    .then(|| overview.log_tail.clone())
            });
        if let Some(fallback) = fallback {
            let (log, names) = assistant_runtime_log_evidence(&before.summary.module_id, &fallback);
            evidence["unboundLog"] = log;
            let mut candidates = evidence["modErrorNames"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect::<std::collections::BTreeSet<_>>();
            candidates.extend(names);
            evidence["modCandidatesMayBeTruncated"] =
                json!(evidence["modCandidatesMayBeTruncated"] == true || candidates.len() >= 5);
            evidence["modErrorNames"] = json!(candidates.into_iter().take(5).collect::<Vec<_>>());
        }
    }
    evidence["health"] = json!({
        "scope": "default_log_summary",
        "status": assistant_runtime_evidence_text(&overview.health.status, 80),
        "reason": assistant_runtime_evidence_text(&overview.health.reason.code, 120),
        "summary": assistant_runtime_evidence_text(&overview.health.summary, 320),
        "matchedLine": overview.health.matched_line.as_deref().map(|line| assistant_runtime_evidence_text(line, 320)),
    });
    evidence["recentRuns"] = json!(overview.recent_runs.iter().take(8).map(|run| json!({
        "runId": run.run_id, "status": assistant_runtime_evidence_text(&run.status, 40),
        "exitCode": run.exit_code, "crashFlag": run.crash_flag, "processCount": run.process_count,
    })).collect::<Vec<_>>());
    evidence["diagnostics"] = json!(
        overview
            .diagnostics
            .iter()
            .take(4)
            .map(|diagnostic| json!({
                "code": assistant_runtime_evidence_text(&diagnostic.code, 120),
                "severity": assistant_runtime_evidence_text(&diagnostic.severity, 40),
                "summary": assistant_runtime_evidence_text(&diagnostic.summary, 256),
            }))
            .collect::<Vec<_>>()
    );
    evidence["omittedDiagnosticCount"] = json!(overview.diagnostics.len().saturating_sub(4));
    let overview_after = read_instance_runtime_overview(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    let after = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| redact_assistant_provider_text(&error.to_string()))?;
    ensure_storage_context_snapshot_current(state, storage, "assistant runtime evidence")?;
    finalize_assistant_runtime_evidence(
        &before,
        overview.recent_runs.first(),
        &after,
        overview_after.recent_runs.first(),
        evidence,
    )
}

fn collect_assistant_registered_runtime_logs(
    instance: &InstanceDetails,
    latest: Option<&app_core::InstanceRunRecord>,
    lines: usize,
) -> Value {
    let (run_id, expected_count, processes, observation_status, scope) =
        if let Some(run) = &instance.active_run {
            (
                Some(run.run_id),
                run.process_count,
                run.processes.as_slice(),
                "current",
                "active_session",
            )
        } else if let Some(run) = latest {
            (
                Some(run.run_id),
                run.process_count,
                run.processes.as_slice(),
                "historical",
                "latest_registered_session",
            )
        } else {
            (None, 0, &[][..], "unknown", "no_registered_session")
        };
    let bindings = assistant_runtime_log_bindings(processes);
    let count = bindings.len().min(ASSISTANT_RUNTIME_EVIDENCE_PROCESSES);
    let per_source = (lines.clamp(1, ASSISTANT_RUNTIME_EVIDENCE_LINES) / count.max(1)).max(1);
    let mut names = std::collections::BTreeSet::new();
    let process_logs = bindings.iter().take(ASSISTANT_RUNTIME_EVIDENCE_PROCESSES).map(|binding| {
        let snapshot = match binding.log_path.as_deref().filter(|path| !path.trim().is_empty()) {
            Some(path) => read_log_path_snapshot(path.to_owned(), per_source),
            None => LogTailSnapshot {
                source_path: None, lines: Vec::new(), total_lines: 0, truncated: false,
                read_error: Some(String::from("No log path is registered for this process.")),
            },
        };
        let (mut log, candidates) = assistant_runtime_log_evidence(&instance.summary.module_id, &snapshot);
        if observation_status == "historical" {
            log["scope"] = json!("historical_registered_process_tail");
        }
        names.extend(candidates);
        json!({
            "runId": binding.run_id,
            "sessionId": binding.session_id.as_deref().map(|id| assistant_runtime_evidence_text(id, 160)),
            "processKey": assistant_runtime_evidence_text(&binding.process_key, 160),
            "pid": binding.pid,
            "identityRecorded": binding.process_identity.is_some(),
            "processCreationTime": binding.process_identity.as_ref().map(|identity| identity.creation_time),
            "recordedStatus": assistant_runtime_evidence_text(&binding.status, 40),
            "exitCode": binding.exit_code, "crashFlag": binding.crash_flag,
            "log": log,
        })
    }).collect::<Vec<_>>();
    json!({
        "observationStatus": observation_status,
        "evidenceScope": scope,
        "instanceId": assistant_runtime_evidence_text(&instance.summary.id, 160),
        "moduleId": assistant_runtime_evidence_text(&instance.summary.module_id, 160),
        "runId": run_id,
        "instanceStatus": instance.summary.status,
        "livenessVerified": false,
        "processLogs": process_logs,
        "omittedProcessCount": bindings.len().saturating_sub(count),
        "missingProcessRecordCount": expected_count.saturating_sub(bindings.len()),
        "modCandidatesMayBeTruncated": names.len() >= 5,
        "modErrorNames": names.into_iter().take(5).collect::<Vec<_>>(),
        "evidenceTruncated": bindings.len() > count,
        "coverage": "Bounded tails of registered process logs, not a complete error history. At most five Mod directory candidates per source are collected. Recorded status is not a live process probe. A default-log health summary does not establish other processes are healthy.",
    })
}

fn assistant_runtime_log_evidence(
    module_id: &str,
    snapshot: &LogTailSnapshot,
) -> (Value, Vec<String>) {
    // Redact the whole excerpt before splitting lines so multi-line YAML secrets
    // retain their context. No model-controlled path is ever passed to a reader.
    let redacted = redact_assistant_provider_text(&snapshot.lines.join("\n"));
    let lines = redacted.lines().map(String::from).collect::<Vec<_>>();
    let names = if module_id == "dontstarve" && snapshot.read_error.is_none() {
        crate::dst_mods::dst_mod_error_names(&lines)
    } else {
        Vec::new()
    };
    let failures = assistant_verification_failure_lines(module_id, &lines);
    let shortened = lines
        .iter()
        .map(|line| assistant_runtime_evidence_text(line, 384))
        .collect::<Vec<_>>();
    let text_truncated = shortened != lines;
    (
        json!({
            "scope": "registered_process_tail",
            "sourceName": snapshot.source_path.as_deref().and_then(|path| path.rsplit(['/', '\\']).find(|part| !part.is_empty())).map(|name| assistant_runtime_evidence_text(name, 160)),
            "lines": shortened,
            "failureLines": failures.iter().take(2).map(|line| assistant_runtime_evidence_text(line, 320)).collect::<Vec<_>>(),
            "failureExcerptsTruncated": failures.len() > 2,
            "observedLineCount": snapshot.total_lines,
            "truncated": snapshot.truncated || text_truncated,
            "omittedTailLineCount": 0,
            "readError": snapshot.read_error.as_deref().map(|error| assistant_runtime_evidence_text(error, 256)),
        }),
        names,
    )
}

fn assistant_runtime_evidence_text(text: &str, limit: usize) -> String {
    let redacted = redact_assistant_provider_text(text);
    const MARKER: &str = " [truncated]";
    let mut encoded = 0;
    let mut output = String::new();
    for character in redacted.chars() {
        let cost = match character {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => character.len_utf8(),
        };
        if encoded + cost > limit.saturating_sub(MARKER.len()) {
            output.push_str(MARKER);
            return output;
        }
        encoded += cost;
        output.push(character);
    }
    output
}

fn finalize_assistant_runtime_evidence(
    before: &InstanceDetails,
    latest_before: Option<&app_core::InstanceRunRecord>,
    after: &InstanceDetails,
    latest_after: Option<&app_core::InstanceRunRecord>,
    mut evidence: Value,
) -> Result<Value, String> {
    if !assistant_runtime_observation_is_current(before, latest_before, after, latest_after) {
        return Ok(json!({
            "observationStatus": "unknown", "reason": "runtime_changed_during_read",
            "observedRunId": before.active_run.as_ref().map(|run| run.run_id).or_else(|| latest_before.map(|run| run.run_id)),
            "currentRunId": after.active_run.as_ref().map(|run| run.run_id).or_else(|| latest_after.map(|run| run.run_id)),
            "processLogs": [], "modErrorNames": [],
            "message": "Run ownership or recorded process state changed while logs were read. Discarded that observation; read runtime again before diagnosing the current run.",
        }));
    }
    if let Some(unbound) = evidence.get_mut("unboundLog") {
        unbound["scope"] = json!("unbound_tail_not_current_run_evidence");
    }
    let redacted = redact_assistant_provider_text(&evidence.to_string());
    evidence = serde_json::from_str(&redacted)
        .map_err(|_| String::from("Runtime evidence could not be safely encoded."))?;
    loop {
        if evidence.to_string().len() <= ASSISTANT_RUNTIME_EVIDENCE_BYTES {
            return Ok(evidence);
        }
        // Remove ordinary tail lines before failure excerpts. Keep every selected
        // process's identity and error/read-failure state visible under pressure.
        let processes = evidence["processLogs"]
            .as_array_mut()
            .ok_or_else(|| String::from("Runtime evidence process list is unavailable."))?;
        let candidate = processes
            .iter()
            .enumerate()
            .filter(|(_, process)| {
                process["log"]["lines"]
                    .as_array()
                    .is_some_and(|lines| !lines.is_empty())
            })
            .max_by_key(|(_, process)| process["log"]["lines"].to_string().len())
            .map(|(index, _)| index);
        if let Some(index) = candidate {
            trim_assistant_runtime_tail(&mut processes[index]["log"]);
        } else if evidence["unboundLog"]["lines"]
            .as_array()
            .is_some_and(|lines| !lines.is_empty())
        {
            trim_assistant_runtime_tail(&mut evidence["unboundLog"]);
        } else {
            return Err(String::from(
                "Runtime identities and diagnostics exceed the evidence budget; no complete observation was supplied.",
            ));
        }
        evidence["evidenceTruncated"] = json!(true);
    }
}

fn trim_assistant_runtime_tail(log: &mut Value) {
    if let Some(lines) = log["lines"].as_array_mut()
        && !lines.is_empty()
    {
        lines.remove(0);
    }
    log["truncated"] = json!(true);
    log["omittedTailLineCount"] = json!(log["omittedTailLineCount"].as_u64().unwrap_or(0) + 1);
}

#[cfg(test)]
#[path = "runtime_evidence_tests.rs"]
mod runtime_evidence_tests;
