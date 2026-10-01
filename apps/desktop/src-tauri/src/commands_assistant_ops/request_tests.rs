use super::*;

#[test]
fn assistant_request_storage_identity_includes_all_paths_before_binding_catalog_ids() {
    let original = app_core::AppState::default();
    let identity = assistant_catalog_storage_identity(&original);
    for field in 0..6 {
        let mut changed = original.clone();
        let path = match field {
            0 => &mut changed.settings.servers_root,
            1 => &mut changed.settings.games_root,
            2 => &mut changed.settings.modules_root,
            3 => &mut changed.settings.steamcmd_root,
            4 => &mut changed.storage.database_path,
            _ => &mut changed.storage.migrations_path,
        };
        path.push_str("/different-context");
        assert_ne!(assistant_catalog_storage_identity(&changed), identity);
    }
    let mut unrelated = original;
    unrelated.storage.schema_version += 1;
    unrelated.snapshot.memory_total_bytes = 32 * 1024 * 1024 * 1024;
    assert_eq!(assistant_catalog_storage_identity(&unrelated), identity);
}

#[test]
fn preparation_goal_requires_saved_requirements_and_never_automatically_starts() {
    let (mut task, instance) = task_tests::task_fixture(true);
    std::sync::Arc::make_mut(&mut task).request.goal = AssistantTaskGoal::PrepareService;
    assert!(task.requires_requirements());
    assert!(!task.requires_running_service());
    assert_eq!(
        task.operation_limit(),
        assistant_operation_limit(AssistantTaskGoal::PrepareService)
    );
    let start = target_plan(AssistantOperationAction::StartServer);
    assert!(task.validate_plan(&start, Some(&instance)).is_err());
    assert!(
        validate_assistant_repair_follow_up(
            &start,
            &instance.summary.id,
            &instance.summary.module_id,
            1,
            AssistantTaskGoal::PrepareService,
        )
        .is_err()
    );
    let follow_up = AssistantOperationMode::FollowUp {
        step: 1,
        instance_id: instance.summary.id.clone(),
        module_id: instance.summary.module_id.clone(),
        original_prompt: String::from("Create and configure without starting."),
        verified_precondition: None,
        task: Some(task.clone()),
        verification: json!({"verification":{"status":"inconclusive","canContinue":true,
            "evidence":{"launchReady":true}}}),
    };
    assert!(
        assistant_saved_repair_start_plan(&follow_up, Some(&instance))
            .unwrap()
            .is_none(),
        "launch readiness must not create an automatic start for preparation-only tasks"
    );
    std::sync::Arc::make_mut(&mut task).requirements = None;
    let mut config = target_plan(AssistantOperationAction::CustomizeConfig);
    config.settings_patch = Some(json!({"cluster_name":"Configured name"}));
    assert!(
        task.bind_requirements(&config, Some(&instance), None)
            .is_err(),
        "preparation may not proceed without binding the user's actual requirements"
    );
}

fn target_plan(action: AssistantOperationAction) -> AssistantOperationPlan {
    AssistantOperationPlan {
        action,
        ..assistant_safe_none_plan(String::from("Target validation fixture"))
    }
}

#[test]
fn resolved_target_rejects_conflicting_planner_ids_before_selected_target_fallback() {
    let (task, instance) = task_tests::task_fixture(true);
    let mut plan = target_plan(AssistantOperationAction::CustomizeConfig);
    plan.instance_id = Some(instance.summary.id.clone());
    plan.module_id = Some(instance.summary.module_id.clone());
    assert!(
        validate_assistant_resolved_target(&task, AssistantIntentTarget::ExistingInstance, &plan)
            .is_ok()
    );

    for (instance_id, module_id) in [
        ("another-instance", instance.summary.module_id.as_str()),
        (instance.summary.id.as_str(), "minecraft"),
        ("another-instance", "minecraft"),
    ] {
        let mut conflicting = plan.clone();
        conflicting.instance_id = Some(instance_id.into());
        conflicting.module_id = Some(module_id.into());
        let error = validate_assistant_resolved_target(
            &task,
            AssistantIntentTarget::ExistingInstance,
            &conflicting,
        )
        .expect_err("a conflicting target must not be replaced by the selected instance");
        assert!(error.contains("conflicts with the request's resolved target"));
    }

    let omitted = target_plan(AssistantOperationAction::CustomizeConfig);
    assert!(
        validate_assistant_resolved_target(
            &task,
            AssistantIntentTarget::ExistingInstance,
            &omitted
        )
        .is_ok(),
        "omitted IDs inherit the already resolved target"
    );
}

