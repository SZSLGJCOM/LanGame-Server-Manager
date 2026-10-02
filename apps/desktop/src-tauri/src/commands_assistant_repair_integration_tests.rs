use super::assistant_tool_fixtures::{
    assert_native_tool_available, openai_tool_response, serve_requirements_draft,
};
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type RepairTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const REPAIR_FIXTURE_MODULE: &str = "minecraft";

pub(super) async fn read_repair_model_request(
    stream: &mut tokio::net::TcpStream,
) -> RepairTestResult<Value> {
    const MAX_REQUEST: usize = 256 * 1024;
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("model request ended before its headers".into());
        }
        request.extend_from_slice(&buffer[..count]);
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if request.len() > 16 * 1024 {
            return Err("model request headers exceed fixture limit".into());
        }
    };
    let headers = std::str::from_utf8(&request[..header_end])?;
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>())
        })
        .ok_or("model request must declare its body length")??;
    if content_length > MAX_REQUEST {
        return Err("model request exceeds fixture limit".into());
    }
    while request.len() < header_end + content_length {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            return Err("model request ended before its body".into());
        }
        request.extend_from_slice(&buffer[..count]);
        if request.len() > MAX_REQUEST + header_end {
            return Err("model request exceeds fixture limit".into());
        }
    }
    Ok(serde_json::from_slice(
        &request[header_end..header_end + content_length],
    )?)
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_repair_integration_previews_start_after_save_without_another_model_call()
-> RepairTestResult {
    assert_assistant_repair_after_saved_settings(true).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_repair_integration_keeps_saved_settings_when_failed_preflight_model_fails()
-> RepairTestResult {
    assert_assistant_repair_after_saved_settings(false).await
}

async fn assert_assistant_repair_after_saved_settings(launch_ready: bool) -> RepairTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("repair-model-failure");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let java = Path::new(&settings.games_root).join("minecraft/jre/bin/java.exe");
    if launch_ready {
        fs::create_dir_all(java.parent().ok_or("fixture Java directory missing")?)?;
        fs::write(
            &java,
            "fixture executable; this configuration-only test never launches it",
        )?;
    }
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_module_instance(state.clone(), REPAIR_FIXTURE_MODULE, "Saved repair").await?;
    let storage = bootstrap_storage()?;
    let before = read_instance_details(&storage.paths, &created.summary.id).await?;
    assert_eq!(
        serde_json::from_str::<Value>(&before.settings_json)?["max_players"],
        20
    );

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        serve_requirements_draft(
            &listener,
            json!({"settings":[
            {"key":"max_players","expected":24,"sourceId":"request_1"}
        ],"ports":[],"forbiddenActions":[],"unverified":[]}),
        )
        .await?;
        for index in 0..if launch_ready { 1 } else { 2 } {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            assert_native_tool_available(&request, "propose_operation");
            let (status, body) = if index == 0 {
                let plan = json!({"action": "customize_config", "settingsPatch": {"max_players": 24},
                    "reason": "Adjust the requested player limit."});
                (
                    "200 OK",
                    openai_tool_response("repair-operation", "propose_operation", plan).to_string(),
                )
            } else {
                assert!(
                    request
                        .to_string()
                        .contains("Previous operation verification")
                );
                let saved = read_instance_details(&storage.paths, &created.summary.id).await?;
                assert_eq!(
                    serde_json::from_str::<Value>(&saved.settings_json)?["max_players"],
                    24,
                    "the write must finish before the follow-up model request"
                );
                (
                    "503 Service Unavailable",
                    json!({"error": {"message": "fixture follow-up unavailable"}}).to_string(),
                )
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await?;
            stream.shutdown().await?;
        }
        // A successful save must produce the next confirmation even after the
        // provider goes away. Any attempted follow-up request now fails closed.
        drop(listener);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let confirm = async {
        let preview = assistant_preview_operation_inner(
            state.clone(),
            AssistantExecuteOperationInput {
                task: commands_assistant_ops::AssistantTaskRequest {
                    goal: commands_assistant_ops::AssistantTaskGoal::RestoreService,
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: String::from(
                    "Inspect the startup error and set max_players to 24 for this server.",
                ),
                context: None,
                selected_instance_id: Some(created.summary.id.clone()),
                selected_module_id: Some(REPAIR_FIXTURE_MODULE.into()),
            },
        )
        .await?;
        assert!(preview.requires_confirmation);
        assert_eq!(
            read_instance_details(&storage.paths, &created.summary.id)
                .await?
                .settings_json,
            before.settings_json
        );
        let input = AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider.clone(),
            confirmation_token: preview.confirmation_token.ok_or("preview token missing")?,
            plan_summary: preview.plan_summary.ok_or("preview summary missing")?,
        };
        let output = assistant_confirm_operation_inner(state.clone(), input.clone()).await?;
        assert!(
            assistant_confirm_operation_inner(state.clone(), input)
                .await
                .is_err(),
            "the completed save confirmation remains single-use"
        );
        Ok::<_, Box<dyn std::error::Error>>(output)
    };
    // Both futures own their network resources; cancellation closes the fixture
    // listener and socket even if the confirmation fails before its second call.
    let (_, output) = tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(Box::pin(serve), Box::pin(confirm))
    })
    .await??;
    assert!(output.handled);
    assert!(!output.requires_confirmation);
    if launch_ready {
        let next = output.follow_up.as_ref().ok_or("start preview missing")?;
        assert_eq!(next.action, AssistantOperationAction::StartServer);
        assert!(next.requires_confirmation);
        assert_eq!(
            next.instance_id.as_deref(),
            Some(created.summary.id.as_str())
        );
        assert_eq!(next.module_id.as_deref(), Some(REPAIR_FIXTURE_MODULE));
        let token = next
            .confirmation_token
            .as_deref()
            .ok_or("start token missing")?;
        let summary = next
            .plan_summary
            .as_deref()
            .ok_or("start summary missing")?;
        assert!(!summary.is_empty());
        let _registered_start = take_assistant_pending_operation(token, summary, &provider)?;
        assert!(take_assistant_pending_operation(token, summary, &provider).is_err());
    } else {
        assert!(output.follow_up.is_none());
    }
    assert_eq!(output.applied_settings_keys, ["max_players"]);
    let verification = output
        .verification
        .ok_or("completed write must retain its verification")?;
    assert_eq!(
        verification.status,
        if launch_ready {
            AssistantVerificationStatus::Inconclusive
        } else {
            AssistantVerificationStatus::Failed
        }
    );
    assert_eq!(verification.can_continue, launch_ready);
    if launch_ready {
        assert!(verification.evidence.get("investigationError").is_none());
    } else {
        assert!(
            verification.evidence["investigationError"]
                .as_str()
                .ok_or("model error missing")?
                .contains("503")
        );
    }
    assert_eq!(verification.evidence["launchReady"], launch_ready);
    let saved = read_instance_details(&storage.paths, &created.summary.id).await?;
    let mut expected: Value = serde_json::from_str(&before.settings_json)?;
    expected["max_players"] = json!(24);
    assert_eq!(
        serde_json::from_str::<Value>(&saved.settings_json)?,
        expected
    );
    let native = fs::read_to_string(
        Path::new(&saved.config_file_path)
            .parent()
            .ok_or("config parent missing")?
            .join("server.properties"),
    )?;
    assert!(native.contains("max-players=24"));
    assert!(native.contains("motd=Saved repair"));
    assert!(saved.active_run.is_none());
    assert!(
        !state
            .runtime_supervisor
            .lock()
            .unwrap()
            .is_tracked(&created.summary.id)
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_repair_integration_unbound_start_is_rejected_without_creating_an_instance()
-> RepairTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("repair-new-start-failure");
    let _env = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let storage = bootstrap_storage()?;
    assert!(list_instances(&storage.paths).await?.is_empty());
    let provider = stored_openai_compatible_ai_mock_settings();
    let input = AssistantExecuteOperationInput {
        task: Default::default(),
        settings: provider.clone(),
        prompt: String::from("Create and start a Minecraft server."),
        context: None,
        selected_instance_id: None,
        selected_module_id: Some(REPAIR_FIXTURE_MODULE.into()),
    };
    // Starting a server requires an existing instance, even with an unbound
    // confirmation record. Creation is a separate action.
    let plan = parse_assistant_operation_plan_response(
        &json!({
            "action": "start_server", "moduleId": REPAIR_FIXTURE_MODULE,
        })
        .to_string(),
    )?;
    assert!(
        plan.instance_id.is_none(),
        "this plan has no existing instance snapshot"
    );
    let summary = String::from("Create and start the Minecraft fixture");
    let (confirmation_token, _) = store_assistant_pending_operation(
        &input,
        plan,
        None,
        summary.clone(),
        0,
        std::sync::Arc::new(
            commands_assistant_ops::AssistantTaskContract::capture(&input, None)
                .expect("test task contract"),
        ),
    )?;
    let output = assistant_confirm_operation_inner(
        state.clone(),
        AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: None,
            settings: provider,
            confirmation_token,
            plan_summary: summary,
        },
    )
    .await?;
    let expected_error = "Create and configure an instance before requesting start_server.";
    assert!(output.handled);
    assert_eq!(output.action, AssistantOperationAction::StartServer);
    assert_eq!(output.message, expected_error);
    assert!(output.instance_id.is_none());
    assert_eq!(output.module_id.as_deref(), Some(REPAIR_FIXTURE_MODULE));
    assert!(!output.requires_confirmation);
    assert!(output.follow_up.is_none());
    assert!(output.runtime_start.is_none());
    assert!(output.runtime_start_failure.is_none());
    let verification = output.verification.as_ref().ok_or("verification missing")?;
    assert_eq!(verification.status, AssistantVerificationStatus::Failed);
    assert!(!verification.can_continue);
    assert!(verification.run_id.is_none());
    assert_eq!(verification.evidence["reason"], "no_bound_instance");
    assert_eq!(verification.evidence["operationError"], expected_error);
    let task = output.task.as_ref().ok_or("task receipt missing")?;
    assert_eq!(
        task.status,
        commands_assistant_ops::AssistantTaskStatus::Failed
    );
    assert!(task.instance_id.is_none());
    let check = task
        .checks
        .iter()
        .find(|check| check.name == "bound_target")
        .ok_or("bound target check missing")?;
    assert_eq!(
        check.status,
        commands_assistant_ops::AssistantTaskCheckStatus::Failed
    );
    assert_eq!(check.evidence["error"], expected_error);
    assert!(
        !Path::new(&settings.games_root)
            .join("minecraft/jre/bin/java.exe")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(Path::new(&settings.games_root).join("minecraft/server.jar"))?,
        "fake minecraft server jar"
    );
    let instances = list_instances(&storage.paths).await?;
    assert!(
        instances.is_empty(),
        "start_server must not provision a new instance"
    );
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    Ok(())
}
