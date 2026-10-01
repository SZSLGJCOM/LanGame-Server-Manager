use super::*;
use app_storage::{
    ArkClusterBackupRestoreResult, ArkClusterBackupSummary, ArkClusterIdentity,
    ArkClusterRecoveryResult, PendingArkClusterRestore,
};

#[derive(Debug, Deserialize)]
pub struct ListArkClusterBackupsInput {
    pub instance_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateArkClusterBackupInput {
    pub instance_id: String,
    pub expected_identity: ArkClusterIdentity,
    pub exclusive_root_confirmed: bool,
}

#[derive(Debug, Deserialize)]
pub struct RestoreArkClusterBackupInput {
    pub instance_id: String,
    pub expected_identity: ArkClusterIdentity,
    pub backup_id: String,
    pub exclusive_root_confirmed: bool,
}

#[tauri::command]
pub async fn list_ark_cluster_backups(
    state: tauri::State<'_, DesktopState>,
    input: ListArkClusterBackupsInput,
) -> Result<Vec<ArkClusterBackupSummary>, String> {
    let _operation = state.begin_storage_context_operation("ARK cluster snapshot listing")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    app_storage::list_ark_cluster_backups(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn create_ark_cluster_backup(
    state: tauri::State<'_, DesktopState>,
    input: CreateArkClusterBackupInput,
) -> Result<ArkClusterBackupSummary, String> {
    let operation = state.begin_storage_context_operation("ARK cluster snapshot creation")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let locks = lock_members(&state, &input.expected_identity).await?;
    spawn_storage_context_task(&operation, async move {
        let _locks = locks;
        app_storage::create_ark_cluster_backup(
            &storage.paths,
            &input.instance_id,
            &input.expected_identity,
            input.exclusive_root_confirmed,
        )
        .await
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Cluster snapshot task failed: {error}"))?
}

#[tauri::command]
pub async fn restore_ark_cluster_backup(
    state: tauri::State<'_, DesktopState>,
    input: RestoreArkClusterBackupInput,
) -> Result<ArkClusterBackupRestoreResult, String> {
    let operation = state.begin_storage_context_operation("ARK cluster restoration")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let locks = lock_members(&state, &input.expected_identity).await?;
    spawn_storage_context_task(&operation, async move {
        let _locks = locks;
        app_storage::restore_ark_cluster_backup(
            &storage.paths,
            &input.instance_id,
            &input.expected_identity,
            &input.backup_id,
            input.exclusive_root_confirmed,
        )
        .await
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Cluster restore task failed: {error}"))?
}

#[tauri::command]
pub async fn read_pending_ark_cluster_restore(
    state: tauri::State<'_, DesktopState>,
    input: ListArkClusterBackupsInput,
) -> Result<Option<PendingArkClusterRestore>, String> {
    let _operation =
        state.begin_storage_context_operation("ARK cluster pending restore inspection")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    app_storage::read_pending_ark_cluster_restore(&storage.paths, &input.instance_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn recover_ark_cluster_restore(
    state: tauri::State<'_, DesktopState>,
    input: RestoreArkClusterBackupInput,
) -> Result<ArkClusterRecoveryResult, String> {
    let operation =
        state.begin_storage_context_operation("ARK cluster interrupted restore recovery")?;
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(&state).await?;
    let locks = lock_members(&state, &input.expected_identity).await?;
    spawn_storage_context_task(&operation, async move {
        let _locks = locks;
        app_storage::recover_ark_cluster_restore(
            &storage.paths,
            &input.instance_id,
            &input.expected_identity,
            &input.backup_id,
            input.exclusive_root_confirmed,
        )
        .await
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Cluster recovery task failed: {error}"))?
}

async fn lock_members(
    state: &DesktopState,
    identity: &ArkClusterIdentity,
) -> Result<Vec<tokio::sync::OwnedMutexGuard<()>>, String> {
    if identity.member_ids.is_empty() || identity.member_ids.len() > 128 {
        return Err(String::from("Invalid cluster member snapshot"));
    }
    let mut ids = identity.member_ids.clone();
    ids.sort();
    ids.dedup();
    let mut locks = Vec::with_capacity(ids.len());
    for id in &ids {
        locks.push(state.try_acquire_instance_mutation(id).await.ok_or_else(|| format!("Cluster member {id} has another operation in progress. Wait for it to finish before modifying this cluster."))?);
    }
    Ok(locks)
}
