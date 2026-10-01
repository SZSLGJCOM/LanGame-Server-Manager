use super::*;

#[test]
fn task_completion_requires_every_check_and_never_treats_unknown_as_success() {
    let satisfied = assistant_task_check(
        "saved",
        AssistantTaskCheckStatus::Satisfied,
        "Saved",
        Value::Null,
    );
    let unknown = assistant_task_check(
        "mods",
        AssistantTaskCheckStatus::Unknown,
        "No evidence",
        Value::Null,
    );
    let failed = assistant_task_check(
        "mods",
        AssistantTaskCheckStatus::Failed,
        "Not loaded",
        Value::Null,
    );
    assert_eq!(
        assistant_task_status(&[]),
        AssistantTaskStatus::Inconclusive
    );
    assert_eq!(
        assistant_task_status(std::slice::from_ref(&satisfied)),
        AssistantTaskStatus::Completed
    );
    assert_eq!(
        assistant_task_status(&[satisfied.clone(), unknown.clone()]),
        AssistantTaskStatus::Inconclusive
    );
    assert_eq!(
        assistant_task_status(&[satisfied, unknown, failed]),
        AssistantTaskStatus::Failed
    );
}

#[test]
fn task_policy_is_structured_and_rejects_model_defined_authorization_fields() {
    assert!(
        serde_json::from_value::<AssistantTaskRequest>(
            json!({"goal":"restore_service", "preserveExistingMods":true, "allowDisable":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<AssistantTaskRequest>(
            json!({"goal":"trust_model", "preserveExistingMods":false})
        )
        .is_err()
    );
    assert!(AssistantTaskRequest::default().preserve_existing_mods);
}

#[test]
fn protected_project_zomboid_order_changes_preserve_exact_native_mod_ids() {
    let (original, _) = task_fixture(true);
    let mut task = original.as_ref().clone();
    task.module_id = Some(String::from("projectzomboid"));
    task.initial_settings = json!({
        "mods":"addon\r\ndependency\r\n# comment\r\naddon",
        "workshop_items":"123;456"
    });
    let reordered = json!({"mods":"dependency;addon", "workshop_items":"123;456"});
    task.validate_settings(&reordered).unwrap();
    let mut repeated = reordered.clone();
    repeated["mods"] = json!("dependency;addon;addon");
    task.validate_settings(&repeated).unwrap();
    for altered in [
        "dependency",
        "dependency;addon;new-mod",
        "dependency;Addon",
        "dependency;# addon",
        "dependency;addon,other",
    ] {
        let mut proposed = reordered.clone();
        proposed["mods"] = json!(altered);
        assert!(task.validate_settings(&proposed).is_err(), "{altered}");
    }
    for invalid in [Value::Null, json!(["dependency", "addon"]), json!(1)] {
        let mut proposed = reordered.clone();
        proposed["mods"] = invalid;
        assert!(task.validate_settings(&proposed).is_err());
    }
    let mut proposed = reordered.clone();
    proposed["workshop_items"] = json!("123");
    assert!(task.validate_settings(&proposed).is_err());
    task.module_id = Some(String::from("unsupported-game"));
    assert!(task.validate_settings(&reordered).is_err());
}

pub(super) fn task_fixture(
    preserve_existing_mods: bool,
) -> (std::sync::Arc<AssistantTaskContract>, InstanceDetails) {
    let settings = json!({
        "master_modoverrides_lua": "return {consumer={enabled=true,configuration_options={mode='normal'}}}",
        "cluster_name": "Initial name"
    });
    let instance = InstanceDetails {
        summary: InstanceSummary {
            id: String::from("task-instance"),
            name: String::from("Task instance"),
            module_id: String::from("dontstarve"),
            status: InstanceStatus::Stopped,
            active_process_count: 0,
            bind_ip: String::from("127.0.0.1"),
            port_count: 0,
            autostart: false,
        },
        config_file_path: String::from("C:/isolated/task-instance/cluster.ini"),
        saves_path: String::from("C:/isolated/task-instance/save"),
        backup_uses_declared_saves_path: true,
        auto_backup_on_stop: true,
        backup_retention_count: 3,
        settings_json: settings.to_string(),
        ports: Vec::new(),
        active_run: None,
    };
    let mut task = AssistantTaskContract {
        session: None,
        run: std::sync::Arc::new(AssistantTaskRun::default()),
        investigation_draft: std::sync::Arc::new(StdMutex::new(None)),
        id: String::from("fixed-task"),
        original_request: String::from("Restore the selected server"),
        requirements_schema: None,
        requirements: Some(
            serde_json::from_value(
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
            )
            .unwrap(),
        ),
        request: AssistantTaskRequest {
            goal: AssistantTaskGoal::RestoreService,
            preserve_existing_mods,
        },
        instance_id: Some(instance.summary.id.clone()),
        module_id: Some(instance.summary.module_id.clone()),
        initial_settings: settings,
        configuration_stage: AssistantConfigurationStage::Protected,
        required_mods: Vec::new(),
        mod_evidence_known: true,
        file_changes: Vec::new(),
    };
    if preserve_existing_mods {
        task.capture_mod_requirements().unwrap();
    }
    (std::sync::Arc::new(task), instance)
}

fn configuration_response(raw: &str) -> String {
    json!({"action":"customize_config", "instanceId":"task-instance", "moduleId":"dontstarve",
        "settingsPatch":{"master_modoverrides_lua":raw}, "reason":"Repair the selected server"})
    .to_string()
}

#[test]
fn task_launch_and_restore_require_corrected_settings_keys_before_confirmation() {
    let (original, instance) = task_fixture(true);
    let current: Value = serde_json::from_str(&instance.settings_json).unwrap();
    let mixed = json!({"cluster_name": "Requested name", "unknown_setting": true});
    for goal in [
        AssistantTaskGoal::LaunchService,
        AssistantTaskGoal::RestoreService,
        AssistantTaskGoal::ApplyChange,
    ] {
        let mut task = original.as_ref().clone();
        task.request.goal = goal;
        for action in [
            AssistantOperationAction::CustomizeConfig,
            AssistantOperationAction::ApplyBeginnerConfig,
        ] {
            for (patch, has_unknown) in [
                (mixed.clone(), true),
                (json!({"unknown_setting": true}), true),
                (json!({"cluster_name": "Requested name"}), false),
            ] {
                let plan = AssistantOperationPlan {
                    action,
                    instance_id: task.instance_id.clone(),
                    module_id: task.module_id.clone(),
                    settings_patch: Some(patch),
                    ..assistant_safe_none_plan(String::from("Apply the requested configuration"))
                };
                let result = task.validate_plan(&plan, Some(&instance));
                if has_unknown && goal != AssistantTaskGoal::ApplyChange {
                    assert!(result.unwrap_err().contains("unknown keys"));
                } else {
                    result.unwrap();
                }
            }
        }
    }
    let partial = merge_assistant_settings_patch(&current, &mixed).unwrap();
    assert_eq!(partial.applied_keys, ["cluster_name"]);
    assert_eq!(partial.rejected_keys, ["unknown_setting"]);
    let mut expected = current;
    expected["cluster_name"] = json!("Requested name");
    assert_eq!(partial.settings, expected);
    assert_eq!(
        instance.settings_json,
        original.initial_settings.to_string()
    );
}

#[tokio::test]
async fn task_investigation_rejects_destructive_model_replies_with_a_fixed_limit() {
    // Deterministic replies exercise the same application policy for each
    // provider/model label; this is not a live model-quality comparison.
    for (provider, model) in [
        ("openai-compatible", "example-small"),
        ("anthropic-compatible", "example-large"),
        ("ollama", "qwen3:4b"),
        ("ollama", "qwen3.5:9b"),
    ] {
        for raw in ["return {}", "return {consumer={enabled=true}}"] {
            let (task, instance) = task_fixture(true);
            let original = instance.settings_json.clone();
            let response = configuration_response(raw);
            let validator = |text: &str| {
                let plan = parse_assistant_operation_plan_response(text)?;
                task.validate_plan(&plan, Some(&instance))
            };
            let mut model_calls = 0;
            let mut reads = 0;
            let result = run_assistant_investigation_checked(
                format!("{provider}/{model}: restore service while preserving the task baseline"),
                Vec::new(),
                |prompt| {
                    model_calls += 1;
                    if model_calls > 1 {
                        assert!(prompt.contains("Response rejected; no operation was executed."));
                    }
                    std::future::ready(Ok(response.clone()))
                },
                |_| {
                    reads += 1;
                    std::future::ready(Ok(json!({})))
                },
                &validator,
            )
            .await;
            assert!(
                result.is_err(),
                "{provider}/{model} must not return an executable rejected plan"
            );
            assert_eq!(model_calls, 3);
            assert_eq!(reads, 0);
            assert_eq!(instance.settings_json, original);
            assert!(task.request.preserve_existing_mods);
        }
    }
}

#[tokio::test]
async fn task_investigation_can_correct_a_rejected_patch_without_weakening_the_contract() {
    let (task, instance) = task_fixture(true);
    let valid = configuration_response(
        "return {consumer={enabled=true,configuration_options={mode='normal'}},dependency={enabled=true}}",
    );
    let validator = |text: &str| {
        let plan = parse_assistant_operation_plan_response(text)?;
        task.validate_plan(&plan, Some(&instance))
    };
    let mut model_calls = 0;
    let mut reads = 0;
    let accepted = run_assistant_investigation_checked(
        task.summary(),
        vec![AssistantReadTool::ReadModState {}],
        |prompt| {
            model_calls += 1;
            let reply = match model_calls {
                1 => configuration_response("return {}"),
                2 => {
                    assert!(prompt.contains("Response rejected; no operation was executed."));
                    assert!(prompt.contains("initially enabled"));
                    configuration_response("return {consumer={enabled=true}}")
                }
                3 => {
                    assert!(prompt.contains("configuration options"));
                    valid.clone()
                }
                _ => panic!("correction must stay bounded"),
            };
            std::future::ready(Ok(reply))
        },
        |tool| {
            assert!(matches!(tool, AssistantReadTool::ReadModState {}));
            reads += 1;
            std::future::ready(Ok(
                json!({"enabled":["consumer"], "dependency":"installed"}),
            ))
        },
        &validator,
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&accepted).unwrap(),
        serde_json::from_str::<Value>(&valid).unwrap()
    );
    assert_eq!((model_calls, reads), (3, 1));
    assert_eq!(task.required_mods.len(), 1);
    assert_eq!(task.required_mods[0].folder_name, "consumer");
    assert_eq!(task.required_mods[0].shard, "master");
    assert_eq!(task.request.goal, AssistantTaskGoal::RestoreService);
}

#[tokio::test]
async fn task_explicit_user_policy_can_allow_a_confirmable_disable_plan() {
    let (task, instance) = task_fixture(false);
    let response = configuration_response("return {}");
    let validator = |text: &str| {
        let plan = parse_assistant_operation_plan_response(text)?;
        task.validate_plan(&plan, Some(&instance))
    };
    let mut model_calls = 0;
    let accepted = run_assistant_investigation_checked(
        task.summary(),
        Vec::new(),
        |_| {
            model_calls += 1;
            std::future::ready(Ok(response.clone()))
        },
        |_| std::future::ready(Err(String::from("Unexpected read"))),
        &validator,
    )
    .await
    .unwrap();
    assert_eq!(model_calls, 1);
    assert_eq!(accepted, response);
    assert!(!task.request.preserve_existing_mods);
    assert_eq!(instance.settings_json, task.initial_settings.to_string());
}

#[test]
fn task_model_fields_cannot_override_the_bound_target_goal_or_preservation_policy() {
    let (task, instance) = task_fixture(true);
    for response in [
        json!({"action":"customize_config", "instanceId":"task-instance", "settingsPatch":{"master_modoverrides_lua":"return {}"},
            "task":{"goal":"apply_change","preserveExistingMods":false}, "preserveExistingMods":false}),
        json!({"action":"broadcast", "broadcastIntent":"Done", "goal":"apply_change"}),
        json!({"action":"start_server", "instanceId":"another-instance", "target":"task-instance"}),
        json!({"action":"start_server", "moduleId":"minecraft", "target":"dontstarve"}),
    ] {
        // Strict wire rejection is also safe; accepted plans must fail task validation.
        if let Ok(plan) = parse_assistant_operation_plan_response(&response.to_string()) {
            assert!(task.validate_plan(&plan, Some(&instance)).is_err());
        }
        assert_eq!(task.request.goal, AssistantTaskGoal::RestoreService);
        assert!(task.request.preserve_existing_mods);
        assert_eq!(task.instance_id.as_deref(), Some("task-instance"));
    }
}

#[test]
fn task_follow_up_keeps_the_initial_baseline_when_step_preconditions_advance() {
    let (task, mut instance) = task_fixture(true);
    let initial = task.initial_settings.clone();
    let mut advanced = initial.clone();
    advanced["cluster_name"] = json!("Saved name");
    advanced["master_modoverrides_lua"] = json!(
        "return {consumer={enabled=true,configuration_options={mode='normal'}},dependency={enabled=true}}"
    );
    instance.settings_json = advanced.to_string();
    let mode = AssistantOperationMode::FollowUp {
        step: 1,
        instance_id: instance.summary.id.clone(),
        module_id: instance.summary.module_id.clone(),
        original_prompt: String::from("Restore the original functionality"),
        verified_precondition: Some(Box::new(AssistantOperationPrecondition::from_details(
            &instance,
        ))),
        task: Some(task.clone()),
        verification: json!({"status":"inconclusive"}),
    };
    let AssistantOperationMode::FollowUp {
        task: Some(continued),
        verified_precondition: Some(precondition),
        ..
    } = mode
    else {
        panic!("follow-up must keep its application-owned contract");
    };
    assert!(std::sync::Arc::ptr_eq(&task, &continued));
    precondition.validate(&instance).unwrap();
    assert_eq!(continued.initial_settings, initial);
    continued.validate_settings(&advanced).unwrap();
    advanced["master_modoverrides_lua"] = json!("return {dependency={enabled=true}}");
    assert!(continued.validate_settings(&advanced).is_err());
}

#[test]
fn task_unknown_mod_requirements_do_not_become_an_empty_successful_set() {
    let (known, _) = task_fixture(true);
    let mut unknown = AssistantTaskContract {
        session: None,
        run: std::sync::Arc::new(AssistantTaskRun::default()),
        investigation_draft: std::sync::Arc::new(StdMutex::new(None)),
        id: known.id.clone(),
        original_request: known.original_request.clone(),
        requirements_schema: known.requirements_schema.clone(),
        requirements: known.requirements.clone(),
        request: known.request.clone(),
        instance_id: known.instance_id.clone(),
        module_id: known.module_id.clone(),
        initial_settings: json!({"master_modoverrides_lua":"return build_mods()"}),
        configuration_stage: AssistantConfigurationStage::Protected,
        required_mods: Vec::new(),
        mod_evidence_known: true,
        file_changes: Vec::new(),
    };
    unknown.capture_mod_requirements().unwrap();
    assert!(!unknown.mod_evidence_known);
    assert!(unknown.required_mods.is_empty());
    assert!(
        unknown
            .validate_settings(&json!({"master_modoverrides_lua":"return {}"}))
            .is_err()
    );
    let checks = vec![
        assistant_task_check(
            "new_run_ready",
            AssistantTaskCheckStatus::Satisfied,
            "Ready",
            Value::Null,
        ),
        assistant_task_check(
            "mod_requirements_known",
            AssistantTaskCheckStatus::Unknown,
            "Opaque configuration",
            Value::Null,
        ),
    ];
    assert_eq!(
        unknown
            .receipt(assistant_task_status(&checks), checks)
            .status,
        AssistantTaskStatus::Inconclusive
    );
}

#[test]
fn task_unbound_baseline_cannot_authorize_mutating_an_existing_instance() {
    let (bound, instance) = task_fixture(true);
    let unbound = AssistantTaskContract {
        session: None,
        run: std::sync::Arc::new(AssistantTaskRun::default()),
        investigation_draft: std::sync::Arc::new(StdMutex::new(None)),
        id: bound.id.clone(),
        original_request: bound.original_request.clone(),
        requirements_schema: bound.requirements_schema.clone(),
        requirements: None,
        request: AssistantTaskRequest::default(),
        instance_id: None,
        module_id: Some(String::from("dontstarve")),
        initial_settings: json!({}),
        configuration_stage: AssistantConfigurationStage::Unconfigured,
        required_mods: Vec::new(),
        mod_evidence_known: true,
        file_changes: Vec::new(),
    };
    let plan =
        parse_assistant_operation_plan_response(&configuration_response("return {}")).unwrap();
    assert!(
        unbound.validate_plan(&plan, Some(&instance)).is_err(),
        "an existing target requires a captured baseline before confirmation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_apply_change_missing_readback_cannot_complete_without_mod_preservation() {
    assert_unreadable_task_target_is_inconclusive(None).await;
}

#[tokio::test(flavor = "current_thread")]
async fn task_apply_change_wrong_readback_target_cannot_complete_without_mod_preservation() {
    let (_, mut other_instance) = task_fixture(false);
    other_instance.summary.id = String::from("another-instance");
    assert_unreadable_task_target_is_inconclusive(Some(&other_instance)).await;
}

async fn assert_unreadable_task_target_is_inconclusive(after: Option<&InstanceDetails>) {
    use tauri::Manager;

    let (_lock, _environment, app, storage) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let (mut task, instance) = task_fixture(false);
    std::sync::Arc::get_mut(&mut task).unwrap().request.goal = AssistantTaskGoal::ApplyChange;
    let plan =
        parse_assistant_operation_plan_response(&configuration_response("return {}")).unwrap();
    let mut output = assistant_operation_output(&plan, 0);
    output.instance_id = Some(instance.summary.id.clone());
    output.module_id = Some(instance.summary.module_id.clone());
    output.applied_settings_keys = vec![String::from("master_modoverrides_lua")];
    let verification = AssistantOperationVerification {
        status: AssistantVerificationStatus::Inconclusive,
        summary: String::from("The saved target could not be verified."),
        run_id: None,
        evidence: Value::Null,
        can_continue: false,
    };
    assert!(!storage.paths.database_path.exists());

    let receipt = assess_assistant_task(
        &state,
        &storage,
        &task,
        after,
        &output,
        None,
        Some(&verification),
    )
    .await;

    assert_eq!(receipt.goal, AssistantTaskGoal::ApplyChange);
    assert!(!receipt.preserve_existing_mods);
    assert_eq!(receipt.instance_id, task.instance_id);
    assert_eq!(receipt.status, AssistantTaskStatus::Inconclusive);
    assert_eq!(
        receipt
            .checks
            .iter()
            .find(|check| check.name == "operation_result")
            .expect("the successful write result must remain in the receipt")
            .status,
        AssistantTaskCheckStatus::Satisfied
    );
    let target_check = receipt
        .checks
        .iter()
        .find(|check| check.name == "task_snapshot_unchanged")
        .expect("target verification is required even when mod preservation is disabled");
    assert_eq!(target_check.status, AssistantTaskCheckStatus::Unknown);
    assert_eq!(
        target_check.evidence["error"],
        if after.is_some() {
            "The task target changed during verification."
        } else {
            "The task target result could not be read."
        }
    );
    assert!(
        !storage.paths.database_path.exists(),
        "unavailable or mismatched readback must be rejected before database access"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_apply_change_pending_runtime_delivery_cannot_complete() {
    use tauri::Manager;

    let (_lock, _environment, app, storage) =
        crate::commands::tests::assistant_assessment_fixture().await;
    let state = app.state::<DesktopState>();
    let (mut task, _) = task_fixture(false);
    let contract = std::sync::Arc::get_mut(&mut task).unwrap();
    contract.request.goal = AssistantTaskGoal::ApplyChange;
    contract.instance_id = None;
    contract.module_id = None;
    let plan = parse_assistant_operation_plan_response(
        &json!({"action":"run_gm_command", "runtimeCommands":["c_save()"]}).to_string(),
    )
    .unwrap();
    let mut output = assistant_operation_output(&plan, 0);
    output.task = Some(task.receipt(
        AssistantTaskStatus::Inconclusive,
        vec![assistant_task_check(
            "runtime_command_delivery",
            AssistantTaskCheckStatus::Unknown,
            "The runtime command is queued; its stdin write is not confirmed.",
            json!({"writeConfirmationPending":true}),
        )],
    ));

    let receipt = assess_assistant_task(&state, &storage, &task, None, &output, None, None).await;

    assert_eq!(receipt.status, AssistantTaskStatus::Inconclusive);
    assert_eq!(receipt.checks.len(), 2);
    assert_eq!(receipt.checks[0].name, "operation_result");
    assert_eq!(
        receipt.checks[0].status,
        AssistantTaskCheckStatus::Satisfied
    );
    assert_eq!(receipt.checks[1].name, "runtime_command_delivery");
    assert_eq!(receipt.checks[1].status, AssistantTaskCheckStatus::Unknown);
    assert_eq!(receipt.checks[1].evidence["writeConfirmationPending"], true);
    assert!(!storage.paths.app_data_root.exists());
}
