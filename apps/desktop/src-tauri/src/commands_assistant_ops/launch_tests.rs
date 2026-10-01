use super::task_tests::task_fixture;
use super::*;

fn unbound_launch_fixture() -> (AssistantTaskContract, InstanceDetails) {
    let (original, details) = task_fixture(true);
    let mut task = original.as_ref().clone();
    task.request.goal = AssistantTaskGoal::LaunchService;
    task.instance_id = None;
    task.initial_settings = json!({});
    task.configuration_stage = AssistantConfigurationStage::Unconfigured;
    task.required_mods.clear();
    (task, details)
}

#[test]
fn assistant_launch_install_check_requires_the_selected_complete_executable() {
    let installed = app_steamcmd::ModuleInstallResult {
        module_id: String::from("dontstarve"),
        steam_app_id: 343050,
        operation: String::from("validate"),
        install_root: String::from("C:/isolated/dontstarve"),
        executable_path: String::from("C:/isolated/dontstarve/server.exe"),
        executable_exists: true,
        install_state: InstallState::Installed,
        current_version: None,
        output_excerpt: String::new(),
    };
    assert_eq!(
        assistant_install_result_check("dontstarve", &installed).status,
        AssistantTaskCheckStatus::Satisfied
    );
    let mut wrong_module = installed.clone();
    wrong_module.module_id = String::from("another-game");
    let mut missing_executable = installed.clone();
    missing_executable.executable_exists = false;
    let mut incomplete = installed;
    incomplete.install_state = InstallState::Incomplete;
    for invalid in [wrong_module, missing_executable, incomplete] {
        let check = assistant_install_result_check("dontstarve", &invalid);
        assert_eq!(check.name, "server_files_ready");
        assert_eq!(check.status, AssistantTaskCheckStatus::Failed);
        assert_eq!(assistant_task_status(&[check]), AssistantTaskStatus::Failed);
    }
}

#[test]
fn assistant_task_goals_share_the_bounded_cumulative_operation_limit() {
    let (original, _) = task_fixture(true);
    for (goal, expected) in [
        (AssistantTaskGoal::LaunchService, 32),
        (AssistantTaskGoal::PrepareService, 32),
        (AssistantTaskGoal::RestoreService, 32),
        (AssistantTaskGoal::ApplyChange, 32),
        (AssistantTaskGoal::Inspect, 32),
    ] {
        let mut task = original.as_ref().clone();
        task.request.goal = goal;
        assert_eq!(assistant_operation_limit(goal), expected);
        assert_eq!(task.operation_limit(), expected);
        assert_eq!(
            task.receipt(AssistantTaskStatus::Proposed, Vec::new())
                .operation_limit,
            expected
        );
    }
}

#[test]
fn assistant_launch_unbound_task_cannot_be_redirected_to_an_existing_instance() {
    let (task, details) = unbound_launch_fixture();
    for action in [
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::CreateServer,
    ] {
        let mut plan = AssistantOperationPlan {
            action,
            module_id: task.module_id.clone(),
            ..assistant_safe_none_plan(String::from("Prepare the selected game"))
        };
        task.validate_plan(&plan, None).unwrap();
        assert!(task.validate_plan(&plan, Some(&details)).is_err());
        plan.instance_id = Some(details.summary.id.clone());
        let error = task.validate_plan(&plan, None).unwrap_err();
        assert!(error.contains("cannot select an existing instance"));
    }
    let start = AssistantOperationPlan {
        action: AssistantOperationAction::StartServer,
        module_id: task.module_id.clone(),
        ..assistant_safe_none_plan(String::from("Start without creating an instance"))
    };
    assert!(task.validate_plan(&start, None).is_err());
    assert!(task.instance_id.is_none());
}

#[test]
fn assistant_launch_created_instance_binds_once_without_replacing_the_task() {
    let (task, details) = unbound_launch_fixture();
    let mut wrong_game = details.clone();
    wrong_game.summary.module_id = String::from("another-game");
    assert!(task.bind_created_instance(&wrong_game).is_err());

    let bound = task.bind_created_instance(&details).unwrap();
    assert_eq!(bound.id, task.id);
    assert_eq!(bound.request.goal, AssistantTaskGoal::LaunchService);
    assert_eq!(bound.module_id, task.module_id);
    assert_eq!(
        bound.instance_id.as_deref(),
        Some(details.summary.id.as_str())
    );
    assert!(bound.request.preserve_existing_mods);
    assert_eq!(
        bound.configuration_stage,
        AssistantConfigurationStage::Unconfigured
    );
    assert_eq!(bound.initial_settings.to_string(), details.settings_json);
    assert!(bound.bind_created_instance(&details).is_err());
    let mut second = details.clone();
    second.summary.id = String::from("another-instance");
    assert!(bound.bind_created_instance(&second).is_err());
    assert!(
        task.instance_id.is_none(),
        "the original contract is immutable"
    );
}

