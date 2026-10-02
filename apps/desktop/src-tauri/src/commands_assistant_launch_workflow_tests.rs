use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{
    assert_native_tool_available, openai_tool_response, serve_requirements_draft,
};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

#[path = "commands_assistant_launch_proxy_fixture.rs"]
mod proxy_fixture;

type LaunchTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const LAUNCH_MODULE: &str = "minecraft";
const LAUNCH_PROMPT: &str = "Create a Minecraft server for 24 players bound to 127.0.0.1, then start it after confirmation.";

#[tokio::test(flavor = "current_thread")]
async fn assistant_launch_workflow_binds_created_instance_and_cancelled_start_stays_stopped()
-> LaunchTestResult {
    Box::pin(exercise_new_server_launch(false)).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_launch_workflow_first_start_failure_keeps_bound_task_and_saved_configuration()
-> LaunchTestResult {
    if proxy_fixture::is_child() {
        Box::pin(exercise_new_server_launch(true)).await
    } else {
        proxy_fixture::run_with_rejected_metadata().await
    }
}

fn launch_confirmation(
    output: &AssistantExecuteOperationOutput,
    provider: &AssistantProviderSettings,
) -> LaunchTestResult<AssistantConfirmOperationInput> {
    assert!(output.requires_confirmation);
    Ok(AssistantConfirmOperationInput {
        continue_task: false,
        conversation_id: output.conversation_id.clone(),
        settings: provider.clone(),
        confirmation_token: output
            .confirmation_token
            .clone()
            .ok_or("confirmation token missing")?,
        plan_summary: output.plan_summary.clone().ok_or("plan summary missing")?,
    })
}

fn launch_task_receipt(
    output: &AssistantExecuteOperationOutput,
) -> LaunchTestResult<&AssistantTaskReceipt> {
    let task = output.task.as_ref().ok_or("launch task receipt missing")?;
    assert_eq!(task.goal, AssistantTaskGoal::LaunchService);
    assert_eq!(
        task.operation_limit,
        assistant_operation_limit(AssistantTaskGoal::LaunchService)
    );
    assert!(task.preserve_existing_mods);
    Ok(task)
}

async fn exercise_new_server_launch(confirm_start: bool) -> LaunchTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("launch-workflow");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let java = Path::new(&settings.games_root).join("minecraft/jre/bin/java.exe");
    fs::create_dir_all(java.parent().ok_or("fixture Java parent missing")?)?;
    fs::write(&java, "preflight-only executable; must never be launched")?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let storage = bootstrap_storage()?;
    assert!(list_instances(&storage.paths).await?.is_empty());

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        serve_requirements_draft(
            &listener,
            json!({"settings":[
            {"key":"max_players","expected":24,"sourceId":"request_1"},
            {"key":"bind_ip","expected":"127.0.0.1","sourceId":"request_1"}
        ],"ports":[],"forbiddenActions":[],"unverified":[]}),
        )
        .await?;
        for index in 0..if confirm_start { 4 } else { 3 } {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            assert_native_tool_available(&request, "propose_operation");
            let prompt = request.to_string();
            assert!(prompt.contains(LAUNCH_PROMPT));
            assert!(prompt.contains("Application task contract"));
            let instances = list_instances(&storage.paths).await?;
            let plan = if index == 0 {
                assert!(
                    instances.is_empty(),
                    "a preview must not create an instance"
                );
                json!({"action":"create_server", "moduleId":LAUNCH_MODULE,
                    "reason":"Create the requested server before applying its settings."})
            } else {
                assert_eq!(instances.len(), 1);
                let created = read_instance_details(&storage.paths, &instances[0].id).await?;
                assert!(created.active_run.is_none());
                assert!(prompt.contains(&created.summary.id));
                let current: Value = serde_json::from_str(&created.settings_json)?;
                if index == 1 {
                    assert_eq!(current["max_players"], 20);
                    assert_eq!(created.summary.bind_ip, "0.0.0.0");
                    assert_eq!(current["bind_ip"], "0.0.0.0");
                    json!({"action":"customize_config", "moduleId":LAUNCH_MODULE,
                        "instanceId":created.summary.id, "settingsPatch":{"max_players":24,"bind_ip":"127.0.0.1"},
                        "reason":"Apply the requested player limit and bind address before starting."})
                } else {
                    assert_eq!(current["max_players"], 24);
                    assert_eq!(created.summary.bind_ip, "127.0.0.1");
                    assert_eq!(current["bind_ip"], "127.0.0.1");
                    if index == 2 {
                        assert!(java.exists(), "only launch preflight has run so far");
                        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
                        json!({"action":"start_server", "moduleId":LAUNCH_MODULE,
                            "instanceId":created.summary.id,
                            "reason":"The requested player limit and bind address are saved; start after confirmation."})
                    } else {
                        assert!(prompt.contains("launch_executable_missing"));
                        json!({"action":"none", "reason":"The required Java executable is missing; startup remains unverified."})
                    }
                }
            };
            let body = openai_tool_response(
                &format!("launch-operation-{index}"),
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
        drop(listener);
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
                prompt: LAUNCH_PROMPT.into(),
                context: None,
                selected_instance_id: None,
                selected_module_id: Some(LAUNCH_MODULE.into()),
            },
        )
        .await?;
        assert_eq!(preview.action, AssistantOperationAction::CreateServer);
        let task_id = launch_task_receipt(&preview)?.id.clone();
        assert!(launch_task_receipt(&preview)?.instance_id.is_none());
        assert!(list_instances(&storage.paths).await?.is_empty());
        let confirmation = launch_confirmation(&preview, &provider)?;
        let created =
            assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
        assert!(
            assistant_confirm_operation_inner(state.clone(), confirmation)
                .await
                .is_err(),
            "replaying creation confirmation must not create a second instance"
        );
        let task = launch_task_receipt(&created)?;
        assert_eq!(task.id, task_id);
        assert_eq!(task.status, AssistantTaskStatus::Inconclusive);
        let instance_id = task
            .instance_id
            .clone()
            .ok_or("created target was not bound")?;
        assert_eq!(created.instance_id.as_deref(), Some(instance_id.as_str()));
        let before = read_instance_details(&storage.paths, &instance_id).await?;
        assert_eq!(before.summary.bind_ip, "0.0.0.0");
        assert_eq!(
            serde_json::from_str::<Value>(&before.settings_json)?["max_players"],
            20
        );
        assert!(before.active_run.is_none());
        let configuration = created
            .follow_up
            .as_ref()
            .ok_or("configuration preview missing")?;
        assert_eq!(
            configuration.action,
            AssistantOperationAction::CustomizeConfig
        );
        assert_eq!(launch_task_receipt(configuration)?.id, task_id);
        assert_eq!(
            launch_task_receipt(configuration)?.instance_id.as_deref(),
            Some(instance_id.as_str())
        );
        let saved = assistant_confirm_operation_inner(
            state.clone(),
            launch_confirmation(configuration, &provider)?,
        )
        .await?;
        assert_eq!(launch_task_receipt(&saved)?.id, task_id);
        assert_eq!(
            launch_task_receipt(&saved)?.status,
            AssistantTaskStatus::Inconclusive
        );
        assert_eq!(saved.applied_settings_keys, ["bind_ip", "max_players"]);
        let after = read_instance_details(&storage.paths, &instance_id).await?;
        assert_eq!(after.summary.bind_ip, "127.0.0.1");
        assert_eq!(
            serde_json::from_str::<Value>(&after.settings_json)?["bind_ip"],
            "127.0.0.1"
        );
        let mut expected: Value = serde_json::from_str(&before.settings_json)?;
        expected["max_players"] = json!(24);
        expected["bind_ip"] = json!("127.0.0.1");
        assert_eq!(
            serde_json::from_str::<Value>(&after.settings_json)?,
            expected
        );
        assert!(after.active_run.is_none());
        let start = saved.follow_up.as_ref().ok_or("start preview missing")?;
        assert_eq!(start.action, AssistantOperationAction::StartServer);
        assert_eq!(launch_task_receipt(start)?.id, task_id);
        assert_eq!(
            launch_task_receipt(start)?.instance_id.as_deref(),
            Some(instance_id.as_str())
        );
        let start_confirmation = launch_confirmation(start, &provider)?;
        if confirm_start {
            // The child process's local proxy rejects automatic update metadata;
            // the configured instance and its missing-executable evidence survive.
            fs::remove_file(&java)?;
            let failed =
                assistant_confirm_operation_inner(state.clone(), start_confirmation).await?;
            assert!(
                failed
                    .runtime_start_failure
                    .as_ref()
                    .ok_or("native startup failure")?
                    .message
                    .contains("version_manifest_v2.json")
            );
            let jobs = state.app_state.read().unwrap().jobs.clone();
            assert!(
                jobs.iter()
                    .any(|job| job.target_id.as_deref() == Some(instance_id.as_str())
                        && matches!(job.status, JobStatus::Failed))
            );
            assert_eq!(launch_task_receipt(&failed)?.id, task_id);
            assert_eq!(
                launch_task_receipt(&failed)?.status,
                AssistantTaskStatus::Failed
            );
            assert!(failed.runtime_start.is_none());
            let verification = failed
                .verification
                .as_ref()
                .ok_or("startup verification missing")?;
            assert_eq!(verification.status, AssistantVerificationStatus::Failed);
            assert_ne!(verification.evidence["reason"], "no_bound_instance");
            assert!(
                verification
                    .evidence
                    .to_string()
                    .contains("launch_executable_missing")
            );
            assert!(failed.follow_up.is_none());
        } else {
            // Closing a preview never calls confirmation. Remove the unused token
            // only to release this test's pending record after checking its target.
            let _cancelled = take_assistant_pending_operation(
                &start_confirmation.confirmation_token,
                &start_confirmation.plan_summary,
                &provider,
            )?;
        }
        let final_details = read_instance_details(&storage.paths, &instance_id).await?;
        assert_eq!(final_details.summary.bind_ip, "127.0.0.1");
        assert_eq!(
            serde_json::from_str::<Value>(&final_details.settings_json)?,
            expected
        );
        assert!(final_details.active_run.is_none());
        assert!(
            !state
                .runtime_supervisor
                .lock()
                .unwrap()
                .is_tracked(&instance_id)
        );
        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
        assert_eq!(list_instances(&storage.paths).await?.len(), 1);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    // Separate the HTTP fixture and multi-operation scenario on the heap so their
    // nested debug poll frames fit the default Windows test-thread stack.
    let serve = Box::pin(serve);
    let exercise = Box::pin(exercise);
    tokio::time::timeout(Duration::from_secs(45), async {
        tokio::try_join!(serve, exercise)
    })
    .await??;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_launch_workflow_requires_explicit_module_selection_before_model_or_creation()
-> LaunchTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("launch-selection");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    isolated_smoke_app_settings(&root)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let storage = bootstrap_storage()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    for stale_instance in [None, Some("missing-selected-instance")] {
        let preview = assistant_preview_operation_inner(
            state.clone(),
            AssistantExecuteOperationInput {
                task: AssistantTaskRequest {
                    goal: AssistantTaskGoal::LaunchService,
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: LAUNCH_PROMPT.into(),
                context: None,
                selected_instance_id: stale_instance.map(str::to_owned),
                selected_module_id: stale_instance.map(|_| LAUNCH_MODULE.to_owned()),
            },
        );
        tokio::select! {
            accepted = listener.accept() => {
                let _connection = accepted?;
                panic!("missing or stale selection must be rejected before requesting a model");
            }
            result = tokio::time::timeout(Duration::from_secs(10), preview) => {
                let error = result?.expect_err("the prompt must not replace missing or stale selection");
                let expected = if stale_instance.is_some() { "selected instance" } else { "module" };
                assert!(error.to_lowercase().contains(expected), "unexpected rejection: {error}");
            }
        }
        assert!(list_instances(&storage.paths).await?.is_empty());
        assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    }
    Ok(())
}
