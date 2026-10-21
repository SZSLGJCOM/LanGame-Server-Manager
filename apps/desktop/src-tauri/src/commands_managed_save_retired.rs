//! Retire only ownership-recorded save observers replaced by native commands.
//! Keep each record until all recorded leaves are gone, so interrupted cleanup
//! can resume. No loader, containing directory, save data or foreign mod is removed.
use super::{InstanceDetails, Ownership, io, payload, process, stopped};
use crate::state::StorageContextTaskLease;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

struct Retired {
    image: &'static str,
    files: &'static [&'static str],
}

fn managed(module: &str) -> Option<Retired> {
    let (image, files): (_, &[_]) = match module {
        "minecraft" => (
            "jre/bin/java.exe",
            &[".langame-world-save/LanGame.Minecraft.Save.dll"],
        ),
        "projectzomboid" => (
            "jre64/bin/java.exe",
            &[
                ".langame-world-save/LanGame.ProjectZomboid.Save.jar",
                ".langame-world-save/LanGame.ProjectZomboid.Native.dll",
                ".langame-world-save/LanGame.ProjectZomboid.Native.minhook.LICENSE.txt",
            ],
        ),
        "terraria" => (
            "1458/Windows/TerrariaServer.exe",
            &["1458/Windows/LanGame.Terraria.SaveControl.dll"],
        ),
        "sevendaystodie" => (
            "7DaysToDieServer.exe",
            &[
                "Mods/LanGame.SevenDays.SaveControl/LanGame.SevenDays.Save.dll",
                "Mods/LanGame.SevenDays.SaveControl/ModInfo.xml",
            ],
        ),
        "unturned" => (
            "Unturned.exe",
            &[
                "Modules/LanGame.SaveControl/LanGame.Unturned.Save.dll",
                "Modules/LanGame.SaveControl/LanGame.SaveControl.module",
                "Modules/LanGame.SaveControl/0Harmony.dll",
                "Modules/LanGame.SaveControl/Harmony.LICENSE.txt",
            ],
        ),
        "necesse" => (
            "jre/bin/java.exe",
            &[".langame-world-save/LanGame.Necesse.Save.jar"],
        ),
        "romestead" => (
            "Server.exe",
            &[".langame-world-save/LanGame.Romestead.Save.dll"],
        ),
        "rimworld" => (
            "RTServer.exe",
            &[".langame-world-save/LanGame.RimWorld.Save.dll"],
        ),
        _ => return None,
    };
    Some(Retired { image, files })
}

fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn check_map(files: &BTreeMap<String, String>, allowed: &[&str]) -> Result<(), String> {
    if files
        .iter()
        .any(|(path, digest)| !allowed.contains(&path.as_str()) || !hash(digest))
    {
        return Err(
            "The retired save ownership record contains an unknown file; it was preserved.".into(),
        );
    }
    Ok(())
}

fn verify_files(
    root: &Path,
    files: &BTreeMap<String, String>,
    pending: &BTreeMap<String, String>,
) -> Result<(), String> {
    for path in files.keys().chain(pending.keys()) {
        let path = root.join(path);
        let _pins =
            payload::inspect_ancestors(root, path.parent().ok_or("Invalid retired save path.")?)?;
        if let Some(bytes) = io::read_optional(&path, 16 * 1024 * 1024)? {
            let digest = io::digest(&bytes);
            let relative = path
                .strip_prefix(root)
                .map_err(|_| "Invalid retired save path.")?;
            let relative = relative
                .to_str()
                .ok_or("Invalid retired save path.")?
                .replace('\\', "/");
            if files.get(&relative) != Some(&digest) && pending.get(&relative) != Some(&digest) {
                return Err(
                    "A retired save extension is unowned or changed; it was preserved.".into(),
                );
            }
        }
    }
    Ok(())
}

fn remove_files(
    root: &Path,
    files: &BTreeMap<String, String>,
    pending: &BTreeMap<String, String>,
) -> Result<(), String> {
    // Complete the read-only validation before removing the first file.
    verify_files(root, files, pending)?;
    for path in files.keys().chain(pending.keys()) {
        let full = root.join(path);
        let _pins =
            payload::inspect_ancestors(root, full.parent().ok_or("Invalid retired save path.")?)?;
        if let Some(bytes) = io::read_optional(&full, 16 * 1024 * 1024)? {
            let digest = io::digest(&bytes);
            if files.get(path) != Some(&digest) && pending.get(path) != Some(&digest) {
                return Err(
                    "A retired save extension changed during cleanup; it was preserved.".into(),
                );
            }
            io::remove_owned(&full, &digest)?;
        }
    }
    Ok(())
}

fn ensure_stopped_image(root: &Path, image: &str) -> Result<(), String> {
    if root.join(image).exists() {
        process::ensure_image_scope(root, image, None)?;
    }
    // A shared portable Java executable can live outside this game directory.
    // Its old observer holds this leaf exclusively for its entire lifetime.
    io::read_optional(&root.join(".langame-world-save/live.lock"), 1024).map(|_| ())
}

