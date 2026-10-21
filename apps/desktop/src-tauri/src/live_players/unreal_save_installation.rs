//! Recoverable publication for the owned native world-save bridges.
use super::Game;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const OWNER: &str = ".langame-native-world-save.json";
fn mod_dir(game: Game) -> String {
    format!("{}/Binaries/Win64/ue4ss/Mods/LgsmWorldSave", game.project())
}
const DIRECTORY: &str = "langame_world_save";
const MAX_FILE: usize = 4 * 1024 * 1024;
const RETIRED_SCUM_BOOTSTRAP: &str = "dlls/main.dll";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ownership {
    protocol: u32,
    module_id: String,
    files: BTreeMap<String, String>,
    pending_files: Option<BTreeMap<String, String>>,
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn profile_entries(text: &str) -> Option<BTreeMap<(String, String), String>> {
    let mut section = String::new();
    let mut result = BTreeMap::new();
    for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            section = value.into();
        } else {
            let (key, value) = line.split_once('=')?;
            if result
                .insert((section.clone(), key.trim().into()), value.trim().into())
                .is_some()
            {
                return None;
            }
        }
    }
    Some(result)
}

pub(crate) fn compatible_profile(actual: &str, required: &str) -> bool {
    crate::unreal_loader::compatible_settings(actual, required)
}

/// Pin both exact binaries and their parent directories for the entire native
/// exchange. Required profile fields allow unrelated operator settings.
pub(super) fn validate(game: Game, root: &Path) -> Result<Vec<File>, String> {
    const LOADER: &str = "91d5444f41d19d0bb502f0fbdc15d76421e20359e898caa39e41e134c8f87958";
    let win64 = game.win64(root);
    let loader = win64.join("ue4ss");
    let game_image = win64.join(game.executable());
    let provider_image = win64.join("Dreamworld.dll");
    let mut provider_hash = None;
    let mut pins = pin(&loader, false).map_err(|_| "The automatically prepared UE4SS loader directory is unavailable. Check the startup preparation error.")?;
    let mut binaries = vec![
        (game_image.clone(), None),
        (loader.join("UE4SS.dll"), Some(LOADER)),
    ];
    if game == Game::Conan {
        binaries.push((provider_image.clone(), None));
    }
    for (path, expected) in binaries {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1).custom_flags(0x0020_0000);
        }
        let mut file = options
            .open(&path)
            .map_err(|_| "The fixed game or UE4SS binary is unavailable.")?;
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        if reparse(&metadata) || !metadata.is_file() || metadata.len() > 256 * 1024 * 1024 {
            return Err("The native save binary is not a bounded ordinary file.".into());
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
        if path == game_image {
            provider_hash = crate::unreal_loader::verify_game_image(game.id(), &actual)?;
        } else if Some(actual.as_str())
            != if path == provider_image {
                provider_hash
            } else {
                expected
            }
        {
            return Err("The game or UE4SS version is outside the verified native save contract. No files were changed.".into());
        }
        pins.push(file);
    }
    let profile = loader.join("UE4SS-settings.ini");
    let content =
        String::from_utf8(read(&profile, 65536)?).map_err(|_| "The UE4SS profile is not UTF-8.")?;
    if !compatible_profile(&content, game.profile()) {
        return Err("The required native-save EngineTick settings are unavailable. Restart while stopped to run automatic loader preparation; ambiguous configuration is preserved.".into());
    }
    let mods = loader.join("Mods/mods.txt");
    if exists(&mods)?
        && String::from_utf8(read(&mods, 65536)?)
            .map_err(|_| "The UE4SS mod list is not UTF-8.")?
            .lines()
            .filter(|line| !line.trim_start().starts_with(';'))
            .any(|line| line.to_ascii_lowercase().contains("lgsmworldsave"))
    {
        return Err("LgsmWorldSave already has an operator mod-list entry. Remove that entry manually before owned enabled.txt preparation; existing files were preserved.".into());
    }
    Ok(pins)
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

