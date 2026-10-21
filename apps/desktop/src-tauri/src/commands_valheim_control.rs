//! Shared automatic and explicit stopped-instance Valheim extension preparation.
use super::*;
use crate::live_players::valheim_control::io;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    plugin_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_plugin_sha256: Option<String>,
}

#[tauri::command]
pub async fn prepare_valheim_player_control(
    state: tauri::State<'_, DesktopState>,
    instance_id: String,
) -> Result<InstanceDetails, String> {
    let operation = state.begin_storage_context_operation("Valheim account control preparation")?;
    let storage = state.bootstrap_storage()?;
    initialize_database(&storage.paths)
        .await
        .map_err(|error| error.to_string())?;
    let instance = read_instance_details(&storage.paths, &instance_id)
        .await
        .map_err(|error| error.to_string())?;
    if instance.summary.module_id != "valheim" {
        return Err("Account control preparation requires a Valheim instance.".into());
    }
    let original_root = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(&instance)?,
    )
    .map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || verify_supported_game(&original_root))
        .await
        .map_err(|_| "Valheim build inspection worker failed.")??;
    let lock =
        super::commands_mods::acquire_stopped_instance_mod_mutation(&state, &storage, &instance_id)
            .await?;
    let program = super::commands_program_storage::prepare_instance_mod_program(
        &state.install_locks,
        &storage,
        &operation,
        &instance_id,
        lock,
    )
    .await?;
    let worker_operation = operation.clone();
    // The worker retains both the instance and program guards until all writes
    // settle, even if the UI closes while its explicit preparation is running.
    spawn_storage_context_task(&operation, async move {
        let _program = program;
        let instance = read_instance_details(&storage.paths, &instance_id)
            .await
            .map_err(|error| error.to_string())?;
        ensure_installed(&storage, &instance, &worker_operation).await?;
        read_instance_details(&storage.paths, &instance_id)
            .await
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("Valheim account control worker failed: {error}"))?
}

/// The startup caller already holds the exclusive runtime and instance leases.
/// Keep them until this future settles; this path never reacquires those locks.
pub(super) async fn ensure_for_start<L: crate::state::StorageContextTaskLease>(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    operation: &L,
) -> Result<Option<(String, String)>, String> {
    if instance.summary.module_id != "valheim" {
        return Ok(None);
    }
    ensure_installed(storage, instance, operation).await?;
    let instance = instance.clone();
    spawn_blocking_storage_context_task(operation, move || startup_environment(&instance))
        .await
        .map_err(|_| "Valheim control startup session worker failed.")?
}

async fn ensure_installed<L: crate::state::StorageContextTaskLease>(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    operation: &L,
) -> Result<(), String> {
    ensure_stopped(instance)?;
    let root = independent_root(instance)?;
    let probe_root = root.clone();
    spawn_blocking_storage_context_task(operation, move || {
        super::commands_managed_save_control::ensure_stopped_program(&probe_root, "valheim")?;
        verify_supported_game(&probe_root)?;
        inspect_plugin_destination(&probe_root).map(|_| ())
    })
    .await
    .map_err(|_| "Valheim extension inspection worker failed.")??;
    super::commands_managed_save_control::ensure_bepinex_loader(
        storage,
        instance,
        operation,
        "https://thunderstore.io/package/denikson/BepInExPack_Valheim/5.4.2351/",
    )
    .await?;
    spawn_blocking_storage_context_task(operation, move || {
        super::commands_managed_save_control::ensure_stopped_program(&root, "valheim")?;
        install_owned_plugin(&root, io::bundled_payload())
    })
    .await
    .map_err(|_| "Valheim extension installation worker failed.")?
}

fn ensure_stopped(instance: &InstanceDetails) -> Result<(), String> {
    if !matches!(
        instance.summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) || instance.summary.active_process_count != 0
        || instance
            .active_run
            .as_ref()
            .is_some_and(|run| run.process_count != 0)
    {
        return Err("Valheim control may only be prepared before the server starts.".into());
    }
    Ok(())
}

fn independent_root(instance: &InstanceDetails) -> Result<PathBuf, String> {
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|error| error.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err(
            "Valheim account control requires this instance's exclusive program directory.".into(),
        );
    }
    app_storage::resolve_instance_runtime_root(instance_root).map_err(|error| error.to_string())
}

