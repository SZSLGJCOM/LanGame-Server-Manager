//! Recoverable publication of the separately owned Dragonwilds control mod.
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const OWNER: &str = ".langame-dragonwilds-player-control.json";
const MOD: &str = "RSDragonwilds/Binaries/Win64/ue4ss/Mods/LgsmPlayerControl";
const DIRECTORY: &str = "langame_player_control";
const MAX_FILE: usize = 4 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}

fn hash(bytes: &[u8]) -> String {
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

fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

fn pin(path: &Path, create: bool) -> Result<Vec<File>, String> {
    if !path.is_absolute() {
        return Err("The control installation path must be absolute.".into());
    }
    let mut pins = Vec::new();
    for directory in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if !exists(directory)? && create {
            fs::create_dir(directory).map_err(|error| error.to_string())?;
        }
        let metadata = fs::symlink_metadata(directory).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err("Control paths cannot use reparse points.".into());
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options
                .access_mode(1)
                .share_mode(3)
                .custom_flags(0x0220_0000);
        }
        let file = options.open(directory).map_err(|error| error.to_string())?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if !metadata.is_dir() || reparse(&metadata) {
            return Err("The control directory changed during preparation.".into());
        }
        pins.push(file);
    }
    Ok(pins)
}

fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > limit as u64 {
        return Err("Control files must be bounded ordinary files.".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).custom_flags(0x0020_0000);
    }
    let file = options.open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || reparse(&metadata) || metadata.len() > limit as u64 {
        return Err("The control file changed during preparation.".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("The control file grew beyond its limit.".into());
    }
    Ok(bytes)
}

fn publish(path: &Path, original: Option<&[u8]>, replacement: &[u8]) -> Result<(), String> {
    let accepted = match original {
        Some(original) => {
            app_storage::compare_and_swap_file_atomically(path, original, replacement)
        }
        None => app_storage::create_file_atomically(path, replacement).map(|()| true),
    }
    .map_err(|error| format!("Cannot publish a Dragonwilds control file: {error}"))?;
    if !accepted {
        return Err("A control file changed outside the installer; it was preserved.".into());
    }
    Ok(())
}

