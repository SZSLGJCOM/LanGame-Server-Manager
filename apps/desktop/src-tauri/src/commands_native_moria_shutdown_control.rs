//! Diagnostic native baseline: deliberately bypass automatic save preparation.
//! This is a test child, never a production startup or manual-save acceptance.
use super::*;

const NORMAL_STOP_BUDGET: Duration = Duration::from_secs(27);
const EXTRA_OBSERVATION: Duration = Duration::from_secs(120);

pub(super) async fn verify<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    details: &InstanceDetails,
) -> Result<(), Box<dyn std::error::Error>> {
    if details.summary.module_id != "returntomoria"
        || std::env::var("LANGAME_NATIVE_MORIA_NO_LOADER_CONTROL").as_deref() != Ok("true")
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE").as_deref() == Ok("true")
        || std::env::var("LANGAME_NATIVE_MANUAL_SAVE_RESTART").as_deref() == Ok("true")
        || details.active_run.is_some()
    {
        return Err("native Moria no-loader control requires an idle isolated fixture and no save/restart probe".into());
    }
    let root = runtime.effective_install_root.canonicalize()?;
    let fixture = runtime.package.root.canonicalize()?;
    if !root.starts_with(&fixture) || root == fixture {
        return Err("native Moria control must own a private copied program".into());
    }
    require_unmodified_loader_layout(&root)?;
    if !inventory::save_inventory(Path::new(&details.saves_path))?.is_empty() {
        return Err("native Moria control requires an empty new-world save directory".into());
    }
    let settings: Value = serde_json::from_str(&details.settings_json)?;
    if settings.get("console_enabled").and_then(Value::as_bool) != Some(true) {
        return Err("native Moria control requires its fresh native console enabled".into());
    }
    // The ordinary materializer publishes only native configuration here.
    // In particular, no desktop startup or loader preparation coordinator runs.
    let configured = app_storage::materialize_instance_configuration_for_start(
        &runtime.storage.paths,
        &details.summary.id,
    )
    .await?;
    require_unmodified_loader_layout(&root)?;
    let module = map_module_details_with_install_state(
        &runtime.storage.settings,
        runtime.descriptor,
        Some(root.to_str().ok_or("native control path is not UTF-8")?),
    );
    let mut plans =
        crate::commands::commands_runtime_supervision::build_process_launch_plans_for_instance(
            &runtime.storage.settings,
            &module,
            &configured,
        )?;
    if plans.len() != 1 {
        return Err("native Moria control requires exactly one bootstrap plan".into());
    }
    let plan = plans.pop().ok_or("native control launch plan missing")?;
    if plan.process_key != "main"
        || plan.launch_plan.requires_admin
        || Path::new(&plan.launch_plan.executable_path).canonicalize()?
            != root.join("MoriaServer.exe")
        || !plan.launch_plan.args.is_empty()
    {
        return Err("native Moria control rejects a changed bootstrap or arguments".into());
    }
    let offsets = runtime.smoke.log_baselines(&configured, &root)?;
    let log = fixture.join("moria-native-control-console.log");
    runtime
        .package
        .cleanup_allowed
        .store(false, Ordering::SeqCst);
    let spawned = app_runtime::spawn_launch_plan(&plan.launch_plan, &log)?;
    let session = uuid::Uuid::new_v4().to_string();
    // The same storage identity rows let the existing read-only smoke probes
    // validate the raw launch; this does not publish a production start result.
    let recorded = app_storage::mark_instance_process_started_with_identity(
        &runtime.storage.paths,
        &app_storage::StartedInstanceProcess {
            instance_id: &configured.summary.id,
            session_id: Some(&session),
            process_key: &plan.process_key,
            display_name: &plan.display_name,
            pid: spawned.pid,
            log_path: &spawned.log_path,
            is_primary: true,
        },
        Some(&spawned.process_identity),
    )
    .await?;
    let id = configured.summary.id.as_str();
    let pid = spawned.pid;
    runtime
        .state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?
        .insert_running(
            configured.summary.clone(),
            Some(session),
            vec![app_runtime::ManagedProcess {
                run_id: recorded.run_id,
                process_key: plan.process_key,
                display_name: plan.display_name,
                pid,
                process_identity: spawned.process_identity,
                root_process_identity: spawned.root_process_identity,
                log_path: spawned.log_path,
                is_primary: true,
                uses_script_entrypoint: spawned.uses_script_entrypoint,
                performance_policy: plan.launch_plan.performance_policy,
                last_performance_refresh: None,
                last_performance_target_count: None,
                last_performance_application: None,
                child: spawned.child,
                hidden_desktop: spawned.hidden_desktop,
            }],
        );
    println!(
        "NATIVE_MORIA_CONTROL phase=raw_started pid={pid} loader_preparation=not_called formal_save=not_called"
    );
    let running = app_storage::read_instance_details(&runtime.storage.paths, id).await?;
    let result: Result<(), Box<dyn std::error::Error>> = async {
        runtime.smoke.wait_ready(runtime.descriptor, &running, &root, &fixture, offsets).await?;
        require_unmodified_loader_layout(&root)?;
        println!("NATIVE_MORIA_CONTROL phase=native_ready declared_probes=passed loader_files=absent");
        // The production fixed-command sender authorizes the owned launch Job,
        // pins its Shipping member and writes exit exactly once. No fallback.
        runtime.state.runtime_supervisor.lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .request_moria_console_exit(id, Some("main"))?;
        println!("NATIVE_MORIA_CONTROL phase=exit_queued attempted_once=true native_exit_confirmed=false");
        let stop_started = Instant::now();
        let (normal_deadline_met, elapsed) = observe_exit(runtime, id, stop_started).await?;
        println!("NATIVE_MORIA_CONTROL phase=observation_finished normal_deadline_met={normal_deadline_met} natural_tree_exit=true elapsed_ms={} formal_save=not_called acceptance_pass=not_claimed", elapsed.as_millis());
        if !normal_deadline_met {
            return Err("native Moria control exited only during extra diagnostic observation; original normal-stop budget failed".into());
        }
        Ok(())
    }.await;
    if let Err(error) = &result {
        let logs =
            diagnostics::failed_run_logs(&running, &root, &fixture).unwrap_or_else(|detail| {
                eprintln!("NATIVE_DIAGNOSTIC phase=moria_native_control log_read={detail}");
                Vec::new()
            });
        runtime::print_native_failure(
            "moria_native_control",
            &error.to_string(),
            &logs,
            runtime.settings,
            &fixture,
        );
    }
    // NativeCleanup remains the sole cleanup owner. Even an observed natural
    // exit is reaped through that owner before the copied package can disappear.
    result
}

