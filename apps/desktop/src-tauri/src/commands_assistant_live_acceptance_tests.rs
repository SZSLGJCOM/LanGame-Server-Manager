use super::*;

#[path = "commands_assistant_live_acceptance_fixtures.rs"]
mod fixtures;
use fixtures::*;

#[cfg(windows)]
#[path = "commands_assistant_live_launch_tests.rs"]
mod launch_tests;

#[cfg(windows)]
#[path = "commands_assistant_live_launch_contract_tests.rs"]
mod launch_contract_tests;

#[cfg(windows)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires LANGAME_ASSISTANT_LIVE=1, a local Ollama model and an owned DST package copy under LANGAME_SMOKE_RUNTIME_ROOT"]
async fn assistant_live_ollama_repairs_native_dst_world_configuration() -> LiveResult {
    run_live_acceptance(LiveFault::WorldConfiguration).await
}

#[cfg(windows)]
#[tokio::test(flavor = "current_thread")]
#[ignore = "requires LANGAME_ASSISTANT_LIVE=1, a local Ollama model and an owned DST package copy under LANGAME_SMOKE_RUNTIME_ROOT"]
async fn assistant_live_ollama_repairs_native_dst_missing_mod_dependency() -> LiveResult {
    run_live_acceptance(LiveFault::MissingModDependency).await
}

#[cfg(windows)]
async fn run_live_acceptance(fault: LiveFault) -> LiveResult {
    let _guard = command_smoke_lock().lock().await;
    let environment = LiveEnvironment::from_env()?;
    let run_root = real_smoke_support::allocate_smoke_run_root("ai")?;
    println!("ASSISTANT_LIVE_EVIDENCE={}", run_root.display());
    let mut evidence = json!({
        "scenario": fault.label(), "model": environment.provider.model,
        "provider": "ollama", "endpoint": environment.provider.base_url,
        "installRoot": environment.install_root,
        "steps": [], "nativeLogsRetained": true,
    });
    let elevation = app_platform_win::WindowsPlatform::current_process_is_elevated();
    evidence["preconditions"] = match &elevation {
        Ok(elevated) => json!({"windowsProcessElevated": elevated}),
        Err(error) => json!({"elevationProbeError": error}),
    };
    if elevation != Ok(true) {
        evidence["cleanup"] = json!({"status": "not_started"});
        let message = elevation.err().unwrap_or_else(|| String::from(
            "native acceptance requires an elevated Windows process for the production firewall workflow; no game or model generation was started and the test was not skipped",
        ));
        return complete_live_report(&run_root, &mut evidence, Err(message.into()), Ok(()));
    }
    let models = assistant_list_ollama_models(Some(environment.provider.base_url.clone())).await?;
    if !models.contains(&environment.provider.model) {
        return Err("the selected model must already be installed in local Ollama; acceptance never downloads models".into());
    }
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    save_app_settings(environment.settings(&run_root))?;
    let storage = bootstrap_storage()?;
    let desktop = DesktopState::default();
    {
        let mut app_state = desktop.app_state.write().unwrap();
        app_state.settings = storage.settings.clone();
        app_state.storage = storage.storage_status.clone();
    }
    let app = tauri::test::mock_builder()
        .manage(desktop)
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    sync_modules_to_storage(state.clone()).await?;
    let (before, mods) =
        create_faulted_instance(state.clone(), &storage, &environment, fault).await?;
    let cleanup = NativeCleanup {
        state: &state,
        instance_id: before.summary.id.clone(),
    };
    evidence["instanceId"] = json!(before.summary.id);
    write_evidence(&run_root, &evidence)?;
    let outcome = exercise_live_repair(
        &state,
        &storage,
        LiveRepairCase {
            environment: &environment,
            before: &before,
            mods: mods.as_ref(),
            fault,
        },
        &run_root,
        &mut evidence,
    )
    .await;
    let cleanup_result = cleanup
        .finalize(state.clone(), &storage, outcome.is_ok(), &mut evidence)
        .await;
    complete_live_report(&run_root, &mut evidence, outcome, cleanup_result)
}

