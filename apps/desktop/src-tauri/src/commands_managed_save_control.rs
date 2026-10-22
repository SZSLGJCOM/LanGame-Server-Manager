//! Automatic stopped-instance preparation and narrow current-world save requests.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "commands_managed_save_io.rs"]
pub(in crate::commands) mod io;
#[cfg(test)]
pub(super) use io::{pin_directories as guard_pin_directories, read_regular as guard_read_regular};
#[path = "commands_managed_save_payload.rs"]
mod payload;
#[path = "commands_managed_save_process.rs"]
mod process;
#[path = "commands_managed_save_redloader.rs"]
mod redloader;
#[path = "commands_managed_save_retired.rs"]
mod retired;
#[path = "commands_managed_save_transport.rs"]
mod transport;
pub(super) use retired::cleanup_for_start;

pub(crate) fn retire_owned_file(path: &Path, expected: &str) -> Result<(), String> {
    io::remove_owned(path, expected)
}

const DIRECTORY: &str = ".langame-world-save";
const OWNER: &str = ".langame-world-save.json";

pub(super) fn validate_startup_hook_environment<'a>(
    values: impl Iterator<Item = (&'a String, &'a String)>,
) -> Result<(), String> {
    if values
        .into_iter()
        .any(|(key, value)| key.eq_ignore_ascii_case("DOTNET_STARTUP_HOOKS") && !value.is_empty())
    {
        return Err("An existing .NET startup hook is configured for this launch. It was preserved; resolve the conflict before starting with this save extension.".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Profile {
    module: &'static str,
    image: &'static str,
    plugin: &'static str,
    environment: &'static str,
}

fn profile(module: &str) -> Result<Profile, String> {
    let (image, plugin, environment) = match module {
        "corekeeper" => (
            "CoreKeeperServer.exe",
            "CoreKeeperServer_Data/StreamingAssets/Mods/LanGameSaveControl/LanGame.CoreKeeper.Save.dll",
            "LGSM_COREKEEPER_SAVE_ROOT",
        ),
        "vrising" => (
            "VRisingServer.exe",
            "BepInEx/plugins/LanGame.VRising.Save.dll",
            "LGSM_VRISING_SAVE_ROOT",
        ),
        "sonsoftheforest" => (
            "SonsOfTheForestDS.exe",
            "Mods/LanGame.Sons.Save.dll",
            "LGSM_SONS_SAVE_ROOT",
        ),
        "barotrauma" => (
            "DedicatedServer.exe",
            ".langame-world-save/LanGame.Barotrauma.Save.dll",
            "DOTNET_STARTUP_HOOKS",
        ),
        _ => return Err("This module has no verified managed save extension.".into()),
    };
    Ok(Profile {
        module: match module {
            "corekeeper" => "corekeeper",
            "vrising" => "vrising",
            "sonsoftheforest" => "sonsoftheforest",
            _ => "barotrauma",
        },
        image,
        plugin,
        environment,
    })
}

pub(super) fn requires_automatic_setup(module: &str) -> bool {
    profile(module).is_ok()
}

pub(super) fn ensure_stopped_program(root: &Path, module: &str) -> Result<(), String> {
    let image = if module == "valheim" {
        "valheim_server.exe"
    } else {
        profile(module)?.image
    };
    process::ensure_image_scope(root, image, None)
}

fn validate_inherited_environment(game: Profile) -> Result<(), String> {
    if game.environment == "DOTNET_STARTUP_HOOKS"
        && std::env::var_os(game.environment).is_some_and(|value| !value.is_empty())
    {
        return Err("An existing .NET startup hook is configured. It was preserved; resolve the conflict before using this managed save extension.".into());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    source_sha256: String,
    files: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pending_files: BTreeMap<String, String>,
}

fn stopped(details: &InstanceDetails) -> Result<(), String> {
    if !matches!(
        details.summary.status,
        InstanceStatus::Stopped | InstanceStatus::Error
    ) || details.summary.active_process_count != 0
        || details
            .active_run
            .as_ref()
            .is_some_and(|run| run.process_count != 0)
    {
        return Err("Stop the instance before preparing its save extension.".into());
    }
    Ok(())
}

fn root(details: &InstanceDetails) -> Result<PathBuf, String> {
    let instance = super::commands_program_storage::instance_root(details)?;
    if app_storage::instance_program_mode(instance).map_err(|e| e.to_string())?
        != app_storage::InstanceProgramMode::Independent
    {
        return Err("The save extension requires this instance's exclusive program directory. Prepare it while stopped first.".into());
    }
    app_storage::resolve_instance_runtime_root(instance).map_err(|e| e.to_string())
}

/// The caller retains the stopped instance, exclusive program and storage leases
/// until this future and its blocking workers complete. It never reacquires
/// those locks or detaches a program directory.
pub(super) async fn ensure_for_start<L: crate::state::StorageContextTaskLease>(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    operation: &L,
) -> Result<Option<Vec<(String, String)>>, String> {
    if !requires_automatic_setup(&details.summary.module_id) {
        return Ok(None);
    }
    if !ensure_installed(storage, details, operation).await? {
        return Ok(None);
    }
    let details = details.clone();
    spawn_blocking_storage_context_task(operation, move || startup_environment(&details))
        .await
        .map_err(|_| "Native save startup session worker failed.")?
        .map(Some)
}

async fn ensure_installed<L: crate::state::StorageContextTaskLease>(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    operation: &L,
) -> Result<bool, String> {
    let game = profile(&details.summary.module_id)?;
    stopped(details)?;
    validate_inherited_environment(game)?;
    let runtime = root(details)?;
    let check = runtime.clone();
    let check_details = details.clone();
    let preparation = spawn_blocking_storage_context_task(operation, move || {
        process::ensure_instance_scope(&check_details, &check, game, None)?;
        let Some(_native) = payload::pin_supported_native(&check, game)? else {
            payload::deactivate_owned(&check, game)?;
            return Ok(None);
        };
        payload::inspect(&check, game)?;
        if game.module == "sonsoftheforest" {
            redloader::requires_archive(&check).map(Some)
        } else {
            Ok(Some(false))
        }
    })
    .await
    .map_err(|_| "Native save inspection worker failed.")??;
    let Some(needs_redloader) = preparation else {
        return Ok(false);
    };
    if game.module == "vrising" {
        ensure_bepinex_loader(
            storage,
            details,
            operation,
            "https://thunderstore.io/package/BepInEx/BepInExPack_V_Rising/1.733.2/",
        )
        .await?;
    }
    let archive = if needs_redloader {
        Some(redloader::download().await?)
    } else {
        None
    };
    let install_details = details.clone();
    spawn_blocking_storage_context_task(operation, move || -> Result<(), String> {
        process::ensure_instance_scope(&install_details, &runtime, game, None)?;
        if let Some(archive) = archive {
            redloader::install(&runtime, &archive)?;
        } else if game.module == "sonsoftheforest" {
            redloader::adopt_complete_release(&runtime)?;
        }
        payload::install(&runtime, game, false)?;
        Ok(())
    })
    .await
    .map_err(|_| "Native save preparation worker failed.")??;
    Ok(true)
}

/// Reuse an intact reviewed loader without network access. The fixed release
/// installer preserves operator configuration and rejects other loader builds.
pub(super) async fn ensure_bepinex_loader<L: crate::state::StorageContextTaskLease>(
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    operation: &L,
    release: &'static str,
) -> Result<(), String> {
    let module = details.summary.module_id.clone();
    let runtime = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(details)?,
    )
    .map_err(|e| e.to_string())?;
    let check = runtime.clone();
    let check_module = module.clone();
    let needs_archive = spawn_blocking_storage_context_task(operation, move || {
        super::commands_mod_staging::runtime_loader_requires_archive(&check_module, &check)
    })
    .await
    .map_err(|_| "Save loader inspection worker failed.")??;
    if !needs_archive {
        return Ok(());
    }
    let target =
        super::commands_mods::resolve_manual_mod_target(storage, details.summary.id.clone(), true)
            .await?;
    let descriptors = discover_modules(&storage.paths.modules_root).map_err(|e| e.to_string())?;
    let descriptor = find_descriptor(&descriptors, &module)?;
    let mods = module_mods_spec_from_manifest(&descriptor.manifest_toml)
        .ok_or("Save loader installation contract is missing.")?;
    let source = mods
        .source
        .as_ref()
        .ok_or("Save loader source is missing.")?;
    let prepared = tokio::time::timeout(
        Duration::from_secs(180),
        super::commands_mods::mod_online::prepare_thunderstore_install(
            &module,
            source,
            &[release.into()],
            &runtime,
        ),
    )
    .await
    .map_err(|_| "Save loader preparation exceeded 180 seconds.")??;
    spawn_blocking_storage_context_task(operation, move || {
        ensure_stopped_program(&runtime, &module)?;
        super::commands_mod_staging::stage_thunderstore_online_mod_sources(
            target,
            prepared.sources(),
            prepared.loader_archive(),
        )
        .map(|_| ())
    })
    .await
    .map_err(|_| "Save loader installation worker failed.")?
}

/// Called after automatic preparation while all startup leases remain held.
pub(super) fn startup_environment(
    details: &InstanceDetails,
) -> Result<Vec<(String, String)>, String> {
    let Ok(game) = profile(&details.summary.module_id) else {
        return Ok(Vec::new());
    };
    let runtime = app_storage::resolve_instance_runtime_root(
        super::commands_program_storage::instance_root(details)?,
    )
    .map_err(|e| e.to_string())?;
    if io::read_optional(&runtime.join(OWNER), 65536)?.is_none() {
        return Ok(Vec::new());
    }
    stopped(details)?;
    let runtime = root(details)?;
    validate_inherited_environment(game)?;
    process::ensure_instance_scope(details, &runtime, game, None)?;
    payload::install(&runtime, game, true)?;
    let directory = runtime.join(DIRECTORY);
    let _pin = io::pin_directory(&directory)?;
    process::ensure_instance_scope(details, &runtime, game, None)?;
    io::publish(
        &directory.join("session.txt"),
        format!(
            "LGSM_SAVE_1\nmodule={}\nrun={}\ntoken={}\n",
            game.module,
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
        .as_bytes(),
    )?;
    Ok(vec![(
        game.environment.into(),
        if game.environment == "DOTNET_STARTUP_HOOKS" {
            runtime.join(game.plugin).to_string_lossy().into_owned()
        } else {
            directory.to_string_lossy().into_owned()
        },
    )])
}

pub(super) async fn save_world(
    state: &DesktopState,
    details: &InstanceDetails,
    submitted_at_unix_ms: u128,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
) -> Result<InstanceRuntimeCommandResult, String> {
    profile(&details.summary.module_id)?;
    let mutation =
        Some(mutation.ok_or("Native saving requires the instance mutation reservation.")?);
    let operation = state.begin_storage_context_operation("native immediate save")?;
    let details = details.clone();
    spawn_worker(&operation, mutation, move || transport::save(&details, submitted_at_unix_ms))
        .await.map_err(|_| "The native save worker failed. Its outcome is unknown; it may already have executed. Verify the server before retrying.".to_owned())?
}

fn spawn_worker<L, F, T>(
    operation: &L,
    mutation: Option<tokio::sync::OwnedMutexGuard<()>>,
    work: F,
) -> tokio::task::JoinHandle<T>
where
    L: crate::state::StorageContextTaskLease,
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    spawn_blocking_storage_context_task(operation, move || {
        let _mutation = mutation;
        work()
    })
}

#[cfg(test)]
#[path = "commands_native_save_control_tests.rs"]
pub(super) mod tests;

#[cfg(all(test, windows))]
#[path = "commands_managed_save_native_installation_tests.rs"]
mod native_installation_tests;
