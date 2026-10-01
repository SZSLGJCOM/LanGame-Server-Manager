use app_core::{InstanceDetails, InstanceStatus, ModulePlayerCountSource, RuntimeLivePlayerStatus};
use app_modules::ModuleDescriptor;

use crate::commands::{commands_global_player_counts, commands_player_counts};
use crate::live_players::service::build_cache_key;
use crate::state::DesktopState;

pub(super) async fn verify<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &tauri::State<'_, DesktopState>,
    descriptor: &ModuleDescriptor,
    running: &InstanceDetails,
) -> Result<(), String> {
    if descriptor.runtime.player_count_source != ModulePlayerCountSource::PlayerList {
        return Ok(());
    }
    let id = &running.summary.id;
    println!(
        "NATIVE_PLAYER_COUNT module={} phase=identity",
        descriptor.summary.id
    );
    require_current_run(running).await?;

    println!(
        "NATIVE_PLAYER_COUNT module={} phase=collect",
        descriptor.summary.id
    );
    let collected = commands_player_counts::collect_player_list_count(state, id)
        .await
        .map_err(|_| "native production player-list count collection failed")?;
    if collected.query.status != "ready" {
        return Err("native production player-list count is not ready".into());
    }
    let players = collected
        .current_players
        .ok_or("native production player-list count is unavailable")?;
    require_authoritative_snapshot(state, descriptor, running, players)?;

    println!(
        "NATIVE_PLAYER_COUNT module={} phase=overview",
        descriptor.summary.id
    );
    let overview =
        crate::commands::read_instance_runtime_overview_from_storage(state.clone(), id.clone())
            .await
            .map_err(|_| "native production runtime overview failed")?;
    if overview.players.query.status != "ready" || overview.players.current_players != Some(players)
    {
        return Err("native production runtime overview player count disagrees".into());
    }

    println!(
        "NATIVE_PLAYER_COUNT module={} phase=global",
        descriptor.summary.id
    );
    let global = commands_global_player_counts::collect_global_player_counts(
        app,
        std::slice::from_ref(&running.summary),
    )
    .await
    .map_err(|_| "native production global player-count collection failed")?;
    if global.total_online_players != players
        || global.queried_instances != 1
        || global.queryable_instances != 1
    {
        return Err("native production global player count disagrees or is incomplete".into());
    }
    require_current_run(running).await?;
    require_authoritative_snapshot(state, descriptor, running, players)?;
    println!(
        "NATIVE_PLAYER_COUNT module={} source=player_list collector=ready overview=ready players={players} global_total={} queried_instances={} queryable_instances={} complete=true stale=false same_run=true",
        descriptor.summary.id,
        global.total_online_players,
        global.queried_instances,
        global.queryable_instances,
    );
    Ok(())
}

fn require_authoritative_snapshot(
    state: &tauri::State<'_, DesktopState>,
    descriptor: &ModuleDescriptor,
    running: &InstanceDetails,
    players: usize,
) -> Result<(), String> {
    // This key binds the production registry result to this run and its exact
    // player-list security contract; a previous run cannot satisfy the check.
    let key = build_cache_key(running, descriptor)
        .ok_or("native player count has no current-run cache key")?;
    let snapshot = state
        .live_player_registry
        .read(&key)
        .ok_or("native production player count has no current-run roster")?;
    if snapshot.instance_id != running.summary.id
        || snapshot.status != RuntimeLivePlayerStatus::Ready
        || !snapshot.complete
        || snapshot.truncated
        || snapshot.stale
        || snapshot.current_players != Some(players)
        || snapshot.observed_at_unix_ms.is_none()
        || snapshot.expires_at_unix_ms.is_none()
        || snapshot.source
            != descriptor
                .runtime
                .player_list
                .as_ref()
                .map(|list| list.source)
    {
        return Err("native production player count lacks a complete fresh roster".into());
    }
    Ok(())
}

async fn require_current_run(expected: &InstanceDetails) -> Result<(), String> {
    let expected_run = expected
        .active_run
        .as_ref()
        .ok_or("native player-count fixture has no active run")?;
    let storage = app_storage::bootstrap_storage()
        .map_err(|_| "native player-count storage is unavailable")?;
    let current = app_storage::read_instance_details(&storage.paths, &expected.summary.id)
        .await
        .map_err(|_| "native player-count fixture is unavailable")?;
    if !matches!(current.summary.status, InstanceStatus::Running)
        || current.active_run.as_ref().map(|run| run.run_id) != Some(expected_run.run_id)
    {
        return Err("native player-count fixture changed its active run".into());
    }
    let targets = crate::commands::commands_runtime_supervision::build_window_inspection_targets_from_instance(expected);
    if targets.is_empty() {
        return Err("native player-count fixture lacks a managed process identity".into());
    }
    for target in targets {
        let actual = app_runtime::inspect_process_identity(target.pid)
            .map_err(|_| "native player-count process identity check failed")?;
        if actual.as_ref() != Some(&target.process_identity) {
            return Err("native player-count fixture changed its process generation".into());
        }
    }
    Ok(())
}
