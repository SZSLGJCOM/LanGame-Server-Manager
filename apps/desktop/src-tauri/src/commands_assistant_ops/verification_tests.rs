use super::*;

fn details(running: bool) -> InstanceDetails {
    let identity = ProcessIdentity {
        creation_time: 12,
        image_path: String::from("C:/fixture/server.exe"),
    };
    InstanceDetails {
        summary: InstanceSummary {
            id: String::from("verification-instance"),
            name: String::from("Verification instance"),
            module_id: String::from("minecraft"),
            status: if running {
                InstanceStatus::Running
            } else {
                InstanceStatus::Stopped
            },
            active_process_count: usize::from(running),
            bind_ip: String::from("127.0.0.1"),
            port_count: 0,
            autostart: false,
        },
        config_file_path: String::from("C:/fixture/config/instance.json"),
        saves_path: String::from("C:/fixture/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 3,
        settings_json: json!({"max_players": 8}).to_string(),
        ports: Vec::new(),
        active_run: running.then(|| ActiveInstanceRun {
            run_id: 12,
            session_id: Some(String::from("verification-session")),
            pid: Some(1234),
            log_path: Some(String::from("C:/fixture/logs/run-12.log")),
            process_count: 1,
            processes: vec![app_core::InstanceProcessState {
                run_id: 12,
                session_id: Some(String::from("verification-session")),
                process_key: String::from("main"),
                display_name: String::from("Server"),
                pid: Some(1234),
                process_identity: Some(identity),
                status: String::from("running"),
                started_at: None,
                stopped_at: None,
                exit_code: None,
                crash_flag: false,
                log_path: Some(String::from("C:/fixture/logs/run-12.log")),
                is_primary: true,
            }],
        }),
    }
}

fn started() -> AssistantVerificationStart {
    AssistantVerificationStart {
        instance_id: String::from("verification-instance"),
        module_id: String::from("minecraft"),
        run_id: 12,
        session_id: Some(String::from("verification-session")),
        processes: vec![(
            12,
            String::from("main"),
            1234,
            String::from("C:/fixture/logs/run-12.log"),
        )],
    }
}

fn sample(running: bool) -> AssistantVerificationSample {
    let details = details(running);
    let process_identities = details.active_run.as_ref().map_or_else(Vec::new, |run| {
        run.processes
            .iter()
            .map(|process| {
                (
                    process.process_key.clone(),
                    Ok(process.process_identity.clone()),
                )
            })
            .collect()
    });
    AssistantVerificationSample {
        details,
        health: app_core::RuntimeHealth {
            status: String::from("ready"),
            summary: String::from("Server reported ready"),
            reason: app_core::RuntimeHealthReason {
                code: String::from("ready_signal"),
                params: Default::default(),
            },
            matched_line: Some(String::from("Server ready")),
        },
        log: LogTailSnapshot {
            source_path: Some(String::from("C:/fixture/logs/run-12.log")),
            lines: vec![String::from("Server ready")],
            total_lines: 1,
            truncated: false,
            read_error: None,
        },
        diagnostics: Vec::new(),
        process_identities,
        launch_ready: Some(true),
        launch_issues: json!([]),
        log_failures: Vec::new(),
        log_read_errors: Vec::new(),
    }
}

#[test]
fn new_run_readiness_requires_the_receipt_and_matching_live_process_identity() {
    let before = details(false);
    let current = sample(true);
    let start = started();
    let result = assess_assistant_verification(
        AssistantOperationAction::StartServer,
        &before,
        Some(&current.details),
        Some(&start),
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Verified);
    assert_eq!(result.run_id, Some(12));
    assert!(!result.can_continue);

    let missing_receipt = assess_assistant_verification(
        AssistantOperationAction::StartServer,
        &before,
        Some(&current.details),
        None,
        &current,
        None,
    );
    assert_eq!(
        missing_receipt.status,
        AssistantVerificationStatus::Inconclusive
    );
}

#[test]
fn dead_process_overrides_a_ready_log_and_preserves_failure_evidence() {
    let mut current = sample(true);
    current.process_identities[0].1 = Ok(None);
    let result = assess_assistant_verification(
        AssistantOperationAction::StartServer,
        &details(false),
        Some(&current.details),
        Some(&started()),
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Failed);
    assert!(result.can_continue);
    assert!(result.summary.contains("process"));
}

#[test]
fn replaced_run_or_reused_pid_ends_the_chain_without_claiming_success() {
    for replacement in [false, true] {
        let mut current = sample(true);
        if replacement {
            current.details.active_run.as_mut().unwrap().run_id += 1;
        } else {
            current.process_identities[0]
                .1
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .creation_time += 1;
        }
        let result = assess_assistant_verification(
            AssistantOperationAction::StartServer,
            &details(false),
            Some(&details(true)),
            Some(&started()),
            &current,
            None,
        );
        assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
        assert!(!result.can_continue);
        assert!(!result.summary.is_empty());
    }
}

#[test]
fn old_run_ready_does_not_verify_new_settings_or_authorize_a_restart() {
    let current = sample(true);
    let result = assess_assistant_verification(
        AssistantOperationAction::CustomizeConfig,
        &details(true),
        Some(&current.details),
        None,
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
    assert!(!result.can_continue);
    assert!(result.summary.contains("restart"));
}

#[test]
fn stopped_configuration_preflight_allows_a_new_preview_without_claiming_runtime_success() {
    let mut current = sample(false);
    let result = assess_assistant_verification(
        AssistantOperationAction::CustomizeConfig,
        &details(false),
        Some(&current.details),
        None,
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
    assert!(result.can_continue);
    current.launch_ready = Some(false);
    let invalid = assess_assistant_verification(
        AssistantOperationAction::CustomizeConfig,
        &details(false),
        Some(&current.details),
        None,
        &current,
        None,
    );
    assert_eq!(invalid.status, AssistantVerificationStatus::Failed);
    assert!(invalid.can_continue);
}

#[test]
fn failed_operation_cannot_be_overridden_by_healthy_evidence_or_leak_a_secret() {
    let current = sample(true);
    let secret = ["fixture", "verification", "secret"].join("-");
    let error = format!("write failed; server_password={secret}");
    let result = assess_assistant_verification(
        AssistantOperationAction::CustomizeConfig,
        &details(false),
        Some(&current.details),
        None,
        &current,
        Some(&error),
    );
    assert_eq!(result.status, AssistantVerificationStatus::Failed);
    assert!(result.can_continue);
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains(&secret));
    assert!(serialized.contains("write failed"));
}

#[test]
fn missing_readiness_and_shared_old_log_are_inconclusive() {
    for stale in [false, true] {
        let mut current = sample(true);
        if stale {
            current.log.source_path = Some(String::from("C:/fixture/shared-native.log"));
        } else {
            current.health.status = String::from("starting");
        }
        let result = assess_assistant_verification(
            AssistantOperationAction::StartServer,
            &details(false),
            Some(&current.details),
            Some(&started()),
            &current,
            None,
        );
        assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
        assert!(result.can_continue);
    }
}

#[test]
fn a_dst_secondary_failure_wins_over_a_completed_start_receipt() {
    let mut before = details(false);
    before.summary.module_id = String::from("dontstarve");
    let mut current = sample(true);
    current.details.summary.module_id = String::from("dontstarve");
    current.log_failures.push(String::from(
        "Caves: ERROR: Failed to load modoverrides.lua",
    ));
    let mut start = started();
    start.module_id = String::from("dontstarve");
    let result = assess_assistant_verification(
        AssistantOperationAction::StartServer,
        &before,
        Some(&current.details),
        Some(&start),
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Failed);
    assert!(result.can_continue);
}

#[tokio::test]
async fn verification_waits_for_two_ready_samples_and_stops_after_a_later_crash() {
    let mut reads = 0;
    let result = observe_assistant_verification_with(
        AssistantOperationAction::StartServer,
        &details(false),
        Some(&details(true)),
        Some(&started()),
        None,
        || {
            reads += 1;
            let mut current = sample(true);
            if reads == 2 {
                current.process_identities[0].1 = Ok(None);
            }
            std::future::ready(Ok(current))
        },
    )
    .await;
    assert_eq!(reads, 2);
    assert_eq!(result.status, AssistantVerificationStatus::Failed);

    reads = 0;
    let stable = observe_assistant_verification_with(
        AssistantOperationAction::StartServer,
        &details(false),
        Some(&details(true)),
        Some(&started()),
        None,
        || {
            reads += 1;
            std::future::ready(Ok(sample(true)))
        },
    )
    .await;
    assert_eq!(reads, 2);
    assert_eq!(stable.status, AssistantVerificationStatus::Verified);
}

#[tokio::test]
async fn verification_bounds_pending_observations_without_turning_absence_into_success() {
    let mut reads = 0;
    let result = observe_assistant_verification_with(
        AssistantOperationAction::StartServer,
        &details(false),
        Some(&details(true)),
        Some(&started()),
        None,
        || {
            reads += 1;
            let mut current = sample(true);
            current.health.status = String::from("starting");
            std::future::ready(Ok(current))
        },
    )
    .await;
    assert_eq!(reads, 3);
    assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
    assert!(result.can_continue);
}

#[test]
fn operation_failure_does_not_authorize_follow_up_on_a_replaced_target() {
    let before = details(false);
    let after = details(true);
    let start = started();
    for changed_identity in [false, true] {
        let mut current = sample(true);
        if changed_identity {
            current.process_identities[0]
                .1
                .as_mut()
                .unwrap()
                .as_mut()
                .unwrap()
                .creation_time += 1;
        } else {
            current.details.active_run.as_mut().unwrap().run_id += 1;
        }
        let result = assess_assistant_verification(
            AssistantOperationAction::StartServer,
            &before,
            Some(&after),
            Some(&start),
            &current,
            Some("startup failed"),
        );
        assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
        assert!(!result.can_continue);
        assert!(result.evidence.to_string().contains("startup failed"));
    }
}

#[test]
fn unreadable_shard_logs_or_unknown_identity_cannot_verify_a_start() {
    for unreadable_log in [false, true] {
        let mut current = sample(true);
        if unreadable_log {
            current
                .log_read_errors
                .push(String::from("Caves: access denied"));
        } else {
            current.process_identities[0].1 = Err(String::from("process query denied"));
        }
        let result = assess_assistant_verification(
            AssistantOperationAction::StartServer,
            &details(false),
            Some(&current.details),
            Some(&started()),
            &current,
            None,
        );
        assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
        assert!(result.can_continue);
    }
}

#[test]
fn changed_configuration_stops_verification_and_evidence_has_a_byte_budget() {
    let mut current = sample(true);
    let after = details(true);
    current.details.settings_json = json!({"max_players": 9}).to_string();
    let result = assess_assistant_verification(
        AssistantOperationAction::StartServer,
        &details(false),
        Some(&after),
        Some(&started()),
        &current,
        None,
    );
    assert_eq!(result.status, AssistantVerificationStatus::Inconclusive);
    assert!(!result.can_continue);

    let noisy = "界".repeat(2000);
    current.log.lines = vec![noisy.clone(); 80];
    current.log_failures = vec![noisy.clone(); 80];
    current.log_read_errors = vec![noisy.clone(); 80];
    current.process_identities = (0..16)
        .map(|_| (noisy.clone(), Err(noisy.clone())))
        .collect();
    let evidence = assistant_verification_evidence(&current, Some("original failure"));
    assert!(evidence.to_string().len() < ASSISTANT_TOOL_RESULT_BYTES);
    assert_eq!(evidence["operationError"], "original failure");
    assert_eq!(evidence["evidenceTruncated"], true);
}

#[test]
fn dst_probe_echo_is_not_a_runtime_failure_but_its_acknowledgement_is() {
    let echo = String::from("loadstring('print(\"[LGSM-DST-FAILED:nonce]\")')()");
    let failed = String::from("[00:12:30]: [LGSM-DST-FAILED:nonce] failed mods");
    let fatal = String::from("fatal Lua error in print(value)");
    assert_eq!(
        assistant_verification_failure_lines("dontstarve", &[echo, failed.clone(), fatal.clone()]),
        [failed, fatal]
    );
}

#[test]
fn verification_distinguishes_recovered_world_generation_from_later_failures() {
    let panic = String::from(
        "[00:00:10]: PANIC: missing required prefab [sculpture_bishop]! Expected 1, got 0",
    );
    let mut lines = vec![
        panic.clone(),
        String::from("[00:00:10]: An error occured during world gen we will retry! [was 1 of 5]"),
        String::from("[00:00:12]: Generation complete, injecting world entities."),
    ];
    assert!(assistant_verification_failure_lines("dontstarve", &lines).is_empty());
    assert_eq!(
        assistant_verification_failure_lines("test", &lines).as_slice(),
        std::slice::from_ref(&panic)
    );
    assert_eq!(
        assistant_verification_failure_lines("dontstarve", &lines[..2]),
        [panic]
    );
    let fatal = String::from("[00:00:13]: FATAL: server stopped");
    lines.push(fatal.clone());
    assert_eq!(
        assistant_verification_failure_lines("dontstarve", &lines),
        [fatal]
    );
}

#[tokio::test]
async fn an_unreadable_target_preserves_the_operation_error_and_stops_follow_up() {
    let result = observe_assistant_verification_with(
        AssistantOperationAction::StartServer,
        &details(false),
        None,
        None,
        Some("original failure"),
        || std::future::ready(Err(String::from("instance missing"))),
    )
    .await;
    assert_eq!(result.status, AssistantVerificationStatus::Failed);
    assert!(!result.can_continue);
    assert_eq!(result.evidence["operationError"], "original failure");
    assert_eq!(result.evidence["readError"], "instance missing");
}
