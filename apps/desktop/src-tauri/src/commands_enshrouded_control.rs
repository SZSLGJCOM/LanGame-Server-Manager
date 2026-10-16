//! Install only our fingerprinted proxy in an exclusively owned private runtime.
use std::fs;
use std::path::Path;

use app_core::InstanceDetails;
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
) -> Result<(), String> {
    if instance.summary.module_id != "enshrouded" {
        return Ok(());
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|e| e.to_string())?
        != InstanceProgramMode::Independent
    {
        // Shared programs retain the existing name-only capability.
        return Ok(());
    }
    let root =
        app_storage::resolve_instance_runtime_root(instance_root).map_err(|e| e.to_string())?;
    let binding = app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Enshrouded program ownership is missing.")?;
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
    let payload = io::bundled_payload();
    tokio::task::spawn_blocking(move || install(&root, payload))
        .await
        .map_err(|_| "Enshrouded native control preparation worker failed.")?
}

fn install(root: &Path, payload: &[u8]) -> Result<(), String> {
    let _root_guard = io::pin_directory(root)?;
    let executable = io::read_regular(&root.join("enshrouded_server.exe"), 32 * 1024 * 1024)?;
    if io::digest(&executable) != io::EXE_SHA256 || payload.is_empty() {
        // An updated executable may import additional dbghelp exports before
        // our fingerprint check can run. Retire only a verified owned proxy.
        retire_owned_proxy(root)?;
        return Ok(());
    }
    install_verified(root, payload)
}

fn retire_owned_proxy(root: &Path) -> Result<(), String> {
    let owner_path = root.join(io::OWNER);
    if !owner_path
        .try_exists()
        .map_err(|_| "Cannot inspect native control ownership.")?
    {
        return Ok(());
    }
    let owner: Ownership = serde_json::from_slice(&io::read_regular(&owner_path, 4096)?)
        .map_err(|_| "Cannot verify the installed native control ownership.")?;
    if owner.schema != 1 || owner.module_id != "enshrouded" {
        return Err("Native control ownership is incompatible with this instance.".into());
    }
    let path = root.join(DLL);
    if path
        .try_exists()
        .map_err(|_| "Cannot inspect the installed native proxy.")?
    {
        let hash = io::digest(&io::read_regular(&path, MAX_DLL)?);
        if owner.plugin_sha256.as_deref() == Some(hash.as_str())
            || owner.pending_plugin_sha256.as_deref() == Some(hash.as_str())
        {
            fs::remove_file(&path)
                .map_err(|_| "Cannot retire the owned proxy for an unsupported server build.")?;
        }
    }
    Ok(())
}

fn install_verified(root: &Path, payload: &[u8]) -> Result<(), String> {
    let owner_path = root.join(io::OWNER);
    let mut owner = if owner_path
        .try_exists()
        .map_err(|_| "Cannot inspect native control ownership.")?
    {
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
    if dll_path
        .try_exists()
        .map_err(|_| "Cannot inspect the existing dbghelp.dll.")?
    {
        let current = io::digest(&io::read_regular(&dll_path, MAX_DLL)?);
        if owner.plugin_sha256.as_deref() != Some(current.as_str())
            && owner.pending_plugin_sha256.as_deref() != Some(current.as_str())
        {
            // A user loader owns this name; do not replace it or block startup.
            return Ok(());
        }
    }
    let directory = root.join(io::DIRECTORY);
    if directory
        .try_exists()
        .map_err(|_| "Cannot inspect the native control directory.")?
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
    io::publish(&directory.join("enable"), io::ENABLE)
}

#[cfg(test)]
#[path = "commands_enshrouded_control_tests.rs"]
mod tests;