#[test]
fn assistant_launch_first_start_requires_confirmed_initial_configuration() {
    let (task, details) = unbound_launch_fixture();
    let created = task.bind_created_instance(&details).unwrap();
    let start = AssistantOperationPlan {
        action: AssistantOperationAction::StartServer,
        instance_id: created.instance_id.clone(),
        module_id: created.module_id.clone(),
        ..assistant_safe_none_plan("Start with the saved configuration".into())
    };
    assert!(created.validate_plan(&start, Some(&details)).is_err());
    let configured = created.record_initial_configuration(&details).unwrap();
    assert_eq!(
        configured.configuration_stage,
        AssistantConfigurationStage::Configuring
    );
    configured.validate_plan(&start, Some(&details)).unwrap();
}

#[test]
fn assistant_launch_first_configuration_can_disable_a_default_caves_shard() {
    let (task, mut defaults) = unbound_launch_fixture();
    let mut settings: Value = serde_json::from_str(&defaults.settings_json).unwrap();
    settings["enable_caves"] = json!(true);
    defaults.settings_json = settings.to_string();
    let created = task.bind_created_instance(&defaults).unwrap();
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: created.instance_id.clone(),
        module_id: created.module_id.clone(),
        settings_patch: Some(json!({"enable_caves": false})),
        ..assistant_safe_none_plan(String::from("Use only the requested surface shard"))
    };
    created.validate_plan(&plan, Some(&defaults)).unwrap();
    settings["enable_caves"] = json!(false);
    defaults.settings_json = settings.to_string();
    let configured = created.record_initial_configuration(&defaults).unwrap();
    let protected = configured.protect_created_configuration(&defaults).unwrap();
    assert_eq!(protected.initial_settings["enable_caves"], false);
}

#[test]
fn assistant_launch_first_start_establishes_mod_and_active_shard_protection() {
    let (task, mut defaults) = unbound_launch_fixture();
    let mut initial: Value = serde_json::from_str(&defaults.settings_json).unwrap();
    initial["enable_caves"] = json!(false);
    initial["caves_modoverrides_lua"] = json!("return {}");
    defaults.settings_json = initial.to_string();
    let created = task.bind_created_instance(&defaults).unwrap();
    let first_settings = json!({
        "cluster_name": "Configured server",
        "enable_caves": true,
        "master_modoverrides_lua": "return {chosen={enabled=true,configuration_options={mode='normal'}}}",
        "caves_modoverrides_lua": "return {underground={enabled=true}}"
    });
    let configure = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: created.instance_id.clone(),
        module_id: created.module_id.clone(),
        settings_patch: Some(first_settings.clone()),
        ..assistant_safe_none_plan(String::from("Configure the newly created instance"))
    };
    created.validate_plan(&configure, Some(&defaults)).unwrap();
    let mut configured = defaults.clone();
    configured.settings_json = first_settings.to_string();
    let configuring = created.record_initial_configuration(&configured).unwrap();
    let protected = configuring
        .protect_created_configuration(&configured)
        .unwrap();
    assert_eq!(
        protected.configuration_stage,
        AssistantConfigurationStage::Protected
    );
    assert_eq!(protected.id, task.id);
    assert_eq!(protected.request.goal, AssistantTaskGoal::LaunchService);
    assert_eq!(protected.initial_settings, first_settings);
    assert!(protected.mod_evidence_known);
    assert!(
        protected
            .required_mods
            .iter()
            .any(|required| { required.shard == "master" && required.folder_name == "chosen" })
    );
    assert!(
        protected
            .required_mods
            .iter()
            .any(|required| { required.shard == "caves" && required.folder_name == "underground" })
    );

    for patch in [
        json!({"enable_caves": false}),
        json!({"master_modoverrides_lua": "return {}"}),
        json!({"master_modoverrides_lua": "return {chosen={enabled=true}}"}),
        json!({"caves_modoverrides_lua": "return {}"}),
    ] {
        let plan = AssistantOperationPlan {
            settings_patch: Some(patch),
            ..configure.clone()
        };
        assert!(protected.validate_plan(&plan, Some(&configured)).is_err());
    }
    let ordinary_change = AssistantOperationPlan {
        settings_patch: Some(json!({"cluster_name": "Renamed server"})),
        ..configure
    };
    protected
        .validate_plan(&ordinary_change, Some(&configured))
        .unwrap();
}

#[test]
fn assistant_launch_configuration_baseline_cannot_be_reinitialized_or_retargeted() {
    let (task, details) = unbound_launch_fixture();
    let created = task.bind_created_instance(&details).unwrap();
    assert!(created.protect_created_configuration(&details).is_err());
    let configuring = created.record_initial_configuration(&details).unwrap();
    for change_module in [false, true] {
        let mut wrong_target = details.clone();
        if change_module {
            wrong_target.summary.module_id = String::from("another-game");
        } else {
            wrong_target.summary.id = String::from("another-instance");
        }
        assert!(created.record_initial_configuration(&wrong_target).is_err());
        assert!(
            configuring
                .record_initial_configuration(&wrong_target)
                .is_err()
        );
        assert!(
            configuring
                .protect_created_configuration(&wrong_target)
                .is_err()
        );
    }
    let protected = configuring.protect_created_configuration(&details).unwrap();
    assert!(protected.record_initial_configuration(&details).is_err());
    assert!(protected.protect_created_configuration(&details).is_err());
    let mut destructive = details.clone();
    destructive.settings_json = json!({"master_modoverrides_lua": "return {}"}).to_string();
    assert!(
        protected
            .record_initial_configuration(&destructive)
            .is_err()
    );
    let changed: Value = serde_json::from_str(&destructive.settings_json).unwrap();
    assert!(protected.validate_settings(&changed).is_err());
    assert_eq!(
        protected.initial_settings.to_string(),
        details.settings_json
    );
}