#[test]
fn resolved_no_target_cannot_be_rebound_by_an_operation() {
    let (mut task, _) = task_tests::task_fixture(true);
    let unbound = std::sync::Arc::make_mut(&mut task);
    unbound.instance_id = None;
    unbound.module_id = None;
    unbound.request.goal = AssistantTaskGoal::Inspect;
    for action in [
        AssistantOperationAction::StartServer,
        AssistantOperationAction::CreateServer,
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::CustomizeConfig,
        AssistantOperationAction::PatchInstanceText,
        AssistantOperationAction::InstallSiteMod,
        AssistantOperationAction::RepairPorts,
        AssistantOperationAction::Broadcast,
    ] {
        let plan = target_plan(action);
        assert!(
            validate_assistant_resolved_target(&task, AssistantIntentTarget::None, &plan).is_err(),
            "{action:?} must not invent a target"
        );
        let named = AssistantOperationPlan {
            instance_id: Some(String::from("task-instance")),
            module_id: Some(String::from("dontstarve")),
            ..plan
        };
        assert!(
            validate_assistant_resolved_target(&task, AssistantIntentTarget::None, &named).is_err(),
            "{action:?} must not add a target after interpretation"
        );
    }
    assert!(
        validate_assistant_resolved_target(
            &task,
            AssistantIntentTarget::None,
            &target_plan(AssistantOperationAction::None)
        )
        .is_ok()
    );
}

#[test]
fn resolved_module_scope_allows_file_preparation_but_not_instance_creation() {
    let (mut task, _) = task_tests::task_fixture(true);
    let module_only = std::sync::Arc::make_mut(&mut task);
    module_only.instance_id = None;
    module_only.request.goal = AssistantTaskGoal::ApplyChange;
    for action in [
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
    ] {
        assert!(
            validate_assistant_resolved_target(
                &task,
                AssistantIntentTarget::Module,
                &target_plan(action)
            )
            .is_ok()
        );
    }
    let error = validate_assistant_resolved_target(
        &task,
        AssistantIntentTarget::Module,
        &target_plan(AssistantOperationAction::CreateServer),
    )
    .expect_err("module-level preparation does not authorize creating an instance");
    assert!(error.contains("new-instance request"));
}

#[test]
fn resolved_new_instance_scope_allows_creation_but_not_changes_to_existing_instances() {
    let (mut task, instance) = task_tests::task_fixture(true);
    let new_instance = std::sync::Arc::make_mut(&mut task);
    new_instance.instance_id = None;
    new_instance.request.goal = AssistantTaskGoal::ApplyChange;
    assert!(
        validate_assistant_resolved_target(
            &task,
            AssistantIntentTarget::NewInstance,
            &target_plan(AssistantOperationAction::CreateServer)
        )
        .is_ok(),
        "create-only requests need no running-service goal"
    );
    for action in [
        AssistantOperationAction::CustomizeConfig,
        AssistantOperationAction::PatchInstanceText,
    ] {
        let plan = target_plan(action);
        assert!(
            validate_assistant_resolved_target(&task, AssistantIntentTarget::NewInstance, &plan)
                .is_err(),
            "instance creation must precede instance changes"
        );
        let redirected = AssistantOperationPlan {
            instance_id: Some(instance.summary.id.clone()),
            ..plan
        };
        assert!(
            validate_assistant_resolved_target(
                &task,
                AssistantIntentTarget::NewInstance,
                &redirected
            )
            .is_err(),
            "new-instance intent must not fall back to an existing instance"
        );
    }
}
