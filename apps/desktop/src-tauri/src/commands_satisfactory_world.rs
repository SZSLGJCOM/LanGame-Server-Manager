use super::*;
use app_core::satisfactory_world::*;

#[path = "satisfactory_world_api.rs"]
mod api;
#[path = "satisfactory_world_context.rs"]
mod context;
#[path = "satisfactory_world_credentials.rs"]
mod credentials;
#[path = "satisfactory_world_protocol.rs"]
mod protocol;
#[path = "satisfactory_world_service.rs"]
mod service;

#[cfg(all(test, windows))]
#[path = "satisfactory_world_native_tests.rs"]
mod native_tests;

struct Prepared {
    instance: InstanceDetails,
    api: Option<api::Api>,
    _instance_lock: tokio::sync::OwnedMutexGuard<()>,
}

async fn prepare(
    state: &tauri::State<'_, DesktopState>,
    instance_id: &str,
) -> Result<Prepared, String> {
    let storage = bootstrap_storage().map_err(|error| error.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    reconcile_runtime_state(state).await?;
    let instance_lock = state.acquire_instance_mutation(instance_id).await;
    let instance = read_instance_details(&storage.paths, instance_id)
        .await
        .map_err(|error| error.to_string())?;
    let api = context::resolve(state, &instance)?
        .map(api::Api::new)
        .transpose()?;
    Ok(Prepared {
        instance,
        api,
        _instance_lock: instance_lock,
    })
}

fn running_api(prepared: &Prepared) -> Result<&api::Api, String> {
    prepared
        .api
        .as_ref()
        .ok_or_else(|| "Start the Satisfactory server before using its world controls.".into())
}

#[tauri::command]
pub async fn read_satisfactory_world_settings(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let operation = state.begin_storage_context_operation("Satisfactory world settings read")?;
    let prepared = prepare(&state, &instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        let _instance_lock = prepared._instance_lock;
        if let Some(api) = &prepared.api {
            service::read(api).await
        } else {
            protocol::empty_snapshot(&instance_id, SatisfactoryConnectionStatus::Stopped, None)
        }
    })
    .await
    .map_err(|error| format!("Satisfactory world settings read did not complete: {error}"))?
}

#[tauri::command]
pub async fn setup_satisfactory_server(
    state: tauri::State<'_, DesktopState>,
    input: SetupSatisfactoryServerInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let operation = state.begin_storage_context_operation("Satisfactory server claim")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::setup(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory claim task did not complete: {error}"))?
}

#[tauri::command]
pub async fn authorize_satisfactory_server(
    state: tauri::State<'_, DesktopState>,
    input: AuthorizeSatisfactoryServerInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let operation =
        state.begin_storage_context_operation("Satisfactory management authorization")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::authorize(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory authorization task did not complete: {error}"))?
}

#[tauri::command]
pub async fn read_satisfactory_admin_password(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<Option<String>, String> {
    let operation =
        state.begin_storage_context_operation("Satisfactory administrator password reveal")?;
    let prepared = prepare(&state, &instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        let _instance_lock = prepared._instance_lock;
        credentials::read(
            &context::credential_identity(&prepared.instance)?,
            "admin-password",
        )
        .await
    })
    .await
    .map_err(|error| {
        format!("Satisfactory administrator password read did not complete: {error}")
    })?
}

#[tauri::command]
pub async fn write_satisfactory_world_rules(
    state: tauri::State<'_, DesktopState>,
    input: WriteSatisfactoryWorldRulesInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let operation = state.begin_storage_context_operation("Satisfactory world rules write")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::write_rules(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory world rules task did not complete: {error}"))?
}

#[tauri::command]
pub async fn create_satisfactory_world(
    state: tauri::State<'_, DesktopState>,
    input: CreateSatisfactoryWorldInput,
) -> Result<SatisfactoryWorldOperationResult, String> {
    let operation = state.begin_storage_context_operation("Satisfactory world creation")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::create(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory world creation task did not complete: {error}"))?
}

#[tauri::command]
pub async fn write_satisfactory_room(
    state: tauri::State<'_, DesktopState>,
    input: WriteSatisfactoryRoomInput,
) -> Result<SatisfactoryWorldSnapshot, String> {
    let operation = state.begin_storage_context_operation("Satisfactory room settings write")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::room(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory room settings task did not complete: {error}"))?
}

#[tauri::command]
pub async fn load_satisfactory_save(
    state: tauri::State<'_, DesktopState>,
    input: LoadSatisfactorySaveInput,
) -> Result<SatisfactoryWorldOperationResult, String> {
    let operation = state.begin_storage_context_operation("Satisfactory world selection")?;
    let prepared = prepare(&state, &input.instance_id).await?;
    spawn_storage_context_task(&operation, async move {
        service::load(running_api(&prepared)?, input).await
    })
    .await
    .map_err(|error| format!("Satisfactory world selection task did not complete: {error}"))?
}
