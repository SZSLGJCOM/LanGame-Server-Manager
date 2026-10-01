use super::*;
use crate::commands::tests::native_firewall::NativeFirewall;

#[path = "commands_assistant_live_launch_config.rs"]
mod config;
use config::validate_new_launch_configuration;

#[path = "commands_assistant_live_network.rs"]
mod network;

#[path = "commands_assistant_live_launch_program.rs"]
mod program;

#[path = "commands_assistant_live_work.rs"]
mod work;

#[derive(Clone, Copy)]
pub(super) enum NativeLaunchDriver {
    Ollama,
    Configured,
    ScriptedLocalContract,
}

#[derive(Default)]
struct NativeLaunchCleanup<'a> {
    process: Option<NativeCleanup<'a>>,
    firewall: Option<NativeFirewall>,
}

struct NativeLaunchBudget {
    operation_limit: usize,
    deadline: tokio::time::Instant,
}

impl NativeLaunchDriver {
    fn operation_limit(self) -> usize {
        match self {
            // Bound this authorized remote trial separately from the production
            // task lifetime; existing Ollama and contract scenarios keep theirs.
            Self::Configured => 6,
            Self::Ollama | Self::ScriptedLocalContract => {
                assistant_operation_limit(commands_assistant_ops::AssistantTaskGoal::LaunchService)
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires elevated opt-in native acceptance, owned DST files and the saved authorized DeepSeek credential"]
async fn assistant_live_saved_deepseek_launches_new_native_dst_server() -> LiveResult {
    let _guard = command_smoke_lock().lock().await;
    let environment = LiveEnvironment::from_saved_deepseek()?;
    let run_root = real_smoke_support::allocate_smoke_run_root("ai")?;
    Box::pin(run_native_new_launch(
        &environment,
        &run_root,
        &mut Value::Null,
        NativeLaunchDriver::Configured,
    ))
    .await
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "requires elevated opt-in native acceptance, owned DST files and a local Ollama model"]
async fn assistant_live_ollama_launches_new_native_dst_server() -> LiveResult {
    let _guard = command_smoke_lock().lock().await;
    let environment = LiveEnvironment::from_env()?;
    let run_root = real_smoke_support::allocate_smoke_run_root("ai")?;
    Box::pin(run_native_new_launch(
        &environment,
        &run_root,
        &mut Value::Null,
        NativeLaunchDriver::Ollama,
    ))
    .await
}

pub(super) async fn run_native_new_launch(
    environment: &LiveEnvironment,
    run_root: &Path,
    evidence: &mut Value,
    driver: NativeLaunchDriver,
) -> LiveResult {
    println!("ASSISTANT_LIVE_EVIDENCE={}", run_root.display());
    let provider_label = match driver {
        NativeLaunchDriver::Ollama => "ollama",
        NativeLaunchDriver::Configured => environment.provider.provider.as_str(),
        NativeLaunchDriver::ScriptedLocalContract => "scripted-local-contract",
    };
    let provider_mode = match driver {
        NativeLaunchDriver::Ollama => "local_ollama_model",
        NativeLaunchDriver::Configured => "configured_saved_credential",
        NativeLaunchDriver::ScriptedLocalContract => "scripted_local_contract",
    };
    let operation_limit = driver.operation_limit();
    *evidence = json!({"scenario":"new-server", "model":environment.provider.model,
        "provider":provider_label, "providerMode":provider_mode,
        "endpoint":environment.provider.base_url, "maxOperations":operation_limit,
        "firewall":{"productionWorkflow":true,"newInstanceRulesRequireCleanup":true},
        "steps":[], "nativeLogsRetained":true, "cleanup":{}});
    let preflight = async {
        if app_platform_win::WindowsPlatform::current_process_is_elevated() != Ok(true) {
            return Err("native launch acceptance requires an elevated process".into());
        }
        // Native startup executes the production firewall workflow, even with
        // loopback binding. Rules added for this isolated instance need cleanup.
        if matches!(driver, NativeLaunchDriver::Ollama) {
            let models =
                assistant_list_ollama_models(Some(environment.provider.base_url.clone())).await?;
            if !models.contains(&environment.provider.model) {
                return Err("native launch acceptance never downloads an Ollama model".into());
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    }
    .await;
    if let Err(error) = preflight {
        evidence["cleanup"] = json!({"status":"not_started"});
        return finish_launch_report(run_root, evidence, Err(error), Ok(()));
    }
    let _env_guard = ProgramDataEnvGuard::set(&run_root.join("programdata"));
    let prepared = async {
        save_app_settings(environment.settings(run_root))?;
        let storage = bootstrap_storage()?;
        let app = tauri::test::mock_builder()
            .manage(DesktopState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
        sync_modules_to_storage(app.state::<DesktopState>()).await?;
        let version = fs::read_to_string(environment.install_root.join("version.txt"))?;
        app_storage::sync_game_installs(
            &storage.paths,
            &[app_storage::GameInstallSyncRecord {
                module_id: "dontstarve".into(),
                install_root: environment.install_root.to_string_lossy().into_owned(),
                install_state: InstallState::Installed,
                current_version: Some(version.trim().into()),
                mark_verified: true,
            }],
        )
        .await?;
        if !list_instances(&storage.paths).await?.is_empty() {
            return Err("native launch requires an empty isolated instance database".into());
        }
        Ok::<_, Box<dyn std::error::Error>>((app, storage))
    }
    .await;
    let (app, storage) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return finish_launch_report(run_root, evidence, Err(error), Ok(())),
    };
    let state = app.state::<DesktopState>();
    evidence["preconditions"] = json!({"ownedPackageAlreadyInstalled":true,
        "installVerifiedBeforeProvider":true, "initialInstanceCount":0});
    write_evidence(run_root, evidence)?;
    let mut cleanup = NativeLaunchCleanup::default();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
    let operation = Box::pin(exercise_new_launch(
        &state,
        &storage,
        environment,
        run_root,
        evidence,
        &mut cleanup,
        NativeLaunchBudget {
            operation_limit,
            deadline,
        },
    ));
    let outcome = work::drain_live_work(deadline, operation).await;
    // Creation may commit before confirmation's continuation returns an error.
    // This unique store was checked empty before any model call; recover its
    // single new instance even if the detail/result read never reached us.
    let cleanup_recovery: LiveResult = if cleanup.process.is_none() {
        match list_instances(&storage.paths).await {
            Ok(instances) if instances.is_empty() => Ok(()),
            Ok(instances) if instances.len() == 1 && instances[0].module_id == "dontstarve" => {
                cleanup.process = Some(NativeCleanup {
                    state: state.inner(),
                    instance_id: instances[0].id.clone(),
                });
                evidence["cleanupOwnerRecoveredFromIsolatedDatabase"] = json!(true);
                Ok(())
            }
            Ok(_) => Err("isolated cleanup recovery found unexpected instance ownership".into()),
            Err(error) => Err(error.into()),
        }
    } else {
        Ok(())
    };
    let process_cleanup = match (cleanup_recovery, cleanup.process) {
        (Err(error), _) => {
            evidence["cleanup"] = json!({"status":"failed", "error":error.to_string()});
            Err(error)
        }
        (Ok(()), Some(cleanup)) => {
            cleanup
                .finalize(state.clone(), &storage, outcome.is_ok(), evidence)
                .await
        }
        (Ok(()), None) => {
            evidence["cleanup"] = json!({"status":"not_started"});
            Ok(())
        }
    };
    // Always attempt both cleanup boundaries, including failed native starts.
    let firewall_result = match cleanup.firewall.as_mut() {
        Some(firewall) => {
            let result = firewall.finish();
            evidence["firewall"]["cleanup"] = json!({
                "status":if result.is_ok() { "complete" } else { "failed" },
                "error":result.as_ref().err(),
            });
            result.map_err(|error| error.into())
        }
        None => {
            evidence["firewall"]["cleanup"] = json!({"status":"not_started"});
            Ok(())
        }
    };
    let cleanup_result: LiveResult = match (process_cleanup, firewall_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(process), Err(firewall)) => Err(format!("{process}; {firewall}").into()),
    };
    finish_launch_report(run_root, evidence, outcome, cleanup_result)
}

async fn exercise_new_launch<'a>(
    state: &'a tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    environment: &LiveEnvironment,
    run_root: &Path,
    evidence: &mut Value,
    cleanup: &mut NativeLaunchCleanup<'a>,
    NativeLaunchBudget {
        operation_limit,
        deadline,
    }: NativeLaunchBudget,
) -> LiveResult {
    let prompt = "使用已经安装的饥荒专用服，新建一个离线、仅局域网、单地上世界服务器，房间名为本地开服验收，小世界、最多4人，禁用数据收集，不需要任何模组。将实例的玩家监听地址设置为127.0.0.1本机回环地址，启动后验证Master游戏端口的实际绑定；本次只供本机验收，不要求其它设备加入，也不要求验证其它内部socket。不要安装、校验更新或下载任何东西，不要编辑游戏二进制或游戏脚本，只允许创建实例和调整实例配置。请真正完成创建、配置和启动验证，每一步都先给完整确认预览。";
    evidence["request"] = json!(prompt);
    let mut preview = assistant_preview_operation_inner(
        state.clone(),
        AssistantExecuteOperationInput {
            task: commands_assistant_ops::AssistantTaskRequest {
                goal: commands_assistant_ops::AssistantTaskGoal::LaunchService,
                preserve_existing_mods: true,
            },
            settings: environment.provider.clone(),
            prompt: prompt.into(),
            context: None,
            selected_instance_id: None,
            selected_module_id: Some("dontstarve".into()),
        },
    )
    .await?;
    let task_id = preview
        .task
        .as_ref()
        .ok_or("new launch preview has no task contract")?
        .id
        .clone();
    let mut created: Option<InstanceDetails> = None;
    let mut configured = false;
    for step in 0..operation_limit {
        if tokio::time::Instant::now() >= deadline {
            return Err("native launch watchdog prevents further confirmed operations".into());
        }
        evidence["steps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"step":step,"preview":preview}));
        write_evidence(run_root, evidence)?;
        let task = preview
            .task
            .as_ref()
            .ok_or("launch preview lost its task")?;
        if task.id != task_id
            || task.goal != commands_assistant_ops::AssistantTaskGoal::LaunchService
            || !task.preserve_existing_mods
            || task.operation_limit
                != assistant_operation_limit(
                    commands_assistant_ops::AssistantTaskGoal::LaunchService,
                )
            || !preview.requires_confirmation
            || preview.module_id.as_deref() != Some("dontstarve")
        {
            return Err("launch preview changed its contract or is not confirmable".into());
        }
        let instances = list_instances(&storage.paths).await?;
        if let Some(original) = &created {
            if instances.len() != 1
                || instances[0].id != original.summary.id
                || preview.instance_id.as_deref() != Some(original.summary.id.as_str())
            {
                return Err("launch continuation escaped its one newly created instance".into());
            }
        } else if !instances.is_empty()
            || preview.instance_id.is_some()
            || preview.action != AssistantOperationAction::CreateServer
        {
            return Err(
                "new launch must first preview creation without creating or downloading anything"
                    .into(),
            );
        }
        match preview.action {
            AssistantOperationAction::CreateServer if created.is_none() => {}
            AssistantOperationAction::CustomizeConfig
            | AssistantOperationAction::ApplyBeginnerConfig
                if created.is_some() => {}
            AssistantOperationAction::StartServer if configured => {
                let original = created.as_ref().ok_or("start has no created instance")?;
                let current = read_instance_details(&storage.paths, &original.summary.id).await?;
                validate_new_launch_configuration(&current, original, true)?;
                let launch =
                    preview_instance_launch(state.clone(), original.summary.id.clone()).await?;
                evidence["ownedLaunchProgram"] = program::validate_owned_launch_program(
                    environment,
                    run_root,
                    &current,
                    &launch,
                )
                .await?;
                if launch
                    .args
                    .iter()
                    .filter(|arg| arg.as_str() == "-bind_ip")
                    .count()
                    != 1
                    || !launch
                        .args
                        .windows(2)
                        .any(|pair| pair[0] == "-bind_ip" && pair[1] == "127.0.0.1")
                {
                    return Err(
                        "native launch arguments do not bind exclusively to loopback".into(),
                    );
                }
                let saved: Value = serde_json::from_str(&current.settings_json)?;
                evidence["loopbackScope"] = json!({"instanceBindIp":current.summary.bind_ip,
                    "settingsBindIp":saved["bind_ip"], "launchBindIp":"127.0.0.1",
                    "evidence":"saved_configuration_and_launch_plan"});
                evidence["preStartLaunchPlan"] = json!(launch);
                evidence["preStartInstance"] = json!(current);
                write_evidence(run_root, evidence)?;
                cleanup.firewall = Some(NativeFirewall::prepare_dst_loopback(&current, run_root)?);
                evidence["firewall"]["ownedRulesAbsentBeforeStart"] = json!(true);
                write_evidence(run_root, evidence)?;
            }
            _ => {
                return Err(
                    "native launch refused an unapproved action, download or premature start"
                        .into(),
                );
            }
        }
        let action = preview.action;
        if tokio::time::Instant::now() >= deadline {
            return Err("native launch watchdog prevents submitting a confirmed operation".into());
        }
        let output = Box::pin(assistant_confirm_operation_with_verification(
            None,
            state.clone(),
            AssistantConfirmOperationInput {
                continue_task: false,
                conversation_id: preview.conversation_id.clone(),
                settings: environment.provider.clone(),
                confirmation_token: preview
                    .confirmation_token
                    .take()
                    .ok_or("confirmation token missing")?,
                plan_summary: preview
                    .plan_summary
                    .take()
                    .ok_or("full confirmation summary missing")?,
            },
        ))
        .await?;
        evidence["steps"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["result"] = json!(output);
        if action == AssistantOperationAction::CreateServer {
            let instances = list_instances(&storage.paths).await?;
            if instances.len() != 1
                || output.instance_id.as_deref() != Some(instances[0].id.as_str())
            {
                return Err(
                    "creation did not return exactly the isolated database's new instance".into(),
                );
            }
            let details = read_instance_details(&storage.paths, &instances[0].id).await?;
            cleanup.process = Some(NativeCleanup {
                state,
                instance_id: details.summary.id.clone(),
            });
            evidence["instanceId"] = json!(details.summary.id);
            evidence["createdInstance"] = json!(details);
            write_evidence(run_root, evidence)?;
            let config = Path::new(&details.config_file_path).canonicalize()?;
            let expected = run_root
                .join("i")
                .join(&details.summary.id)
                .join("config/instance.json")
                .canonicalize()?;
            if config != expected
                || !config.starts_with(run_root.canonicalize()?)
                || details.summary.module_id != "dontstarve"
                || details.summary.autostart
                || read_active_instance_run(&storage.paths, &details.summary.id)
                    .await?
                    .is_some()
            {
                return Err("new instance was not created stopped inside the owned run".into());
            }
            created = Some(details);
        }
        let original = created
            .as_ref()
            .ok_or("confirmed launch lost its created instance")?;
        let current = read_instance_details(&storage.paths, &original.summary.id).await?;
        if matches!(
            action,
            AssistantOperationAction::CustomizeConfig
                | AssistantOperationAction::ApplyBeginnerConfig
        ) {
            validate_new_launch_configuration(&current, original, false)?;
            configured = true;
        }
        let task = output
            .task
            .as_ref()
            .ok_or("confirmed launch has no task receipt")?;
        if task.id != task_id
            || task.goal != commands_assistant_ops::AssistantTaskGoal::LaunchService
            || task.instance_id.as_deref() != Some(original.summary.id.as_str())
            || !task.preserve_existing_mods
        {
            return Err("confirmed operation changed the launch contract".into());
        }
        write_evidence(run_root, evidence)?;
        if action == AssistantOperationAction::StartServer {
            validate_new_launch_configuration(&current, original, true)?;
            if evidence["preStartInstance"]["ports"] != serde_json::to_value(&current.ports)? {
                return Err("native start changed its confirmed firewall ports".into());
            }
            evidence["finalInstance"] = json!(current);
            let verification = output
                .verification
                .as_ref()
                .ok_or("start verification missing")?;
            let started = output
                .runtime_start
                .as_ref()
                .ok_or("native launch has no real start result")?;
            let active = read_active_instance_run(&storage.paths, &original.summary.id)
                .await?
                .ok_or("native launch has no active run")?;
            if verification.status != AssistantVerificationStatus::Verified
                || task.status != commands_assistant_ops::AssistantTaskStatus::Completed
                || verification.run_id != Some(started.run_id)
                || active.run_id != started.run_id
                || started.process_count != 1
            {
                return Err(
                    "native launch failed the production task or fresh single-Master verification"
                        .into(),
                );
            }
            let process = started
                .processes
                .first()
                .ok_or("native process evidence missing")?;
            let world = native_probe(
                state,
                &original.summary.id,
                process,
                started.run_id,
                "tostring(TheWorld ~= nil and TheWorld.ismastersim and TheWorld.Map:GetSize() > 0)",
            )
            .await?;
            evidence["nativeWorldLoaded"] = json!(world);
            if world != "true" {
                return Err("native launch world probe did not prove a loaded Master world".into());
            }
            let master_port = current
                .ports
                .iter()
                .find(|port| port.name == "master")
                .ok_or("native launch has no declared Master port")?
                .port;
            evidence["nativeNetwork"] = network::inspect_live_network(
                storage,
                &original.summary.id,
                started.run_id,
                master_port,
            )
            .await?;
            write_evidence(run_root, evidence)?;
            return Ok(());
        }
        if task.status == commands_assistant_ops::AssistantTaskStatus::Completed {
            return Err("the launch task claimed completion before native startup".into());
        }
        preview = *output
            .follow_up
            .ok_or("new launch has no confirmable continuation")?;
    }
    Err(format!(
        "new server did not complete within {operation_limit} separately confirmed operations"
    )
    .into())
}

fn finish_launch_report(
    run_root: &Path,
    evidence: &mut Value,
    outcome: LiveResult,
    cleanup: LiveResult,
) -> LiveResult {
    evidence["launchPassed"] = json!(outcome.is_ok());
    let result = complete_live_report(run_root, evidence, outcome, cleanup);
    evidence.as_object_mut().unwrap().remove("repairPassed");
    write_evidence(run_root, evidence)?;
    result
}
