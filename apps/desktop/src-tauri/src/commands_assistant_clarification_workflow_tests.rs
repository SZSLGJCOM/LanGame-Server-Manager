use super::assistant_intent_workflow_tests::{confirmation, serve_call};
use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{
    assert_native_tool_available, openai_tool_response, serve_requirements_draft,
};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreparationFiles {
    Available,
    RemovedBeforeSave,
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_clarification_prepares_requested_capacity_without_starting_or_reusing_selected_server()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_preparation(PreparationFiles::Available).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_preparation_rechecks_missing_executable_before_reporting_completion()
-> Result<(), Box<dyn std::error::Error>> {
    exercise_preparation(PreparationFiles::RemovedBeforeSave).await
}

async fn exercise_preparation(files: PreparationFiles) -> Result<(), Box<dyn std::error::Error>> {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-clarification-context");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let old = create_fake_module_instance(state.clone(), "dontstarve", "Preserved server").await?;
    let storage = bootstrap_storage()?;
    let before = read_instance_details(&storage.paths, &old.summary.id).await?;
    let native_before = fs::read(&before.config_file_path)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let earlier_request = "只创建一个12人服，不要启动。";
    let current_request = "饥荒";
    let conversation_id = seed_assistant_session(
        &state,
        &provider,
        earlier_request,
        "要创建哪个游戏的服务器？",
    )?;
    let serve = async {
        let resolver = serve_call(
            &listener,
            "continue-intent",
            "resolve_task",
            json!({
                "goal":"prepare_service","target":"new_instance","instanceId":null,
                "moduleId":"dontstarve","preserveExistingMods":true,"clarification":null,
                "priorRequestIds":["prior-1"]
            }),
            true,
        )
        .await?;
        assert!(resolver.to_string().contains(earlier_request));
        assert!(resolver.to_string().contains(current_request));
        serve_requirements_draft(
            &listener,
            json!({
                "settings":[{"key":"max_players","expected":12,"sourceId":"request_2"}],
                "ports":[],"forbiddenActions":[{"action":"start_server","sourceId":"request_3"}],
                "unverified":[]
            }),
        )
        .await?;
        let planner = serve_call(&listener, "create-preview", "propose_operation", json!({
            "action":"create_server","moduleId":"dontstarve",
            "reason":"Create the requested instance before configuring its player limit. Do not start it."
        }), true).await?;
        let messages = planner["messages"]
            .as_array()
            .ok_or("planner messages missing")?;
        assert!(
            messages.iter().any(|message| message["role"] == "user"
                && message["content"]
                    .as_str()
                    .is_some_and(|content| content.contains(earlier_request)
                        && content.contains(current_request))),
            "the operation planner must retain the exact original constraints alongside the clarification answer"
        );
        assert_eq!(
            list_instances(&storage.paths).await?.len(),
            1,
            "interpreting and previewing the continued request must not create an instance"
        );
        let (mut stream, _) = listener.accept().await?;
        let request = read_repair_model_request(&mut stream).await?;
        assert_native_tool_available(&request, "propose_operation");
        assert!(
            request["tools"]
                .as_array()
                .ok_or("native tools missing")?
                .iter()
                .all(|tool| tool["function"]["name"] != "resolve_task"),
            "the confirmed preparation must not reclassify its intent"
        );
        let instances = list_instances(&storage.paths).await?;
        assert_eq!(instances.len(), 2);
        let new = instances
            .iter()
            .find(|instance| instance.id != old.summary.id)
            .ok_or("prepared instance missing")?;
        let body = openai_tool_response("configure-capacity", "propose_operation", json!({
            "action":"customize_config","instanceId":new.id,"moduleId":"dontstarve",
            "settingsPatch":{"max_players":12},"reason":"Save the requested player capacity without starting the server."
        })).to_string();
        stream.write_all(format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ).as_bytes()).await?;
        stream.shutdown().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let preview = assistant_request_operation_inner(
            None,
            state.clone(),
            AssistantRequestInput {
                conversation_id: Some(conversation_id.clone()),
                settings: provider.clone(),
                prompt: current_request.into(),
                prior_requests: Vec::new(),
                conversation_messages: Vec::new(),
                context: Some("Assistant previously asked which game to create.".into()),
                selected_instance_id: Some(old.summary.id.clone()),
                selected_module_id: Some("dontstarve".into()),
            },
        )
        .await?;
        assert_eq!(preview.action, AssistantOperationAction::CreateServer);
        assert_eq!(
            preview.conversation_id.as_deref(),
            Some(conversation_id.as_str())
        );
        assert!(preview.requires_confirmation);
        let task = preview
            .task
            .as_ref()
            .ok_or("continued task receipt missing")?;
        assert_eq!(
            task.goal,
            AssistantTaskGoal::PrepareService,
            "create-only intent must not acquire a startup requirement after clarification"
        );
        assert!(task.instance_id.is_none());
        assert_eq!(task.module_id.as_deref(), Some("dontstarve"));
        assert!(preview.follow_up.is_none());
        let task_id = task.id.clone();
        let requirements = serde_json::to_value(&task.requirements)?;
        let created =
            assistant_confirm_operation_inner(state.clone(), confirmation(&preview, &provider)?)
                .await?;
        let receipt = created
            .task
            .as_ref()
            .ok_or("created task receipt missing")?;
        assert_eq!(receipt.id, task_id);
        assert_eq!(
            receipt.status,
            AssistantTaskStatus::Inconclusive,
            "creating defaults does not satisfy the requested player capacity: {:?}",
            created.verification
        );
        let new_id = receipt
            .instance_id
            .clone()
            .ok_or("created instance not bound")?;
        assert_ne!(new_id, old.summary.id);
        let next = created
            .follow_up
            .as_ref()
            .ok_or("configuration preview missing")?;
        assert_eq!(next.action, AssistantOperationAction::CustomizeConfig);
        assert_eq!(
            next.task.as_ref().ok_or("configuration task missing")?.id,
            task_id
        );
        if files == PreparationFiles::RemovedBeforeSave {
            prepare_fake_dontstarve_install(&settings)?;
            let prepared = read_instance_details(&storage.paths, &new_id).await?;
            let program_root = commands_runtime_lifecycle::private_runtime_install_root(&prepared)?;
            fs::remove_file(
                Path::new(&program_root)
                    .join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
            )?;
            assert!(
                Path::new(&commands_runtime_lifecycle::private_runtime_install_root(
                    &before
                )?)
                .join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe")
                .is_file(),
                "another healthy instance must not hide the prepared instance's missing executable"
            );
            assert!(
                Path::new(&settings.games_root)
                    .join("dontstarve/bin64/dontstarve_dedicated_server_nullrenderer_x64.exe")
                    .is_file(),
                "a healthy library must not hide the prepared instance's missing executable"
            );
        }
        let saved =
            assistant_confirm_operation_inner(state.clone(), confirmation(next, &provider)?)
                .await?;
        let receipt = saved.task.as_ref().ok_or("prepared task receipt missing")?;
        assert_eq!(receipt.id, task_id);
        assert_eq!(receipt.goal, AssistantTaskGoal::PrepareService);
        assert_eq!(
            receipt.status,
            if files == PreparationFiles::Available {
                AssistantTaskStatus::Completed
            } else {
                AssistantTaskStatus::Failed
            }
        );
        assert_eq!(serde_json::to_value(&receipt.requirements)?, requirements);
        for check in &receipt.checks {
            assert_eq!(
                check.status,
                if files == PreparationFiles::RemovedBeforeSave
                    && check.name == "prepared_files_ready"
                {
                    AssistantTaskCheckStatus::Failed
                } else {
                    AssistantTaskCheckStatus::Satisfied
                },
                "{}",
                check.name
            );
        }
        assert!(
            !receipt
                .checks
                .iter()
                .any(|check| check.name == "new_run_ready")
        );
        assert!(
            saved.follow_up.is_none(),
            "preparation must not produce a startup preview"
        );
        let verification = saved
            .verification
            .as_ref()
            .ok_or("preparation verification missing")?;
        assert_eq!(
            verification.status,
            if files == PreparationFiles::Available {
                AssistantVerificationStatus::Verified
            } else {
                AssistantVerificationStatus::Failed
            }
        );
        assert!(verification.run_id.is_none());
        assert!(!verification.can_continue);
        assert!(
            verification
                .summary
                .contains("No start operation was performed")
        );
        let prepared = read_instance_details(&storage.paths, &new_id).await?;
        assert_eq!(
            serde_json::from_str::<Value>(&prepared.settings_json)?["max_players"],
            12
        );
        assert!(prepared.active_run.is_none());
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(Box::pin(serve), Box::pin(exercise))
    })
    .await??;
    assert_eq!(list_instances(&storage.paths).await?.len(), 2);
    assert_eq!(
        read_instance_details(&storage.paths, &old.summary.id)
            .await?
            .settings_json,
        before.settings_json
    );
    assert_eq!(fs::read(&before.config_file_path)?, native_before);
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    Ok(())
}
