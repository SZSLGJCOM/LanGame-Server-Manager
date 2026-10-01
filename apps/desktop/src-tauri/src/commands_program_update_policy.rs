use super::*;
use app_core::InstanceProgramUpdatePolicy;

/// The caller retains its instance mutation lock before acquiring this lease.
/// Shared startups hold only their own instance lock, so policy changes also
/// need the shared install lease until the new setting is durably saved.
pub(super) async fn acquire_policy_change(
    state: &DesktopState,
    storage: &StorageBootstrap,
    current: &InstanceDetails,
    incoming_settings_json: &str,
) -> Result<Option<app_steamcmd::GameInstallLifecycleGuard>, String> {
    let previous = InstanceProgramUpdatePolicy::from_settings_json(&current.settings_json)?;
    let next = InstanceProgramUpdatePolicy::from_settings_json(incoming_settings_json)?;
    if previous == next {
        return Ok(None);
    }
    ensure_instance_idle(state, storage, current).await?;
    let root = app_storage::resolve_instance_runtime_root(
        super::super::commands_program_storage::instance_root(current)?,
    )
    .map_err(|error| error.to_string())?;
    let guard = app_steamcmd::acquire_game_install_lifecycle(
        &current.summary.module_id,
        std::slice::from_ref(&root),
    )
    .await
    .map_err(|error| steamcmd_error_message(&error))?;
    // A pending start can be admitted while this save waits for another
    // instance's update. Recheck under both leases before saving the policy.
    let refreshed = read_instance_details(&storage.paths, &current.summary.id)
        .await
        .map_err(|error| error.to_string())?;
    let refreshed_root = app_storage::resolve_instance_runtime_root(
        super::super::commands_program_storage::instance_root(&refreshed)?,
    )
    .map_err(|error| error.to_string())?;
    guard
        .ensure_scope(&refreshed.summary.module_id, &refreshed_root)
        .map_err(|error| steamcmd_error_message(&error))?;
    ensure_instance_idle(state, storage, &refreshed).await?;
    Ok(Some(guard))
}

async fn ensure_instance_idle(
    state: &DesktopState,
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
) -> Result<(), String> {
    let id = &instance.summary.id;
    let active = read_active_instance_run(&storage.paths, id)
        .await
        .map_err(|error| error.to_string())?
        .is_some();
    let tracked = state
        .runtime_supervisor
        .lock()
        .map_err(|_| String::from("runtime supervisor lock poisoned"))?
        .is_tracked(id);
    if active
        || tracked
        || instance.summary.active_process_count > 0
        || matches!(
            instance.summary.status,
            InstanceStatus::Starting | InstanceStatus::Running | InstanceStatus::Stopping
        )
        || state.pending_runtime_start_instance_ids()?.contains(id)
    {
        return Err(String::from(
            "请先停止实例并取消待启动操作，再更改程序更新策略。",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_program_update_policy_tests.rs"]
mod tests;
