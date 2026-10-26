use super::commands_ark_tools as ark;
use super::*;
use std::sync::{Arc, OnceLock};

#[path = "ark_map_protocol.rs"]
mod protocol;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapInput {
    pub instance_id: String,
    pub process_key: String,
    pub run_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandInput {
    pub instance_id: String,
    pub process_key: String,
    pub run_id: i64,
    pub command: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapSnapshot {
    instance_id: String,
    process_key: String,
    run_id: i64,
    map_name: String,
    sampled_at_unix_ms: u64,
    projection: Option<protocol::Projection>,
    players: Vec<protocol::Player>,
    artwork: Option<MapArtwork>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapArtwork {
    data_url: String,
    source: String,
}

/// Keep the configured map, owned process, port and run as one admission unit.
/// In particular, another map's listener must not satisfy the endpoint check.
pub(super) fn selected_target(
    details: &InstanceDetails,
    process_key: &str,
    run_id: i64,
) -> Result<InstanceDetails, String> {
    let current_run = details.active_run.as_ref().and_then(|run| {
        if run.processes.is_empty() && process_key == "main" {
            Some(run.run_id)
        } else {
            run.processes
                .iter()
                .find(|process| process.process_key == process_key)
                .map(|process| process.run_id)
        }
    });
    if run_id <= 0 || current_run != Some(run_id) {
        return Err(
            "ARK_MAP_RUN_CHANGED: The viewed map is not running or its run has changed. Refresh the map."
                .into(),
        );
    }
    if process_key.is_empty() || process_key.len() > 36 {
        return Err("ARK_MAP_INVALID_TARGET: Select an enabled map.".into());
    }
    let mut target = commands_runtime_ark::project_running_command(details, Some(process_key))?
        .ok_or("This instance is not an ARK server.")?;
    if let Some(run) = target.active_run.as_mut() {
        if !run.processes.is_empty() {
            run.processes
                .retain(|process| process.process_key == process_key);
            run.pid = run.processes.first().and_then(|process| process.pid);
            run.process_count = run.processes.len();
        }
    }
    Ok(target)
}

fn collection_permit() -> Result<tokio::sync::OwnedSemaphorePermit, String> {
    static WORKERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    WORKERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| "ARK_MAP_BUSY: Player map collection is busy. Try again shortly.".into())
}

#[tauri::command]
pub async fn read_ark_map(
    state: tauri::State<'_, DesktopState>,
    input: MapInput,
) -> Result<MapSnapshot, String> {
    let _operation = state.begin_storage_context_operation("ARK player map")?;
    let _permit = collection_permit()?;
    let storage = state.bootstrap_storage()?;
    reconcile_runtime_state(&state).await?;
    let _guard = state.acquire_instance_mutation(&input.instance_id).await;
    let details = ark::instance(&storage, &input.instance_id).await?;
    let target = selected_target(&details, &input.process_key, input.run_id)?;
    ark::ensure_endpoint(&target).await?;
    let map = app_core::ark_maps::processes(&details)?
        .into_iter()
        .find(|map| map.process_key == input.process_key)
        .ok_or("ARK map was removed.")?;
    let request_id = uuid::Uuid::new_v4().simple().to_string();
    let response = ark::dispatch(&target, &format!("LgsmArkTools.Players {request_id}")).await?;
    let snapshot = protocol::snapshot(
        &response,
        &target.summary.module_id,
        &request_id,
        &map.map_name,
    )?;
    let sampled_at_unix_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_millis() as u64;
    Ok(MapSnapshot {
        instance_id: input.instance_id,
        process_key: input.process_key,
        run_id: input.run_id,
        map_name: snapshot.map_name,
        sampled_at_unix_ms,
        projection: snapshot.projection,
        players: snapshot.players,
        artwork: None,
    })
}

#[tauri::command]
pub async fn send_ark_map_gm_command(
    state: tauri::State<'_, DesktopState>,
    input: CommandInput,
) -> Result<InstanceRuntimeCommandResult, String> {
    let _operation = state.begin_storage_context_operation("ARK map tool command")?;
    let storage = state.bootstrap_storage()?;
    let details = ark::instance(&storage, &input.instance_id).await?;
    selected_target(&details, &input.process_key, input.run_id)?;
    // The existing dispatcher reacquires the mutation guard and compares this
    // expected instance/run before sending. No check/use gap on restart.
    commands_assistant_ops::send_ark_map_runtime_command(
        state,
        commands_assistant_ops::InstanceRuntimeCommandInput {
            instance_id: input.instance_id,
            process_key: Some(input.process_key),
            command: input.command,
            transport: Some("source_rcon".into()),
            port_name: Some("rcon".into()),
            password_setting_key: Some("admin_password".into()),
            enabled_setting_key: Some("rcon_enabled".into()),
            runtime_action_id: None,
            runtime_action_target: None,
            runtime_action_role: None,
        },
        &details,
    )
    .await
}
