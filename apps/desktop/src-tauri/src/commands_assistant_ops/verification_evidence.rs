async fn collect_assistant_verification_sample(
    storage: &StorageBootstrap,
    before: &InstanceDetails,
    started: Option<&AssistantVerificationStart>,
) -> Result<AssistantVerificationSample, String> {
    let details = read_instance_details(&storage.paths, &before.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    // This storage read never dispatches the global supervisor or scheduled restarts.
    let overview = read_instance_runtime_overview(&storage.paths, &before.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let processes = details.active_run.as_ref().map_or_else(Vec::new, |run| {
        run.processes
            .iter()
            .take(ASSISTANT_VERIFICATION_MAX_PROCESSES)
            .map(|process| (process.process_key.clone(), process.pid))
            .collect::<Vec<_>>()
    });
    let log_paths = started.map_or_else(Vec::new, |receipt| {
        receipt
            .processes
            .iter()
            .take(ASSISTANT_VERIFICATION_MAX_PROCESSES)
            .map(|process| (process.1.clone(), process.3.clone()))
            .collect::<Vec<_>>()
    });
    let module_id = before.summary.module_id.clone();
    let (process_identities, log_failures, log_read_errors) =
        tokio::task::spawn_blocking(move || {
            let identities = processes
                .into_iter()
                .map(|(key, pid)| {
                    (
                        key,
                        pid.map_or(Ok(None), |pid| {
                            inspect_process_identity(pid).map_err(|error| error.to_string())
                        }),
                    )
                })
                .collect();
            let mut failures = Vec::new();
            let mut read_errors = Vec::new();
            for (key, path) in log_paths {
                let log = read_log_path_snapshot(path, 80);
                if let Some(error) = log.read_error {
                    read_errors.push(format!("{key}: {error}"));
                }
                for line in assistant_verification_failure_lines(&module_id, &log.lines) {
                    failures.push(format!("{key}: {line}"));
                }
            }
            (identities, failures, read_errors)
        })
        .await
        .map_err(|error| format!("Runtime observation worker failed: {error}"))?;
    let (launch_ready, launch_issues) = if details.active_run.is_none() && started.is_none() {
        let modules_root = storage.paths.modules_root.clone();
        let settings = storage.settings.clone();
        let instance = details.clone();
        let plan = tokio::task::spawn_blocking(move || {
            let descriptors = discover_modules(&modules_root).map_err(|error| error.to_string())?;
            let descriptor = find_descriptor(&descriptors, &instance.summary.module_id)?;
            super::commands_runtime_lifecycle::build_instance_launch_preview(
                &settings, descriptor, &instance,
            )
        })
        .await
        .map_err(|error| format!("Launch preflight worker failed: {error}"))??;
        (Some(plan.ready_to_launch), json!(plan.validation_issues))
    } else {
        (None, Value::Null)
    };
    Ok(AssistantVerificationSample {
        details,
        health: overview.health,
        log: overview.log_tail,
        diagnostics: overview.diagnostics,
        process_identities,
        launch_ready,
        launch_issues,
        log_failures,
        log_read_errors,
    })
}

fn assistant_verification_failure_lines(module_id: &str, lines: &[String]) -> Vec<String> {
    let fatal_lines = app_storage::runtime_fatal_log_lines(module_id, lines)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    lines
        .iter()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            // A console echo contains the probe's failure branch, not its result.
            if lower.contains("[lgsm-dst-")
                && (lower.contains("print(") || lower.contains("loadstring("))
            {
                return false;
            }
            fatal_lines.contains(*line)
                || [
                    "[lgsm-dst-failed:",
                    "failed to load modoverrides.lua",
                    "failed to load ../worldgenoverride.lua",
                    "worlddictionary: cannot load world",
                ]
                .iter()
                .any(|pattern| lower.contains(pattern))
        })
        .take(4)
        .cloned()
        .collect()
}

fn verification_text(value: &str) -> String {
    let redacted = redact_assistant_provider_text(value);
    let mut shortened = redacted
        .chars()
        .scan(0, |bytes, character| {
            *bytes += character.len_utf8();
            (*bytes <= 320).then_some(character)
        })
        .collect::<String>();
    if shortened.len() < redacted.len() {
        shortened.push_str(" [truncated]");
    }
    shortened
}

fn assistant_verification_read_error(
    error: &str,
    operation_error: Option<&str>,
    can_continue: bool,
) -> AssistantOperationVerification {
    AssistantOperationVerification {
        status: if operation_error.is_some() {
            AssistantVerificationStatus::Failed
        } else {
            AssistantVerificationStatus::Inconclusive
        },
        summary: String::from(
            "Runtime verification could not complete; recovery has not been established.",
        ),
        run_id: None,
        evidence: json!({"readError": verification_text(error), "operationError": operation_error.map(verification_text)}),
        can_continue,
    }
}

fn assistant_verification_evidence(
    sample: &AssistantVerificationSample,
    operation_error: Option<&str>,
) -> Value {
    let mut evidence = json!({
        "observedRunId": sample.details.active_run.as_ref().map(|run| run.run_id),
        "instanceStatus": sample.details.summary.status,
        "operationError": operation_error.map(verification_text),
        "health": {"status": verification_text(&sample.health.status), "reason": verification_text(&sample.health.reason.code), "summary": verification_text(&sample.health.summary), "matchedLine": sample.health.matched_line.as_deref().map(verification_text)},
        "log": {"lines": sample.log.lines.iter().rev().take(8).rev().map(|line| verification_text(line)).collect::<Vec<_>>(), "totalLines": sample.log.total_lines, "truncated": sample.log.truncated || sample.log.lines.len() > 8, "readError": sample.log.read_error.as_deref().map(verification_text)},
        "diagnostics": sample.diagnostics.iter().take(6).map(|diagnostic| json!({"code": verification_text(&diagnostic.code), "severity": verification_text(&diagnostic.severity), "summary": verification_text(&diagnostic.summary)})).collect::<Vec<_>>(),
        "processes": sample.process_identities.iter().take(ASSISTANT_VERIFICATION_MAX_PROCESSES).map(|(key, identity)| json!({"key": verification_text(key), "observedAlive": identity.as_ref().ok().map(Option::is_some), "readError": identity.as_ref().err().map(|error| verification_text(error))})).collect::<Vec<_>>(),
        "launchReady": sample.launch_ready,
        "launchIssues": sample.launch_issues.as_array().map(|issues| issues.iter().take(8).map(|issue| verification_text(&issue.to_string())).collect::<Vec<_>>()),
        "freshLogFailures": sample.log_failures.iter().take(8).map(|line| verification_text(line)).collect::<Vec<_>>(),
        "freshLogReadErrors": sample.log_read_errors.iter().take(4).map(|line| verification_text(line)).collect::<Vec<_>>()
    });
    if evidence.to_string().len() > ASSISTANT_TOOL_RESULT_BYTES {
        evidence["diagnostics"] = json!({"omittedCount": sample.diagnostics.len()});
        evidence["launchIssues"] = json!({"omitted": true});
        evidence["processes"] = json!({"observedCount": sample.process_identities.len()});
        evidence["evidenceTruncated"] = json!(true);
    }
    evidence
}
