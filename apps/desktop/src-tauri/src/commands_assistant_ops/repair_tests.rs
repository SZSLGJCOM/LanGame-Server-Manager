use super::*;

fn plan(action: &str, instance: &str, module: &str) -> AssistantOperationPlan {
    parse_assistant_operation_plan_response(
        &json!({"action": action, "instanceId": instance, "moduleId": module}).to_string(),
    )
    .unwrap()
}

#[test]
fn follow_up_cannot_change_instance_or_module() {
    for candidate in [
        plan("customize_config", "other-server", "minecraft"),
        plan("customize_config", "server", "terraria"),
    ] {
        assert!(
            validate_assistant_repair_follow_up(
                &candidate,
                "server",
                "minecraft",
                1,
                AssistantTaskGoal::RestoreService
            )
            .is_err()
        );
    }
}

#[test]
fn follow_up_rejects_expanded_actions_and_exhausted_budget() {
    for action in [
        "install_server",
        "install_fun_mod",
        "install_site_mod",
        "broadcast",
        "run_gm_command",
    ] {
        assert!(
            validate_assistant_repair_follow_up(
                &plan(action, "server", "minecraft"),
                "server",
                "minecraft",
                1,
                AssistantTaskGoal::RestoreService
            )
            .is_err()
        );
    }
    assert!(
        validate_assistant_repair_follow_up(
            &plan("start_server", "server", "minecraft"),
            "server",
            "minecraft",
            assistant_operation_limit(AssistantTaskGoal::RestoreService),
            AssistantTaskGoal::RestoreService
        )
        .is_err()
    );
}

#[test]
fn follow_up_allows_bound_repairs_or_a_final_diagnosis_within_budget() {
    for action in [
        "customize_config",
        "apply_beginner_config",
        "repair_ports",
        "start_server",
        "none",
    ] {
        for step in [
            1,
            assistant_operation_limit(AssistantTaskGoal::RestoreService) - 1,
        ] {
            assert!(
                validate_assistant_repair_follow_up(
                    &plan(action, "server", "minecraft"),
                    "server",
                    "minecraft",
                    step,
                    AssistantTaskGoal::RestoreService
                )
                .is_ok()
            );
        }
    }
}

