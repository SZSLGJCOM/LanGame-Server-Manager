//! Install only our fingerprinted proxy in an exclusively owned private runtime.
use std::fs;
use std::path::Path;

use app_core::{InstanceDetails, RuntimePlayerExtensionStatus};
use app_storage::{InstanceProgramMode, ProgramInstallScope, StorageBootstrap};
use serde::{Deserialize, Serialize};

use crate::live_players::enshrouded_native::io;

const DLL: &str = "dbghelp.dll";
const MAX_DLL: usize = 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    schema: u32,
    module_id: String,
    plugin_sha256: Option<String>,
    pending_plugin_sha256: Option<String>,
}

pub(super) async fn prepare(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    log_path: Option<&str>,
) -> Result<bool, String> {
    if instance.summary.module_id != "enshrouded" {
        return Ok(true);
    }
    let binding = app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
        .await
        .map_err(|error| error.to_string());
    let worker_storage = storage.clone();
    let worker_instance = instance.clone();
    let log_path = log_path.map(str::to_owned);
    let prepared = tokio::task::spawn_blocking(move || {
        let result = prepare_local(
            &worker_storage,
            &worker_instance,
            binding,
            log_path.as_deref(),
        );
        if let Err(error) = &result {
            report_preparation_failure(
                &worker_storage,
                &worker_instance.summary.id,
                log_path.as_deref(),
                error,
            );
        }
        result
    })
    .await
    .map_err(
        |_| "The Enshrouded extension worker failed; its inactive state could not be verified.",
    )??;
    let native_extension_prepared = match prepared {
        Some((executable, installed)) => {
            let updater_storage = storage.clone();
            let instance_id = instance.summary.id.clone();
            crate::enshrouded_extension::start_refresh(
                storage.paths.app_data_root.clone(),
                executable.clone(),
                move |result| {
                    use crate::enshrouded_extension::RefreshResult;
                    let (level, action, version, issue) = match result {
                        Ok(RefreshResult::Cached { version }) => (
                            "info",
                            "instance.enshrouded_extension.cached",
                            Some(version),
                            None,
                        ),
                        Ok(RefreshResult::Unsupported) => (
                            "info",
                            "instance.enshrouded_extension.update_finished",
                            None,
                            None,
                        ),
                        Err(error) => (
                            "warn",
                            "instance.enshrouded_extension.update_failed",
                            None,
                            Some(error),
                        ),
                    };
                    super::commands_runtime_supervision::append_desktop_app_log(
                        &updater_storage,
                        level,
                        action,
                        "Background player-management extension check completed; the running server is unchanged",
                        serde_json::json!({ "instance_id": instance_id, "executable_sha256": executable,
                        "extension_version": version, "update_issue": issue, "applies_on_next_start": true }),
                    );
                },
            );
            installed
        }
        None => false,
    };
    Ok(native_extension_prepared)
}

fn report_preparation_failure(
    storage: &StorageBootstrap,
    instance_id: &str,
    log_path: Option<&str>,
    error: &str,
) {
    super::commands_runtime_supervision::append_desktop_app_log(
        storage,
        "warn",
        "instance.enshrouded_extension.preparation_failed",
        "Player-management extension preparation failed its local safety checks; game startup was blocked",
        serde_json::json!({ "instance_id": instance_id, "extension_issue": error }),
    );
    if let Some(path) = log_path {
        let _ = super::commands_runtime_supervision::append_startup_console_line(
            path,
            "Enshrouded extension could not be safely prepared or disabled; game startup was blocked.",
            error,
        );
    }
}