#[cfg(windows)]
struct LiveRepairCase<'a> {
    environment: &'a LiveEnvironment,
    before: &'a InstanceDetails,
    mods: Option<&'a FixtureMods>,
    fault: LiveFault,
}

#[cfg(windows)]
async fn exercise_live_repair(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    case: LiveRepairCase<'_>,
    run_root: &Path,
    evidence: &mut Value,
) -> LiveResult {
    let LiveRepairCase {
        environment,
        before,
        mods,
        fault,
    } = case;
    let id = &before.summary.id;
    let failed_start = start_instance_process_with_evidence(
        None,
        state,
        storage,
        id.clone(),
        "manual",
        RuntimeStartPreconditions {
            world_start: None,
            instance: Some(before.clone()),
            file_changes: Vec::new(),
        },
    )
    .await;
    evidence["initialStart"] = match &failed_start {
        Ok(started) => json!({"unexpectedSuccess": started}),
        Err(error) => json!({"error": error.message, "log": error.log}),
    };
    let original_log = failed_start
        .as_ref()
        .err()
        .and_then(|failure| failure.log.clone());
    evidence["initialNativeLog"] = json!(original_log);
    write_evidence(run_root, evidence)?;
    if failed_start.is_ok() {
        return Err("the deliberately broken native game unexpectedly started; fixture is not a proven fault".into());
    }
    let original_log =
        original_log.ok_or("failed native startup did not retain its bound log receipt")?;
    if original_log.read_error.is_some()
        || !original_log
            .lines
            .iter()
            .any(|line| line.contains(fault.failure_marker()))
    {
        return Err(format!(
            "native log did not contain the required real failure marker {}",
            fault.failure_marker()
        )
        .into());
    }
    if read_active_instance_run(&storage.paths, id)
        .await?
        .is_some()
    {
        return Err("failed native startup left an active instance run".into());
    }
    let assistant_log = read_assistant_instance_runtime_log_snapshot(state, storage, id)
        .await?
        .ok_or("assistant's normal evidence reader cannot see the failed-start log")?;
    evidence["assistantInitialNativeLog"] = json!(assistant_log);
    write_evidence(run_root, evidence)?;
    if assistant_log.read_error.is_some()
        || !assistant_log
            .lines
            .iter()
            .any(|line| line.contains(fault.failure_marker()))
    {
        return Err(
            "assistant's normal evidence reader did not return the real native failure".into(),
        );
    }
    let prompt = match fault {
        LiveFault::WorldConfiguration => {
            "这个离线饥荒服务器启动失败了，请实际查看最近的终端错误和配置，找出原因并提出最小修复。每次改动先给我完整确认预览，确认后继续检查；需要启动验证时再给我确认。保持现有离线、单地上世界和局域网设置，不下载东西，不安装新软件。"
        }
        LiveFault::MissingModDependency => {
            "这个离线饥荒服务器启用本地模组后进不去了。请实际查看最近的终端错误和模组配置，找出原因并提出最小修复。保留已启用模组的功能，不要通过全部禁用模组掩盖问题。每一步先给我完整确认预览，确认后再检查；需要启动验证时再给我确认。保持离线，不联网下载，不修改游戏或模组代码。"
        }
    };
    let input = AssistantExecuteOperationInput {
        task: commands_assistant_ops::AssistantTaskRequest {
            goal: commands_assistant_ops::AssistantTaskGoal::RestoreService,
            preserve_existing_mods: true,
        },
        settings: environment.provider.clone(),
        prompt: prompt.into(),
        context: None,
        selected_instance_id: Some(id.clone()),
        selected_module_id: Some("dontstarve".into()),
    };
    evidence["request"] = json!(prompt);
    let mut preview = assistant_preview_operation_inner(state.clone(), input).await?;
    let task_id = preview
        .task
        .as_ref()
        .ok_or("native preview has no task receipt")?
        .id
        .clone();
    let mut verified = false;
    for step in
        0..assistant_operation_limit(commands_assistant_ops::AssistantTaskGoal::RestoreService)
    {
        let preview_record = json!({"step": step, "preview": preview});
        evidence["steps"]
            .as_array_mut()
            .unwrap()
            .push(preview_record);
        write_evidence(run_root, evidence)?;
        if !preview.requires_confirmation
            || preview.instance_id.as_deref() != Some(id.as_str())
            || preview.module_id.as_deref() != Some("dontstarve")
            || !matches!(
                preview.action,
                AssistantOperationAction::CustomizeConfig | AssistantOperationAction::StartServer
            )
        {
            return Err("model did not propose a confirmable configuration repair or start for the isolated instance".into());
        }
        let unchanged = read_instance_details(&storage.paths, id).await?;
        if step == 0 && unchanged.settings_json != before.settings_json {
            return Err("investigation or preview changed settings before confirmation".into());
        }
        let output = assistant_confirm_operation_with_verification(
            None,
            state.clone(),
            AssistantConfirmOperationInput {
                continue_task: false,
                conversation_id: preview.conversation_id.clone(),
                settings: environment.provider.clone(),
                confirmation_token: preview
                    .confirmation_token
                    .take()
                    .ok_or("missing confirmation token")?,
                plan_summary: preview
                    .plan_summary
                    .take()
                    .ok_or("missing full confirmation summary")?,
            },
        )
        .await?;
        let current = read_instance_details(&storage.paths, id).await?;
        let runtime = read_instance_runtime_overview(&storage.paths, id).await?;
        let step_record = evidence["steps"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        step_record["result"] = json!(output);
        step_record["persistedSettings"] = serde_json::from_str(&current.settings_json)?;
        step_record["runtime"] = json!(runtime);
        write_evidence(run_root, evidence)?;
        validate_isolation_after_repair(&current, before)?;
        let verification = output
            .verification
            .as_ref()
            .ok_or("confirmed operation did not return verification")?;
        let task = output
            .task
            .as_ref()
            .ok_or("confirmed operation has no task receipt")?;
        if task.id != task_id
            || task.goal != commands_assistant_ops::AssistantTaskGoal::RestoreService
            || !task.preserve_existing_mods
        {
            return Err("the repair changed its original task contract".into());
        }
        if verification.status == AssistantVerificationStatus::Verified {
            if task.status != commands_assistant_ops::AssistantTaskStatus::Completed
                || (mods.is_some()
                    && !task.checks.iter().any(|check| {
                        check.name == "required_mods_running"
                            && check.status
                                == commands_assistant_ops::AssistantTaskCheckStatus::Satisfied
                    }))
            {
                return Err("startup did not satisfy the production task completion checks".into());
            }
            if output.action != AssistantOperationAction::StartServer {
                return Err(
                    "a stopped configuration change was incorrectly reported as verified recovery"
                        .into(),
                );
            }
            let started = output
                .runtime_start
                .as_ref()
                .ok_or("verified recovery has no real start result")?;
            if verification.run_id != Some(started.run_id) || started.process_count != 1 {
                return Err("verification is not bound to the real single-Master run".into());
            }
            let active = read_active_instance_run(&storage.paths, id)
                .await?
                .ok_or("verified run is no longer active")?;
            if active.run_id != started.run_id {
                return Err("verified run does not match storage's active run".into());
            }
            let process = started
                .processes
                .first()
                .ok_or("started run has no process")?;
            let world = native_probe(
                state,
                id,
                process,
                started.run_id,
                "tostring(TheWorld ~= nil and TheWorld.ismastersim and TheWorld.Map:GetSize() > 0)",
            )
            .await?;
            evidence["nativeWorldLoaded"] = json!(world);
            if world != "true" {
                return Err("native world probe did not prove a loaded Master world".into());
            }
            if let Some(mods) = mods {
                let expression = format!(
                    "tostring(rawget(_G, '{DEPENDENCY_GLOBAL}') == true)..' '..tostring(rawget(_G, '{CONSUMER_GLOBAL}') == true)..' '..tostring(KnownModIndex:IsModEnabledAny({:?}))..' '..tostring(KnownModIndex:IsModEnabledAny({:?}))..' '..tostring(#ModManager.failedmods)",
                    mods.dependency, mods.consumer
                );
                let result = native_probe(state, id, process, started.run_id, &expression).await?;
                evidence["nativeModDependencyProbe"] = json!(result);
                if result != "true true true true 0" {
                    return Err("native engine did not load both dependency and consumer without failed mods".into());
                }
            }
            verified = true;
            write_evidence(run_root, evidence)?;
            break;
        }
        if step == 2 {
            break;
        }
        preview = *output
            .follow_up
            .ok_or("recovery remains unverified and has no confirmable continuation")?;
    }
    if !verified {
        return Err(
            "native game recovery was not verified within three separately confirmed operations"
                .into(),
        );
    }
    Ok(())
}

fn complete_live_report(
    run_root: &Path,
    evidence: &mut Value,
    outcome: LiveResult,
    cleanup: LiveResult,
) -> LiveResult {
    let mut errors = Vec::new();
    evidence["repairPassed"] = json!(outcome.is_ok());
    evidence["cleanup"]["succeeded"] = json!(cleanup.is_ok());
    if let Err(error) = outcome {
        evidence["failure"] = json!(error.to_string());
        errors.push(error.to_string());
    }
    if let Err(error) = cleanup {
        evidence["cleanup"]["failure"] = json!(error.to_string());
        errors.push(format!("cleanup failed: {error}"));
    }
    evidence["passed"] = json!(errors.is_empty());
    if let Err(error) = write_evidence(run_root, evidence) {
        errors.push(format!("writing acceptance evidence failed: {error}"));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n").into())
    }
}

fn validate_isolation_after_repair(
    current: &InstanceDetails,
    before: &InstanceDetails,
) -> LiveResult {
    let values: Value = serde_json::from_str(&current.settings_json)?;
    if values["offline_cluster"] != true
        || values["lan_only_cluster"] != true
        || values["enable_caves"] != false
        || values["cluster_token"] != ""
        || current.summary.bind_ip != "127.0.0.1"
        || serde_json::to_value(&current.ports)? != serde_json::to_value(&before.ports)?
    {
        return Err(
            "repair altered the isolated native acceptance network or shard boundary".into(),
        );
    }
    for key in [
        "shared_workshop_mod_ids",
        "shared_workshop_collection_ids",
        "master_enabled_workshop_mod_ids",
        "caves_enabled_workshop_mod_ids",
    ] {
        if values[key]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Err(
                "repair tried to introduce a Workshop download into local-only acceptance".into(),
            );
        }
    }
    Ok(())
}

