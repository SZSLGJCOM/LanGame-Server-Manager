//! The server's own DllModLoader hosts this stdin bridge. Only a verified,
//! instance-owned installation may receive it; arbitrary DLLs are never replaced.
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use app_core::InstanceDetails;
use app_storage::{InstanceProgramMode, ProgramInstallScope, StorageBootstrap};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const GAME_ASSEMBLY: &str = "TheForestDedicatedServer_Data/Managed/Assembly-CSharp.dll";
const GAME_SHA256: &str = "bc0c9b79b125ce9a6d96d532359fde8a539159378bd9f558309572b9744a9d7e";
const PLUGIN: &str = "LanGame.TheForest.Control.dll";
const OWNER: &str = ".langame-theforest-control.json";
const MAX_PLUGIN: u64 = 512 * 1024;

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
    if instance.summary.module_id != "theforest" {
        return Ok(());
    }
    let instance_root = super::commands_program_storage::instance_root(instance)?;
    if app_storage::instance_program_mode(instance_root).map_err(|e| e.to_string())?
        != InstanceProgramMode::Independent
    {
        return Err("The Forest 原生控制需要独立服务器程序，请先在实例维护中分离程序。".into());
    }
    let root =
        app_storage::resolve_instance_runtime_root(instance_root).map_err(|e| e.to_string())?;
    let binding = app_storage::read_instance_program_install(&storage.paths, &instance.summary.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("The Forest control: instance program ownership is missing")?;
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
        || binding.install.module_id != "theforest"
        || !owner_matches
        || fs::canonicalize(&binding.install.install_root).map_err(|e| e.to_string())?
            != fs::canonicalize(&root).map_err(|e| e.to_string())?
    {
        return Err("The Forest control: program ownership does not match this instance".into());
    }
    #[cfg(windows)]
    let payload = include_bytes!(concat!(
        env!("OUT_DIR"),
        "/theforest-control/LanGame.TheForest.Control.dll"
    ));
    #[cfg(not(windows))]
    let payload: &[u8] = &[];
    let saves = PathBuf::from(&instance.saves_path);
    tokio::task::spawn_blocking(move || {
        verify_save_layout(&saves)?;
        install_files(&root, payload, GAME_SHA256)
    })
    .await
    .map_err(|e| format!("The Forest control preparation worker failed: {e}"))?
}

fn verify_save_layout(saves: &Path) -> Result<(), String> {
    let parent = saves
        .parent()
        .ok_or("The Forest save directory has no parent")?;
    let basename = saves
        .file_name()
        .ok_or("The Forest save directory has no name")?;
    // Native GetLocalSlotPath concatenates this setting with the player mode.
    // Earlier templates omitted its trailing separator. Never silently ignore
    // retained worlds in the resulting sibling directory after correcting it.
    for suffix in ["Multiplayer", "SinglePlayer"] {
        let mut name = basename.to_os_string();
        name.push(suffix);
        let misplaced = parent.join(name);
        let metadata = match fs::symlink_metadata(&misplaced) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("Cannot inspect The Forest save layout: {error}")),
        };
        if !metadata.is_dir()
            || reparse(&metadata)
            || fs::read_dir(&misplaced)
                .map_err(|e| e.to_string())?
                .next()
                .transpose()
                .map_err(|e| e.to_string())?
                .is_some()
        {
            return Err("检测到 The Forest 旧版错位存档目录。为防止创建新世界，已取消启动；请先备份并将 savesMultiplayer 或 savesSinglePlayer 中的存档恢复到实例 saves 下对应的 Multiplayer 或 SinglePlayer 目录，保留原数据时请将旧目录改为备份名称。非普通目录需要先人工检查。".into());
        }
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn pin_directory(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options
            .access_mode(0x1)
            .share_mode(0x1 | 0x2)
            .custom_flags(0x0200_0000 | 0x0020_0000);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("The Forest control directory: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_dir() || reparse(&metadata) {
        return Err("The Forest control requires plain, non-reparse directories".into());
    }
    Ok(file)
}

fn read_regular(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1).custom_flags(0x0020_0000);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > limit {
        return Err("The Forest control file is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("The Forest control file grew beyond its bound".into());
    }
    Ok(Some(bytes))
}

fn publish(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Missing The Forest control destination")?;
    let temporary = parent.join(format!(
        ".langame-theforest-{}.pending",
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Cannot publish The Forest control resource: {e}"))
}

fn install_files(root: &Path, payload: &[u8], expected_game_hash: &str) -> Result<(), String> {
    if payload.is_empty()
        || payload.len() as u64 > MAX_PLUGIN
        || !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("Invalid The Forest control installation boundary".into());
    }
    let mut pins = Vec::new();
    let mut current = PathBuf::new();
    for component in root.components() {
        current.push(component);
        if !matches!(component, Component::Prefix(_)) {
            pins.push(pin_directory(&current)?);
        }
    }
    for directory in [
        root.join("TheForestDedicatedServer_Data"),
        root.join("TheForestDedicatedServer_Data/Managed"),
    ] {
        pins.push(pin_directory(&directory)?);
    }
    let game = read_regular(&root.join(GAME_ASSEMBLY), 16 * 1024 * 1024)?
        .ok_or("The Forest native control assembly is missing")?;
    if digest(&game) != expected_game_hash {
        return Err(
            "The Forest 当前服务器版本尚未验证原生保存/停止接口；为保护存档，控制桥未安装。".into(),
        );
    }
    let directory = root.join("DllMods");
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.to_string()),
    }
    pins.push(pin_directory(&directory)?);
    let plugin_path = directory.join(PLUGIN);
    let owner_path = directory.join(OWNER);
    let existing = read_regular(&plugin_path, MAX_PLUGIN)?;
    let existing_hash = existing.as_deref().map(digest);
    let wanted = digest(payload);
    let owner = read_regular(&owner_path, 4096)?
        .map(|bytes| {
            serde_json::from_slice::<Ownership>(&bytes)
                .map_err(|_| "The Forest control ownership record is invalid")
        })
        .transpose()?;
    if owner
        .as_ref()
        .is_some_and(|o| o.schema != 1 || o.module_id != "theforest")
    {
        return Err("The Forest control ownership record belongs to another resource".into());
    }
    if existing_hash.as_deref().is_some_and(|hash| hash != wanted)
        && !owner.as_ref().is_some_and(|o| {
            existing_hash == o.plugin_sha256 || existing_hash == o.pending_plugin_sha256
        })
    {
        return Err("The Forest DllMods 中存在同名用户 DLL；未覆盖，请先处理冲突。".into());
    }
    if existing_hash.as_deref() != Some(wanted.as_str()) {
        // The small intent record makes a crash between DLL replacement and
        // ownership publication recoverable without trusting unknown DLLs.
        let pending = Ownership {
            schema: 1,
            module_id: "theforest".into(),
            plugin_sha256: existing_hash,
            pending_plugin_sha256: Some(wanted.clone()),
        };
        publish(
            &owner_path,
            &serde_json::to_vec(&pending).map_err(|e| e.to_string())?,
        )?;
        publish(&plugin_path, payload)?;
    }
    let committed = Ownership {
        schema: 1,
        module_id: "theforest".into(),
        plugin_sha256: Some(wanted),
        pending_plugin_sha256: None,
    };
    publish(
        &owner_path,
        &serde_json::to_vec(&committed).map_err(|e| e.to_string())?,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "commands_theforest_control_tests.rs"]
mod tests;