/// Only this stopped-instance worker may mutate an instance runtime. Network
/// refresh receives the application cache path, never this runtime directory.
fn prepare_local(
    storage: &StorageBootstrap,
    instance: &InstanceDetails,
    binding: Result<Option<app_storage::InstanceProgramInstall>, String>,
    log_path: Option<&str>,
) -> Result<Option<(String, bool)>, String> {
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|e| e.to_string())?
        != InstanceProgramMode::Independent
    {
        return Err("Enshrouded extension preparation requires this instance's exclusive program directory.".into());
    }
    let root =
        app_storage::resolve_instance_runtime_root(instance_root).map_err(|e| e.to_string())?;
    let binding = binding?.ok_or("Enshrouded program ownership is missing.")?;
    let exclusive =
        app_storage::instance_uses_exclusive_program(instance_root).map_err(|e| e.to_string())?;
    let owner_matches = match binding.install.scope {
        ProgramInstallScope::Instance => {
            binding.install.owner_instance_id.as_deref() == Some(instance.summary.id.as_str())
        }
        ProgramInstallScope::Library => binding.install.owner_instance_id.is_none() && exclusive,
    };
    if binding.runtime_mode != "independent"
        || binding.instance_id != instance.summary.id
        || binding.install.module_id != "enshrouded"
        || !owner_matches
        || fs::canonicalize(&binding.install.install_root).map_err(|e| e.to_string())?
            != fs::canonicalize(&root).map_err(|e| e.to_string())?
    {
        return Err("Enshrouded native control ownership does not match this instance.".into());
    }
    let _root_guard = io::pin_directory(&root)?;
    let executable = match io::read_regular(&root.join("enshrouded_server.exe"), 32 * 1024 * 1024) {
        Ok(bytes) => io::digest(&bytes),
        Err(error) => {
            // No fingerprint means no known-safe ABI. Retire our own proxy even
            // when a future executable exceeds the bounded inspection limit.
            return Err(match retire_owned_proxy(&root) {
                Ok(()) => error,
                Err(cleanup) => format!("{error} Extension cleanup also failed: {cleanup}"),
            });
        }
    };
    let resolution =
        crate::enshrouded_extension::resolve_local(&storage.paths.app_data_root, &executable);
    let installed = prepare_optional_install(&root, &executable, resolution.package.as_ref())?;
    let issue = resolution.update_issue;
    super::commands_runtime_supervision::append_desktop_app_log(
        storage,
        if issue.is_some() { "warn" } else { "info" },
        "instance.enshrouded_extension.prepared",
        "Selected locally verified Enshrouded player-management extension without waiting for updates",
        serde_json::json!({ "instance_id": instance.summary.id, "executable_sha256": executable,
            "source": resolution.source, "extension_version": resolution.version, "extension_issue": issue }),
    );
    if let Some(path) = log_path {
        let _ = super::commands_runtime_supervision::append_startup_console_line(
            path,
            &format!(
                "Enshrouded player-management extension: {}; background updates apply on next start.",
                resolution.source
            ),
            issue.as_deref().unwrap_or(""),
        );
    }
    Ok(Some((executable, installed)))
}

/// Compatibility misses are optional; local ownership, path and publication
/// failures must retain a hard boundary even if the old proxy can be retired.
fn prepare_optional_install(
    root: &Path,
    executable: &str,
    selected: Option<&crate::enshrouded_extension::SignedPackage>,
) -> Result<bool, String> {
    let error = match install_selected(root, executable, selected) {
        Ok(installed) => return Ok(installed),
        Err(error) => error,
    };
    let cleanup = (|| {
        let _root_guard = io::pin_directory(root)?;
        retire_owned_proxy(root)
    })();
    Err(match cleanup {
        Ok(()) => error,
        Err(cleanup) => format!("{error} Extension cleanup also failed: {cleanup}"),
    })
}

fn install_selected(
    root: &Path,
    executable: &str,
    selected: Option<&crate::enshrouded_extension::SignedPackage>,
) -> Result<bool, String> {
    let _root_guard = io::pin_directory(root)?;
    if io::digest(&io::read_regular(
        &root.join("enshrouded_server.exe"),
        32 * 1024 * 1024,
    )?) != executable
    {
        return Err("The Enshrouded executable changed during extension preparation.".into());
    }
    let payload = selected.map(|package| package.payload()).or_else(|| {
        io::BUNDLED_EXE_SHA256
            .contains(&executable)
            .then(io::bundled_payload)
    });
    if let Some(payload) = payload.filter(|bytes| !bytes.is_empty()) {
        let installed = install_payload(root, payload, selected)?;
        io::publish_manager_state(
            root,
            executable,
            if installed {
                RuntimePlayerExtensionStatus::Loading
            } else {
                RuntimePlayerExtensionStatus::NotInstalled
            },
        )?;
        Ok(installed)
    } else {
        retire_owned_proxy(root)?;
        io::publish_manager_state(
            root,
            executable,
            RuntimePlayerExtensionStatus::UnsupportedGameBuild,
        )?;
        Ok(false)
    }
}

#[cfg(test)]
fn install(root: &Path, payload: &[u8]) -> Result<(), String> {
    let _root_guard = io::pin_directory(root)?;
    let executable = io::read_regular(&root.join("enshrouded_server.exe"), 32 * 1024 * 1024)?;
    if !io::BUNDLED_EXE_SHA256.contains(&io::digest(&executable).as_str()) || payload.is_empty() {
        // An updated executable may import additional dbghelp exports before
        // our fingerprint check can run. Retire only a verified owned proxy.
        retire_owned_proxy(root)?;
        return Ok(());
    }
    install_verified(root, payload)
}