fn write_evidence(run_root: &Path, evidence: &Value) -> LiveResult {
    fn remove_confirmation_tokens(value: &mut Value) {
        match value {
            Value::Object(values) => {
                values.remove("confirmationToken");
                for value in values.values_mut() {
                    remove_confirmation_tokens(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    remove_confirmation_tokens(value);
                }
            }
            _ => {}
        }
    }
    let mut record = evidence.clone();
    remove_confirmation_tokens(&mut record);
    let text = redact_assistant_provider_text(&serde_json::to_string_pretty(&record)?);
    fs::write(run_root.join("assistant-native-acceptance.json"), text)?;
    Ok(())
}

#[test]
fn native_mod_fixture_requires_its_real_lua_dependency() -> LiveResult {
    let mods = FixtureMods {
        dependency: "fixture_dependency".into(),
        consumer: "fixture_consumer".into(),
    };
    let lua = mlua::Lua::new();
    let globals = lua.globals();
    lua.load(
        "setmetatable(_G, { __index = function(_, name) error(\"variable '\" .. name .. \"' is not declared\", 2) end })",
    )
    .exec()?;
    // mods.lua creates a separate environment, exposing _G as GLOBAL but not
    // exporting error/rawget. util.lua runs each mod chunk in that environment.
    let mod_env = lua.create_table()?;
    mod_env.set("GLOBAL", globals.clone())?;
    assert!(
        lua.load("return error == nil and rawget == nil")
            .set_environment(mod_env.clone())
            .eval::<bool>()?
    );
    let undeclared = lua
        .load(format!("return GLOBAL.{DEPENDENCY_GLOBAL}"))
        .set_environment(mod_env.clone())
        .eval::<mlua::Value>()
        .unwrap_err();
    assert!(undeclared.to_string().contains("is not declared"));
    let error = lua
        .load(mods.consumer_script())
        .set_environment(mod_env.clone())
        .exec()
        .unwrap_err();
    assert!(error.to_string().contains(MISSING_DEPENDENCY_MARKER));
    assert!(!error.to_string().contains(&mods.dependency));
    assert!(!error.to_string().contains("is not declared"));
    assert!(globals.raw_get::<Option<bool>>(CONSUMER_GLOBAL)?.is_none());
    globals.raw_set(DEPENDENCY_GLOBAL, false)?;
    let disabled = lua
        .load(mods.consumer_script())
        .set_environment(mod_env.clone())
        .exec()
        .unwrap_err();
    assert!(disabled.to_string().contains(MISSING_DEPENDENCY_MARKER));
    assert!(globals.raw_get::<Option<bool>>(CONSUMER_GLOBAL)?.is_none());
    lua.load(FixtureMods::dependency_script())
        .set_environment(mod_env.clone())
        .exec()?;
    lua.load(mods.consumer_script())
        .set_environment(mod_env.clone())
        .exec()?;
    assert!(globals.get::<bool>(DEPENDENCY_GLOBAL)?);
    assert!(globals.get::<bool>(CONSUMER_GLOBAL)?);
    assert!(
        mod_env
            .raw_get::<Option<bool>>(DEPENDENCY_GLOBAL)?
            .is_none()
    );
    assert!(mod_env.raw_get::<Option<bool>>(CONSUMER_GLOBAL)?.is_none());
    Ok(())
}

#[test]
fn native_acceptance_rejects_installations_outside_owned_evidence() -> LiveResult {
    let root = temp_test_dir("live-owned");
    let evidence = root.join("evidence");
    let outside = root.join("unrelated-games");
    fs::create_dir_all(evidence.join("games/dontstarve/mods"))?;
    fs::create_dir_all(outside.join("dontstarve/mods"))?;
    assert!(validate_owned_install(&evidence, &outside).is_err());
    assert_eq!(
        validate_owned_install(&evidence, &evidence.join("games"))?,
        evidence.canonicalize()?.join("games/dontstarve")
    );
    fs::remove_dir_all(&root)?;
    Ok(())
}

#[test]
fn native_acceptance_preserves_primary_and_cleanup_failures_in_report() -> LiveResult {
    let root = temp_test_dir("live-report");
    let mut evidence = json!({"cleanup": {}});
    let error = complete_live_report(
        &root,
        &mut evidence,
        Err("original native failure".into()),
        Err("stop rejected".into()),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "original native failure\ncleanup failed: stop rejected"
    );
    let saved: Value = serde_json::from_str(&fs::read_to_string(
        root.join("assistant-native-acceptance.json"),
    )?)?;
    assert_eq!(saved["failure"], "original native failure");
    assert_eq!(saved["cleanup"]["failure"], "stop rejected");
    assert_eq!(saved["passed"], false);
    fs::remove_dir_all(root)?;
    Ok(())
}