fn validate_hashes(files: &BTreeMap<String, String>) -> bool {
    files.keys().all(|name| {
        matches!(
            name.as_str(),
            "Scripts/main.lua" | "native/LgsmPlayerControl.dll" | "enabled.txt"
        )
    }) && files
        .values()
        .all(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Read-only admission: an older complete payload can keep serving the roster,
/// but only this application's verified payload may authorize native actions.
pub(crate) fn is_complete(root: &Path, script: &[u8], bridge: &[u8]) -> Result<bool, String> {
    let _root = pin(root, false)?;
    let owner_path = root.join(OWNER);
    if !exists(&owner_path)? {
        return Ok(false);
    }
    let owner: Ownership = serde_json::from_slice(&read(&owner_path, 8192)?)
        .map_err(|_| "The Dragonwilds control ownership record is invalid.")?;
    if owner.protocol != 1
        || owner.module_id != "runescapedragonwilds"
        || owner.files.len() != 3
        || !validate_hashes(&owner.files)
        || owner.pending_files.is_some()
    {
        return Err("The Dragonwilds control installation is incomplete; restart this instance to prepare it.".into());
    }
    let payload = [
        ("Scripts/main.lua", script),
        ("native/LgsmPlayerControl.dll", bridge),
        ("enabled.txt", &[] as &[u8]),
    ];
    for (name, bytes) in payload {
        let path = root.join(MOD).join(name);
        let _parents = pin(
            path.parent().ok_or("The control file has no directory.")?,
            false,
        )?;
        let actual = hash(&read(&path, MAX_FILE)?);
        if owner.files.get(name) != Some(&actual) {
            return Err(
                "An owned Dragonwilds control file changed; native actions are unavailable.".into(),
            );
        }
        if actual != hash(bytes) {
            return Ok(false);
        }
    }
    let _exchange = pin(&root.join(DIRECTORY), false)?;
    Ok(true)
}

/// The caller owns both instance and program permits and has verified the fixed
/// loader and game build. No caller may publish this payload into a live game.
pub(crate) fn install(root: &Path, script: &[u8], bridge: &[u8]) -> Result<(), String> {
    if script.is_empty()
        || script.len() > 128 * 1024
        || !bridge.starts_with(b"MZ")
        || bridge.len() > MAX_FILE
    {
        return Err("The bundled Dragonwilds control payload is invalid.".into());
    }
    let _root = pin(root, false)?;
    let owner_path = root.join(OWNER);
    let original_owner = if exists(&owner_path)? {
        Some(read(&owner_path, 8192)?)
    } else {
        None
    };
    let payload = BTreeMap::from([
        ("Scripts/main.lua".to_owned(), script),
        ("native/LgsmPlayerControl.dll".to_owned(), bridge),
        ("enabled.txt".to_owned(), &[] as &[u8]),
    ]);
    let expected = payload
        .iter()
        .map(|(name, bytes)| (name.clone(), hash(bytes)))
        .collect::<BTreeMap<_, _>>();
    let mut owner = if let Some(bytes) = &original_owner {
        let owner: Ownership = serde_json::from_slice(bytes)
            .map_err(|_| "The control ownership record is invalid; files were preserved.")?;
        if owner.protocol != 1
            || owner.module_id != "runescapedragonwilds"
            || !validate_hashes(&owner.files)
            || owner
                .pending_files
                .as_ref()
                .is_some_and(|pending| !validate_hashes(pending) || pending.len() != 3)
            || (owner.pending_files.is_none() && owner.files.len() != 3)
        {
            return Err(
                "The control ownership does not match this installation; files were preserved."
                    .into(),
            );
        }
        owner
    } else {
        if exists(&root.join(MOD))? || exists(&root.join(DIRECTORY))? {
            return Err(
                "An unowned Dragonwilds control directory already exists; it was preserved.".into(),
            );
        }
        Ownership {
            protocol: 1,
            module_id: "runescapedragonwilds".into(),
            files: BTreeMap::new(),
            pending_files: None,
        }
    };
    let mut originals = BTreeMap::new();
    for name in payload.keys() {
        let path = root.join(MOD).join(name);
        if exists(&path)? {
            let _parents = pin(
                path.parent().ok_or("The control file has no directory.")?,
                false,
            )?;
            let bytes = read(&path, MAX_FILE)?;
            let current = hash(&bytes);
            if owner.files.get(name) != Some(&current)
                && owner
                    .pending_files
                    .as_ref()
                    .and_then(|pending| pending.get(name))
                    != Some(&current)
            {
                return Err(
                    "An owned Dragonwilds control file was modified; it was preserved.".into(),
                );
            }
            originals.insert(name.clone(), bytes);
        } else if owner.pending_files.is_none() && owner.files.contains_key(name) {
            return Err(
                "An owned Dragonwilds control file is missing; the installation was preserved."
                    .into(),
            );
        }
    }
    owner.pending_files = Some(expected.clone());
    let intent = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
    publish(&owner_path, original_owner.as_deref(), &intent)?;
    let _mod = pin(&root.join(MOD), true)?;
    let _scripts = pin(&root.join(MOD).join("Scripts"), true)?;
    let _native = pin(&root.join(MOD).join("native"), true)?;
    let _exchange = pin(&root.join(DIRECTORY), true)?;
    // Enable last. The ownership intent recognizes every partial publication,
    // while an unknown external modification can never authorize replacement.
    for name in [
        "native/LgsmPlayerControl.dll",
        "Scripts/main.lua",
        "enabled.txt",
    ] {
        publish(
            &root.join(MOD).join(name),
            originals.get(name).map(Vec::as_slice),
            payload[name],
        )?;
    }
    owner.files = expected;
    owner.pending_files = None;
    publish(
        &owner_path,
        Some(&intent),
        &serde_json::to_vec(&owner).map_err(|error| error.to_string())?,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "dragonwilds_control_installation_tests.rs"]
mod tests;
