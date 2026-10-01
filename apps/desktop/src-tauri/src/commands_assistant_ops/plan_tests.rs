use super::task_tests::task_fixture;
use super::*;

#[test]
fn assistant_bound_preview_ignores_referenced_or_excluded_servers() {
    let (mut task, details) = task_fixture(true);
    let mut referenced = details.summary.clone();
    referenced.id = "reference-a".into();
    referenced.name = "ArchiveExample".into();
    let instances = vec![referenced.clone(), details.summary.clone()];
    for prompt in [
        "参考 ArchiveExample 的说明，只修改当前选中的服务器。",
        "不要修改 ArchiveExample，修复选中的服务器。",
    ] {
        std::sync::Arc::make_mut(&mut task).original_request = prompt.into();
        assert_eq!(
            assistant_unique_text_matched_instance(prompt, &instances)
                .unwrap()
                .id,
            referenced.id
        );
        for explicit_id in [false, true] {
            for action in [
                AssistantOperationAction::CustomizeConfig,
                AssistantOperationAction::StartServer,
                AssistantOperationAction::ValidateServer,
            ] {
                let mut plan = AssistantOperationPlan {
                    action,
                    instance_id: explicit_id.then(|| details.summary.id.clone()),
                    module_id: Some(details.summary.module_id.clone()),
                    settings_patch: (action == AssistantOperationAction::CustomizeConfig)
                        .then(|| json!({ "cluster_name": "Bound server" })),
                    ..assistant_safe_none_plan("Use the already resolved task target".into())
                };
                bind_assistant_task_preview_target(
                    &task,
                    &mut plan,
                    &instances,
                    &preparation_modules(&details),
                )
                .unwrap();
                task.validate_plan(&plan, Some(&details)).unwrap();
                assert_eq!(
                    plan.instance_id.as_deref(),
                    Some(details.summary.id.as_str())
                );
                assert_eq!(
                    find_assistant_bound_instance_target(&plan, &instances)
                        .unwrap()
                        .id,
                    details.summary.id
                );
            }
        }
    }
    let mut conflicting = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: Some(referenced.id.clone()),
        ..assistant_safe_none_plan("A model cannot change the task target".into())
    };
    assert!(
        bind_assistant_task_preview_target(
            &task,
            &mut conflicting,
            &instances,
            &preparation_modules(&details)
        )
        .is_err()
    );
    assert_eq!(
        conflicting.instance_id.as_deref(),
        Some(referenced.id.as_str())
    );
}

#[test]
fn assistant_bound_preview_never_discovers_an_instance_for_a_module_or_new_server_task() {
    let (mut task, details) = task_fixture(true);
    let task = std::sync::Arc::make_mut(&mut task);
    task.instance_id = None;
    task.request.goal = AssistantTaskGoal::ApplyChange;
    task.original_request = format!("{} is only a reference", details.summary.name);
    let instances = std::slice::from_ref(&details.summary);
    for action in [
        AssistantOperationAction::CreateServer,
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
    ] {
        let mut plan = AssistantOperationPlan {
            action,
            ..assistant_safe_none_plan("Bound module".into())
        };
        bind_assistant_task_preview_target(
            task,
            &mut plan,
            instances,
            &preparation_modules(&details),
        )
        .unwrap();
        assert!(plan.instance_id.is_none());
        assert_eq!(plan.module_id, task.module_id);
    }
    let mut plan = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        ..assistant_safe_none_plan("An instance was never resolved".into())
    };
    assert!(
        bind_assistant_task_preview_target(
            task,
            &mut plan,
            instances,
            &preparation_modules(&details)
        )
        .is_err()
    );
    assert!(plan.instance_id.is_none());
}

#[test]
fn assistant_workshop_plan_requires_explicit_previewed_items() {
    let mut plan = AssistantOperationPlan {
        action: AssistantOperationAction::InstallFunMod,
        ..assistant_safe_none_plan(String::from("Install requested Workshop items"))
    };
    for ids in [
        vec![],
        vec![String::from("not-a-workshop-id")],
        vec![String::from("1"); 21],
    ] {
        plan.workshop_item_ids = ids;
        assert!(validate_assistant_workshop_plan(&plan).is_err());
    }
    plan.workshop_item_ids = vec![String::from("123456789")];
    validate_assistant_workshop_plan(&plan).unwrap();
    let (_, details) = task_fixture(true);
    let summary = summarize_assistant_operation_plan(&preparation_input(&details), &plan);
    assert!(summary.contains("Workshop items=123456789"));
}

