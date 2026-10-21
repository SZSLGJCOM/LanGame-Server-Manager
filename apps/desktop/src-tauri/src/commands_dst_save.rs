//! Request the game's own coordinated cluster save, without a disk observer.
use super::*;
use crate::commands::commands_stdin_dispatch::{
    RuntimeStdinDispatchConfirmation, RuntimeStdinDispatchReceipt,
};

pub(in crate::commands) async fn save_world(
    state: &DesktopState,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
) -> Result<InstanceRuntimeCommandResult, String> {
    let _storage_lease = state.begin_storage_context_operation("DST immediate world save")?;
    ensure_storage_context_snapshot_current(state, storage, "DST immediate world save")?;
    let receipt = request_save(state, details).await?;
    let pending = receipt.confirmation == RuntimeStdinDispatchConfirmation::Pending;
    append_desktop_app_log(
        storage,
        "info",
        "instance.world_save.requested",
        "Sent the native cluster-save request to the primary shard",
        json!({ "instance_id": details.summary.id, "write_confirmation_pending": pending }),
    );
    Ok(InstanceRuntimeCommandResult {
        instance_id: details.summary.id.clone(),
        process_key: receipt.target.process_key,
        display_name: receipt.target.display_name,
        pid: receipt.target.pid,
        command: "c_save()".into(),
        response_text: Some(if pending {
            "The native save request is still awaiting console delivery confirmation; it was not resent."
        } else {
            "Sent c_save() once to the primary shard. The game coordinates saving its connected shards."
        }.into()),
        write_confirmation_pending: pending,
        save_completion: Some(if pending { WorldSaveCompletion::Unconfirmed } else { WorldSaveCompletion::Requested }),
        submitted_at_unix_ms,
    })
}

pub(super) async fn request_save(
    state: &DesktopState,
    details: &InstanceDetails,
) -> Result<RuntimeStdinDispatchReceipt, String> {
    let processes = enabled_processes(details)?;
    for process in &processes {
        if !shutdown_process_is_running(
            state,
            &details.summary.id,
            process,
            process_matches_identity,
        )? {
            return Err(format!(
                "Start every enabled DST shard before saving; {} is not running.",
                process.display_name
            ));
        }
    }
    // The stock Master autosaver broadcasts master_autosaverupdate. Sending a
    // second c_save to each secondary would request another coordinated save.
    dispatch_managed_stdin_command(
        state,
        &details.summary.id,
        Some("master"),
        "c_save()",
        details.active_run.as_ref().map(|run| run.run_id),
    )
    .await
}

pub(super) fn enabled_processes(
    details: &InstanceDetails,
) -> Result<Vec<&app_core::InstanceProcessState>, String> {
    let settings: Value =
        serde_json::from_str(&details.settings_json).map_err(|error| error.to_string())?;
    let shards = app_core::dst_shards::dst_shards(&settings)?;
    let run = details
        .active_run
        .as_ref()
        .ok_or("Start DST before saving its world.")?;
    shards
        .into_iter()
        .map(|shard| {
            run.processes
                .iter()
                .find(|process| {
                    process.process_key == shard.process_key
                        && process.status.eq_ignore_ascii_case("running")
                })
                .ok_or_else(|| {
                    format!(
                        "Start every enabled DST shard before saving; {} is not running.",
                        shard.directory
                    )
                })
        })
        .collect()
}
