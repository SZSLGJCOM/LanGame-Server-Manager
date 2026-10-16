use super::*;

use crate::live_players::dragonwilds_control::{bundled_payload, installation};

/// Called inside the start worker while it owns both instance and program
/// permits. The roster loader remains independently usable and unchanged.
pub(super) async fn prepare(
    instance: &InstanceDetails,
    log_path: Option<&Path>,
) -> Result<(), String> {
    if instance.summary.module_id != "runescapedragonwilds" {
        return Ok(());
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|error| error.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Ok(());
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root)
        .map_err(|error| error.to_string())?;
    let prepared = tokio::task::spawn_blocking(move || {
        if !super::commands_dragonwilds_player_query::owns_compatible_loader(&root)? {
            return Ok(false);
        }
        super::commands_dragonwilds_player_query::ensure_program_stopped(&root)?;
        let (script, bridge) = bundled_payload();
        installation::install(&root, script, bridge)?;
        Ok::<_, String>(true)
    })
    .await
    .map_err(|_| "Dragonwilds control preparation worker failed.")??;
    if prepared && let Some(path) = log_path {
        append_startup_console_line(
            path,
            "Dragonwilds 玩家管理扩展已准备，正在启动游戏服务器。",
            "",
        )?;
    }
    Ok(())
}

pub(super) async fn execute(
    state: &DesktopState,
    details: InstanceDetails,
    command: String,
    permit: tokio::sync::OwnedMutexGuard<()>,
) -> Result<(), String> {
    let operation = state.begin_storage_context_operation("Dragonwilds native moderation")?;
    let storage = state.bootstrap_storage()?;
    let instance_id = details.summary.id.clone();
    let action = command
        .split_ascii_whitespace()
        .next()
        .ok_or("The native command is empty.")?
        .to_owned();
    let receipt = spawn_blocking_storage_context_task(&operation, move || {
        // Window/pipe cancellation does not release the lifecycle permit while
        // native dispatch, persistence, or the correlated receipt is pending.
        let _permit = permit;
        crate::live_players::dragonwilds_control::execute_blocking(&details, &command)
    })
    .await
    .map_err(|_| "The Dragonwilds moderation worker failed; its outcome is unknown.")??;
    append_desktop_app_log(
        &storage,
        "info",
        "instance.dragonwilds.moderation_confirmed",
        "Native moderation and its required effects were confirmed",
        json!({"instance_id": instance_id, "action": action,
            "account_reference": super::commands_live_players::audit_reference(&receipt.account_id),
            "banned": receipt.banned}),
    );
    Ok(())
}
