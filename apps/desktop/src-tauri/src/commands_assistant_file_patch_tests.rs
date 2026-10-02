use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{
    assert_native_tool_available, openai_tool_response, serve_requirements_draft,
};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const FILE: &str = "data/ugc/Master/content/322330/123/modmain.lua";
const ORIGINAL: &str = "local value = 1\r\nreturn value\r\n";
const MODIFIED: &str = "local value = 2\r\nreturn value\r\n";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Apply,
    Conflict,
    Restore,
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_file_patch_reads_previews_confirms_backs_up_and_verifies() -> TestResult {
    exercise(Scenario::Apply).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_file_patch_rejects_external_edit_after_confirmation_preview() -> TestResult {
    exercise(Scenario::Conflict).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_file_patch_restore_previews_start_and_rejects_overwritten_patch() -> TestResult {
    exercise(Scenario::Restore).await
}

async fn exercise(scenario: Scenario) -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-file-patch");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let instance =
        create_fake_module_instance(state.clone(), "dontstarve", "File patch fixture").await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let mut values: Value = serde_json::from_str(&details.settings_json)?;
    values["offline_cluster"] = json!(true);
    values["enable_caves"] = json!(false);
    update_instance(
        &storage.paths,
        UpdateInstanceInput {
            id: details.summary.id.clone(),
            bind_ip: details.summary.bind_ip.clone(),
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: values.to_string(),
            ports: details.ports.clone(),
        },
    )
    .await?;
    if scenario == Scenario::Restore {
        let launch = preview_instance_launch(state.clone(), instance.summary.id.clone()).await?;
        assert!(launch.ready_to_launch, "{:?}", launch.validation_issues);
    }
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("fixture root")?;
    let path = instance_root.join(FILE);
    fs::create_dir_all(path.parent().ok_or("fixture file parent")?)?;
    fs::write(&path, ORIGINAL)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        if scenario == Scenario::Restore {
            serve_requirements_draft(
                &listener,
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
            )
            .await?;
        }
        for step in 0..3 {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            let (name, arguments) = match step {
                0 => ("list_instance_files", json!({})),
                1 => {
                    assert!(request.to_string().contains(FILE));
                    ("read_instance_file", json!({"file":FILE}))
                }
                _ => {
                    let message = request["messages"]
                        .as_array()
                        .ok_or("tool messages")?
                        .iter()
                        .rev()
                        .find(|message| message["role"] == "tool")
                        .ok_or("read result")?;
                    let data: Value = serde_json::from_str(
                        message["content"].as_str().ok_or("tool result text")?,
                    )?;
                    assert_eq!(data["ok"], true);
                    assert_eq!(data["data"]["content"], ORIGINAL);
                    (
                        "propose_operation",
                        json!({"action":"patch_instance_text", "textPatch":{
                        "file":FILE,"sourceSha256":data["data"]["sourceSha256"],
                        "before":ORIGINAL,"after":MODIFIED
                    },"reason":"Repair the observed private Mod constant."}),
                    )
                }
            };
            assert_native_tool_available(&request, name);
            let body = openai_tool_response(&format!("step-{step}"), name, arguments).to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
            stream.shutdown().await?;
        }
        drop(listener);
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let work = async {
        let preview = assistant_preview_operation_inner(
            state.clone(),
            AssistantExecuteOperationInput {
                task: commands_assistant_ops::AssistantTaskRequest {
                    goal: if scenario == Scenario::Restore {
                        commands_assistant_ops::AssistantTaskGoal::RestoreService
                    } else {
                        commands_assistant_ops::AssistantTaskGoal::ApplyChange
                    },
                    preserve_existing_mods: true,
                },
                settings: provider.clone(),
                prompt: "Change the private Mod constant from 1 to 2 after reading its source."
                    .into(),
                context: None,
                selected_instance_id: Some(instance.summary.id.clone()),
                selected_module_id: Some("dontstarve".into()),
            },
        )
        .await?;
        assert_eq!(
            fs::read_to_string(&path)?,
            ORIGINAL,
            "preview must not write"
        );
        let change = preview
            .file_change_preview
            .as_ref()
            .ok_or("file preview missing")?;
        assert_eq!(change.file, FILE);
        assert_eq!(change.before, ORIGINAL);
        assert_eq!(change.after, MODIFIED);
        let confirmation = AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider.clone(),
            confirmation_token: preview.confirmation_token.ok_or("confirmation token")?,
            plan_summary: preview.plan_summary.ok_or("plan summary")?,
        };
        if scenario == Scenario::Conflict {
            fs::write(&path, "external edit\r\n")?;
        }
        let output = assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
        let task = output.task.as_ref().ok_or("task receipt")?;
        if scenario == Scenario::Conflict {
            assert_eq!(task.status, AssistantTaskStatus::Failed);
            assert!(output.file_change_result.is_none());
            assert_eq!(fs::read_to_string(&path)?, "external edit\r\n");
        } else {
            assert_eq!(
                task.status,
                if scenario == Scenario::Restore {
                    AssistantTaskStatus::Inconclusive
                } else {
                    AssistantTaskStatus::Completed
                },
                "{:?}",
                output.verification
            );
            let result = output.file_change_result.as_ref().ok_or("file receipt")?;
            assert!(result.read_back_verified);
            assert_eq!(fs::read_to_string(&path)?, MODIFIED);
            let backup = instance_root
                .join("data/.langame/file-patches")
                .join(&result.backup_id);
            assert_eq!(fs::read_to_string(backup.join("original"))?, ORIGINAL);
            assert_eq!(result.source_sha256, change.source_sha256);
            assert_eq!(result.result_sha256, change.result_sha256);
            assert!(
                output
                    .verification
                    .as_ref()
                    .ok_or("verification")?
                    .run_id
                    .is_none()
            );
            assert!(
                read_instance_details(&storage.paths, &instance.summary.id)
                    .await?
                    .active_run
                    .is_none()
            );
            if scenario == Scenario::Restore {
                let start = output.follow_up.as_ref().ok_or_else(|| {
                    format!("start confirmation missing: {:?}", output.verification)
                })?;
                assert_eq!(start.action, AssistantOperationAction::StartServer);
                assert_eq!(start.task.as_ref().ok_or("start task")?.id, task.id);
                fs::write(&path, "update overwrote the patch\r\n")?;
                let error = assistant_confirm_operation_inner(
                    state.clone(),
                    AssistantConfirmOperationInput {
                        continue_task: false,
                        conversation_id: start.conversation_id.clone(),
                        settings: provider,
                        confirmation_token: start
                            .confirmation_token
                            .clone()
                            .ok_or("start token")?,
                        plan_summary: start.plan_summary.clone().ok_or("start summary")?,
                    },
                )
                .await
                .expect_err("a changed patch must stop the repair chain before launch");
                assert!(error.contains("changed after its patch"), "{error}");
                assert!(
                    read_instance_details(&storage.paths, &instance.summary.id)
                        .await?
                        .active_run
                        .is_none()
                );
                assert_eq!(fs::read_to_string(backup.join("original"))?, ORIGINAL);
            }
        }
        assert!(
            assistant_confirm_operation_inner(state.clone(), confirmation)
                .await
                .is_err(),
            "confirmation is single-use"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    // Keep nested debug poll frames within the default Windows test-thread stack.
    tokio::time::timeout(Duration::from_secs(60), async {
        tokio::try_join!(Box::pin(serve), Box::pin(work))
    })
    .await??;
    Ok(())
}
