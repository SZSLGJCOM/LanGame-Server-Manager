use super::assistant_repair_integration_tests::read_repair_model_request;
use super::assistant_tool_fixtures::{assert_native_tool_available, openai_tool_response};
use super::*;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Change {
    None,
    Backup,
    CurrentSaves,
    PreservedMods,
    ReplaceMods,
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_backup_creation_and_restore_require_confirmation_and_verify_real_files()
-> TestResult {
    exercise(Change::None).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restore_rejects_modified_backup_after_preview() -> TestResult {
    exercise(Change::Backup).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restore_preserves_new_saves_written_after_preview() -> TestResult {
    exercise(Change::CurrentSaves).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restore_rejects_losing_protected_mods_before_replacing_saves() -> TestResult {
    exercise(Change::PreservedMods).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_restore_can_recover_backup_mods_when_preservation_is_not_requested() -> TestResult
{
    exercise(Change::ReplaceMods).await
}

async fn preview(
    state: tauri::State<'_, DesktopState>,
    instance_id: &str,
    backup_id: Option<&str>,
    preserve_existing_mods: bool,
) -> Result<(AssistantExecuteOperationOutput, AssistantProviderSettings), Box<dyn std::error::Error>>
{
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let action = if backup_id.is_some() {
        "restore_backup"
    } else {
        "create_backup"
    };
    let serve = async {
        let read_steps = usize::from(backup_id.is_some());
        for index in 0..read_steps + 4 {
            let (mut stream, _) = listener.accept().await?;
            let request = read_repair_model_request(&mut stream).await?;
            let (tool, arguments) = if backup_id.is_some() && index == 0 {
                ("list_backups", json!({"offset":0}))
            } else if index == read_steps + 1 {
                (
                    "record_task_requirements",
                    json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
                )
            } else if index == read_steps + 2 {
                ("finish_task_requirements", json!({}))
            } else {
                if index == read_steps
                    && let Some(backup_id) = backup_id
                {
                    let tool_result = request["messages"]
                        .as_array()
                        .ok_or("messages")?
                        .iter()
                        .rev()
                        .find(|message| message["role"] == "tool")
                        .ok_or("backup evidence")?;
                    let data: Value =
                        serde_json::from_str(tool_result["content"].as_str().ok_or("tool text")?)?;
                    assert_eq!(data["ok"], true);
                    assert!(
                        data["data"]["backups"]
                            .as_array()
                            .ok_or("backup list")?
                            .iter()
                            .any(|backup| backup["backupId"] == backup_id)
                    );
                    assert!(!data.to_string().contains("backup_path"));
                }
                let mut args = json!({"action":action,"reason":"Perform the explicitly requested save operation."});
                if let Some(backup_id) = backup_id {
                    args["backupId"] = json!(backup_id);
                }
                ("propose_operation", args)
            };
            assert_native_tool_available(&request, tool);
            let body =
                openai_tool_response(&format!("backup-{index}"), tool, arguments).to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
            stream.shutdown().await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let work = assistant_preview_operation_inner(
        state,
        AssistantExecuteOperationInput {
            task: commands_assistant_ops::AssistantTaskRequest {
                goal: commands_assistant_ops::AssistantTaskGoal::ApplyChange,
                preserve_existing_mods,
            },
            settings: provider.clone(),
            prompt: format!(
                "Please {action} for this stopped instance {}.",
                backup_id.unwrap_or_default()
            ),
            context: Some(r#"{"interfaceLanguage":"zh-CN"}"#.into()),
            selected_instance_id: Some(instance_id.into()),
            selected_module_id: Some("dontstarve".into()),
        },
    );
    let ((), preview) = tokio::time::timeout(Duration::from_secs(45), async {
        tokio::try_join!(Box::pin(serve), async {
            work.await.map_err(Into::<Box<dyn std::error::Error>>::into)
        })
    })
    .await??;
    Ok((preview, provider))
}

async fn exercise(change: Change) -> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-backup-workflow");
    let _environment = ProgramDataEnvGuard::set(&root.join("programdata"));
    let settings = isolated_smoke_app_settings(&root)?;
    prepare_fake_dontstarve_install(&settings)?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::default())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let created =
        create_fake_module_instance(state.clone(), "dontstarve", "Backup workflow fixture").await?;
    let storage = bootstrap_storage()?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let saves = Path::new(&details.saves_path);
    let world_relative = Path::new("Master/save/session/ASSISTANT-BACKUP/0000000001");
    let world = saves.join(world_relative);
    fs::create_dir_all(world.parent().ok_or("world session directory")?)?;
    fs::write(
        saves.join("Master/save/shardindex"),
        b"return { session_id = 'ASSISTANT-BACKUP', enabled_mods = {} }",
    )?;
    fs::write(world.with_extension("meta"), b"synthetic snapshot metadata")?;
    fs::write(&world, b"earlier save")?;
    let cluster_notes = saves.join("operator-notes.txt");
    fs::write(&cluster_notes, b"earlier operator notes")?;
    let (create_preview, provider) =
        preview(state.clone(), &created.summary.id, None, true).await?;
    assert!(create_preview.requires_confirmation);
    assert!(create_preview.message.starts_with("待确认："));
    assert!(
        create_preview
            .plan_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("Backup workflow fixture")
                && summary.contains("创建当前存档的备份"))
    );
    assert_eq!(
        create_preview.action,
        AssistantOperationAction::CreateBackup
    );
    assert!(
        app_storage::list_instance_backups(&storage.paths, &created.summary.id)
            .await?
            .is_empty(),
        "preview must not create backups"
    );
    let confirmation = AssistantConfirmOperationInput {
        continue_task: false,
        conversation_id: create_preview.conversation_id,
        settings: provider,
        confirmation_token: create_preview.confirmation_token.ok_or("create token")?,
        plan_summary: create_preview.plan_summary.ok_or("create summary")?,
    };
    let output = assistant_confirm_operation_inner(state.clone(), confirmation.clone()).await?;
    assert_eq!(
        output.task.as_ref().ok_or("create task")?.status,
        AssistantTaskStatus::Completed,
        "{:?}",
        output.verification
    );
    assert!(output.follow_up.is_none());
    assert!(
        output
            .message
            .starts_with("已为「Backup workflow fixture」创建存档备份")
    );
    assert!(
        assistant_confirm_operation_inner(state.clone(), confirmation)
            .await
            .is_err(),
        "one-use confirmation"
    );
    let backups = app_storage::list_instance_backups(&storage.paths, &created.summary.id).await?;
    let backup = backups.first().ok_or("new backup")?;
    assert_eq!(
        fs::read(
            Path::new(&backup.backup_path)
                .join("saves")
                .join(world_relative)
        )?,
        b"earlier save"
    );
    fs::write(&world, b"current save")?;
    fs::write(&cluster_notes, b"current operator notes")?;
    let protected_settings = if matches!(change, Change::PreservedMods | Change::ReplaceMods) {
        let current = read_instance_details(&storage.paths, &created.summary.id).await?;
        let mut values: Value = serde_json::from_str(&current.settings_json)?;
        values["master_modoverrides_lua"] =
            json!("return {consumer={enabled=true,configuration_options={mode='normal'}}}");
        Some(
            update_instance(
                &storage.paths,
                UpdateInstanceInput {
                    id: current.summary.id,
                    bind_ip: current.summary.bind_ip,
                    auto_backup_on_stop: current.auto_backup_on_stop,
                    backup_retention_count: current.backup_retention_count,
                    settings_json: values.to_string(),
                    ports: current.ports,
                },
            )
            .await?
            .settings_json,
        )
    } else {
        None
    };
    let (restore_preview, provider) = preview(
        state.clone(),
        &created.summary.id,
        Some(&backup.backup_id),
        change != Change::ReplaceMods,
    )
    .await?;
    assert!(restore_preview.requires_confirmation);
    let summary = restore_preview.plan_summary.ok_or("restore summary")?;
    assert!(summary.contains(&backup.backup_id));
    assert!(summary.contains("Backup workflow fixture") && summary.contains("创建时间："));
    assert!(summary.contains(&format!("{} 个文件", backup.file_count)));
    assert!(summary.contains(&format!("{} 字节", backup.total_bytes)));
    assert!(!summary.contains("SHA256") && !summary.contains("sourceSha256"));
    assert_eq!(
        fs::read(&world)?,
        b"current save",
        "preview cannot replace current saves"
    );
    match change {
        Change::Backup => fs::write(
            Path::new(&backup.backup_path)
                .join("saves")
                .join(world_relative),
            b"changed save",
        )?,
        Change::CurrentSaves => fs::write(&world, b"new progress")?,
        Change::None | Change::PreservedMods | Change::ReplaceMods => {}
    }
    let output = assistant_confirm_operation_inner(
        state.clone(),
        AssistantConfirmOperationInput {
            continue_task: false,
            conversation_id: restore_preview.conversation_id,
            settings: provider,
            confirmation_token: restore_preview.confirmation_token.ok_or("restore token")?,
            plan_summary: summary,
        },
    )
    .await?;
    if matches!(change, Change::None | Change::ReplaceMods) {
        assert!(
            output
                .message
                .starts_with("已为「Backup workflow fixture」恢复备份"),
            "restore output: {}; task: {:?}; verification: {:?}",
            output.message,
            output.task,
            output.verification
        );
        assert_eq!(
            output.task.as_ref().ok_or("restore task")?.status,
            AssistantTaskStatus::Completed,
            "{:?}",
            output.verification
        );
        assert_eq!(fs::read(&world)?, b"earlier save");
        let evidence = &output.verification.as_ref().ok_or("verification")?.evidence;
        let safeguard_id = evidence["safeguardBackupId"]
            .as_str()
            .ok_or("safeguard receipt")?;
        let backups =
            app_storage::list_instance_backups(&storage.paths, &created.summary.id).await?;
        let safeguard = backups
            .iter()
            .find(|backup| backup.backup_id == safeguard_id)
            .ok_or("retained safeguard")?;
        assert_eq!(
            fs::read(
                Path::new(&safeguard.backup_path)
                    .join("saves")
                    .join(world_relative)
            )?,
            b"current save"
        );
    } else {
        assert_eq!(
            output.task.as_ref().ok_or("failed task")?.status,
            AssistantTaskStatus::Failed
        );
        assert_eq!(
            fs::read(&world)?,
            if matches!(change, Change::Backup | Change::PreservedMods) {
                b"current save"
            } else {
                b"new progress"
            }
        );
    }
    if change == Change::PreservedMods {
        assert_eq!(
            read_instance_details(&storage.paths, &created.summary.id)
                .await?
                .settings_json,
            protected_settings.ok_or("protected settings")?
        );
        assert_eq!(
            app_storage::list_instance_backups(&storage.paths, &created.summary.id)
                .await?
                .len(),
            1,
            "a rejected Mod change must not begin the restore or create its safeguard"
        );
    }
    if change == Change::ReplaceMods {
        let settings = read_instance_details(&storage.paths, &created.summary.id)
            .await?
            .settings_json;
        let values: Value = serde_json::from_str(&settings)?;
        assert!(
            !values["master_modoverrides_lua"]
                .as_str()
                .ok_or("restored Mod script")?
                .contains("consumer")
        );
    }
    assert_eq!(
        fs::read(&cluster_notes)?,
        b"current operator notes",
        "DST world restoration must retain unrelated cluster-root files"
    );
    assert!(output.follow_up.is_none());
    assert!(
        read_instance_details(&storage.paths, &created.summary.id)
            .await?
            .active_run
            .is_none()
    );
    Ok(())
}