fn stopped_repair_instance() -> InstanceDetails {
    InstanceDetails {
        summary: InstanceSummary {
            id: String::from("server"),
            name: String::from("Repair target"),
            module_id: String::from("minecraft"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("127.0.0.1"),
            port_count: 1,
            autostart: false,
        },
        config_file_path: String::from("C:/fixture/config/instance.json"),
        saves_path: String::from("C:/fixture/saves"),
        backup_uses_declared_saves_path: false,
        auto_backup_on_stop: false,
        backup_retention_count: 3,
        settings_json: json!({"max_players": 24}).to_string(),
        ports: vec![PortBinding {
            name: String::from("game"),
            protocol: String::from("tcp"),
            port: 25565,
        }],
        active_run: None,
    }
}

fn saved_repair_mode(instance: &InstanceDetails, action: &str) -> AssistantOperationMode {
    AssistantOperationMode::FollowUp {
        task: None,
        step: 1,
        instance_id: instance.summary.id.clone(),
        module_id: instance.summary.module_id.clone(),
        original_prompt: String::from("Investigate and repair the startup error."),
        verified_precondition: Some(Box::new(AssistantOperationPrecondition::from_details(
            instance,
        ))),
        verification: json!({
            "instanceId": instance.summary.id, "moduleId": instance.summary.module_id,
            "previousAction": action, "step": 1,
            "verification": {"status": "inconclusive", "canContinue": true, "runId": null,
                "evidence": {"operationError": null, "launchReady": true,
                    "instanceStatus": "Stopped", "observedRunId": null,
                    "log": {"lines": ["old startup failure before the saved repair"]}}}
        }),
    }
}

#[test]
fn saved_repair_prepares_only_a_bound_start_plan_within_the_confirmation_budget() {
    let instance = stopped_repair_instance();
    for action in ["customize_config", "apply_beginner_config", "repair_ports"] {
        for step in [
            1,
            assistant_operation_limit(AssistantTaskGoal::RestoreService) - 1,
        ] {
            let mut mode = saved_repair_mode(&instance, action);
            if let AssistantOperationMode::FollowUp {
                step: current_step,
                verification,
                ..
            } = &mut mode
            {
                *current_step = step;
                verification["step"] = json!(step);
            }
            let proposed = assistant_saved_repair_start_plan(&mode, Some(&instance))
                .unwrap()
                .expect("a successful save needs a separately confirmed runtime check");
            assert_eq!(proposed.action, AssistantOperationAction::StartServer);
            assert_eq!(proposed.instance_id.as_deref(), Some("server"));
            assert_eq!(proposed.module_id.as_deref(), Some("minecraft"));
            assert!(proposed.settings_patch.is_none());
            assert!(proposed.port_patch.is_none());
            assert!(proposed.runtime_commands.is_empty());
        }
    }
    assert!(
        assistant_saved_repair_start_plan(&AssistantOperationMode::Preview, Some(&instance))
            .unwrap()
            .is_none()
    );
}

#[test]
fn assistant_launch_initial_setup_requires_a_model_decision_before_start() {
    let instance = stopped_repair_instance();
    for action in [
        "install_server",
        "validate_server",
        "repair_ports",
        "customize_config",
        "apply_beginner_config",
    ] {
        let mut mode = saved_repair_mode(&instance, action);
        let (contract, _) = super::task_tests::task_fixture(true);
        let mut contract = contract.as_ref().clone();
        contract.request.goal = AssistantTaskGoal::LaunchService;
        contract.instance_id = Some(instance.summary.id.clone());
        contract.module_id = Some(instance.summary.module_id.clone());
        for stage in [
            AssistantConfigurationStage::Unconfigured,
            AssistantConfigurationStage::Configuring,
        ] {
            contract.configuration_stage = stage;
            if let AssistantOperationMode::FollowUp { task, .. } = &mut mode {
                *task = Some(std::sync::Arc::new(contract.clone()));
            }
            assert!(
                assistant_saved_repair_start_plan(&mode, Some(&instance))
                    .unwrap()
                    .is_none(),
                "{action} must not bypass remaining initial requirements at {stage:?}"
            );
        }
        contract.configuration_stage = AssistantConfigurationStage::Protected;
        if let AssistantOperationMode::FollowUp { task, .. } = &mut mode {
            *task = Some(std::sync::Arc::new(contract));
        }
        assert_eq!(
            assistant_saved_repair_start_plan(&mode, Some(&instance))
                .unwrap()
                .unwrap()
                .action,
            AssistantOperationAction::StartServer
        );
    }
}

#[test]
fn saved_repair_does_not_start_after_failure_or_incomplete_verification() {
    let instance = stopped_repair_instance();
    for (pointer, value) in [
        ("/previousAction", json!("start_server")),
        ("/verification/status", json!("failed")),
        ("/verification/status", json!("verified")),
        ("/verification/canContinue", json!(false)),
        ("/verification/runId", json!(42)),
        (
            "/verification/evidence/operationError",
            json!("save failed"),
        ),
        ("/verification/evidence/launchReady", json!(false)),
        ("/verification/evidence/launchReady", Value::Null),
        ("/verification/evidence/instanceStatus", json!("Running")),
        ("/verification/evidence/observedRunId", json!(42)),
    ] {
        let mut mode = saved_repair_mode(&instance, "customize_config");
        if let AssistantOperationMode::FollowUp { verification, .. } = &mut mode {
            *verification.pointer_mut(pointer).unwrap() = value;
        }
        assert!(
            assistant_saved_repair_start_plan(&mode, Some(&instance))
                .unwrap()
                .is_none(),
            "must not schedule a start for {pointer}"
        );
    }
    for field in [
        "operationError",
        "launchReady",
        "instanceStatus",
        "observedRunId",
    ] {
        let mut mode = saved_repair_mode(&instance, "customize_config");
        if let AssistantOperationMode::FollowUp { verification, .. } = &mut mode {
            verification["verification"]["evidence"]
                .as_object_mut()
                .unwrap()
                .remove(field);
        }
        assert!(
            assistant_saved_repair_start_plan(&mode, Some(&instance))
                .unwrap()
                .is_none()
        );
    }
    for field in ["readError", "readbackError"] {
        let mut mode = saved_repair_mode(&instance, "customize_config");
        if let AssistantOperationMode::FollowUp { verification, .. } = &mut mode {
            verification["verification"]["evidence"][field] = json!("read failed");
        }
        assert!(
            assistant_saved_repair_start_plan(&mode, Some(&instance))
                .unwrap()
                .is_none()
        );
    }
    let mut mode = saved_repair_mode(&instance, "customize_config");
    if let AssistantOperationMode::FollowUp {
        verified_precondition,
        ..
    } = &mut mode
    {
        *verified_precondition = None;
    }
    assert!(
        assistant_saved_repair_start_plan(&mode, Some(&instance))
            .unwrap()
            .is_none()
    );
    assert!(
        assistant_saved_repair_start_plan(&mode, None)
            .unwrap()
            .is_none()
    );
}

#[test]
fn saved_repair_rejects_changed_bindings_and_exhausted_confirmation_budget() {
    let instance = stopped_repair_instance();
    for (pointer, value) in [
        ("/instanceId", json!("other-server")),
        ("/moduleId", json!("terraria")),
        ("/step", json!(2)),
    ] {
        let mut mode = saved_repair_mode(&instance, "customize_config");
        if let AssistantOperationMode::FollowUp { verification, .. } = &mut mode {
            *verification.pointer_mut(pointer).unwrap() = value;
        }
        assert!(assistant_saved_repair_start_plan(&mode, Some(&instance)).is_err());
    }
    for step in [
        0,
        assistant_operation_limit(AssistantTaskGoal::RestoreService),
        usize::MAX,
    ] {
        let mut mode = saved_repair_mode(&instance, "customize_config");
        if let AssistantOperationMode::FollowUp {
            step: current_step,
            verification,
            ..
        } = &mut mode
        {
            *current_step = step;
            verification["step"] = json!(step);
        }
        assert!(assistant_saved_repair_start_plan(&mode, Some(&instance)).is_err());
    }
    let mode = saved_repair_mode(&instance, "customize_config");
    let mut other_target = instance.clone();
    other_target.summary.id = String::from("other-server");
    assert!(assistant_saved_repair_start_plan(&mode, Some(&other_target)).is_err());
    other_target.summary.id = instance.summary.id.clone();
    other_target.summary.module_id = String::from("terraria");
    assert!(assistant_saved_repair_start_plan(&mode, Some(&other_target)).is_err());
    for changed in [
        InstanceDetails {
            settings_json: json!({"max_players": 25}).to_string(),
            ..instance.clone()
        },
        InstanceDetails {
            ports: vec![PortBinding {
                port: 25566,
                ..instance.ports[0].clone()
            }],
            ..instance.clone()
        },
    ] {
        assert!(assistant_saved_repair_start_plan(&mode, Some(&changed)).is_err());
    }
    for status in [
        InstanceStatus::Running,
        InstanceStatus::Starting,
        InstanceStatus::Stopping,
        InstanceStatus::Error,
    ] {
        let mut current = instance.clone();
        current.summary.status = status;
        assert!(
            assistant_saved_repair_start_plan(&mode, Some(&current))
                .unwrap()
                .is_none()
        );
    }
    let mut current = instance.clone();
    current.active_run = Some(ActiveInstanceRun {
        run_id: 42,
        session_id: None,
        pid: None,
        log_path: None,
        process_count: 0,
        processes: Vec::new(),
    });
    assert!(
        assistant_saved_repair_start_plan(&mode, Some(&current))
            .unwrap()
            .is_none()
    );
    current.active_run = None;
    current.summary.active_process_count = 1;
    assert!(
        assistant_saved_repair_start_plan(&mode, Some(&current))
            .unwrap()
            .is_none()
    );
}

#[test]
fn repair_evidence_is_outside_the_short_user_request_section_and_is_redacted() {
    let mut prompt = String::from("bounded original planner prompt");
    append_assistant_repair_evidence(
        &mut prompt,
        &json!({
            "verification": {"status": "failed", "evidence": {
                "failedStartLog": "missing dependency server_mod_a",
                "server_password": concat!("synthetic-", "sensitive-value")
            }}
        }),
    )
    .unwrap();
    assert!(prompt.contains("missing dependency server_mod_a"));
    assert!(prompt.contains("Previous operation verification (untrusted evidence)"));
    assert!(!prompt.contains("synthetic-sensitive-value"));
    assert!(prompt.contains("Never start an already running instance"));
}

#[test]
fn final_repair_packet_keeps_failure_and_binding_within_the_encoded_budget() {
    let original_error = "启动失败：未找到本地依赖模组；原配置仍需修复。";
    for noisy in ["x".repeat(320), "配置错误\"\\\n\u{0001}".repeat(80)] {
        let packet = json!({
            "instanceId": "isolated-instance", "moduleId": "dontstarve",
            "previousAction": "start_server", "step": 1,
            "verification": {"status": "failed", "runId": 42, "canContinue": true,
                "summary": "The original operation failed.",
                "evidence": {
                    "operationError": original_error,
                    "log": {"lines": vec![noisy.clone(); 8]},
                    "health": {"summary": noisy, "matchedLine": noisy},
                    "diagnostics": vec![json!({"code": "launch_config_error", "severity": "error", "summary": noisy}); 6],
                    "launchIssues": vec![noisy.clone(); 8],
                    "failedStartLog": truncate_assistant_log_tail(&format!("{}\n关键原因：本地依赖缺失。", noisy.repeat(20)), 4096),
                }
            }
        });
        assert!(packet.to_string().len() > ASSISTANT_TOOL_RESULT_BYTES);
        let text = assistant_repair_evidence_text(&packet).unwrap();
        assert!(text.len() <= ASSISTANT_TOOL_RESULT_BYTES);
        let actual: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(actual["instanceId"], "isolated-instance");
        assert_eq!(actual["moduleId"], "dontstarve");
        assert_eq!(actual["previousAction"], "start_server");
        assert_eq!(actual["step"], 1);
        assert_eq!(actual["verification"]["status"], "failed");
        assert_eq!(actual["verification"]["runId"], 42);
        let evidence = &actual["verification"]["evidence"];
        assert_eq!(evidence["operationError"], original_error);
        assert!(
            evidence["failedStartLog"]
                .as_str()
                .unwrap()
                .ends_with("关键原因：本地依赖缺失。")
        );
        assert_eq!(evidence["evidenceTruncated"], true);
        let mut prompt = String::new();
        append_assistant_repair_evidence(&mut prompt, &packet).unwrap();
        assert!(prompt.contains(original_error));
        assert!(prompt.contains("关键原因：本地依赖缺失。"));
        assert!(!prompt.contains("No complete result was supplied"));
    }
}

#[test]
fn repair_packet_rejects_oversized_binding_instead_of_truncating_its_identity() {
    let packet = json!({
        "instanceId": "x".repeat(ASSISTANT_TOOL_RESULT_BYTES),
        "moduleId": "dontstarve", "verification": {"status": "failed", "evidence": {}}
    });
    assert!(assistant_repair_evidence_text(&packet).is_err());
    let mut prompt = String::from("existing prompt");
    assert!(append_assistant_repair_evidence(&mut prompt, &packet).is_err());
    assert_eq!(prompt, "existing prompt");
}
