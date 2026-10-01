use super::*;

#[test]
fn failed_file_set_cannot_continue_into_automatic_repair_or_start() {
    for status in [
        app_storage::InstanceFilePatchesStatus::NotApplied,
        app_storage::InstanceFilePatchesStatus::RolledBack,
        app_storage::InstanceFilePatchesStatus::Partial,
    ] {
        let result = app_storage::InstanceFilePatchesResult {
            status,
            files: Vec::new(),
            error: Some("write failed".into()),
        };
        assert!(!assistant_file_set_allows_follow_up(Some(&result)));
    }
    let mut result = app_storage::InstanceFilePatchesResult {
        status: app_storage::InstanceFilePatchesStatus::Applied,
        files: vec![app_storage::InstanceFilePatchOutcome {
            file: "data/plugins/service/config.json".into(),
            source_sha256: "a".repeat(64),
            result_sha256: "b".repeat(64),
            backup_id: Some("backup".into()),
            state: app_storage::InstanceFilePatchState::Applied,
            read_back_verified: false,
            error: None,
        }],
        error: None,
    };
    assert!(!assistant_file_set_allows_follow_up(Some(&result)));
    result.files[0].read_back_verified = true;
    assert!(assistant_file_set_allows_follow_up(Some(&result)));
}

#[test]
fn storage_failure_preserves_the_real_output_and_prevents_follow_up() {
    let mut output = assistant_operation_output(&assistant_safe_none_plan("file repair".into()), 0);
    output.file_changes_result = Some(app_storage::InstanceFilePatchesResult {
        status: app_storage::InstanceFilePatchesStatus::Partial,
        files: Vec::new(),
        error: Some("rollback failed".into()),
    });
    output.follow_up = Some(Box::new(assistant_operation_output(
        &assistant_safe_none_plan("start".into()),
        0,
    )));
    assistant_unrecorded_operation(&mut output, "disk write failed");
    assert_eq!(
        output.file_changes_result.as_ref().unwrap().status,
        app_storage::InstanceFilePatchesStatus::Partial
    );
    assert!(
        output
            .message
            .contains("Earlier file and runtime effects remain")
    );
    assert!(output.follow_up.is_none());
}

#[test]
fn task_grant_never_covers_install_download_admin_or_read_only_changes() {
    for action in [
        AssistantOperationAction::InstallServer,
        AssistantOperationAction::ValidateServer,
        AssistantOperationAction::InstallFunMod,
        AssistantOperationAction::InstallSiteMod,
        AssistantOperationAction::RunGmCommand,
        AssistantOperationAction::Broadcast,
        AssistantOperationAction::CreateServer,
        AssistantOperationAction::None,
    ] {
        assert!(!assistant_action_allows_continuation(
            action,
            AssistantTaskGoal::RestoreService
        ));
    }
    for action in [
        AssistantOperationAction::CustomizeConfig,
        AssistantOperationAction::PatchInstanceFiles,
        AssistantOperationAction::RepairPorts,
        AssistantOperationAction::StartServer,
    ] {
        assert!(!assistant_action_allows_continuation(
            action,
            AssistantTaskGoal::Inspect
        ));
        assert!(assistant_action_allows_continuation(
            action,
            AssistantTaskGoal::RestoreService
        ));
    }
    for goal in [
        AssistantTaskGoal::ApplyChange,
        AssistantTaskGoal::PrepareService,
    ] {
        assert!(!assistant_action_allows_continuation(
            AssistantOperationAction::StartServer,
            goal
        ));
    }
}

#[test]
fn task_grant_expires_on_target_revision_cancel_or_task_change() {
    let _guard = crate::commands::tests::command_smoke_lock().blocking_lock();
    let store = crate::assistant_sessions::AssistantSessionStore::default();
    let binding = AssistantSessionBinding {
        provider: "ollama".into(),
        model: "fixture".into(),
        base_url: "http://127.0.0.1:11434".into(),
        storage_identity: std::array::from_fn(|index| format!("fixture-{index}")),
    };
    let lease = store.begin(None, binding.clone(), true).unwrap();
    let session = lease.session();
    let (mut task, instance) = task_tests::task_fixture(true);
    std::sync::Arc::make_mut(&mut task).session = Some(session.clone());
    let input = AssistantExecuteOperationInput {
        settings: intent_tests::intent_input("fixture").settings,
        prompt: task.original_request.clone(),
        context: None,
        task: task.request.clone(),
        selected_instance_id: task.instance_id.clone(),
        selected_module_id: task.module_id.clone(),
    };
    let plan = AssistantOperationPlan {
        action: AssistantOperationAction::CustomizeConfig,
        instance_id: Some(instance.summary.id),
        module_id: Some(instance.summary.module_id),
        ..assistant_safe_none_plan("Observed repair".into())
    };
    let (token, _) =
        store_assistant_pending_operation(&input, plan, None, "reviewed".into(), 0, task).unwrap();
    let mut pending =
        take_assistant_pending_operation(&token, "reviewed", &input.settings).unwrap();
    let mut grant = AssistantTaskAuthorization::capture(&pending).unwrap();
    assert!(grant.allows(&pending));
    pending.plan.instance_id = Some("another-instance".into());
    assert!(!grant.allows(&pending));
    pending.plan.instance_id = input.selected_instance_id.clone();
    std::sync::Arc::make_mut(&mut pending.task).id = "another-task".into();
    assert!(!grant.allows(&pending));
    std::sync::Arc::make_mut(&mut pending.task).id = grant.task_id.clone();
    grant.expires_at = Instant::now();
    assert!(!grant.allows(&pending));
    grant.expires_at = Instant::now() + Duration::from_secs(1);
    drop(lease);
    let _new_turn = store.begin(Some(session.id()), binding, true).unwrap();
    assert!(!grant.allows(&pending));
}

#[test]
fn continuation_authorization_is_explicit_and_unavailable_to_model_plans() {
    let input: AssistantConfirmOperationInput = serde_json::from_value(json!({
        "settings":{"provider":"ollama","model":"fixture","baseUrl":"http://127.0.0.1:11434","apiKey":""},
        "confirmationToken":"reviewed", "planSummary":"scope"
    })).unwrap();
    assert!(!input.continue_task);
    let tool = assistant_operation_tool();
    assert!(tool.parameters["properties"].get("continueTask").is_none());
}

#[test]
fn malformed_nested_tool_arguments_receive_shape_feedback_without_echoing_payload() {
    let malformed = json!({"action":"patch_instance_files", "filePatches":"private evidence that is not an array"});
    let error = parse_assistant_operation_plan_response(&malformed.to_string()).unwrap_err();
    assert!(error.contains("native JSON array"));
    assert!(!error.contains("private evidence"));
    assert!(
        parse_assistant_operation_plan_response(
            &json!({"action":"patch_instance_files", "filePatches":[{"edits":"[]"}]}).to_string()
        )
        .is_err()
    );
}
