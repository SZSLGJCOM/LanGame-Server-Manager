use super::commands_program_storage::instance_root;
use super::*;

#[path = "ark_tools_install.rs"]
mod install;
#[path = "ark_tools_protocol.rs"]
pub(crate) mod protocol;
pub use protocol::SpawnInput;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstanceInput {
    pub instance_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrepareInput {
    pub instance_id: String,
    pub allow_matching_symbols_download: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    pub installed: bool,
    pub connected: bool,
    pub issue: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpawnResult {
    pub instance_id: String,
    pub request_id: String,
    pub creature: protocol::Creature,
}

#[cfg(windows)]
fn embedded(module: &str) -> Result<install::EmbeddedArkTools, String> {
    match protocol::edition(module)? {
        "ase" => Ok(install::EmbeddedArkTools {
            plugin: include_bytes!(concat!(env!("OUT_DIR"), "/ark-tools/LgsmArkTools-ase.dll")),
            proxy: None,
        }),
        _ => Ok(install::EmbeddedArkTools {
            plugin: include_bytes!(concat!(env!("OUT_DIR"), "/ark-tools/LgsmArkTools-asa.dll")),
            proxy: Some(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/ark-tools/asa-version.dll"
            ))),
        }),
    }
}

#[cfg(not(windows))]
fn embedded(_module: &str) -> Result<install::EmbeddedArkTools, String> {
    Err("ARK creature extensions require a Windows server host.".into())
}

async fn instance(storage: &StorageBootstrap, id: &str) -> Result<InstanceDetails, String> {
    let details = read_instance_details(&storage.paths, id)
        .await
        .map_err(|e| e.to_string())?;
    protocol::edition(&details.summary.module_id)?;
    Ok(details)
}

async fn owned_root(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
) -> Result<PathBuf, String> {
    let instance_path = instance_root(details)?;
    if app_storage::instance_program_mode(instance_path).map_err(|e| e.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("请先在实例维护中分离为独立服务器程序，再安装方舟生物工具扩展。".into());
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root(details)?)
        .map_err(|e| e.to_string())?;
    let binding = app_storage::read_instance_program_install(&storage.paths, &details.summary.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Instance program ownership is missing.")?;
    // A first independent instance can exclusively reference the library in
    // place. The storage reader validates its sole database reference; the
    // marker validates the program identity. Shared libraries remain rejected.
    let ownership_matches = match binding.install.scope {
        app_storage::ProgramInstallScope::Instance => {
            binding.install.owner_instance_id.as_deref() == Some(details.summary.id.as_str())
        }
        app_storage::ProgramInstallScope::Library => {
            binding.install.owner_instance_id.is_none()
                && app_storage::instance_uses_exclusive_program(instance_path)
                    .map_err(|e| e.to_string())?
        }
    };
    if binding.runtime_mode != "independent"
        || !ownership_matches
        || binding.install.module_id != details.summary.module_id
        || fs::canonicalize(&binding.install.install_root).map_err(|e| e.to_string())?
            != fs::canonicalize(&root).map_err(|e| e.to_string())?
    {
        return Err("Instance program ownership does not match its runtime.".into());
    }
    Ok(root)
}

pub(super) async fn ensure_can_start_instance(details: &InstanceDetails) -> Result<(), String> {
    if !matches!(
        details.summary.module_id.as_str(),
        "arksurvivalevolved" | "arksurvivalascended"
    ) {
        return Ok(());
    }
    let root = app_storage::resolve_instance_runtime_root(instance_root(details)?)
        .map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || install::ensure_can_start(&root))
        .await
        .map_err(|error| format!("ARK extension recovery inspection failed: {error}"))?
}

