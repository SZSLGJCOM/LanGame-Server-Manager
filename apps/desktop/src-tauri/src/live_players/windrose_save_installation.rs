//! Windrose's optional native DLL installation. Stable file admission and CAS
//! publication reuse the same bounded, pinned helpers as other Unreal bridges.
use super::super::unreal_world_save::installation::{
    compatible_profile, exists, hash, pin, publish, read,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::Path;
const OWNER: &str = ".langame-windrose-world-save.json";
const MOD: &str = "R5/Binaries/Win64/ue4ss/Mods/LgsmWorldSave";
const NATIVE: &str = "native/LgsmWindroseSaveControl.dll";
const EXCHANGE: &str = "langame_world_save";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}
fn hashes(files: &BTreeMap<String, String>) -> bool {
    files
        .keys()
        .all(|name| matches!(name.as_str(), "Scripts/main.lua" | "enabled.txt" | NATIVE))
        && files
            .values()
            .all(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
fn decode(bytes: &[u8]) -> Result<Ownership, String> {
    let owner: Ownership = serde_json::from_slice(bytes)
        .map_err(|_| "The Windrose save ownership record is invalid.")?;
    if owner.protocol != 1
        || owner.module_id != "windrose"
        || !hashes(&owner.files)
        || owner
            .pending_files
            .as_ref()
            .is_some_and(|files| files.len() != 3 || !hashes(files))
        || (owner.pending_files.is_none() && owner.files.len() != 3)
    {
        return Err("The Windrose save ownership record does not match this installation; files were preserved.".into());
    }
    Ok(owner)
}
pub(super) fn validate(root: &Path) -> Result<Vec<File>, String> {
    let loader = root.join("R5/Binaries/Win64/ue4ss");
    let mut pins = pin(&loader, false)
        .map_err(|_| "The automatically prepared UE4SS loader is unavailable. Check the startup preparation error.")?;
    for (path, expected) in [
        (
            root.join("R5/Binaries/Win64/WindroseServer-Win64-Shipping.exe"),
            "441acd04428aa5bd6535bc6565ed659a343001615d5322e8c65141b08b356854",
        ),
        (
            loader.join("UE4SS.dll"),
            "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958",
        ),
    ] {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| "The fixed Windrose game or loader file is unavailable.")?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("Native binaries cannot use reparse points.".into());
            }
        }
        if !metadata.is_file() || metadata.len() > 320 * 1024 * 1024 {
            return Err("The native binary is not a bounded ordinary file.".into());
        }
        let mut digest = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        let actual = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual != expected {
            return Err("Windrose or UE4SS is outside the fixed verified save version; files were preserved.".into());
        }
        pins.push(file);
    }
    let profile = read(&loader.join("UE4SS-settings.ini"), 65536)?;
    if !compatible_profile(
        std::str::from_utf8(&profile).map_err(|_| "UE4SS settings must be UTF-8.")?,
        include_str!("../../../../../modules/windrose/extensions/UE4SS-settings.ini"),
    ) {
        return Err("The required Windrose EngineTick settings are unavailable. Restart while stopped to run automatic loader preparation; ambiguous configuration is preserved.".into());
    }
    let list = loader.join("Mods/mods.txt");
    if exists(&list)?
        && String::from_utf8(read(&list, 65536)?)
            .map_err(|_| "The UE4SS mod list is invalid.")?
            .lines()
            .filter(|line| !line.trim_start().starts_with(';'))
            .any(|line| line.to_ascii_lowercase().contains("lgsmworldsave"))
    {
        return Err("Remove the operator LgsmWorldSave mods.txt entry manually before owned enabled.txt preparation; it was preserved.".into());
    }
    Ok(pins)
}
fn payload<'a>(script: &'a [u8], dll: &'a [u8]) -> BTreeMap<String, &'a [u8]> {
    BTreeMap::from([
        ("Scripts/main.lua".into(), script),
        (NATIVE.into(), dll),
        ("enabled.txt".into(), &[] as &[u8]),
    ])
}
pub(super) fn complete(root: &Path, script: &[u8], dll: &[u8]) -> Result<bool, String> {
    let _pins = pin(root, false)?;
    if !exists(&root.join(OWNER))? {
        return Ok(false);
    }
    let owner = decode(&read(&root.join(OWNER), 8192)?)?;
    if owner.pending_files.is_some() {
        return Ok(false);
    }
    for (name, bytes) in payload(script, dll) {
        let path = root.join(MOD).join(&name);
        let _parents = pin(
            path.parent().ok_or("No native control parent directory.")?,
            false,
        )?;
        let actual = hash(&read(&path, 4 * 1024 * 1024)?);
        if owner.files.get(&name) != Some(&actual) {
            return Err("An owned Windrose save file changed; it was preserved.".into());
        }
        if actual != hash(bytes) {
            return Ok(false);
        }
    }
    let _exchange = pin(&root.join(EXCHANGE), false)?;
    Ok(true)
}
pub(super) fn install(root: &Path, script: &[u8], dll: &[u8]) -> Result<(), String> {
    if script.is_empty()
        || script.len() > 128 * 1024
        || dll.len() < 512
        || dll.len() > 4 * 1024 * 1024
        || !dll.starts_with(b"MZ")
    {
        return Err("The bundled Windrose save payload is invalid.".into());
    }
    let _pins = pin(root, false)?;
    let _version = validate(root)?;
    publish_installation(root, script, dll, publish)
}
fn publish_installation(
    root: &Path,
    script: &[u8],
    dll: &[u8],
    mut commit: impl FnMut(&Path, Option<&[u8]>, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let _pins = pin(root, false)?;
    let owner_path = root.join(OWNER);
    let original = if exists(&owner_path)? {
        Some(read(&owner_path, 8192)?)
    } else {
        None
    };
    let mut owner = if let Some(bytes) = &original {
        decode(bytes)?
    } else {
        if exists(&root.join(MOD))? || exists(&root.join(EXCHANGE))? {
            return Err("An unowned Windrose save directory exists; it was preserved.".into());
        }
        Ownership {
            protocol: 1,
            module_id: "windrose".into(),
            files: BTreeMap::new(),
            pending_files: None,
        }
    };
    let payload = payload(script, dll);
    let expected = payload
        .iter()
        .map(|(name, bytes)| (name.clone(), hash(bytes)))
        .collect::<BTreeMap<_, _>>();
    let mut originals = BTreeMap::new();
    for name in payload.keys() {
        let path = root.join(MOD).join(name);
        if exists(&path)? {
            let _parents = pin(
                path.parent().ok_or("No native control parent directory.")?,
                false,
            )?;
            let bytes = read(&path, 4 * 1024 * 1024)?;
            let current = hash(&bytes);
            if owner.files.get(name) != Some(&current)
                && owner
                    .pending_files
                    .as_ref()
                    .and_then(|files| files.get(name))
                    != Some(&current)
            {
                return Err("An owned Windrose save file was modified; it was preserved.".into());
            }
            // A previous interrupted payload may be different from both the
            // original complete version and this release. Preserve the exact
            // verified current hash before replacing its old pending intent.
            owner.files.insert(name.clone(), current);
            originals.insert(name.clone(), bytes);
        } else if owner.pending_files.is_none() && owner.files.contains_key(name) {
            return Err(
                "An owned Windrose save file is missing; the installation was preserved.".into(),
            );
        }
    }
    owner.pending_files = Some(expected.clone());
    let intent = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
    commit(&owner_path, original.as_deref(), &intent)?;
    let _mod = pin(&root.join(MOD), true)?;
    let _scripts = pin(&root.join(MOD).join("Scripts"), true)?;
    let _native = pin(&root.join(MOD).join("native"), true)?;
    let _exchange = pin(&root.join(EXCHANGE), true)?;
    for name in ["Scripts/main.lua", NATIVE, "enabled.txt"] {
        commit(
            &root.join(MOD).join(name),
            originals.get(name).map(Vec::as_slice),
            payload[name],
        )?;
    }
    owner.files = expected;
    owner.pending_files = None;
    commit(
        &owner_path,
        Some(&intent),
        &serde_json::to_vec(&owner).map_err(|error| error.to_string())?,
    )
}

#[cfg(test)]
#[path = "windrose_save_installation_tests.rs"]
mod tests;