async fn observe_exit<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    id: &str,
    started: Instant,
) -> Result<(bool, Duration), Box<dyn std::error::Error>> {
    let mut original_expired = false;
    loop {
        let alive = runtime
            .state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .instance_process_tree_is_running(id)?;
        let elapsed = started.elapsed();
        match alive {
            Some(false) => {
                return Ok((!original_expired && elapsed <= NORMAL_STOP_BUDGET, elapsed));
            }
            Some(true) => {}
            None => return Err("native Moria control lost its complete owned-tree proof".into()),
        }
        if elapsed >= NORMAL_STOP_BUDGET && !original_expired {
            original_expired = true;
            println!(
                "NATIVE_MORIA_CONTROL phase=normal_deadline_failed elapsed_ms={} extra_read_only_observation_ms={} exit_retried=false",
                elapsed.as_millis(),
                EXTRA_OBSERVATION.as_millis()
            );
        }
        if elapsed >= NORMAL_STOP_BUDGET + EXTRA_OBSERVATION {
            println!(
                "NATIVE_MORIA_CONTROL phase=observation_finished normal_deadline_met=false natural_tree_exit=false elapsed_ms={} exit_retried=false",
                elapsed.as_millis()
            );
            return Err("native Moria no-loader tree remained alive through normal budget and 120-second read-only observation".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn require_unmodified_loader_layout(root: &Path) -> Result<(), String> {
    let binaries = root.join("Moria/Binaries/Win64");
    for path in [
        binaries.join("ue4ss"),
        binaries.join("UE4SS.dll"),
        binaries.join("dwmapi.dll"),
        binaries.join("version.dll"),
        binaries.join("dsound.dll"),
        binaries.join("xinput1_3.dll"),
        root.join("langame_world_save"),
    ] {
        match fs::symlink_metadata(path) {
            Ok(_) => return Err("native Moria no-loader control found loader/control artifacts; original files preserved".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("native Moria no-loader layout could not be inspected".into()),
        }
    }
    Ok(())
}
