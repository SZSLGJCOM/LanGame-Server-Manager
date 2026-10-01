use app_core::{
    ExecuteInstancePlayerActionInput, ExecuteInstancePlayerActionResult, ModulePlayerListCodec,
    RuntimeLivePlayerActionStatus, RuntimeLivePlayerIssueCode, RuntimeLivePlayerSnapshot,
};
use app_modules::ModuleDescriptor;
use app_storage::StorageBootstrap;

use crate::live_players::cache::{
    LivePlayerCacheKey, LivePlayerCollectionResult, LivePlayerRegistry,
};
use crate::live_players::service::{
    build_cache_key, collect_dst_structured_log, failed_snapshot, misconfigured_snapshot,
    new_request_id, refreshing_snapshot, select_running_process, stopped_snapshot,
    unsupported_snapshot,
};

use super::commands_assistant_ops::{
    DeclaredRuntimeActionRequest, dispatch_declared_runtime_action,
    dispatch_declared_runtime_action_until,
};
use super::commands_runtime_supervision::reconcile_runtime_state;
use super::*;

#[path = "commands_live_player_collection.rs"]
mod collection;
use collection::refresh_live_players_uncached;

#[path = "commands_seven_days_ban.rs"]
mod seven_days_ban;

#[tauri::command]
pub async fn read_instance_live_players(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<RuntimeLivePlayerSnapshot, String> {
    let (_, details, descriptor) = load_live_player_context(&state, &instance_id)
        .await
        .map_err(|_| String::from("Unable to read the live-player capability."))?;
    let snapshot_id = new_request_id();
    let Some(player_list) = descriptor.runtime.player_list.as_ref() else {
        return Ok(unsupported_snapshot(&instance_id, snapshot_id));
    };
    if details.active_run.is_none() {
        return Ok(stopped_snapshot(
            &instance_id,
            snapshot_id,
            player_list.source,
        ));
    }
    let Some(key) = build_cache_key(&details, &descriptor) else {
        return Ok(stopped_snapshot(
            &instance_id,
            snapshot_id,
            player_list.source,
        ));
    };
    Ok(state
        .live_player_registry
        .read(&key)
        .unwrap_or_else(|| refreshing_snapshot(&instance_id, snapshot_id, player_list.source)))
}

#[tauri::command]
pub async fn refresh_instance_live_players(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<RuntimeLivePlayerSnapshot, String> {
    let (_, details, descriptor) = load_live_player_context(&state, &instance_id)
        .await
        .map_err(|_| String::from("Unable to refresh the live-player capability."))?;
    let capability_id = new_request_id();
    let Some(player_list) = descriptor.runtime.player_list.as_ref() else {
        let snapshot = unsupported_snapshot(&instance_id, capability_id);
        audit_live_player_refresh(&instance_id, &snapshot);
        return Ok(snapshot);
    };
    let source = player_list.source;
    if details.active_run.is_none() {
        let snapshot = stopped_snapshot(&instance_id, capability_id, source);
        audit_live_player_refresh(&instance_id, &snapshot);
        return Ok(snapshot);
    }
    let Some(key) = build_cache_key(&details, &descriptor) else {
        let snapshot = stopped_snapshot(&instance_id, capability_id, source);
        audit_live_player_refresh(&instance_id, &snapshot);
        return Ok(snapshot);
    };

    let requested_at = state.live_player_registry.now_unix_ms();
    let snapshot = state
        .live_player_registry
        .refresh_or_join(key.clone(), requested_at, || async {
            refresh_live_players_uncached(&state, &instance_id, &key, requested_at).await
        })
        .await;
    audit_live_player_refresh(&instance_id, &snapshot);
    Ok(snapshot)
}

pub(super) async fn read_or_refresh_live_players_for_count(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Result<RuntimeLivePlayerSnapshot, String> {
    let (_, details, descriptor) = load_live_player_context(state, instance_id).await?;
    let Some(list) = descriptor.runtime.player_list.as_ref() else {
        return Ok(unsupported_snapshot(instance_id, new_request_id()));
    };
    let Some(key) = build_cache_key(&details, &descriptor) else {
        return Ok(stopped_snapshot(instance_id, new_request_id(), list.source));
    };
    let requested_at = state.live_player_registry.now_unix_ms();
    state
        .live_player_registry
        .refresh_if_expired(key.clone(), requested_at, || async {
            refresh_live_players_uncached(state, instance_id, &key, requested_at).await
        })
        .await;

    // Never hold an instance mutation while joining the registry gate: its
    // collector acquires the same mutation. Validate after joining so a cached
    // roster cannot outlive a changed run, process identity, or disabled RCON.
    let _mutation = state.acquire_instance_mutation(instance_id).await;
    let (_, details, descriptor) = load_persisted_live_player_context(instance_id).await?;
    if let Err(failure) =
        collection::validate_cached_collection_context(state, &details, &descriptor, &key)
    {
        return Ok(*failure);
    }
    Ok(state.live_player_registry.read(&key).unwrap_or_else(|| {
        failed_snapshot(
            instance_id,
            new_request_id(),
            list.source,
            RuntimeLivePlayerIssueCode::ProcessUnavailable,
            "The player snapshot was invalidated during collection.",
            false,
        )
    }))
}

#[tauri::command]
pub async fn execute_instance_player_action(
    state: tauri::State<'_, DesktopState>,
    input: ExecuteInstancePlayerActionInput,
) -> Result<ExecuteInstancePlayerActionResult, String> {
    let audit_instance_id = input.instance_id.clone();
    let audit_snapshot_id = input.snapshot_id.clone();
    let audit_player_key = input.player_key.clone();
    let audit_action_id = input.action_id.clone();
    let result = execute_instance_player_action_inner(&state, input).await;
    audit_live_player_action(
        &audit_instance_id,
        &audit_snapshot_id,
        &audit_player_key,
        &audit_action_id,
        result.as_ref().map(|_| ()).map_err(String::as_str),
    );
    result
}

async fn execute_instance_player_action_inner(
    state: &tauri::State<'_, DesktopState>,
    input: ExecuteInstancePlayerActionInput,
) -> Result<ExecuteInstancePlayerActionResult, String> {
    let _storage_context_operation = state.begin_storage_context_operation("player action")?;
    let instance_id = input.instance_id.trim();
    let snapshot_id = input.snapshot_id.trim();
    let player_key = input.player_key.trim();
    let action_id = input.action_id.trim();
    if instance_id.is_empty()
        || snapshot_id.is_empty()
        || player_key.is_empty()
        || action_id.is_empty()
    {
        return Err(String::from(
            "instance_id, snapshot_id, player_key, and action_id are required",
        ));
    }

    reconcile_runtime_state(state)
        .await
        .map_err(|_| String::from("Unable to validate the current server run."))?;
    let _mutation = state.acquire_instance_mutation(instance_id).await;
    let (storage, details, descriptor) = load_persisted_live_player_context(instance_id)
        .await
        .map_err(|_| String::from("Unable to load the current player-action contract."))?;
    let player_list = descriptor
        .runtime
        .player_list
        .as_ref()
        .ok_or_else(|| String::from("this module does not expose structured live players"))?;
    let run_id = details
        .active_run
        .as_ref()
        .map(|run| run.run_id)
        .ok_or_else(|| String::from("the server is not running"))?;
    let key = build_cache_key(&details, &descriptor)
        .ok_or_else(|| String::from("the current server run has no player cache key"))?;
    if !player_list
        .player_action_ids
        .iter()
        .any(|candidate| candidate == action_id)
    {
        return Err(String::from(
            "the requested action is not available for structured player rows",
        ));
    }
    let action = descriptor
        .runtime
        .player_actions
        .iter()
        .find(|candidate| candidate.id == action_id)
        .ok_or_else(|| String::from("the requested player action is no longer declared"))?;
    if !action.target_required || !action.command_template.contains("{{target}}") {
        return Err(String::from(
            "the requested player action does not bind an authoritative target",
        ));
    }
    let canonical_target = state
        .live_player_registry
        .resolve_action_binding(&key, snapshot_id, player_key, action_id)
        .ok_or_else(|| {
            String::from(
                "the player snapshot is expired, incomplete, stale, or no longer authorizes this action",
            )
        })?;

    let dispatched = dispatch_after_live_player_authorization_revocation(
        &state.live_player_registry,
        instance_id,
        dispatch_declared_runtime_action(
            state,
            DeclaredRuntimeActionRequest {
                instance_id,
                expected_run_id: run_id,
                action_id,
                target: Some(&canonical_target),
                role: None,
                request_id: None,
                require_target_binding: true,
            },
        ),
    )
    .await
    .map_err(|error| {
        if details.summary.module_id == "sevendaystodie" && action_id == "ban_player" {
            format!("The server could not complete or confirm the ban command: {error}")
        } else {
            String::from("The server could not complete the declared player action.")
        }
    })?;

    if details.summary.module_id == "sevendaystodie" && action_id == "ban_player" {
        // The game can acknowledge a ban even when its native file write fails. Read
        // back every account explicitly confirmed by this command, including a
        // family-sharing owner, before recording their native expiry and reason.
        let receipts = seven_days_ban::confirmed_ban_receipts(
            dispatched.response_text.as_deref(),
            &canonical_target,
        )
        .map_err(|error| {
            format!("The ban command was sent, but its result could not be confirmed: {error}")
        })?;
        app_storage::record_seven_days_bans_from_native(
            &storage.paths,
            instance_id,
            run_id,
            &canonical_target,
            &receipts,
        )
        .await
        .map_err(|error| {
            format!(
                "The ban command was sent, but saving the confirmed blacklist entry failed: {error}"
            )
        })?;
    }

    Ok(ExecuteInstancePlayerActionResult {
        action_id: action_id.to_owned(),
        status: RuntimeLivePlayerActionStatus::Sent,
        executed_at_unix_ms: state.live_player_registry.now_unix_ms(),
        summary: String::from("Player action accepted. Refreshing the live player list."),
    })
}

pub(super) async fn dispatch_after_live_player_authorization_revocation<DispatchFuture>(
    registry: &LivePlayerRegistry,
    instance_id: &str,
    dispatch: DispatchFuture,
) -> DispatchFuture::Output
where
    DispatchFuture: std::future::Future,
{
    registry.invalidate_instance(instance_id);
    dispatch.await
}

async fn load_live_player_context(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Result<(StorageBootstrap, InstanceDetails, ModuleDescriptor), String> {
    reconcile_runtime_state(state).await?;
    load_persisted_live_player_context(instance_id).await
}

pub(super) async fn load_persisted_live_player_context(
    instance_id: &str,
) -> Result<(StorageBootstrap, InstanceDetails, ModuleDescriptor), String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let details = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let descriptors =
        discover_modules(&storage.paths.modules_root).map_err(|error| error.to_string())?;
    let descriptor = find_descriptor(&descriptors, &details.summary.module_id)?.clone();
    Ok((storage, details, descriptor))
}

fn audit_live_player_refresh(instance_id: &str, snapshot: &RuntimeLivePlayerSnapshot) {
    let Ok(storage) = bootstrap_storage() else {
        return;
    };
    append_desktop_app_log(
        &storage,
        if snapshot.status == app_core::RuntimeLivePlayerStatus::Ready {
            "info"
        } else {
            "warning"
        },
        "instance.live_players.refresh",
        "Live-player refresh completed",
        json!({
            "instance_id": audit_reference(instance_id),
            "snapshot_id": snapshot.snapshot_id,
            "status": snapshot.status,
            "complete": snapshot.complete,
            "truncated": snapshot.truncated,
            "stale": snapshot.stale,
            "entry_count": snapshot.entries.len(),
            "issue_code": snapshot.issue.as_ref().map(|issue| issue.code),
        }),
    );
}

fn audit_live_player_action(
    instance_id: &str,
    snapshot_id: &str,
    player_key: &str,
    action_id: &str,
    result: Result<(), &str>,
) {
    let Ok(storage) = bootstrap_storage() else {
        return;
    };
    let (level, outcome) = match result {
        Ok(()) => ("info", "sent"),
        Err(_) => ("warning", "rejected_or_failed"),
    };
    append_desktop_app_log(
        &storage,
        level,
        "instance.live_players.action",
        "Structured live-player action completed",
        json!({
            "instance_id": audit_reference(instance_id),
            "snapshot_id": audit_reference(snapshot_id),
            "player_key": audit_reference(player_key),
            "action_id": audit_reference(action_id),
            "outcome": outcome,
        }),
    );
}

pub(super) fn audit_reference(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(128)
        .collect()
}