pub(super) const NATIVE_PREPARATION_FILES: [&str; 3] = [
    "valheim_server_Data/Managed/assembly_valheim.dll",
    "valheim_server_Data/Managed/assembly_utils.dll",
    "valheim_server_Data/Managed/Splatform.dll",
];

fn verify_supported_game(root: &Path) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    let managed = root.join("valheim_server_Data/Managed");
    let _managed = io::pin_directory(&managed)?;
    let main = io::digest(&io::read_regular(
        &root.join(NATIVE_PREPARATION_FILES[0]),
        64 * 1024 * 1024,
    )?);
    let utils = io::digest(&io::read_regular(
        &root.join(NATIVE_PREPARATION_FILES[1]),
        16 * 1024 * 1024,
    )?);
    let platform = io::digest(&io::read_regular(
        &root.join(NATIVE_PREPARATION_FILES[2]),
        16 * 1024 * 1024,
    )?);
    let known = [
        (
            "ef8c8d3a1f45694ef7a701e8e2a25bf8a3874c951a2639c81dfe614ce3aa5fa3",
            "0d66bd029bf753e2721304c775126b34af329bfcd6e632d8d38edaa96b5cf1a5",
        ),
        (
            "0dfc7e81436f822121148efed859ba1e661d58b9484bd45e3b6f33b6dd86bfad",
            "63e114e7cfdcc38e4f4391c18b91f774290274d2b9200e6d85d757cbe3673e18",
        ),
    ];
    if platform != "8953c24a2f2af7ae257cc64fb36dde9d102c871c29b4cdd97d0c3c001de5bd5a"
        || !known.iter().any(|(expected_main, expected_utils)| {
            main == *expected_main && utils == *expected_utils
        })
    {
        return Err("This Valheim server build has not been verified for native account control. Existing game files were preserved.".into());
    }
    Ok(())
}

