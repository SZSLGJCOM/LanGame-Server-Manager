use std::collections::BTreeMap;

use app_steamcmd::{GameInstallLifecycleGuard, InstallLockContext};
use app_storage::{InstanceStorageResources, StoragePaths};

use super::steamcmd_error_message;

async fn acquire(
    install_locks: &InstallLockContext,
    resources: &InstanceStorageResources,
    fallback_id: &str,
) -> Result<GameInstallLifecycleGuard, String> {
    let fallback = format!("archive-{fallback_id}");
    app_steamcmd::acquire_game_install_lifecycle(
        install_locks,
        resources.module_id.as_deref().unwrap_or(&fallback),
        &resources.program_roots,
    )
    .await
    .map_err(|error| steamcmd_error_message(&error))
}

fn ensure_unchanged(
    before: &InstanceStorageResources,
    after: &InstanceStorageResources,
) -> Result<(), String> {
    if before != after {
        return Err(String::from("实例或归档的程序来源已改变，请刷新后重试。"));
    }
    Ok(())
}

pub(super) async fn acquire_retirement(
    install_locks: &InstallLockContext,
    paths: &StoragePaths,
    instance_id: &str,
) -> Result<GameInstallLifecycleGuard, String> {
    let resources = app_storage::read_instance_retirement_resources(paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let guard = acquire(install_locks, &resources, instance_id).await?;
    let current = app_storage::read_instance_retirement_resources(paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    ensure_unchanged(&resources, &current)?;
    Ok(guard)
}

pub(super) async fn acquire_archive(
    install_locks: &InstallLockContext,
    paths: &StoragePaths,
    archive_id: &str,
    resources: &InstanceStorageResources,
) -> Result<GameInstallLifecycleGuard, String> {
    let guard = acquire(install_locks, resources, archive_id).await?;
    let current = app_storage::read_instance_archive_resources(paths, archive_id)
        .await
        .map_err(|error| error.to_string())?;
    ensure_unchanged(resources, &current)?;
    Ok(guard)
}

pub(super) async fn acquire_archive_cleanup(
    install_locks: &InstallLockContext,
    paths: &StoragePaths,
    archive_id: &str,
    resources: &InstanceStorageResources,
) -> Result<GameInstallLifecycleGuard, String> {
    let guard = acquire(install_locks, resources, archive_id).await?;
    let current = app_storage::read_instance_archive_cleanup_resources(paths, archive_id)
        .await
        .map_err(|error| error.to_string())?;
    ensure_unchanged(resources, &current)?;
    Ok(guard)
}

// Recovery can span several games with overlapping external paths. Acquire
// their union once, so a lease never waits for another root it already owns.
pub(super) async fn acquire_recovery(
    install_locks: &InstallLockContext,
    paths: &StoragePaths,
    instance_ids: &[String],
) -> Result<GameInstallLifecycleGuard, String> {
    let mut snapshots = Vec::with_capacity(instance_ids.len());
    let mut modules = BTreeMap::<String, Vec<std::path::PathBuf>>::new();
    for id in instance_ids {
        let resources = app_storage::read_instance_retirement_resources(paths, id)
            .await
            .map_err(|error| error.to_string())?;
        let module = resources
            .module_id
            .clone()
            .unwrap_or_else(|| format!("archive-{id}"));
        modules
            .entry(module)
            .or_default()
            .extend(resources.program_roots.iter().cloned());
        snapshots.push((id, resources));
    }
    let resources: Vec<_> = modules.into_iter().collect();
    let guard = app_steamcmd::acquire_game_install_lifecycles(install_locks, &resources)
        .await
        .map_err(|error| steamcmd_error_message(&error))?;
    for (id, before) in snapshots {
        let after = app_storage::read_instance_retirement_resources(paths, id)
            .await
            .map_err(|error| error.to_string())?;
        ensure_unchanged(&before, &after)?;
    }
    Ok(guard)
}
