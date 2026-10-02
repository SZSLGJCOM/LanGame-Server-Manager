use super::*;

#[tokio::test(flavor = "current_thread")]
async fn assistant_resolved_preview_keeps_b_when_a_is_only_a_reference() -> IntentTestResult {
    exercise_bound_target(false).await
}

#[tokio::test(flavor = "current_thread")]
async fn assistant_repair_follow_up_keeps_b_when_a_is_explicitly_excluded() -> IntentTestResult {
    exercise_bound_target(true).await
}

async fn exercise_bound_target(restore: bool) -> IntentTestResult {
    let _guard = command_smoke_lock().lock().await;
    let root = temp_test_dir("assistant-bound-target-reference");
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
    let a = create_fake_module_instance(state.clone(), "dontstarve", "ArchiveExample").await?;
    let b = create_fake_module_instance(state.clone(), "dontstarve", "BoundServer").await?;
    let storage = bootstrap_storage()?;
    let a_before = read_instance_details(&storage.paths, &a.summary.id).await?;
    let a_native = fs::read(&a_before.config_file_path)?;
    let mut b_before = read_instance_details(&storage.paths, &b.summary.id).await?;
    let mut values: Value = serde_json::from_str(&b_before.settings_json)?;
    values["offline_cluster"] = json!(true);
    values["enable_caves"] = json!(false);
    b_before = update_instance_record(
        state.clone(),
        UpdateInstanceInput {
            id: b.summary.id.clone(),
            bind_ip: b_before.summary.bind_ip.clone(),
            auto_backup_on_stop: b_before.auto_backup_on_stop,
            backup_retention_count: b_before.backup_retention_count,
            settings_json: values.to_string(),
            ports: b_before.ports.clone(),
        },
    )
    .await?;
    let prompt = if restore {
        "不要修改 ArchiveExample，修复选中的服务器，把描述改成 Bound verified，并核验启动。"
    } else {
        "ArchiveExample 只是参考，把当前选中的服务器描述改成 Bound verified，不要启动。"
    };
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let mut provider = stored_openai_compatible_ai_mock_settings();
    provider.base_url = format!("http://{}/v1", listener.local_addr()?);
    let serve = async {
        serve_call(
            &listener,
            "bound-intent",
            "resolve_task",
            resolution(
                if restore {
                    "restore_service"
                } else {
                    "apply_change"
                },
                "existing_instance",
                Some(&b.summary.id),
            ),
            true,
        )
        .await?;
        if restore {
            serve_requirements_draft(
                &listener,
                json!({"settings":[],"ports":[],"forbiddenActions":[],"unverified":[]}),
            )
            .await?;
        }
        serve_call(&listener, "bound-operation", "propose_operation", json!({
            "action":"customize_config","instanceId":b.summary.id,"moduleId":"dontstarve",
            "settingsPatch":{"cluster_description":"Bound verified"},"reason":"Change only the resolved server."
        }), true).await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let exercise = async {
        let preview = assistant_request_operation_inner(
            None,
            state.clone(),
            AssistantRequestInput {
                conversation_id: None,
                settings: provider.clone(),
                prompt: prompt.into(),
                prior_requests: Vec::new(),
                conversation_messages: Vec::new(),
                context: None,
                selected_instance_id: Some(b.summary.id.clone()),
                selected_module_id: Some("dontstarve".into()),
            },
        )
        .await?;
        assert!(preview.requires_confirmation);
        assert!(
            preview.conversation_id.is_some(),
            "the target preview belongs to a backend session"
        );
        assert_eq!(preview.instance_id.as_deref(), Some(b.summary.id.as_str()));
        assert_eq!(
            read_instance_details(&storage.paths, &a.summary.id)
                .await?
                .settings_json,
            a_before.settings_json
        );
        assert_eq!(
            read_instance_details(&storage.paths, &b.summary.id)
                .await?
                .settings_json,
            b_before.settings_json
        );
        let confirmed =
            assistant_confirm_operation_inner(state.clone(), confirmation(&preview, &provider)?)
                .await?;
        assert_eq!(confirmed.conversation_id, preview.conversation_id);
        assert_eq!(
            confirmed.instance_id.as_deref(),
            Some(b.summary.id.as_str())
        );
        if restore {
            let follow_up = confirmed
                .follow_up
                .as_ref()
                .ok_or("bound repair must produce its start preview")?;
            assert_eq!(follow_up.action, AssistantOperationAction::StartServer);
            assert_eq!(follow_up.conversation_id, preview.conversation_id);
            assert_eq!(
                follow_up.instance_id.as_deref(),
                Some(b.summary.id.as_str())
            );
            let pending = confirmation(follow_up, &provider)?;
            take_assistant_pending_operation(
                &pending.confirmation_token,
                &pending.plan_summary,
                &provider,
            )?;
        } else {
            assert!(confirmed.follow_up.is_none());
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    tokio::time::timeout(Duration::from_secs(30), async {
        tokio::try_join!(serve, exercise)
    })
    .await??;
    let a_after = read_instance_details(&storage.paths, &a.summary.id).await?;
    let b_after = read_instance_details(&storage.paths, &b.summary.id).await?;
    assert_eq!(a_after.settings_json, a_before.settings_json);
    assert_eq!(fs::read(&a_before.config_file_path)?, a_native);
    let saved: Value = serde_json::from_str(&b_after.settings_json)?;
    assert_eq!(saved["cluster_description"], "Bound verified");
    assert!(a_after.active_run.is_none() && b_after.active_run.is_none());
    assert!(state.pending_runtime_start_instance_ids()?.is_empty());
    Ok(())
}
