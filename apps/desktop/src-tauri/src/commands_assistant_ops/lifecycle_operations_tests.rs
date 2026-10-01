use super::*;

fn lifecycle_fixture(
    action: AssistantOperationAction,
) -> (
    AssistantTaskContract,
    InstanceDetails,
    AssistantOperationPlan,
) {
    let (task, mut instance) = task_tests::task_fixture(true);
    let mut task = task.as_ref().clone();
    task.request.goal = AssistantTaskGoal::ApplyChange;
    if matches!(
        action,
        AssistantOperationAction::StopServer | AssistantOperationAction::RestartServer
    ) {
        instance.summary.status = InstanceStatus::Running;
        instance.summary.active_process_count = 1;
        instance.active_run = Some(ActiveInstanceRun {
            run_id: 7,
            session_id: Some("confirmed-run".into()),
            pid: Some(42),
            log_path: None,
            process_count: 1,
            processes: Vec::new(),
        });
    }
    let plan = AssistantOperationPlan {
        action,
        instance_id: Some(instance.summary.id.clone()),
        module_id: Some(instance.summary.module_id.clone()),
        backup_id: (action == AssistantOperationAction::RestoreBackup)
            .then(|| "saves-123-abc".into()),
        ..assistant_safe_none_plan("Requested lifecycle operation".into())
    };
    (task, instance, plan)
}

#[test]
fn lifecycle_actions_require_explicit_change_and_never_enter_continuous_repair() {
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::CreateBackup,
        AssistantOperationAction::RestoreBackup,
    ] {
        let (mut task, instance, plan) = lifecycle_fixture(action);
        task.validate_plan(&plan, Some(&instance)).unwrap();
        for goal in [
            AssistantTaskGoal::Inspect,
            AssistantTaskGoal::RestoreService,
            AssistantTaskGoal::LaunchService,
            AssistantTaskGoal::PrepareService,
        ] {
            task.request.goal = goal;
            assert!(
                task.validate_plan(&plan, Some(&instance)).is_err(),
                "{action:?} {goal:?}"
            );
            assert!(!assistant_action_allows_continuation(action, goal));
            assert!(
                validate_assistant_repair_follow_up(
                    &plan,
                    &instance.summary.id,
                    &instance.summary.module_id,
                    0,
                    goal
                )
                .is_err()
            );
        }
        assert!(!assistant_action_allows_continuation(
            action,
            AssistantTaskGoal::ApplyChange
        ));
    }
}

#[test]
fn lifecycle_plans_reject_missing_targets_wrong_states_and_mixed_mutations() {
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::CreateBackup,
        AssistantOperationAction::RestoreBackup,
    ] {
        let (task, mut instance, mut plan) = lifecycle_fixture(action);
        assert!(task.validate_plan(&plan, None).is_err());
        plan.settings_patch = Some(json!({"cluster_name":"unexpected"}));
        assert!(task.validate_plan(&plan, Some(&instance)).is_err());
        plan.settings_patch = None;
        instance.summary.status = InstanceStatus::Starting;
        assert!(task.validate_plan(&plan, Some(&instance)).is_err());
    }
}

#[test]
fn restore_requires_an_exact_plain_backup_id_and_restart_respects_no_start_or_stop() {
    let (task, instance, mut plan) = lifecycle_fixture(AssistantOperationAction::RestoreBackup);
    for backup_id in [
        None,
        Some(""),
        Some("../saves-123"),
        Some("C:\\other"),
        Some("a/b"),
    ] {
        plan.backup_id = backup_id.map(str::to_owned);
        assert!(task.validate_plan(&plan, Some(&instance)).is_err());
    }
    let (mut task, instance, plan) = lifecycle_fixture(AssistantOperationAction::RestartServer);
    for action in [
        AssistantOperationAction::StartServer,
        AssistantOperationAction::StopServer,
    ] {
        task.requirements.as_mut().unwrap().forbidden_actions =
            vec![AssistantForbiddenActionRequirement {
                action,
                source_text: "Do not interrupt".into(),
                description: "Preserve runtime".into(),
            }];
        assert!(task.validate_plan(&plan, Some(&instance)).is_err());
    }
}