fn cleanup_managed(details: &InstanceDetails, root: &Path, retired: Retired) -> Result<(), String> {
    let record = root.join(super::OWNER);
    let Some(bytes) = io::read_optional(&record, 65536)? else {
        if matches!(
            details.summary.module_id.as_str(),
            "unturned" | "sevendaystodie"
        ) {
            for path in retired.files {
                let full = root.join(path);
                let _pins = payload::inspect_ancestors(
                    root,
                    full.parent().ok_or("Invalid retired save path.")?,
                )?;
                if io::read_optional(&full, 16 * 1024 * 1024)?.is_some() {
                    return Err("An unowned save extension remains on the native mod scan path; it was preserved. Remove or relocate it before starting.".into());
                }
            }
        }
        return Ok(());
    };
    let (owner_pin, pinned_owner) = io::pin_regular(&record, 65536)?;
    if pinned_owner != bytes {
        return Err("The retired save ownership changed; it was preserved.".into());
    }
    let owner: Ownership = serde_json::from_slice(&bytes)
        .map_err(|_| "The retired save ownership record is invalid.")?;
    if owner.protocol != 1
        || owner.module_id != details.summary.module_id
        || (!hash(&owner.source_sha256)
            && !(owner.source_sha256.is_empty()
                && owner.files.is_empty()
                && !owner.pending_files.is_empty()))
    {
        return Err(
            "The retired save ownership record does not match this instance; it was preserved."
                .into(),
        );
    }
    check_map(&owner.files, retired.files)?;
    check_map(&owner.pending_files, retired.files)?;
    ensure_stopped_image(root, retired.image)?;
    remove_files(root, &owner.files, &owner.pending_files)?;
    drop(owner_pin);
    io::remove_owned(&record, &io::digest(&bytes))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NecesseOwner {
    protocol: u32,
    sha256: String,
    pending_sha256: Option<String>,
}

fn cleanup_necesse(details: &InstanceDetails) -> Result<(), String> {
    let instance = super::super::commands_program_storage::instance_root(details)?;
    let record = instance.join("data/.langame-necesse-save.json");
    let Some(bytes) = io::read_optional(&record, 2048)? else {
        let plugin = instance.join("data/mods/LanGame.Necesse.Save.jar");
        let _pins = payload::inspect_ancestors(
            instance,
            plugin.parent().ok_or("Invalid Necesse mod path.")?,
        )?;
        if io::read_optional(&plugin, 4 * 1024 * 1024)?.is_some() {
            return Err("An unowned Necesse save mod remains installed; it was preserved. Remove or relocate it before starting.".into());
        }
        return Ok(());
    };
    let (owner_pin, pinned_owner) = io::pin_regular(&record, 65536)?;
    if pinned_owner != bytes {
        return Err("The retired save ownership changed; it was preserved.".into());
    }
    let owner: NecesseOwner = serde_json::from_slice(&bytes)
        .map_err(|_| "The retired Necesse mod ownership is invalid.")?;
    if owner.protocol != 1
        || (!owner.sha256.is_empty() && !hash(&owner.sha256))
        || owner
            .pending_sha256
            .as_ref()
            .is_some_and(|value| !hash(value))
    {
        return Err("The retired Necesse mod ownership is invalid; it was preserved.".into());
    }
    let path = instance.join("data/mods/LanGame.Necesse.Save.jar");
    let _pins =
        payload::inspect_ancestors(instance, path.parent().ok_or("Invalid Necesse mod path.")?)?;
    if let Some(plugin) = io::read_optional(&path, 4 * 1024 * 1024)? {
        let digest = io::digest(&plugin);
        if owner.sha256 != digest && owner.pending_sha256.as_ref() != Some(&digest) {
            return Err("The retired Necesse save mod changed; it was preserved.".into());
        }
        io::remove_owned(&path, &digest)?;
    }
    drop(owner_pin);
    io::remove_owned(&record, &io::digest(&bytes))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArkOwner {
    schema: u32,
    module: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}

fn cleanup_ark(module: &str, root: &Path) -> Result<(), String> {
    let (owner_file, directory, image, dll) = match module {
        "arksurvivalevolved" => (
            ".langame-ase-save.json",
            "LgsmAseWorldSave",
            "ShooterGameServer.exe",
            "LgsmAseWorldSave.dll",
        ),
        "arksurvivalascended" => (
            ".langame-asa-save.json",
            "LgsmAsaWorldSave",
            "ArkAscendedServer.exe",
            "LgsmAsaWorldSave.dll",
        ),
        _ => return Ok(()),
    };
    let record = root.join(owner_file);
    let Some(bytes) = io::read_optional(&record, 4096)? else {
        let plugin = root
            .join("ShooterGame/Binaries/Win64/ArkApi/Plugins")
            .join(directory)
            .join(dll);
        let _pins = payload::inspect_ancestors(
            root,
            plugin.parent().ok_or("Invalid ARK save plugin path.")?,
        )?;
        if io::read_optional(&plugin, 16 * 1024 * 1024)?.is_some() {
            return Err("An unowned ARK save plugin remains installed; it was preserved. Remove or relocate it before starting.".into());
        }
        return Ok(());
    };
    let (owner_pin, pinned_owner) = io::pin_regular(&record, 65536)?;
    if pinned_owner != bytes {
        return Err("The retired save ownership changed; it was preserved.".into());
    }
    let owner: ArkOwner =
        serde_json::from_slice(&bytes).map_err(|_| "The retired ARK save ownership is invalid.")?;
    if owner.schema != 1 || owner.module != module {
        return Err(
            "The retired ARK save ownership does not match this instance; it was preserved.".into(),
        );
    }
    let allowed = [dll, "PluginInfo.json", "THIRD_PARTY_NOTICES.txt"];
    if (owner.files.len() != allowed.len()
        && !(owner.files.is_empty() && owner.pending_files.is_some()))
        || owner
            .pending_files
            .as_ref()
            .is_some_and(|files| files.len() != allowed.len())
    {
        return Err(
            "The retired ARK save ownership has an incomplete payload; it was preserved.".into(),
        );
    }
    check_map(&owner.files, &allowed)?;
    let pending = owner.pending_files.unwrap_or_default();
    check_map(&pending, &allowed)?;
    let image = format!("ShooterGame/Binaries/Win64/{image}");
    ensure_stopped_image(root, &image)?;
    let directory = root
        .join("ShooterGame/Binaries/Win64/ArkApi/Plugins")
        .join(directory);
    let _pins = payload::inspect_ancestors(root, &directory)?;
    remove_files(&directory, &owner.files, &pending)?;
    drop(owner_pin);
    io::remove_owned(&record, &io::digest(&bytes))
}

fn cleanup(details: &InstanceDetails) -> Result<(), String> {
    let module = details.summary.module_id.as_str();
    let retired = managed(module);
    if retired.is_none() && !app_core::ark_maps::is_ark(module) {
        return Ok(());
    }
    let root = app_storage::resolve_instance_runtime_root(
        super::super::commands_program_storage::instance_root(details)?,
    )
    .map_err(|error| error.to_string())?;
    let _pins = io::pin_directories(&root)?;
    if !needs_cleanup(details, &root)? {
        return Ok(());
    }
    stopped(details)?;
    if let Some(retired) = retired {
        if module == "necesse" {
            // This mod lives in this instance's data directory. Another server
            // may legitimately use the same shared Java executable; only the
            // old observer's own lifetime lock may block retiring this instance's mod.
            io::read_optional(&root.join(".langame-world-save/live.lock"), 1024)?;
            cleanup_necesse(details)?;
        }
        cleanup_managed(details, &root, retired)?;
    } else {
        cleanup_ark(module, &root)?;
    }
    Ok(())
}

fn needs_cleanup(details: &InstanceDetails, root: &Path) -> Result<bool, String> {
    let module = details.summary.module_id.as_str();
    let present = |base: &Path, relative: &str| -> Result<bool, String> {
        let path = base.join(relative);
        let _pins =
            payload::inspect_ancestors(base, path.parent().ok_or("Invalid retired save path.")?)?;
        io::read_optional(&path, 16 * 1024 * 1024).map(|bytes| bytes.is_some())
    };
    if let Some(retired) = managed(module) {
        if present(root, super::OWNER)? {
            return Ok(true);
        }
        if module == "necesse" {
            let instance = super::super::commands_program_storage::instance_root(details)?;
            return Ok(present(instance, "data/.langame-necesse-save.json")?
                || present(instance, "data/mods/LanGame.Necesse.Save.jar")?);
        }
        if matches!(module, "unturned" | "sevendaystodie") {
            for relative in retired.files {
                if present(root, relative)? {
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }
    let (owner, plugin) = match module {
        "arksurvivalevolved" => (
            ".langame-ase-save.json",
            "ShooterGame/Binaries/Win64/ArkApi/Plugins/LgsmAseWorldSave/LgsmAseWorldSave.dll",
        ),
        "arksurvivalascended" => (
            ".langame-asa-save.json",
            "ShooterGame/Binaries/Win64/ArkApi/Plugins/LgsmAsaWorldSave/LgsmAsaWorldSave.dll",
        ),
        _ => return Ok(false),
    };
    Ok(present(root, owner)? || present(root, plugin)?)
}

pub(in crate::commands) async fn cleanup_for_start<L: StorageContextTaskLease>(
    details: &InstanceDetails,
    operation: &L,
) -> Result<(), String> {
    let details = details.clone();
    crate::commands::spawn_blocking_storage_context_task(operation, move || cleanup(&details))
        .await
        .map_err(|error| format!("Retired save extension cleanup failed: {error}"))?
}

#[cfg(test)]
#[path = "commands_managed_save_retired_tests.rs"]
mod tests;