#[test]
fn assistant_launch_multiple_initial_configuration_steps_do_not_protect_defaults() {
    let (task, mut details) = unbound_launch_fixture();
    let defaults = json!({
        "cluster_name": "Default server",
        "enable_caves": true,
        "master_modoverrides_lua": "return {default_surface={enabled=true}}",
        "caves_modoverrides_lua": "return {default_underground={enabled=true}}"
    });
    details.settings_json = defaults.to_string();
    let created = task.bind_created_instance(&details).unwrap();
    let mut settings = defaults.clone();
    settings["cluster_name"] = json!("Requested server");
    details.settings_json = settings.to_string();
    let configuring = created.record_initial_configuration(&details).unwrap();
    let second_patch = json!({
        "enable_caves": false,
        "master_modoverrides_lua": "return {chosen={enabled=true}}"
    });
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: configuring.instance_id.clone(),
        module_id: configuring.module_id.clone(),
        settings_patch: Some(second_patch.clone()),
        ..assistant_safe_none_plan(String::from("Finish the requested initial setup"))
    };
    configuring.validate_plan(&plan, Some(&details)).unwrap();
    for (key, value) in second_patch.as_object().unwrap() {
        settings[key] = value.clone();
    }
    details.settings_json = settings.to_string();
    let configured = configuring.record_initial_configuration(&details).unwrap();
    assert_eq!(configured.id, task.id);
    assert_eq!(
        configured.configuration_stage,
        AssistantConfigurationStage::Configuring
    );
    let protected = configured.protect_created_configuration(&details).unwrap();
    assert_eq!(protected.initial_settings, settings);
    assert_eq!(protected.initial_settings["enable_caves"], false);
    assert!(protected.mod_evidence_known);
    assert_eq!(protected.required_mods.len(), 1);
    assert_eq!(protected.required_mods[0].shard, "master");
    assert_eq!(protected.required_mods[0].folder_name, "chosen");
    assert_eq!(created.initial_settings, defaults);
}

#[test]
fn assistant_launch_after_install_preserves_task_and_requires_a_fresh_single_use_confirmation() {
    let (task, _) = unbound_launch_fixture();
    let provider = AssistantProviderSettings {
        provider: String::from("ollama"),
        base_url: String::from("http://127.0.0.1:11434"),
        model: String::from("fixture"),
        api_key: String::new(),
    };
    let task = std::sync::Arc::new(task);
    let preview = assistant_create_after_install_preview(
        &provider,
        "Original launch request",
        task.clone(),
        1,
    )
    .unwrap();
    assert_eq!(preview.action, AssistantOperationAction::CreateServer);
    assert!(preview.requires_confirmation);
    assert!(preview.instance_id.is_none());
    assert_eq!(preview.task.as_ref().unwrap().id, task.id);
    let token = preview.confirmation_token.as_deref().unwrap();
    let summary = preview.plan_summary.as_deref().unwrap();
    let pending = take_assistant_pending_operation(token, summary, &provider).unwrap();
    assert_eq!(pending.repair_step, 1);
    assert_eq!(pending.original_prompt, "Original launch request");
    assert_eq!(pending.task.id, task.id);
    assert!(pending.expected_instance.is_none());
    assert!(take_assistant_pending_operation(token, summary, &provider).is_err());
    assert!(
        assistant_create_after_install_preview(
            &provider,
            "Original launch request",
            task,
            assistant_operation_limit(AssistantTaskGoal::LaunchService)
        )
        .is_err()
    );
}

#[test]
fn assistant_launch_unreadable_created_instance_retains_its_identity_and_stops_continuation() {
    let (task, _) = unbound_launch_fixture();
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CreateServer,
        ..assistant_safe_none_plan(String::from("Create instance"))
    };
    let mut output = assistant_operation_output(&plan, 0);
    output.instance_id = Some(String::from("created-server"));
    output.module_id = task.module_id.clone();
    assistant_unbound_operation_result(&task, &mut output, Some("configuration readback failed"));
    assert_eq!(output.instance_id.as_deref(), Some("created-server"));
    assert_eq!(
        output.task.as_ref().unwrap().status,
        AssistantTaskStatus::Failed
    );
    assert!(!output.verification.as_ref().unwrap().can_continue);
    assert!(output.follow_up.is_none());
}
