use super::assistant_intent_workflow_tests::serve_call;
use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::openai_tool_response;
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type RequirementTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const REQUEST: &str = "Create a Minecraft server with 24 players on 127.0.0.1 and start it.";

fn requirements() -> Value {
    json!({"settings":[
        {"key":"max_players","expected":24,"sourceId":"request_1"},
        {"key":"bind_ip","expected":"127.0.0.1","sourceId":"request_1"}
    ],"ports":[],"forbiddenActions":[],"unverified":[]})
}

fn native_result(request: &Value, id: &str) -> RequirementTestResult<Value> {
    let result = request["messages"]
        .as_array()
        .ok_or("native messages missing")?
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .ok_or("native evidence result missing")?;
    Ok(serde_json::from_str(
        result["content"]
            .as_str()
            .ok_or("native result body missing")?,
    )?)
}

fn confirmation(
    output: &AssistantExecuteOperationOutput,
    provider: &AssistantProviderSettings,
) -> RequirementTestResult<AssistantConfirmOperationInput> {
    assert!(output.requires_confirmation);
    Ok(AssistantConfirmOperationInput {
        continue_task: false,
        conversation_id: output.conversation_id.clone(),
        settings: provider.clone(),
        confirmation_token: output
            .confirmation_token
            .clone()
            .ok_or("confirmation token missing")?,
        plan_summary: output
            .plan_summary
            .clone()
            .ok_or("confirmation summary missing")?,
    })
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_requirements_workflow_rejects_premature_start_and_removed_requirement()
-> RequirementTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("requirements-workflow");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let java = Path::new(&settings.games_root).join("minecraft/jre/bin/java.exe");
    fs::create_dir_all(java.parent().ok_or("fixture Java parent missing")?)?;
    fs::write(&java, "preflight-only fixture; never execute")?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let storage = bootstrap_storage()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        let initial = serve_call(
            &listener,
            "module-catalog",
            "list_module_settings",
            json!({"offset":0}),
            true,
        )
        .await?;
        assert!(initial.to_string().contains(REQUEST));
        assert!(list_instances(&storage.paths).await?.is_empty());
        let catalog_request = serve_call(
            &listener,
            "module-declarations",
            "read_module_settings",
            json!({"keys":["max_players","bind_ip"],"offset":0}),
            true,
        )
        .await?;
        let catalog = native_result(&catalog_request, "module-catalog")?;
        assert_eq!(catalog["ok"], true);
        assert_eq!(catalog["data"]["scope"], "module_schema");
        let keys = catalog["data"]["keys"]
            .as_array()
            .ok_or("module catalog keys missing")?;
        assert!(keys.contains(&json!("max_players")) && keys.contains(&json!("bind_ip")));
        let draft_request = serve_call(
            &listener,
            "requirements-record",
            "record_task_requirements",
            requirements(),
            true,
        )
        .await?;
        let schema = native_result(&draft_request, "module-declarations")?;
        assert_eq!(schema["ok"], true);
        assert_eq!(schema["data"]["scope"], "module_schema");
        assert_eq!(schema["data"]["moduleId"], "minecraft");
        assert_eq!(schema["data"]["instanceExists"], false);
        let entries = schema["data"]["entries"]
            .as_array()
            .ok_or("module declarations missing")?;
        assert!(entries.iter().any(|entry| entry["key"] == "max_players"
            && entry["source"] == "module"
            && entry["schema"]["type"] == "integer"));
        assert!(entries.iter().any(|entry| entry["key"] == "bind_ip"
            && entry["source"] == "manager"
            && entry["declaration"]["type"] == "string"));
        assert!(
            list_instances(&storage.paths).await?.is_empty(),
            "reading declarations and drafting must not create the server"
        );
        let finish_request = serve_call(
            &listener,
            "requirements-finish",
            "finish_task_requirements",
            json!({}),
            true,
        )
        .await?;
        let recorded = native_result(&finish_request, "requirements-record")?;
        assert_eq!(recorded["ok"], true);
        assert_eq!(recorded["data"]["errors"], json!([]));
        for request in [&draft_request, &finish_request] {
            assert!(
                request["tools"]
                    .as_array()
                    .ok_or("draft tools missing")?
                    .iter()
                    .all(|tool| tool["function"]["name"] != "propose_operation"),
                "a draft must finish before any operation can be proposed"
            );
        }
        for index in 0..6 {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?.to_string();
            assert!(request.contains(REQUEST));
            let instances = list_instances(&storage.paths).await?;
            let plan = if index == 0 {
                assert!(instances.is_empty());
                assert!(request.contains("module_schema"));
                json!({"action":"create_server","moduleId":"minecraft"})
            } else {
                assert_eq!(instances.len(), 1);
                let current = read_instance_details(&storage.paths, &instances[0].id).await?;
                assert!(current.active_run.is_none());
                assert!(state.pending_runtime_start_instance_ids()?.is_empty());
                let values: Value = serde_json::from_str(&current.settings_json)?;
                let action = match index {
                    1 => {
                        json!({"action":"customize_config","settingsPatch":{"bind_ip":"127.0.0.1"}})
                    }
                    2 => {
                        assert_eq!(values["max_players"], 20);
                        json!({"action":"start_server"})
                    }
                    3 => {
                        assert!(request.contains("Response rejected"));
                        assert!(request.contains("requirement_1"));
                        json!({"action":"start_server","taskRequirements":{
                            "settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}})
                    }
                    4 => {
                        assert!(
                            request.contains("application supplies the fixed task requirements")
                        );
                        assert_eq!(values["max_players"], 20);
                        json!({"action":"customize_config","settingsPatch":{"max_players":24}})
                    }
                    5 => {
                        assert_eq!(values["max_players"], 24);
                        assert_eq!(current.summary.bind_ip, "127.0.0.1");
                        json!({"action":"start_server"})
                    }
                    _ => unreachable!(),
                };
                let mut action = action;
                action["instanceId"] = json!(current.summary.id);
                action["moduleId"] = json!("minecraft");
                action
            };
            let body =
                openai_tool_response(&format!("operation-{index}"), "propose_operation", plan)
                    .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let preview = assistant_preview_operation_inner(
            state.clone(),
            AssistantExecuteOperationInput {
                task: AssistantTaskRequest {
                    goal: AssistantTaskGoal::LaunchService,
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: REQUEST.into(),
                context: None,
                selected_instance_id: None,
                selected_module_id: Some("minecraft".into()),
            },
        )
        .await?;
        let initial = preview.task.as_ref().ok_or("initial task missing")?;
        let task_id = initial.id.clone();
        let bound_requirements = serde_json::to_value(&initial.requirements)?;
        assert_eq!(initial.requirements.len(), 2);
        let summary = preview.plan_summary.as_deref().ok_or("summary missing")?;
        assert!(
            summary.contains("max_players")
                && summary.contains("24")
                && summary.contains("127.0.0.1")
        );
        let created =
            assistant_confirm_operation_inner(state.clone(), confirmation(&preview, &provider)?)
                .await?;
        let config = created
            .follow_up
            .as_ref()
            .ok_or("first configuration missing")?;
        let partial =
            assistant_confirm_operation_inner(state.clone(), confirmation(config, &provider)?)
                .await?;
        let task = partial.task.as_ref().ok_or("partial task missing")?;
        assert_eq!(task.status, AssistantTaskStatus::Inconclusive);
        assert!(task.checks.iter().any(|check| check.name == "requirement_1"
            && check.status == AssistantTaskCheckStatus::Failed));
        let remaining = partial
            .follow_up
            .as_ref()
            .ok_or("remaining configuration missing")?;
        assert_eq!(remaining.action, AssistantOperationAction::CustomizeConfig);
        let saved =
            assistant_confirm_operation_inner(state.clone(), confirmation(remaining, &provider)?)
                .await?;
        let task = saved.task.as_ref().ok_or("saved task missing")?;
        assert_eq!(task.id, task_id);
        assert_eq!(
            serde_json::to_value(&task.requirements)?,
            bound_requirements
        );
        assert_eq!(task.status, AssistantTaskStatus::Inconclusive);
        for id in ["requirement_1", "requirement_2"] {
            assert!(task.checks.iter().any(
                |check| check.name == id && check.status == AssistantTaskCheckStatus::Satisfied
            ));
        }
        let start = saved.follow_up.as_ref().ok_or("start preview missing")?;
        assert_eq!(start.action, AssistantOperationAction::StartServer);
        let cancelled = confirmation(start, &provider)?;
        take_assistant_pending_operation(
            &cancelled.confirmation_token,
            &cancelled.plan_summary,
            &provider,
        )?;
        let id = task
            .instance_id
            .as_deref()
            .ok_or("bound instance missing")?;
        assert!(
            read_instance_details(&storage.paths, id)
                .await?
                .active_run
                .is_none()
        );
        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
        assert_eq!(list_instances(&storage.paths).await?.len(), 1);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(45), async {
        tokio::try_join!(Box::pin(serve), Box::pin(exercise))
    })
    .await??;
    Ok(())
}