pub(crate) fn exists(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn pin(path: &Path, create: bool) -> Result<Vec<File>, String> {
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

pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
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

pub(crate) fn publish(
    path: &Path,
    original: Option<&[u8]>,
    replacement: &[u8],
) -> Result<(), String> {
    let accepted = match original {
        Some(original) => {
            app_storage::compare_and_swap_file_atomically(path, original, replacement)
        }
        None => app_storage::create_file_atomically(path, replacement).map(|()| true),
    }
    .map_err(|error| format!("Cannot publish a native world-save control file: {error}"))?;
    if !accepted {
        return Err("A control file changed outside the installer; it was preserved.".into());
    }
    Ok(())
}

pub(super) fn payload<'a>(game: Game, script: &'a [u8]) -> BTreeMap<&'static str, &'a [u8]> {
    let mut files = BTreeMap::from([("Scripts/main.lua", script), ("enabled.txt", &[] as &[u8])]);
    if let Some((name, bytes)) = game.native_payload() {
        files.insert(name, bytes);
    }
    files
}

fn validate_hashes(game: Game, files: &BTreeMap<String, String>) -> bool {
    let allowed = payload(game, &[]);
    files.keys().all(|name| {
        allowed.contains_key(name.as_str())
            || (game == Game::Scum && name == RETIRED_SCUM_BOOTSTRAP)
    }) && files
        .values()
        .all(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn upgradable_record(game: Game, files: &BTreeMap<String, String>) -> bool {
    let current = payload(game, &[]);
    (files.len() == current.len() && current.keys().all(|name| files.contains_key(*name)))
        // The original owned Moria mod had no native DLL. Add only the newly
        // bundled DLL while retaining CAS checks on both previously owned files.
        || (game == Game::Moria && files.len() == 2
            && files.contains_key("Scripts/main.lua") && files.contains_key("enabled.txt"))
        || (game == Game::Scum && files.len() == current.len() + 1
            && current.keys().all(|name| files.contains_key(*name))
            && files.contains_key(RETIRED_SCUM_BOOTSTRAP))
}

pub(super) fn owned(game: Game, root: &Path) -> Result<bool, String> {
    let _root = pin(root, false)?;
    if !exists(&root.join(OWNER))? {
        return Ok(false);
    }
    let owner: Ownership = serde_json::from_slice(&read(&root.join(OWNER), 8192)?)
        .map_err(|_| "The native save ownership record is invalid; files were preserved.")?;
    if owner.protocol != 1
        || owner.module_id != game.id()
        || !validate_hashes(game, &owner.files)
        || owner.pending_files.as_ref().is_some_and(|files| {
            files.len() < payload(game, &[]).len()
                || !upgradable_record(game, files)
                || !validate_hashes(game, files)
        })
        || (owner.pending_files.is_none() && !upgradable_record(game, &owner.files))
    {
        return Err(
            "The native save ownership record does not match this instance; files were preserved."
                .into(),
        );
    }
    Ok(true)
}

/// Read-only admission: an older complete payload can keep serving the roster,
/// but only this application's verified payload may authorize native actions.
pub(super) fn complete(game: Game, root: &Path, script: &[u8]) -> Result<bool, String> {
    let _root = pin(root, false)?;
    let owner_path = root.join(OWNER);
    if !exists(&owner_path)? {
        return Ok(false);
    }
    let _version = validate(game, root)?;
    let owner: Ownership = serde_json::from_slice(&read(&owner_path, 8192)?)
        .map_err(|_| "The native world-save control ownership record is invalid.")?;
    if owner.protocol != 1
        || owner.module_id != game.id()
        || owner.files.len() != payload(game, &[]).len()
        || !validate_hashes(game, &owner.files)
        || owner.pending_files.is_some()
    {
        return Err("The native world-save control installation is incomplete; restart this instance to prepare it.".into());
    }
    let payload = payload(game, script);
    if game == Game::Scum && exists(&root.join(mod_dir(game)).join(RETIRED_SCUM_BOOTSTRAP))? {
        return Err("The retired SCUM observer is still installed; restart this instance to prepare native save requests.".into());
    }
    for (name, bytes) in payload {
        let path = root.join(mod_dir(game)).join(name);
        let _parents = pin(
            path.parent().ok_or("The control file has no directory.")?,
            false,
        )?;
        let actual = hash(&read(&path, MAX_FILE)?);
        if owner.files.get(name) != Some(&actual) {
            return Err(
                "An owned native world-save control file changed; native actions are unavailable."
                    .into(),
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
pub(super) fn install(game: Game, root: &Path, script: &[u8]) -> Result<(), String> {
    if script.is_empty() || script.len() > 128 * 1024 {
        return Err("The bundled native world-save control payload is invalid.".into());
    }
    let _root = pin(root, false)?;
    let _version = validate(game, root)?;
    publish_payload(game, root, script)
}

// The caller has retained the fixed native validation pins. Keeping publication
// separate lets fixtures exercise the real recovery/CAS contract without a game.
pub(super) fn publish_payload(game: Game, root: &Path, script: &[u8]) -> Result<(), String> {
    publish_payload_with(game, root, script, publish)
}

fn publish_payload_with(
    game: Game,
    root: &Path,
    script: &[u8],
    mut commit: impl FnMut(&Path, Option<&[u8]>, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let _root = pin(root, false)?;
    let owner_path = root.join(OWNER);
    let original_owner = if exists(&owner_path)? {
        Some(read(&owner_path, 8192)?)
    } else {
        None
    };
    let payload = payload(game, script)
        .into_iter()
        .map(|(name, bytes)| (name.to_owned(), bytes))
        .collect::<BTreeMap<_, _>>();
    let expected = payload
        .iter()
        .map(|(name, bytes)| (name.clone(), hash(bytes)))
        .collect::<BTreeMap<_, _>>();
    let mut owner = if let Some(bytes) = &original_owner {
        let owner: Ownership = serde_json::from_slice(bytes)
            .map_err(|_| "The control ownership record is invalid; files were preserved.")?;
        if owner.protocol != 1
            || owner.module_id != game.id()
            || !validate_hashes(game, &owner.files)
            || owner.pending_files.as_ref().is_some_and(|pending| {
                pending.len() < payload.len()
                    || !validate_hashes(game, pending)
                    || !upgradable_record(game, pending)
            })
            || (owner.pending_files.is_none() && !upgradable_record(game, &owner.files))
        {
            return Err(
                "The control ownership does not match this installation; files were preserved."
                    .into(),
            );
        }
        owner
    } else {
        if exists(&root.join(mod_dir(game)))? || exists(&root.join(DIRECTORY))? {
            return Err(
                "An unowned native world-save control directory already exists; it was preserved."
                    .into(),
            );
        }
        Ownership {
            protocol: 1,
            module_id: game.id().into(),
            files: BTreeMap::new(),
            pending_files: None,
        }
    };
    // Only the former, unchanged owned bootstrap may be retired. An interrupted
    // retirement may leave its old receipt with an absent file and is resumable.
    let retired = root.join(mod_dir(game)).join(RETIRED_SCUM_BOOTSTRAP);
    let retired_hash = if game == Game::Scum && exists(&retired)? {
        let _parents = pin(
            retired.parent().ok_or("Invalid retired bootstrap path")?,
            false,
        )?;
        let actual = hash(&read(&retired, MAX_FILE)?);
        if owner.files.get(RETIRED_SCUM_BOOTSTRAP) != Some(&actual)
            && owner
                .pending_files
                .as_ref()
                .and_then(|files| files.get(RETIRED_SCUM_BOOTSTRAP))
                != Some(&actual)
        {
            return Err(
                "The retired SCUM bootstrap is unowned or changed; it was preserved.".into(),
            );
        }
        Some(actual)
    } else {
        None
    };
    let mut originals = BTreeMap::new();
    for name in payload.keys() {
        let path = root.join(mod_dir(game)).join(name);
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
                    "An owned native world-save control file was modified; it was preserved."
                        .into(),
                );
            }
            // A previously interrupted version can still be the actual owned
            // payload. Preserve that identity before replacing its old intent.
            owner.files.insert(name.clone(), current);
            originals.insert(name.clone(), bytes);
        } else if owner.pending_files.is_none() && owner.files.contains_key(name) {
            return Err(
                "An owned native world-save control file is missing; the installation was preserved."
                    .into(),
            );
        }
    }
    if let Some(expected) = retired_hash {
        crate::commands::commands_managed_save_control::retire_owned_file(&retired, &expected)?;
    }
    owner.files.remove(RETIRED_SCUM_BOOTSTRAP);
    owner.pending_files = Some(expected.clone());
    let intent = serde_json::to_vec(&owner).map_err(|error| error.to_string())?;
    commit(&owner_path, original_owner.as_deref(), &intent)?;
    let _mod = pin(&root.join(mod_dir(game)), true)?;
    let _scripts = pin(&root.join(mod_dir(game)).join("Scripts"), true)?;
    let _exchange = pin(&root.join(DIRECTORY), true)?;
    // Enable last. The ownership intent recognizes every partial publication,
    // while an unknown external modification can never authorize replacement.
    let order = payload
        .keys()
        .map(String::as_str)
        .filter(|name| *name != "enabled.txt")
        .chain(std::iter::once("enabled.txt"));
    for name in order {
        let _parents = pin(
            root.join(mod_dir(game))
                .join(name)
                .parent()
                .ok_or("Invalid native payload parent")?,
            true,
        )?;
        commit(
            &root.join(mod_dir(game)).join(name),
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
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "unreal_save_installation_tests.rs"]
mod tests;
