use super::*;

pub(super) async fn run_initial<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    primary: &InstanceDetails,
    peer: &InstanceDetails,
    observation_seconds: u64,
) -> Result<(i64, InstanceDetails), Box<dyn std::error::Error>> {
    let peer = app_storage::update_instance(
        &runtime.storage.paths,
        UpdateInstanceInput {
            id: peer.summary.id.clone(),
            bind_ip: primary.summary.bind_ip.clone(),
            auto_backup_on_stop: false,
            backup_retention_count: 2,
            settings_json: serde_json::to_string(runtime.settings)?,
            ports: peer.ports.clone(),
        },
    )
    .await?;
    let peer_root = creation::program_root(&peer)?;
    runtime
        .smoke
        .require_fresh_world(&peer_root, &runtime.package.root)?;
    let baselines = runtime.smoke.log_baselines(&peer, &peer_root)?;
    let prestart =
        prestart_probe::PrestartProbe::begin(runtime.storage, &peer, runtime.descriptor)?;
    runtime
        .package
        .cleanup_allowed
        .store(false, Ordering::SeqCst);
    Box::pin(start_instance_process_after_reconcile(
        None,
        runtime.state,
        runtime.storage,
        peer.summary.id.clone(),
        "manual",
        None,
    ))
    .await?;
    prestart.verify(runtime.storage, &peer, "concurrent_peer")?;
    let running = read_instance_details(&runtime.storage.paths, &peer.summary.id).await?;
    runtime
        .smoke
        .wait_ready(
            runtime.descriptor,
            &running,
            &peer_root,
            &runtime.package.root,
            baselines,
        )
        .await?;
    let identity = running
        .active_run
        .as_ref()
        .ok_or("native concurrent peer has no managed run")?;
    verify_peer_running(runtime, &running, identity).await?;
    println!(
        "NATIVE_MULTI_INSTANCE module={} phase=peer_ready peer_run_id={} process_count={}",
        peer.summary.module_id, identity.run_id, identity.process_count
    );

    // A live handle for the same owned process tree after this complete cycle
    // proves continuous overlap; a dead process cannot revive under that handle.
    let primary_result = Box::pin(runtime.run_cycle(primary, "initial", observation_seconds)).await;
    runtime
        .package
        .cleanup_allowed
        .store(false, Ordering::SeqCst);
    let primary_run_id = primary_result?;
    verify_peer_running(runtime, &running, identity).await?;
    let primary_stopped =
        read_instance_details(&runtime.storage.paths, &primary.summary.id).await?;
    verify_isolation(&primary_stopped, &running)?;
    if primary_stopped.active_run.is_some() || primary_run_id == identity.run_id {
        return Err("native primary stop did not preserve distinct run ownership".into());
    }
    println!(
        "NATIVE_MULTI_INSTANCE module={} phase=primary_cycle_completed primary_run_id={primary_run_id} peer_run_id={} primary=stopped peer=running ports_config_saves=isolated overlap=start_ready_save_normal_stop",
        peer.summary.module_id, identity.run_id
    );

    stop_instance_process((*runtime.state).clone(), peer.summary.id.clone()).await?;
    let stopped = read_instance_details(&runtime.storage.paths, &peer.summary.id).await?;
    if stopped.active_run.is_some()
        || runtime
            .state
            .runtime_supervisor
            .lock()
            .map_err(|_| "runtime supervisor lock poisoned")?
            .is_tracked(&peer.summary.id)
    {
        return Err("native concurrent peer stop left a managed process".into());
    }
    let overview =
        app_storage::read_instance_runtime_overview(&runtime.storage.paths, &peer.summary.id)
            .await?;
    let completed = overview
        .recent_runs
        .iter()
        .find(|run| run.run_id == identity.run_id)
        .ok_or("native concurrent peer stop lost its run history")?;
    if completed.crash_flag || completed.processes.iter().any(|process| process.crash_flag) {
        return Err("native concurrent peer stop recorded a crash".into());
    }
    diagnostics::report_success(
        "stopped",
        &running,
        &peer_root,
        &runtime.package.root,
        runtime.settings,
    )
    .await?;
    runtime
        .package
        .cleanup_allowed
        .store(true, Ordering::SeqCst);
    println!(
        "NATIVE_MULTI_INSTANCE module={} phase=peer_stopped peer_run_id={} active_runs=none normal_stop=passed",
        peer.summary.module_id, identity.run_id
    );
    Ok((primary_run_id, stopped))
}

async fn verify_peer_running<R: tauri::Runtime>(
    runtime: &runtime::NativeRuntime<'_, '_, R>,
    peer: &InstanceDetails,
    expected: &ActiveInstanceRun,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = read_active_instance_run(&runtime.storage.paths, &peer.summary.id)
        .await?
        .ok_or("native concurrent peer no longer has an active run")?;
    if current.run_id != expected.run_id
        || current.processes.is_empty()
        || current.process_count != expected.process_count
        || current.processes.len() != expected.processes.len()
    {
        return Err("native concurrent peer run identity changed".into());
    }
    let mut supervisor = runtime
        .state
        .runtime_supervisor
        .lock()
        .map_err(|_| "runtime supervisor lock poisoned")?;
    for process in &expected.processes {
        let pid = process
            .pid
            .ok_or("native concurrent peer has no process ID")?;
        if !current.processes.iter().any(|actual| {
            actual.run_id == process.run_id
                && actual.process_key == process.process_key
                && actual.pid == process.pid
                && actual.process_identity == process.process_identity
        }) || supervisor.owned_process_tree_is_running(
            &peer.summary.id,
            process.run_id,
            &process.process_key,
            pid,
        )? != Some(true)
        {
            return Err("native concurrent peer lost its exact live process ownership".into());
        }
    }
    Ok(())
}

fn verify_isolation(primary: &InstanceDetails, peer: &InstanceDetails) -> Result<(), &'static str> {
    let primary_saves = Path::new(&primary.saves_path);
    let peer_saves = Path::new(&peer.saves_path);
    if primary.summary.id == peer.summary.id
        || primary.config_file_path == peer.config_file_path
        || primary_saves.starts_with(peer_saves)
        || peer_saves.starts_with(primary_saves)
        || primary.ports.iter().any(|a| {
            peer.ports
                .iter()
                .any(|b| a.protocol.eq_ignore_ascii_case(&b.protocol) && a.port == b.port)
        })
    {
        return Err("native concurrent instances share configuration, saves or bound ports");
    }
    Ok(())
}
