//! Prepare save prerequisites under the same leases as the subsequent launch.
//! First installation and later refresh use one path; no game is hot-patched.
use super::*;
use crate::state::StorageContextTaskLease;

pub(super) fn requires_private_program(module: &str) -> bool {
    super::commands_managed_save_control::requires_automatic_setup(module)
        || crate::unreal_loader::supported(module)
        || matches!(
            module,
            "valheim" | "runescapedragonwilds" | "theforest" | "enshrouded"
        )
}

/// Reserve both roots together: lifecycle locks also contain the module key,
/// so acquiring a second root while retaining the first would deadlock.
pub(super) fn program_roots(
    instance: &InstanceDetails,
    current: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut roots = vec![current.to_owned()];
    if requires_private_program(&instance.summary.module_id) {
        let target = super::commands_program_storage::instance_root(instance)?.join("runtime");
        if !roots.contains(&target) {
            roots.push(target);
        }
    }
    Ok(roots)
}

pub(super) async fn ensure_private_program(
    storage: &StorageBootstrap,
    descriptor: &ModuleDescriptor,
    instance: InstanceDetails,
    reservation: &RuntimeStartReservationLease,
    guards: super::commands_runtime_lifecycle::RuntimeProgramGuards,
) -> Result<
    (
        super::commands_runtime_lifecycle::RuntimeProgramGuards,
        InstanceDetails,
    ),
    String,
> {
    if !requires_private_program(&instance.summary.module_id)
        || super::commands_program_storage::program_mode(&instance)?
            == app_storage::InstanceProgramMode::Independent
    {
        return Ok((guards, instance));
    }
    let storage = storage.clone();
    let descriptor = descriptor.clone();
    let cancellation = reservation.storage_operation().cancellation_token();
    let (guards, result) = spawn_storage_context_task(reservation, async move {
        let result = async {
            let target = super::commands_program_storage::instance_root(&instance)?.join("runtime");
            guards
                .1
                .as_ref()
                .ok_or("startup program lifecycle lock is missing")?
                .ensure_scope(&instance.summary.module_id, &target)
                .map_err(|error| steamcmd_error_message(&error))?;
            let current = app_storage::resolve_instance_runtime_root(
                super::commands_program_storage::instance_root(&instance)?,
            )
            .map_err(|error| error.to_string())?;
            guards
                .1
                .as_ref()
                .ok_or("startup program lifecycle lock is missing")?
                .ensure_scope(&instance.summary.module_id, &current)
                .map_err(|error| steamcmd_error_message(&error))?;
            app_storage::ensure_program_archive_dependencies(&storage.paths, &current)
                .await
                .map_err(|error| error.to_string())?;
            app_storage::detach_instance_program(
                &storage.paths,
                &descriptor,
                &instance.summary.id,
                Some(cancellation),
            )
            .await
            .map_err(|error| error.to_string())?;
            read_instance_details(&storage.paths, &instance.summary.id)
                .await
                .map_err(|error| error.to_string())
        }
        .await;
        (guards, result)
    })
    .await
    .map_err(|error| format!("Automatic save program preparation failed: {error}"))?;
    Ok((guards, result?))
}