fn preparation_input(details: &InstanceDetails) -> AssistantExecuteOperationInput {
    AssistantExecuteOperationInput {
        task: AssistantTaskRequest {
            goal: AssistantTaskGoal::LaunchService,
            preserve_existing_mods: true,
        },
        settings: AssistantProviderSettings {
            provider: "ollama".into(),
            model: "fixture".into(),
            base_url: "http://127.0.0.1:11434".into(),
            api_key: String::new(),
        },
        prompt: "Prepare the selected server files and start it after confirmation.".into(),
        context: None,
        selected_instance_id: Some(details.summary.id.clone()),
        selected_module_id: Some(details.summary.module_id.clone()),
    }
}

fn preparation_modules(details: &InstanceDetails) -> Vec<ModuleSummary> {
    vec![ModuleSummary {
        id: details.summary.module_id.clone(),
        name: "Selected game".into(),
        version: "1".into(),
        description: None,
        steam_app_id: None,
        install_state: InstallState::Installed,
        instance_program_count: 0,
        archived_program_count: 0,
        supported_platforms: vec!["windows".into()],
    }]
}

#[test]
fn assistant_file_preview_binds_selected_instance_when_model_omits_instance_id() {
    let (_, details) = task_fixture(true);
    let mut input = preparation_input(&details);
    // The core resolved the selected instance's owning module; an old Library
    // selection cannot redirect this file operation to another game.
    input.selected_module_id = Some("stale-library-selection".into());
    for action in [
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
    ] {
        let mut plan = AssistantOperationPlan {
            action,
            module_id: Some(details.summary.module_id.clone()),
            ..assistant_safe_none_plan("Prepare files".into())
        };
        bind_assistant_preview_target(
            &input,
            &mut plan,
            Some(&details.summary.module_id),
            std::slice::from_ref(&details.summary),
            &preparation_modules(&details),
            Some(&details),
        )
        .unwrap();
        assert_eq!(
            plan.instance_id.as_deref(),
            Some(details.summary.id.as_str())
        );
        assert_eq!(
            plan.module_id.as_deref(),
            Some(details.summary.module_id.as_str())
        );
    }
}

#[test]
fn assistant_file_preview_uses_the_already_resolved_context_instance() {
    let (_, details) = task_fixture(true);
    let mut input = preparation_input(&details);
    input.task.goal = AssistantTaskGoal::ApplyChange;
    input.selected_instance_id = None;
    input.selected_module_id = None;
    let mut plan = AssistantOperationPlan {
        action: AssistantOperationAction::ValidateServer,
        ..assistant_safe_none_plan("Verify the task's server files".into())
    };
    bind_assistant_preview_target(
        &input,
        &mut plan,
        None,
        std::slice::from_ref(&details.summary),
        &preparation_modules(&details),
        Some(&details),
    )
    .unwrap();
    assert_eq!(
        plan.instance_id.as_deref(),
        Some(details.summary.id.as_str())
    );
    assert_eq!(
        plan.module_id.as_deref(),
        Some(details.summary.module_id.as_str())
    );
}

#[test]
fn assistant_file_preview_rejects_a_model_target_conflicting_with_its_context() {
    let (_, details) = task_fixture(true);
    let input = preparation_input(&details);
    for (instance_id, module_id) in [
        (Some("another-instance".into()), None),
        (None, Some("another-game".into())),
    ] {
        let mut plan = AssistantOperationPlan {
            action: AssistantOperationAction::ValidateServer,
            instance_id,
            module_id,
            ..assistant_safe_none_plan("Conflicting file target".into())
        };
        assert!(
            bind_assistant_preview_target(
                &input,
                &mut plan,
                Some(&details.summary.module_id),
                std::slice::from_ref(&details.summary),
                &preparation_modules(&details),
                Some(&details),
            )
            .is_err()
        );
    }
}

#[test]
fn assistant_module_only_file_preparation_and_creation_remain_unbound() {
    let (_, details) = task_fixture(true);
    let mut input = preparation_input(&details);
    input.selected_instance_id = None;
    for action in [
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::CreateServer,
    ] {
        let mut plan = AssistantOperationPlan {
            action,
            module_id: input.selected_module_id.clone(),
            ..assistant_safe_none_plan("Prepare a new server".into())
        };
        bind_assistant_preview_target(
            &input,
            &mut plan,
            Some(&details.summary.module_id),
            std::slice::from_ref(&details.summary),
            &preparation_modules(&details),
            None,
        )
        .unwrap();
        assert!(plan.instance_id.is_none());
    }
}
