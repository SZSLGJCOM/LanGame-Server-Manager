use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{
    assert_native_tool_available, openai_tool_response, serve_requirements_draft,
};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TaskWorkflowResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[tokio::test(flavor = "current_thread")]
async fn assistant_task_workflow_corrects_patch_then_preserves_contract_through_confirmation()
-> TaskWorkflowResult {
    assert_task_workflow(true).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_task_workflow_captures_model_selected_existing_target_before_authorizing_patch()
-> TaskWorkflowResult {
    assert_task_workflow(false).await
}

async fn assert_task_workflow(select_instance: bool) -> TaskWorkflowResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("task-contract-http");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    for (folder, metadata) in [
        (
            "consumer",
            "name='Consumer';mod_dependencies={{dependency=false}}",
        ),
        ("dependency", "name='Dependency';priority=10"),
    ] {
        let directory = Path::new(&settings.games_root)
            .join("dontstarve/mods")
            .join(folder);
        fs::create_dir_all(&directory)?;
        fs::write(directory.join("modinfo.lua"), metadata)?;
        fs::write(
            directory.join("modmain.lua"),
            "error('configuration-only fixture must not launch')",
        )?;
    }
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_module_instance(state.clone(), "dontstarve", "Contract fixture").await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let original_lua = "return {consumer={enabled=true,configuration_options={mode='normal'}}}";
    let repaired_lua = "return {consumer={enabled=true,configuration_options={mode='normal'}},dependency={enabled=true}}";
    let mut initial: Value = serde_json::from_str(&details.settings_json)?;
    initial["master_modoverrides_lua"] = json!(original_lua);
    initial["enable_caves"] = json!(false);
    initial["offline_cluster"] = json!(true);
    initial["cluster_description"] = json!("Keep this description");
    let before = update_instance_record(
        state.clone(),
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: initial.to_string(),
            ports: details.ports.clone(),
        },
    )
    .await?;
    let native_path = Path::new(&before.config_file_path)
        .parent()
        .ok_or("configuration parent missing")?
        .join("clusters/main/Master/modoverrides.lua");
    let original_native = fs::read_to_string(&native_path)?;
    assert_eq!(original_native.trim(), original_lua);

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let prompt = "Review the Mod configuration and apply a safe correction.";
    if !select_instance {
        assert!(
            infer_assistant_prompt_context_instance(prompt, &list_instances(&storage.paths).await?)
                .is_none(),
            "this case must bind the target from the model plan, not prompt inference"
        );
    }
    let serve = async {
        if select_instance {
            serve_requirements_draft(
                &listener,
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
            )
            .await?;
        }
        for index in 0..if select_instance { 2 } else { 1 } {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            assert_native_tool_available(&request, "propose_operation");
            assert!(request.to_string().contains("Application task contract"));
            if index == 1 {
                let prompt = request.to_string();
                assert!(prompt.contains("Response rejected; no operation was executed."));
                assert!(prompt.contains("initially enabled"));
            }
            assert_eq!(
                read_instance_details(&storage.paths, &created.summary.id)
                    .await?
                    .settings_json,
                before.settings_json,
                "neither rejected responses nor corrected previews may write settings"
            );
            assert_eq!(fs::read_to_string(&native_path)?, original_native);
            let plan = json!({"action":"customize_config", "instanceId":created.summary.id,
                "moduleId":"dontstarve", "settingsPatch":{"master_modoverrides_lua":if index == 0 { "return {}" } else { repaired_lua }},
                "reason":"Adjust the selected instance configuration."});
            let body = openai_tool_response(
                &format!("task-operation-{index}"),
                "propose_operation",
                plan,
            )
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
        }
        // The post-save start preview must be generated from the application
        // receipt; no third model request is available after this point.
        drop(listener);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let result = assistant_preview_operation_inner(
            state.clone(),
            AssistantExecuteOperationInput {
                task: AssistantTaskRequest {
                    goal: if select_instance {
                        AssistantTaskGoal::RestoreService
                    } else {
                        AssistantTaskGoal::ApplyChange
                    },
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: prompt.into(),
                context: None,
                selected_instance_id: select_instance.then(|| created.summary.id.clone()),
                selected_module_id: Some("dontstarve".into()),
            },
        )
        .await;
        if !select_instance {
            let error = result.expect_err(
                "capturing the model-selected target must reject clearing its existing Mods",
            );
            assert!(
                error.contains("initially enabled"),
                "unexpected failure: {error}"
            );
            return Ok::<_, Box<dyn std::error::Error>>(None);
        }
        let preview = result?;
        assert!(preview.requires_confirmation);
        assert_eq!(preview.action, AssistantOperationAction::CustomizeConfig);
        let task = preview
            .task
            .as_ref()
            .ok_or("preview task receipt missing")?;
        assert_eq!(task.goal, AssistantTaskGoal::RestoreService);
        assert_eq!(task.status, AssistantTaskStatus::Proposed);
        assert!(task.preserve_existing_mods);
        assert_eq!(
            task.instance_id.as_deref(),
            Some(created.summary.id.as_str())
        );
        let task_id = task.id.clone();
        assert_eq!(
            read_instance_details(&storage.paths, &created.summary.id)
                .await?
                .settings_json,
            before.settings_json
        );
        assert_eq!(fs::read_to_string(&native_path)?, original_native);
        let confirmation = AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider.clone(),
            confirmation_token: preview.confirmation_token.ok_or("patch token missing")?,
            plan_summary: preview.plan_summary.ok_or("patch summary missing")?,
        };
        let output = assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
        assert!(
            assistant_confirm_operation_inner(state.clone(), confirmation)
                .await
                .is_err(),
            "confirmation remains single-use"
        );
        let completed = output.task.as_ref().ok_or("saved task receipt missing")?;
        assert_eq!(completed.id, task_id);
        assert_eq!(completed.goal, AssistantTaskGoal::RestoreService);
        assert_eq!(
            completed.status,
            AssistantTaskStatus::Inconclusive,
            "saved settings alone do not complete restoration"
        );
        assert!(completed.preserve_existing_mods);
        let next = output
            .follow_up
            .as_ref()
            .ok_or("same-task start preview missing")?;
        assert_eq!(next.action, AssistantOperationAction::StartServer);
        assert!(next.requires_confirmation);
        let next_task = next
            .task
            .as_ref()
            .ok_or("start preview task receipt missing")?;
        assert_eq!(next_task.id, task_id);
        assert_eq!(next_task.goal, AssistantTaskGoal::RestoreService);
        assert!(next_task.preserve_existing_mods);
        assert_eq!(
            next_task.instance_id.as_deref(),
            Some(created.summary.id.as_str())
        );
        let _unused_start = take_assistant_pending_operation(
            next.confirmation_token
                .as_deref()
                .ok_or("start token missing")?,
            next.plan_summary
                .as_deref()
                .ok_or("start summary missing")?,
            &provider,
        )?;
        Ok(Some(output))
    };
    let (_, output) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(Box::pin(serve), Box::pin(exercise))
    })
    .await??;
    let saved = read_instance_details(&storage.paths, &created.summary.id).await?;
    if let Some(output) = output {
        assert!(output.handled);
        assert!(!output.requires_confirmation);
        assert_eq!(output.applied_settings_keys, ["master_modoverrides_lua"]);
        let mut expected: Value = serde_json::from_str(&before.settings_json)?;
        expected["master_modoverrides_lua"] = json!(repaired_lua);
        assert_eq!(
            serde_json::from_str::<Value>(&saved.settings_json)?,
            expected
        );
        assert_eq!(fs::read_to_string(&native_path)?.trim(), repaired_lua);
        let verification = output.verification.ok_or("write verification missing")?;
        assert_eq!(
            verification.status,
            AssistantVerificationStatus::Inconclusive
        );
        assert_eq!(verification.evidence["launchReady"], true);
        assert!(verification.can_continue);
    } else {
        assert_eq!(saved.settings_json, before.settings_json);
        assert_eq!(fs::read_to_string(&native_path)?, original_native);
    }
    assert!(saved.active_run.is_none());
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .unwrap()
            .is_tracked(&created.summary.id)
    );
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    Ok(())
}