pub(super) async fn ensure_for_start<L: StorageContextTaskLease>(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    operation: &L,
    log_path: Option<&str>,
) -> Result<Vec<(String, String)>, String> {
    super::commands_managed_save_control::cleanup_for_start(instance, operation).await?;
    let retired = instance.clone();
    spawn_blocking_storage_context_task(operation, move || {
        crate::live_players::file_ipc::unreal_world_save::cleanup_retired(&retired)
    })
    .await
    .map_err(|error| format!("Retired Unreal save cleanup failed: {error}"))??;
    let module = instance.summary.module_id.as_str();
    if !crate::unreal_loader::supported(module)
        && module != "runescapedragonwilds"
        && module != "valheim"
        && !super::commands_managed_save_control::requires_automatic_setup(module)
    {
        return Ok(Vec::new());
    }
    if let Some(path) = log_path {
        let _ = append_startup_console_line(path, "正在检查并自动准备保存扩展及所需加载器…", "");
    }
    let environment = if module == "runescapedragonwilds" {
        validate_unreal_environment(std::iter::empty())?;
        let details = instance.clone();
        let environment = spawn_blocking_storage_context_task(operation, move || {
            let root = crate::live_players::dragonwilds_control::preparation_root(&details)?;
            if !super::commands_dragonwilds_player_query::owns_compatible_loader(&root)? {
                return Err(
                    "The Dragonwilds owned save loader must be prepared before launch.".into(),
                );
            }
            super::commands_dragonwilds_player_query::ensure_program_stopped(&root)?;
            crate::live_players::dragonwilds_control::startup_environment(&details)
        })
        .await
        .map_err(|error| format!("Dragonwilds save session preparation failed: {error}"))??;
        vec![environment]
    } else if crate::unreal_loader::supported(module) {
        ensure_unreal(instance, operation)
            .await?
            .into_iter()
            .collect()
    } else if module == "valheim" {
        super::commands_valheim_control::ensure_for_start(storage, instance, operation)
            .await?
            .into_iter()
            .collect()
    } else {
        super::commands_managed_save_control::ensure_for_start(storage, instance, operation).await?
    };
    if let Some(path) = log_path {
        let _ = append_startup_console_line(path, "保存扩展及加载器检查完成。", "");
    }
    Ok(environment)
}

/// Caller owns the stopped instance and its independent program lifecycle lock.
pub(super) async fn ensure_unreal<L: StorageContextTaskLease>(
    instance: &InstanceDetails,
    operation: &L,
) -> Result<Option<(String, String)>, String> {
    validate_unreal_environment(std::iter::empty())?;
    let details = instance.clone();
    let (root, installed) = spawn_blocking_storage_context_task(operation, move || {
        let root = if details.summary.module_id == "windrose" {
            crate::live_players::file_ipc::windrose_world_save::validate_preparation(&details)?
        } else {
            crate::live_players::file_ipc::unreal_world_save::validate_preparation(&details)?
        };
        let installed = crate::unreal_loader::installed(&root, &details.summary.module_id)?;
        Ok::<_, String>((root, installed))
    })
    .await
    .map_err(|error| format!("Save loader inspection failed: {error}"))??;
    let prepared = if installed {
        None
    } else {
        Some(crate::unreal_loader::prepare(&instance.summary.module_id).await?)
    };
    let details = instance.clone();
    spawn_blocking_storage_context_task(operation, move || {
        let current = if details.summary.module_id == "windrose" {
            crate::live_players::file_ipc::windrose_world_save::validate_preparation(&details)?
        } else {
            crate::live_players::file_ipc::unreal_world_save::validate_preparation(&details)?
        };
        if current != root {
            return Err("The native save program changed during loader preparation.".into());
        }
        if let Some(prepared) = prepared {
            crate::unreal_loader::install(&root, &details.summary.module_id, &prepared)?;
        }
        if details.summary.module_id == "windrose" {
            crate::live_players::file_ipc::windrose_world_save::ensure_prepared(&details)?;
            Ok(None)
        } else {
            crate::live_players::file_ipc::unreal_world_save::ensure_prepared(&details)?;
            crate::live_players::file_ipc::unreal_world_save::startup_environment(&details)
        }
    })
    .await
    .map_err(|error| format!("Automatic save loader installation failed: {error}"))?
}

pub(super) fn validate_unreal_environment<'a>(
    environment: impl IntoIterator<Item = (&'a String, &'a String)>,
) -> Result<(), String> {
    if std::env::var_os("UE4SS_MODS_PATHS").is_some_and(|value| !value.is_empty())
        || environment
            .into_iter()
            .any(|(key, value)| key.eq_ignore_ascii_case("UE4SS_MODS_PATHS") && !value.is_empty())
    {
        return Err("An existing UE4SS_MODS_PATHS override can replace the managed save mod. It was preserved; resolve that configuration conflict before starting this native save extension.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "commands_save_startup_tests.rs"]
mod tests;