async fn ensure_endpoint(details: &InstanceDetails) -> Result<(), String> {
    let port = details
        .ports
        .iter()
        .find(|p| p.name == "rcon" && p.protocol.eq_ignore_ascii_case("tcp"))
        .ok_or("This instance has no TCP RCON binding.")?
        .port;
    let targets =
        commands_runtime_supervision::build_window_inspection_targets_from_instance(details);
    if targets.is_empty() {
        return Err("The ARK server is not running under this manager.".into());
    }
    let target = normalize_query_host(&details.summary.bind_ip)
        .parse::<std::net::IpAddr>()
        .map_err(|_| "RCON requires an IP-address endpoint")?;
    tokio::task::spawn_blocking(move || {
        let endpoints = WindowsPlatform::inspect_process_network_endpoints(&targets, &[port])?;
        let local_addresses = if_addrs::get_if_addrs()
            .map_err(|e| format!("Cannot verify the local RCON address: {e}"))?
            .into_iter()
            .map(|interface| interface.ip())
            .collect::<Vec<_>>();
        if !endpoints.endpoints.iter().any(|endpoint| {
            endpoint.local_port == port
                && endpoint.protocol.eq_ignore_ascii_case("tcp")
                && rcon_endpoint_covers(&endpoint.local_address, target, &local_addresses)
        }) {
            return Err(
                "The RCON listener does not belong to this instance's current server process."
                    .into(),
            );
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("RCON ownership inspection failed: {e}"))?
}

pub(super) fn rcon_endpoint_covers(
    local: &str,
    target: std::net::IpAddr,
    interfaces: &[std::net::IpAddr],
) -> bool {
    let Ok(local) = local.parse::<std::net::IpAddr>() else {
        return false;
    };
    let local = local.to_canonical();
    let target = target.to_canonical();
    if target.is_unspecified() {
        return false;
    }
    if local == target {
        return true;
    }
    // A wildcard listener covers only addresses on this host and in its own
    // family. A TCP6 wildcard alone does not prove IPv4 dual-stack acceptance.
    local.is_unspecified()
        && local.is_ipv4() == target.is_ipv4()
        && (target.is_loopback() || interfaces.iter().any(|ip| ip.to_canonical() == target))
}

async fn dispatch(details: &InstanceDetails, command: &str) -> Result<String, String> {
    dispatch_source_rcon_command(
        details,
        command,
        Some("rcon"),
        Some("admin_password"),
        Some("rcon_enabled"),
    )
    .await
}

async fn handshake(details: &InstanceDetails) -> Result<(), String> {
    let id = uuid::Uuid::new_v4().simple().to_string();
    let response = dispatch(details, &format!("LgsmArkTools.Status {id}")).await?;
    protocol::status(&response, &details.summary.module_id, &id)
}

#[tauri::command]
pub async fn read_ark_tools_status(
    state: tauri::State<'_, DesktopState>,
    input: InstanceInput,
) -> Result<ToolsStatus, String> {
    let _operation = state.begin_storage_context_operation("ARK creature tool inspection")?;
    let storage = bootstrap_storage().map_err(|e| e.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|e| e.to_string())?;
    reconcile_runtime_state(&state).await?;
    let _guard = state.acquire_instance_mutation(&input.instance_id).await;
    let details = instance(&storage, &input.instance_id).await?;
    let root = owned_root(&storage, &details).await?;
    let payload = embedded(&details.summary.module_id)?;
    let module = details.summary.module_id.clone();
    let status = tokio::task::spawn_blocking(move || install::inspect(&root, &module, payload))
        .await
        .map_err(|e| format!("ARK extension inspection failed: {e}"))??;
    if !status.ready || details.active_run.is_none() {
        return Ok(ToolsStatus {
            installed: status.ready,
            connected: false,
            issue: status.issue,
        });
    }
    let connected = async {
        ensure_endpoint(&details).await?;
        handshake(&details).await
    }
    .await;
    Ok(ToolsStatus {
        installed: true,
        connected: connected.is_ok(),
        issue: connected.err(),
    })
}

#[tauri::command]
pub async fn prepare_ark_tools(
    state: tauri::State<'_, DesktopState>,
    input: PrepareInput,
) -> Result<ToolsStatus, String> {
    let operation = state.begin_storage_context_operation("ARK creature extension installation")?;
    let storage = bootstrap_storage().map_err(|e| e.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|e| e.to_string())?;
    reconcile_runtime_state(&state).await?;
    let guard = state.acquire_instance_mutation(&input.instance_id).await;
    let details = instance(&storage, &input.instance_id).await?;
    if details.active_run.is_some()
        || state
            .runtime_supervisor
            .lock()
            .map_err(|_| "Runtime supervisor lock poisoned.")?
            .is_tracked(&input.instance_id)
        || state
            .pending_runtime_start_instance_ids()?
            .contains(&input.instance_id)
    {
        return Err("请先停止实例并取消待启动操作，再安装方舟生物工具扩展。".into());
    }
    if details.summary.module_id == "arksurvivalascended" && !input.allow_matching_symbols_download
    {
        return Err("ASA requires permission to download server-version symbol offsets from the extension maintainer's CDN.".into());
    }
    let root = owned_root(&storage, &details).await?;
    let install_guard = app_steamcmd::acquire_game_install_lifecycle(
        &details.summary.module_id,
        std::slice::from_ref(&root),
    )
    .await
    .map_err(|e| steamcmd_error_message(&e))?;
    app_storage::ensure_program_archive_dependencies(&storage.paths, &root)
        .await
        .map_err(|e| e.to_string())?;
    let payload = embedded(&details.summary.module_id)?;
    // A closed tab must not release the instance lock halfway through publication.
    spawn_storage_context_task(&operation, async move {
        let (_instance, _install) = (guard, install_guard);
        let status = install::install(root, details.summary.module_id, payload).await?;
        Ok(ToolsStatus {
            installed: status.ready,
            connected: false,
            issue: status.issue,
        })
    })
    .await
    .map_err(|e| format!("ARK extension installation task failed: {e}"))?
}

#[tauri::command]
pub async fn spawn_ark_creature(
    state: tauri::State<'_, DesktopState>,
    input: SpawnInput,
) -> Result<SpawnResult, String> {
    let command = protocol::spawn_command(&input)?;
    let operation = state.begin_storage_context_operation("ARK creature generation")?;
    let storage = bootstrap_storage().map_err(|e| e.to_string())?;
    initialize_database(&storage.paths)
        .await
        .map_err(|e| e.to_string())?;
    reconcile_runtime_state(&state).await?;
    let guard = state.acquire_instance_mutation(&input.instance_id).await;
    let details = instance(&storage, &input.instance_id).await?;
    ensure_endpoint(&details).await?;
    // Retain ownership through both the mutation and independent read-back, even
    // if the IPC waiter disappears. Never automatically resend a mutation.
    spawn_storage_context_task(&operation, async move {
        let _instance = guard;
        handshake(&details).await?;
        let response = dispatch(&details, &command).await.map_err(|e| format!("生成请求的回执未确认；请检查服务器后再决定是否重试。{e}"))?;
        let spawned = protocol::creature(&response, &details.summary.module_id, "spawn", &input.request_id)
            .map_err(|e| format!("生成请求已发出，但结果未确认；请检查服务器后再决定是否重试。{e}"))?;
        let response = dispatch(&details, &format!("LgsmArkTools.Inspect {} {} {}", input.request_id, spawned.id1, spawned.id2))
            .await.map_err(|e| format!("生物 {}:{} 已返回，但读回失败；请勿重复生成。{e}", spawned.id1, spawned.id2))?;
        let inspected = protocol::creature(&response, &details.summary.module_id, "inspect", &input.request_id)
            .map_err(|e| format!("生物 {}:{} 已返回，但读回结果未确认；请勿重复生成。{e}", spawned.id1, spawned.id2))?;
        protocol::verify_spawn(&input, &spawned, &inspected)
            .map_err(|error| format!("生物 {}:{} 已返回，但与请求不一致；请勿重复生成。{error}", inspected.id1, inspected.id2))?;
        append_desktop_app_log(&storage, "info", "instance.ark_creature.spawned", "ARK creature generated and independently read back",
            json!({"instance_id":input.instance_id,"request_id":input.request_id,"id1":inspected.id1,"id2":inspected.id2,"class":inspected.class_name,"level":inspected.level}));
        Ok(SpawnResult { instance_id: input.instance_id, request_id: input.request_id, creature: inspected })
    }).await.map_err(|e| format!("ARK creature generation task failed: {e}"))?
}