fn retire_owned_proxy(root: &Path) -> Result<(), String> {
    let owner_path = root.join(io::OWNER);
    let path = root.join(DLL);
    if !path_exists(&owner_path)? {
        if path_exists(&path)? {
            return Err("An unowned Enshrouded dbghelp.dll would still be injected; it was preserved and startup was blocked.".into());
        }
        return Ok(());
    }
    let owner: Ownership = serde_json::from_slice(&io::read_regular(&owner_path, 4096)?)
        .map_err(|_| "Cannot verify the installed native control ownership.")?;
    if owner.schema != 1 || owner.module_id != "enshrouded" {
        return Err("Native control ownership is incompatible with this instance.".into());
    }
    if path_exists(&path)? {
        let hash = io::digest(&io::read_regular(&path, MAX_DLL)?);
        if owner.plugin_sha256.as_deref() != Some(hash.as_str())
            && owner.pending_plugin_sha256.as_deref() != Some(hash.as_str())
        {
            return Err("The Enshrouded native proxy was changed; it was preserved and cannot be safely disabled.".into());
        }
        super::commands_managed_save_control::retire_owned_file(&path, &hash)?;
    }
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!("Cannot inspect Enshrouded extension path: {error}")),
    }
}

#[cfg(test)]
fn install_verified(root: &Path, payload: &[u8]) -> Result<(), String> {
    install_payload(root, payload, None).map(|_| ())
}

fn install_payload(
    root: &Path,
    payload: &[u8],
    signed: Option<&crate::enshrouded_extension::SignedPackage>,
) -> Result<bool, String> {
    let owner_path = root.join(io::OWNER);
    let mut owner = if path_exists(&owner_path)? {
        let owner: Ownership = serde_json::from_slice(&io::read_regular(&owner_path, 4096)?)
            .map_err(|_| "Invalid Enshrouded native control ownership record.")?;
        if owner.schema != 1 || owner.module_id != "enshrouded" {
            return Err("Enshrouded native control ownership is incompatible.".into());
        }
        owner
    } else {
        Ownership {
            schema: 1,
            module_id: "enshrouded".into(),
            plugin_sha256: None,
            pending_plugin_sha256: None,
        }
    };
    let dll_path = root.join(DLL);
    if path_exists(&dll_path)? {
        let current = io::digest(&io::read_regular(&dll_path, MAX_DLL)?);
        if owner.plugin_sha256.as_deref() != Some(current.as_str())
            && owner.pending_plugin_sha256.as_deref() != Some(current.as_str())
        {
            return Err("An unknown or modified Enshrouded dbghelp.dll was preserved; it cannot be safely replaced or disabled.".into());
        }
    }
    let directory = root.join(io::DIRECTORY);
    if path_exists(&directory)?
        && owner.plugin_sha256.is_none()
        && owner.pending_plugin_sha256.is_none()
    {
        return Err("An unowned Enshrouded control directory already exists.".into());
    }
    // Record the pending payload before creating any extension-owned path, so
    // interrupted preparation remains distinguishable from an unrelated folder.
    let payload_hash = io::digest(payload);
    owner.pending_plugin_sha256 = Some(payload_hash.clone());
    io::publish(
        &owner_path,
        &serde_json::to_vec(&owner).map_err(|_| "Cannot encode native control ownership.")?,
    )?;
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // The ownership intent above makes this a recoverable preparation.
        }
        Err(_) => return Err("Cannot create the Enshrouded control directory.".into()),
    }
    let _directory_guard = io::pin_directory(&directory)?;
    for name in ["players.json.tmp", "response.json.tmp", "status.json.tmp"] {
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata)
                if metadata.is_file()
                    && !io::reparse(&metadata)
                    && metadata.len() <= io::MAX_BYTES as u64 =>
            {
                fs::remove_file(&path)
                    .map_err(|_| "Cannot recover an interrupted native control publication.")?;
            }
            Ok(_) => {
                return Err(
                    "An interrupted native control file is not an ordinary bounded file.".into(),
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err("Cannot inspect an interrupted native control publication.".into());
            }
        }
    }
    if let Some(signed) = signed {
        signed.publish_proof(&directory)?;
    }
    io::publish(&dll_path, payload)?;
    owner.plugin_sha256 = Some(payload_hash);
    owner.pending_plugin_sha256 = None;
    io::publish(
        &owner_path,
        &serde_json::to_vec(&owner).map_err(|_| "Cannot encode native control ownership.")?,
    )?;
    io::publish(
        &directory.join("LICENSE.txt"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../modules/enshrouded/control/vendor/minhook/LICENSE.txt"
        )),
    )?;
    io::publish(&directory.join("enable"), io::ENABLE)?;
    Ok(true)
}

#[cfg(test)]
#[path = "commands_enshrouded_control_tests.rs"]
mod tests;