fn read_owner(root: &Path) -> Result<Option<Ownership>, String> {
    let path = root.join(io::OWNER);
    match std::fs::symlink_metadata(&path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    decode_owner(&io::read_regular(&path, 4096)?).map(Some)
}

fn decode_owner(bytes: &[u8]) -> Result<Ownership, String> {
    let owner: Ownership = serde_json::from_slice(bytes)
        .map_err(|_| "The Valheim account control ownership record is invalid.")?;
    if owner.protocol != 1 || owner.module_id != "valheim" {
        return Err(
            "The Valheim account control ownership belongs to a different protocol or module."
                .into(),
        );
    }
    Ok(owner)
}

fn inspect_plugin_destination(root: &Path) -> Result<Ownership, String> {
    let mut guards = io::pin_directories(root)?;
    let mut existing = root.to_owned();
    for part in ["BepInEx", "plugins"] {
        existing.push(part);
        match std::fs::symlink_metadata(&existing) {
            Ok(_) => guards.push(io::pin_directory(&existing)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    let plugin = root.join(io::PLUGIN);
    let directory = root.join(io::DIRECTORY);
    let previous = read_owner(root)?;
    // Missing targets do not make a dangling link an available destination.
    let plugin_exists = match std::fs::symlink_metadata(&plugin) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    let directory_exists = match std::fs::symlink_metadata(&directory) {
        Ok(_) => {
            guards.push(io::pin_directory(&directory)?);
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    if previous.is_none() && (plugin_exists || directory_exists) {
        return Err(
            "An unowned Valheim control plugin or directory already exists; it was preserved."
                .into(),
        );
    }
    let owner = previous.unwrap_or(Ownership {
        protocol: 1,
        module_id: "valheim".into(),
        plugin_sha256: None,
        pending_plugin_sha256: None,
    });
    verified_plugin_hash(root, &owner)?;
    Ok(owner)
}

fn verified_plugin_hash(root: &Path, owner: &Ownership) -> Result<Option<String>, String> {
    let current =
        io::read_optional(&root.join(io::PLUGIN), 1024 * 1024)?.map(|bytes| io::digest(&bytes));
    if let Some(hash) = &current
        && owner.plugin_sha256.as_ref() != Some(hash)
        && owner.pending_plugin_sha256.as_ref() != Some(hash)
    {
        return Err(
            "The Valheim control plugin was modified outside its installer; it was preserved."
                .into(),
        );
    }
    Ok(current)
}

fn install_owned_plugin(root: &Path, payload: &[u8]) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    let plugin = root.join(io::PLUGIN);
    let mut owner = inspect_plugin_destination(root)?;
    if payload.is_empty() || payload.len() > 1024 * 1024 {
        return Err(
            "The bundled Valheim control plugin is unavailable or exceeds its size limit.".into(),
        );
    }
    let hash = io::digest(payload);
    // A previous publication may already have installed its pending payload.
    // Keep those verified bytes as the rollback identity before recording a
    // different pending payload, including an application update after a crash.
    owner.plugin_sha256 = verified_plugin_hash(root, &owner)?;
    owner.pending_plugin_sha256 = Some(hash.clone());
    io::publish(
        &root.join(io::OWNER),
        &serde_json::to_vec(&owner).map_err(|error| error.to_string())?,
    )?;
    let _plugins = create_plain_descendants(root, &["BepInEx", "plugins"])?;
    io::publish(&plugin, payload)?;
    let _control = create_plain_descendants(root, &[io::DIRECTORY])?;
    owner.plugin_sha256 = Some(hash);
    owner.pending_plugin_sha256 = None;
    io::publish(
        &root.join(io::OWNER),
        &serde_json::to_vec(&owner).map_err(|error| error.to_string())?,
    )
}

fn create_plain_descendants(root: &Path, parts: &[&str]) -> Result<Vec<std::fs::File>, String> {
    let mut guards = io::pin_directories(root)?;
    let mut path = root.to_owned();
    for part in parts {
        path.push(part);
        match std::fs::create_dir(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        guards.push(io::pin_directory(&path)?);
    }
    Ok(guards)
}

/// Refresh only a previously enabled plugin with known old/pending bytes.
/// This also finishes interrupted owned updates without a manual repair step.
fn refresh_prepared_plugin(root: &Path, payload: &[u8]) -> Result<(), String> {
    let _root = io::pin_directories(root)?;
    let _plugins = io::pin_directories(&root.join("BepInEx/plugins"))?;
    let _control = io::pin_directory(&root.join(io::DIRECTORY))?;
    let owner_path = root.join(io::OWNER);
    let original_owner = io::read_regular(&owner_path, 4096)?;
    let mut owner = decode_owner(&original_owner)?;
    let plugin_path = root.join(io::PLUGIN);
    let installed = io::read_regular(&plugin_path, 1024 * 1024)?;
    let installed_hash = io::digest(&installed);
    if owner.plugin_sha256.as_deref() != Some(installed_hash.as_str())
        && owner.pending_plugin_sha256.as_deref() != Some(installed_hash.as_str())
    {
        return Err(
            "The Valheim control plugin was modified outside its installer; it was preserved."
                .into(),
        );
    }
    if payload.is_empty() || payload.len() > 1024 * 1024 {
        return Err(
            "The bundled Valheim control plugin is unavailable or exceeds its size limit.".into(),
        );
    }
    if installed != payload || owner.pending_plugin_sha256.is_some() {
        let mut expected_owner = original_owner;
        if installed != payload {
            owner.plugin_sha256 = Some(installed_hash);
            owner.pending_plugin_sha256 = Some(io::digest(payload));
            let intent = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
            replace_prepared_file(&owner_path, &expected_owner, &intent)?;
            expected_owner = intent;
            replace_prepared_file(&plugin_path, &installed, payload)?;
        }
        owner.plugin_sha256 = Some(io::digest(payload));
        owner.pending_plugin_sha256 = None;
        let complete = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
        replace_prepared_file(&owner_path, &expected_owner, &complete)?;
    }
    io::verify_owner(&io::read_regular(&root.join(io::OWNER), 4096)?, payload)?;
    if io::read_regular(&root.join(io::PLUGIN), 1024 * 1024)? != payload {
        return Err("The refreshed Valheim control plugin could not be verified.".into());
    }
    Ok(())
}

fn replace_prepared_file(path: &Path, expected: &[u8], replacement: &[u8]) -> Result<(), String> {
    // Recheck the bounded ordinary leaf before the existing storage CAS. A
    // missing or changed destination is a conflict, never a fresh installation.
    if io::read_regular(path, 1024 * 1024)? != expected {
        return Err("A Valheim control file changed during its update; it was preserved.".into());
    }
    if !app_storage::compare_and_swap_file_atomically(path, expected, replacement)
        .map_err(|error| format!("Cannot update the prepared Valheim control file: {error}"))?
    {
        return Err("A Valheim control file changed during its update; it was preserved.".into());
    }
    Ok(())
}

/// Publishes the launch session after stopped-instance automatic preparation.
pub(super) fn startup_environment(
    instance: &InstanceDetails,
) -> Result<Option<(String, String)>, String> {
    if instance.summary.module_id != "valheim" {
        return Ok(None);
    }
    let root = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(instance)?,
    )
    .map_err(|error| error.to_string())?;
    if read_owner(&root)?.is_none() {
        return Ok(None);
    }
    ensure_stopped(instance)?;
    let root = independent_root(instance)?;
    let _root = io::pin_directories(&root)?;
    super::commands_managed_save_control::ensure_stopped_program(&root, "valheim")?;
    if super::commands_mod_staging::runtime_loader_requires_archive("valheim", &root)? {
        return Err("The prepared Valheim account control loader was changed or disabled. Prepare it again while the server is stopped.".into());
    }
    verify_supported_game(&root)?;
    refresh_prepared_plugin(&root, io::bundled_payload())?;
    let directory = root.join(io::DIRECTORY);
    let _control = io::pin_directory(&directory)?;
    let session =
        serde_json::json!({"protocol":1,"token":uuid::Uuid::new_v4().simple().to_string()});
    io::publish(
        &directory.join("session.json"),
        &serde_json::to_vec(&session).map_err(|error| error.to_string())?,
    )?;
    Ok(Some((
        "LGSM_VALHEIM_CONTROL_ROOT".into(),
        directory.to_string_lossy().into_owned(),
    )))
}

pub(super) fn is_prepared(instance: &InstanceDetails) -> Result<bool, String> {
    if instance.summary.module_id != "valheim" {
        return Ok(false);
    }
    let root = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(instance)?,
    )
    .map_err(|error| error.to_string())?;
    Ok(read_owner(&root)?.is_some())
}

pub(super) async fn sync_roster(
    storage: &StorageBootstrap,
    persistent: &app_storage::PlayerAccessPersistentMutationResult,
) -> Result<(), String> {
    // Authenticate availability before copying native role files. A failed
    // exchange leaves the persistent change intact and reports partial success.
    let available = crate::live_players::valheim_control::query(&persistent.details).await?;
    if !available
        .capabilities
        .contains(&crate::live_players::valheim_control::Operation::Reload)
    {
        return Err("The native Valheim extension does not support role reload; the persistent change remains saved.".into());
    }
    let current = app_storage::materialize_valheim_player_roster(
        &storage.paths,
        &persistent.details.summary.id,
        &persistent.field_key,
    )
    .await
    .map_err(|error| error.to_string())?;
    let snapshot = crate::live_players::valheim_control::reload(&current).await?;
    verify_role_state(
        &persistent.field_key,
        &persistent.live_target,
        persistent.operation,
        &snapshot,
    )
}

fn verify_role_state(
    field: &str,
    target: &str,
    operation: app_storage::PlayerAccessMutationOperation,
    snapshot: &crate::live_players::valheim_control::ControlSnapshot,
) -> Result<(), String> {
    let expected = match operation {
        app_storage::PlayerAccessMutationOperation::Add => true,
        app_storage::PlayerAccessMutationOperation::Remove => false,
    };
    let entries = match field {
        "admin_list" => &snapshot.access.admin,
        "permitted_list" => &snapshot.access.permitted,
        _ => return Err("This Valheim roster has no verified live role operation.".into()),
    };
    if entries.iter().any(|entry| entry == target) != expected {
        return Err("The native Valheim role roster did not reach the requested state; the persistent change remains saved.".into());
    }
    if field == "admin_list"
        && snapshot
            .players
            .iter()
            .filter(|peer| peer.platform_id == target)
            .any(|peer| peer.admin != expected)
    {
        return Err("The native Valheim player privilege did not reach the requested state; refresh before retrying.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_valheim_control_tests.rs"]
mod tests;