#[test]
fn lifecycle_readback_cannot_accept_changed_settings_or_backup_policy() {
    let (_, before, _) = lifecycle_fixture(AssistantOperationAction::RestartServer);
    let mut stopped = before.clone();
    stopped.summary.status = InstanceStatus::Stopped;
    stopped.summary.active_process_count = 0;
    stopped.active_run = None;
    validate_assistant_lifecycle_saved_state(&before, &stopped).unwrap();
    stopped.settings_json = "{}".into();
    assert!(validate_assistant_lifecycle_saved_state(&before, &stopped).is_err());
    let mut altered = before.clone();
    altered.backup_retention_count += 1;
    assert!(
        AssistantOperationPrecondition::from_details(&before)
            .validate(&altered)
            .is_err()
    );
}

#[test]
fn restore_readback_requires_the_exact_verified_native_state() {
    let (_, before, plan) = lifecycle_fixture(AssistantOperationAction::RestoreBackup);
    let mut restored = before.clone();
    let mut settings: Value = serde_json::from_str(&restored.settings_json).unwrap();
    settings["master_worldgenoverride_lua"] = json!("return {overrides={world_size='huge'}}");
    restored.settings_json = settings.to_string();
    let mut output = assistant_operation_output(&plan, 0);
    assert!(validate_assistant_lifecycle_readback(&before, &restored, &output).is_err());
    output.restored_instance = Some(Box::new(restored.clone()));
    validate_assistant_lifecycle_readback(&before, &restored, &output).unwrap();
    assert!(validate_assistant_lifecycle_readback(&before, &before, &output).is_err());
    let serialized = serde_json::to_value(&output).unwrap();
    assert!(serialized.get("restoredInstance").is_none());
    for field in ["identity", "module", "bind", "port", "policy", "settings", "world"] {
        let mut changed = restored.clone();
        match field {
            "identity" => changed.summary.id.push_str("-other"),
            "module" => changed.summary.module_id = "minecraft".into(),
            "bind" => changed.summary.bind_ip = "0.0.0.0".into(),
            "port" => changed.ports.push(PortBinding {
                name: "game".into(),
                protocol: "udp".into(),
                port: 10999,
            }),
            "policy" => changed.backup_retention_count += 1,
            "settings" | "world" => {
                let mut values: Value = serde_json::from_str(&changed.settings_json).unwrap();
                let key = if field == "settings" { "cluster_name" } else { "master_worldgenoverride_lua" };
                values[key] = json!("unconfirmed change");
                changed.settings_json = values.to_string();
            }
            _ => unreachable!(),
        }
        assert!(validate_assistant_lifecycle_readback(&before, &changed, &output).is_err(), "{field}");
    }
    output.action = AssistantOperationAction::CreateBackup;
    assert!(validate_assistant_lifecycle_readback(&before, &restored, &output).is_err());
}

#[test]
fn lifecycle_respects_backup_prohibitions_including_native_side_effects() {
    for action in [
        AssistantOperationAction::StopServer,
        AssistantOperationAction::RestartServer,
        AssistantOperationAction::RestoreBackup,
    ] {
        let (mut task, mut instance, plan) = lifecycle_fixture(action);
        task.requirements.as_mut().unwrap().forbidden_actions =
            vec![AssistantForbiddenActionRequirement {
                action: AssistantOperationAction::CreateBackup,
                source_text: "Do not create backups".into(),
                description: "No new backups".into(),
            }];
        assert!(task.validate_plan(&plan, Some(&instance)).is_err());
        instance.auto_backup_on_stop = false;
        if action == AssistantOperationAction::RestoreBackup {
            assert!(
                task.validate_plan(&plan, Some(&instance)).is_err(),
                "restore always creates a safeguard"
            );
        } else {
            task.validate_plan(&plan, Some(&instance)).unwrap();
        }
    }
}
