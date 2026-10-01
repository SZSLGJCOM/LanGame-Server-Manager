use super::task_tests::task_fixture;
use super::*;

fn requirement_plan(expected: &str) -> AssistantOperationPlan {
    serde_json::from_value(json!({"action":"customize_config",
        "settingsPatch":{"cluster_name":expected},
        "taskRequirements":{"settings":[{"key":"cluster_name","expected":expected,
            "description":"Requested name","sourceText":"final"}],
            "ports":[],"forbiddenActions":[],"unverified":[]}}))
    .unwrap()
}

fn bound_requirements() -> (AssistantTaskContract, InstanceDetails) {
    let (original, instance) = task_fixture(false);
    let mut task = original.as_ref().clone();
    task.original_request = "Set the cluster name to final and restore the server.".into();
    task.requirements = None;
    let task = task
        .bind_requirements(&requirement_plan("final"), Some(&instance), None)
        .unwrap();
    (task, instance)
}

#[test]
fn service_task_cannot_propose_a_mutation_without_reviewable_requirements() {
    let (original, instance) = task_fixture(false);
    let mut task = original.as_ref().clone();
    task.requirements = None;
    let mut plan = requirement_plan("final");
    plan.task_requirements = None;
    assert!(
        task.bind_requirements(&plan, Some(&instance), None)
            .unwrap_err()
            .contains("taskRequirements")
    );
    plan.action = AssistantOperationAction::None;
    assert!(task.bind_requirements(&plan, Some(&instance), None).is_ok());
}

#[test]
fn confirmed_requirements_cannot_be_removed_or_redefined_by_follow_up() {
    let (task, instance) = bound_requirements();
    let mut continuation = requirement_plan("final");
    continuation.task_requirements = None;
    let inherited = task
        .bind_requirements(&continuation, Some(&instance), None)
        .unwrap();
    assert_eq!(inherited.requirements, task.requirements);
    let mut removed = requirement_plan("final");
    removed.task_requirements = Some(
        serde_json::from_value(json!({
            "settings":[],"ports":[],"forbiddenActions":[],"unverified":[]
        }))
        .unwrap(),
    );
    for changed in [removed, requirement_plan("different")] {
        assert!(
            task.bind_requirements(&changed, Some(&instance), None)
                .unwrap_err()
                .contains("immutable")
        );
    }
    assert!(task.summary().contains("final"));
    assert!(task.summary().contains("cluster_name"));
}

#[test]
fn creation_initial_configuration_and_failed_start_keep_the_same_requirements() {
    let (mut task, mut instance) = bound_requirements();
    task.request.goal = AssistantTaskGoal::LaunchService;
    task.instance_id = None;
    task.configuration_stage = AssistantConfigurationStage::Unconfigured;
    let created = task.bind_created_instance(&instance).unwrap();
    assert_eq!(created.requirements, task.requirements);
    let start = AssistantOperationPlan {
        action: AssistantOperationAction::StartServer,
        instance_id: Some(instance.summary.id.clone()),
        module_id: Some(instance.summary.module_id.clone()),
        ..assistant_safe_none_plan("Start the server".into())
    };
    let partial = created.record_initial_configuration(&instance).unwrap();
    assert!(partial.validate_plan(&start, Some(&instance)).is_err());
    let mut settings: Value = serde_json::from_str(&instance.settings_json).unwrap();
    settings["cluster_name"] = json!("final");
    instance.settings_json = settings.to_string();
    let configured = partial.record_initial_configuration(&instance).unwrap();
    configured.validate_plan(&start, Some(&instance)).unwrap();
    let protected = configured.protect_created_configuration(&instance).unwrap();
    assert_eq!(protected.requirements, task.requirements);
    assert_eq!(protected.id, task.id);
    settings["cluster_name"] = json!("changed after preview");
    instance.settings_json = settings.to_string();
    assert!(protected.validate_plan(&start, Some(&instance)).is_err());
}

#[test]
fn missing_requirement_evidence_prevents_task_completion_even_with_runtime_ready() {
    let (task, instance) = bound_requirements();
    let mut checks = vec![assistant_task_check(
        "new_run_ready",
        AssistantTaskCheckStatus::Satisfied,
        "Ready",
        Value::Null,
    )];
    checks.extend(
        task.requirements
            .as_ref()
            .unwrap()
            .checks(Some(&instance), None),
    );
    assert_ne!(
        assistant_task_status(&checks),
        AssistantTaskStatus::Completed
    );
    let unknown = task.requirements.as_ref().unwrap().checks(None, None);
    assert!(
        unknown
            .iter()
            .any(|check| check.status == AssistantTaskCheckStatus::Unknown)
    );
    assert_eq!(
        assistant_task_status(&unknown),
        AssistantTaskStatus::Inconclusive
    );
}
