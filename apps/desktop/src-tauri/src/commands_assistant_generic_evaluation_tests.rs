use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{assert_native_tool_available, openai_tool_response};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const FILES: [&str; 2] = [
    "data/plugins/generic-synthetic-service/bridge.json",
    "data/plugins/generic-synthetic-service/retry.toml",
];
const ORIGINALS: [&str; 2] = ["{\"listenPort\": }\r\n", "retry_limit =\r\n"];
const REPAIRED: [&str; 2] = ["{\"listenPort\": 8088}\r\n", "retry_limit = 3\r\n"];

#[path = "commands_assistant_generic_live_tests.rs"]
mod live;

// Scripted native-tool protocol evaluation, not a live model capability score or
// proof of a real plugin's runtime behavior. Neither fixture has a game adapter.
#[tokio::test(flavor = "current_thread")]
async fn generic_synthetic_service_files_are_read_validated_previewed_and_repaired() -> TestResult {
    exercise(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn generic_synthetic_second_file_conflict_rejects_the_complete_edit_set() -> TestResult {
    exercise(true).await
}

fn tool_data(request: &Value, id: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let message = request["messages"]
        .as_array()
        .ok_or("tool messages")?
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == id)
        .ok_or_else(|| format!("missing result for {id}"))?;
    let result: Value =
        serde_json::from_str(message["content"].as_str().ok_or("tool result content")?)?;
    assert_eq!(result["ok"], true, "{id}: {result}");
    Ok(result["data"].clone())
}

async fn serve_scripted_investigation(listener: TcpListener) -> TestResult {
    for step in 0..6 {
        let (mut stream, _) = listener.accept().await?;
        let request = read_repair_model_request(&mut stream).await?;
        let (id, name, arguments) = match step {
            0 => (
                "generic-list",
                "list_instance_files",
                json!({"directory":"data/plugins"}),
            ),
            1 => {
                let listing = tool_data(&request, "generic-list")?;
                for file in FILES {
                    assert!(listing.to_string().contains(file));
                }
                (
                    "generic-read-json",
                    "read_instance_file",
                    json!({"file":FILES[0]}),
                )
            }
            2 => {
                let source = tool_data(&request, "generic-read-json")?;
                assert_eq!(source["content"], ORIGINALS[0]);
                assert_eq!(source["editable"], true);
                (
                    "generic-validate-json",
                    "validate_instance_file",
                    json!({"file":FILES[0]}),
                )
            }
            3 => {
                let validation = tool_data(&request, "generic-validate-json")?;
                assert_eq!(validation["status"], "invalid");
                assert_eq!(validation["issues"][0]["code"], "json_syntax");
                (
                    "generic-read-toml",
                    "read_instance_file",
                    json!({"file":FILES[1]}),
                )
            }
            4 => {
                let source = tool_data(&request, "generic-read-toml")?;
                assert_eq!(source["content"], ORIGINALS[1]);
                assert_eq!(source["editable"], true);
                (
                    "generic-validate-toml",
                    "validate_instance_file",
                    json!({"file":FILES[1]}),
                )
            }
            _ => {
                let validation = tool_data(&request, "generic-validate-toml")?;
                assert_eq!(validation["status"], "invalid");
                assert_eq!(validation["issues"][0]["code"], "toml_syntax");
                let json_source = tool_data(&request, "generic-read-json")?;
                let toml_source = tool_data(&request, "generic-read-toml")?;
                (
                    "generic-propose",
                    "propose_operation",
                    json!({
                        "action":"patch_instance_files", "filePatches":[
                            {"file":FILES[0], "sourceSha256":json_source["sourceSha256"],
                                "edits":[{"before":ORIGINALS[0],"after":REPAIRED[0]}]},
                            {"file":FILES[1], "sourceSha256":toml_source["sourceSha256"],
                                "edits":[{"before":ORIGINALS[1],"after":REPAIRED[1]}]}
                        ], "reason":"Repair the two observed syntax errors in the synthetic service fixture."
                    }),
                )
            }
        };
        assert_native_tool_available(&request, name);
        let body = openai_tool_response(id, name, arguments).to_string();
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await?;
        stream.shutdown().await?;
    }
    Ok(())
}

async fn exercise(conflict: bool) -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("generic-service-evaluation");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_minecraft_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(
            &bootstrap_storage().expect("bootstrap isolated fixture storage"),
        ))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let instance = create_fake_module_instance(
        state.clone(),
        "minecraft",
        "Generic synthetic service fixture",
    )
    .await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &instance.summary.id).await?;
    let instance_root = Path::new(&details.config_file_path)
        .parent()
        .and_then(Path::parent)
        .ok_or("instance root")?;
    for (file, content) in FILES.into_iter().zip(ORIGINALS) {
        let path = instance_root.join(file);
        fs::create_dir_all(path.parent().ok_or("fixture parent")?)?;
        fs::write(path, content)?;
    }
    assert!(serde_json::from_str::<Value>(ORIGINALS[0]).is_err());
    assert!(toml::from_str::<toml::Value>(ORIGINALS[1]).is_err());
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let work = async {
        let preview = assistant_preview_operation_inner(state.clone(), AssistantExecuteOperationInput {
            task: commands_assistant_ops::AssistantTaskRequest {
                goal: commands_assistant_ops::AssistantTaskGoal::ApplyChange, preserve_existing_mods: true,
            },
            settings: provider.clone(),
            prompt:"Inspect the synthetic service's JSON and TOML files, fix their syntax, set listenPort to 8088 and retry_limit to 3. Preserve other files.".into(),
            context:None, selected_instance_id:Some(instance.summary.id.clone()), selected_module_id:Some("minecraft".into()),
        }).await?;
        assert_eq!(preview.action, AssistantOperationAction::PatchInstanceFiles);
        assert_eq!(preview.file_change_previews.len(), 2);
        for (index, change) in preview.file_change_previews.iter().enumerate() {
            assert_eq!(change.file, FILES[index]);
            assert_eq!(change.edits[0].before, ORIGINALS[index]);
            assert_eq!(change.edits[0].after, REPAIRED[index]);
            assert_eq!(
                fs::read_to_string(instance_root.join(FILES[index]))?,
                ORIGINALS[index],
                "preview cannot write"
            );
        }
        let confirmation = AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: preview.conversation_id.clone(),
            settings: provider,
            confirmation_token: preview.confirmation_token.ok_or("confirmation token")?,
            plan_summary: preview.plan_summary.ok_or("plan summary")?,
        };
        if conflict {
            fs::write(instance_root.join(FILES[1]), "retry_limit = 9\r\n")?;
        }
        let output = assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
        if conflict {
            assert_eq!(
                output.task.as_ref().ok_or("task")?.status,
                AssistantTaskStatus::Failed
            );
            assert!(
                output.file_changes_result.is_none(),
                "preflight rejected the set before writes"
            );
            assert_eq!(
                fs::read_to_string(instance_root.join(FILES[0]))?,
                ORIGINALS[0]
            );
            assert_eq!(
                fs::read_to_string(instance_root.join(FILES[1]))?,
                "retry_limit = 9\r\n"
            );
            assert!(!instance_root.join("data/.langame/file-patches").exists());
            assert!(output.follow_up.is_none());
        } else {
            assert_eq!(
                output.task.as_ref().ok_or("task")?.status,
                AssistantTaskStatus::Completed,
                "{:?}",
                output.verification
            );
            let result = output
                .file_changes_result
                .as_ref()
                .ok_or("file-set receipt")?;
            assert_eq!(
                result.status,
                app_storage::InstanceFilePatchesStatus::Applied
            );
            assert_eq!(result.files.len(), 2);
            for (index, receipt) in result.files.iter().enumerate() {
                assert!(receipt.read_back_verified);
                assert_eq!(
                    receipt.source_sha256,
                    preview.file_change_previews[index].source_sha256
                );
                assert_eq!(
                    receipt.result_sha256,
                    preview.file_change_previews[index].result_sha256
                );
                assert_eq!(
                    fs::read_to_string(instance_root.join(FILES[index]))?,
                    REPAIRED[index]
                );
                let backup = instance_root
                    .join("data/.langame/file-patches")
                    .join(receipt.backup_id.as_ref().ok_or("backup ID")?);
                assert_eq!(
                    fs::read_to_string(backup.join("original"))?,
                    ORIGINALS[index]
                );
                let validation = commands_assistant_ops::validate_assistant_workspace_file(
                    state.inner(),
                    &storage,
                    &details,
                    FILES[index],
                )
                .await?;
                assert_eq!(validation["status"], "valid");
                assert_eq!(validation["sourceSha256"], receipt.result_sha256);
            }
            assert_eq!(
                serde_json::from_str::<Value>(&fs::read_to_string(instance_root.join(FILES[0]))?)?
                    ["listenPort"],
                8088
            );
            assert_eq!(
                toml::from_str::<toml::Value>(&fs::read_to_string(instance_root.join(FILES[1]))?)?
                    ["retry_limit"]
                    .as_integer(),
                Some(3)
            );
        }
        assert!(
            read_instance_details(&storage.paths, &instance.summary.id)
                .await?
                .active_run
                .is_none()
        );
        assert!(
            assistant_confirm_operation_inner(state.clone(), confirmation)
                .await
                .is_err(),
            "confirmation remains single-use"
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(60), async {
        tokio::try_join!(serve_scripted_investigation(listener), work)
    })
    .await??;
    Ok(())
}
