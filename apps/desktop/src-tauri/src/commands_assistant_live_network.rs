use super::*;
use app_platform_win::{WindowInspectionTarget, WindowsPlatform};

/// Observe every owned endpoint, including Steam sockets outside the module's
/// strict game-port check. Configuration alone cannot prove native listeners.
pub(super) async fn inspect_live_network(
    storage: &StorageBootstrap,
    instance_id: &str,
    run_id: i64,
    master_port: u16,
) -> LiveResult<Value> {
    let before = read_active_instance_run(&storage.paths, instance_id)
        .await?
        .ok_or("network inspection has no active native run")?;
    if before.run_id != run_id || before.process_count != 1 || before.processes.len() != 1 {
        return Err("network inspection lost its single native Master run".into());
    }
    let process = &before.processes[0];
    let target = WindowInspectionTarget {
        pid: process.pid.ok_or("native network inspection has no PID")?,
        process_key: process.process_key.clone(),
        display_name: process.display_name.clone(),
        process_identity: process
            .process_identity
            .clone()
            .ok_or("native network inspection has no process creation identity")?,
    };
    let inspection = tokio::task::spawn_blocking(move || {
        WindowsPlatform::inspect_process_network_endpoints(&[target], &[])
    })
    .await??;
    let after = read_active_instance_run(&storage.paths, instance_id)
        .await?
        .ok_or("native run exited during network inspection")?;
    if serde_json::to_value(&before)? != serde_json::to_value(&after)?
        || inspection.inspected_process_count == 0
        || inspection.endpoints.len() > 64
    {
        return Err("native network inspection has changed or incomplete ownership".into());
    }
    let game_endpoints: Vec<_> = inspection
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.protocol == "udp" && endpoint.local_port == master_port)
        .collect();
    let game_loopback = !game_endpoints.is_empty()
        && game_endpoints
            .iter()
            .all(|endpoint| endpoint.local_address == "127.0.0.1");
    let non_loopback = inspection
        .endpoints
        .iter()
        .filter(|endpoint| endpoint.local_address != "127.0.0.1" && endpoint.local_address != "::1")
        .count();
    let evidence = json!({
        "scope":"verified_native_process_tree", "runId":run_id,
        "inspectedProcessCount":inspection.inspected_process_count,
        "endpoints":inspection.endpoints, "masterPort":master_port,
        "gamePortLoopbackVerified":game_loopback,
        "allObservedEndpointsLoopback":non_loopback == 0,
        "nonLoopbackEndpointCount":non_loopback,
    });
    if !game_loopback {
        return Err(format!("native game-port loopback observation failed: {evidence}").into());
    }
    Ok(evidence)
}
