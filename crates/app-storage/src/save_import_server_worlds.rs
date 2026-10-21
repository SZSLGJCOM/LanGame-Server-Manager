//! Existing dedicated-server data only. Native identities remain unchanged;
//! mappings isolate world payloads from each target's host configuration.
use super::*;
// Native slot paths and names are shared with the read-only save slot listing.
use crate::backups::save_file_listing::slots;

#[path = "save_import_server_worlds_bindings.rs"]
mod bindings;
use bindings::*;

pub(super) fn hint(context: &Context) -> Option<String> {
    Some(match context.record.summary.module_id.as_str() {
        "rimworld" => "Select the original RimWorld Together Assets directory, including WorldValuesFile.json, Users/ and Saves/. Match Together/Mods and keep original account names. Target Configs remain separate.",
        "rust" => "Select one stopped server identity directory containing its matching .sav, .map and complete player databases, including SQLite sidecars. Match Map Type, Custom Map URL, Seed and World Size first; host cfg and Rust+ pairing identity are excluded.",
        "satisfactory" => "Select the stopped server's SaveGames directory (containing server/*.sav), or its server subdirectory containing world .sav files. World saves are added to the target server directory; ServerSettings identity is excluded. Select and load the imported session through native Server Manager after starting.",
        "sevendaystodie" => "Select the original UserDataFolder containing Saves/<GameWorld>/<GameName> and GeneratedWorlds/<GameWorld>. For an installed built-in map, the original Saves root is also accepted. Match World Type / Name and Save Name; set RWG to the actual generated-map folder name first.",
        "sonsoftheforest" => "Select the original Saves root containing DedicatedServer/Multiplayer/<ten-digit SaveSlot> and the complete native slot. Set World Startup Mode to Continue first. Source owners and server configuration are excluded.",
        "theforest" => "Select the original saveFolderPath root containing Multiplayer/Slot<SaveSlot> and its native __RESUME__ checkpoint. Set World Startup Mode to Continue first. The complete selected slot is replaced; other slots remain.",
        "unturned" => "Select the original Servers/<ServerID> directory containing Level/<configured Map>/ and Players/. Keep the complete Level and Players trees, including native backups. Host settings, commands, workshop configuration and access lists are excluded.",
        "windrose" => "Select the complete Default save profile from a cleanly stopped dedicated server, including its RocksDB_v2 world and all backup trees. Match World Island ID and the target's native ServerDescription WorldIslandId first; keep the original world ID and native database files.",
        _ => return None,
    }.into())
}

pub(super) fn plan(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<Option<ServerPlan>, StorageError> {
    let (scopes, warning) = match context.record.summary.module_id.as_str() {
        "rimworld" => {
            require_file(tree, source, "WorldValuesFile.json")?;
            // A populated planet can be substantially larger than configuration
            // JSON. Check its native marker without bounding the whole world to
            // the small metadata limit used for Windrose's description.
            let planet = require_native_path(tree, source, "WorldValuesFile.json")?;
            let bytes = io::read_prefix(&source.join(planet), 65536)?;
            if bytes
                .strip_prefix(b"\xef\xbb\xbf")
                .unwrap_or(&bytes)
                .iter()
                .copied()
                .find(|byte| !byte.is_ascii_whitespace())
                != Some(b'{')
            {
                return Err(invalid(
                    source,
                    "Expected the original Together WorldValuesFile.json planet object.",
                ));
            }
            for directory in ["Users", "Saves"] {
                require_directory(tree, source, directory)?;
            }
            (
                vec![ServerScope::new(
                    context.target.clone(),
                    vec![mapping("", "", true)],
                )],
                "Together accounts and shared-world links are copied unchanged. Match the original client, Together, Mods and UseClientSave policy; target Configs and server credentials remain authoritative.",
            )
        }
        "rust" => (
            vec![rust(context, source, tree)?],
            "The complete recognized native world/player payload replaces its old target counterparts. Host cfg, plugin data and Rust+ pairing credentials remain separate. Native save protocol/forced wipes and custom-map compatibility require startup verification; saves are never renamed to bypass a wipe.",
        ),
        "satisfactory" => (
            vec![satisfactory(context, source, tree)?],
            "Only world .sav files are added or replaced; existing sessions and target ServerSettings identity remain. Start the server, select the imported save in native Server Manager and configure auto-load there. Player online IDs and Mods are not converted.",
        ),
        "sevendaystodie" => (
            sevendays(context, source, tree)?,
            "The selected game save and, when required, its generated map are imported together. Other target maps/saves remain. Match game version, Mods and player IDs; the source serveradmin and network configuration are excluded.",
        ),
        "sonsoftheforest" => {
            continuing(context, source, "save_mode")?;
            let slot = slot(context, source, None)?;
            let relative = PathBuf::from(slots::sons_slot_dir(slot));
            let directory = require_directory(tree, source, &relative)?;
            let zip = relative.join("SaveData.zip");
            if file(tree, &zip) {
                let path = require_native_path(tree, source, &zip)?;
                if !io::read_prefix(&source.join(path), 4)?.starts_with(b"PK\x03\x04") {
                    return Err(invalid(
                        source,
                        "Expected the original native SaveData.zip container.",
                    ));
                }
            } else {
                require_file(tree, source, relative.join("GameStateSaveData.json"))?;
                require_file(tree, source, relative.join("SaveData.json"))?;
            }
            (
                vec![ServerScope::new(
                    context.target.clone(),
                    vec![mapping(directory, &relative, true)],
                )],
                "Only the complete selected dedicated-server slot is replaced. SaveMode must stay Continue. Native container contents, player ownership and successful world loading still require in-game verification; no single-player or P2P host conversion is performed.",
            )
        }
        "theforest" => {
            continuing(context, source, "init_type")?;
            let relative =
                PathBuf::from(slots::theforest_slot_dir(slot(context, source, Some(5))?));
            let directory = require_directory(tree, source, &relative)?;
            if !tree.entries.keys().any(|path| {
                path.parent() == Some(directory.as_path())
                    && file(tree, path)
                    && path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().ends_with("__RESUME__"))
            }) {
                return Err(invalid(
                    source,
                    "The selected Multiplayer slot has no nonempty native __RESUME__ checkpoint.",
                ));
            }
            (
                vec![ServerScope::new(
                    context.target.clone(),
                    vec![mapping(directory, &relative, true)],
                )],
                "The complete selected multiplayer slot is replaced with native names unchanged. initType must stay Continue. Original server configuration and other slots remain; original host characters are not converted.",
            )
        }
        "unturned" => (
            vec![unturned(context, source, tree)?],
            "Level and Players are replaced together, preserving native SteamID/character/map links and backups. Target commands, gameplay settings, workshop files and access lists remain. Match map assets, Mods and game versions separately.",
        ),
        "windrose" => (
            vec![windrose(context, source, tree)?],
            "A stopped native Default profile is copied as a complete unit, including runtime databases and backup trees. The original islandId is retained; target ServerDescription is not imported or rewritten. Native version migration, world rules, database consistency and client character continuity require startup verification.",
        ),
        _ => return Ok(None),
    };
    // Mixed source roots may legitimately contain host configuration. Inspect
    // only copied payloads; unmapped source configuration is never published.
    let roots: std::collections::BTreeSet<_> = scopes
        .iter()
        .flat_map(|scope| &scope.mappings)
        .map(|map| path_key(&map.source))
        .collect();
    let mut payload = tree.clone();
    payload.entries.retain(|path, _| {
        roots.contains(&String::new())
            || path
                .ancestors()
                .any(|parent| roots.contains(&path_key(parent)))
    });
    io::reject_non_save_entries(&payload, source, &[])?;
    let mut plan = ServerPlan::new(scopes, vec![warning.into()]);
    if context.record.summary.module_id == "windrose" {
        plan.guards
            .push(context.install.join("R5/ServerDescription.json"));
    }
    Ok(Some(plan))
}

fn mapping(source: impl AsRef<Path>, target: impl AsRef<Path>, replace: bool) -> ServerMapping {
    ServerMapping {
        source: source.as_ref().to_owned(),
        target: target.as_ref().to_owned(),
        replace,
    }
}

fn within(path: &Path, root: &Path) -> bool {
    root.as_os_str().is_empty()
        || path
            .ancestors()
            .any(|parent| io::same_relative_path(parent, root))
}

fn actual<'a>(tree: &'a io::Inventory, path: &Path) -> Option<&'a PathBuf> {
    tree.entries
        .get_key_value(path)
        .map(|(path, _)| path)
        .or_else(|| {
            tree.entries
                .keys()
                .find(|candidate| io::same_relative_path(candidate, path))
        })
}

fn file(tree: &io::Inventory, path: impl AsRef<Path>) -> bool {
    tree.entries
        .get(path.as_ref())
        .or_else(|| actual(tree, path.as_ref()).and_then(|path| tree.entries.get(path)))
        .is_some_and(|value| value.as_ref().is_some_and(|(size, _)| *size > 0))
}

fn path_key(path: &Path) -> String {
    let key = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn require_native_path<'a>(
    tree: &'a io::Inventory,
    source: &Path,
    path: impl AsRef<Path>,
) -> Result<&'a PathBuf, StorageError> {
    actual(tree, path.as_ref()).ok_or_else(|| {
        invalid(
            source,
            format!("Missing native file {}.", path.as_ref().display()),
        )
    })
}

fn require_file(
    tree: &io::Inventory,
    source: &Path,
    path: impl AsRef<Path>,
) -> Result<(), StorageError> {
    if file(tree, &path) {
        Ok(())
    } else {
        Err(invalid(
            source,
            format!(
                "Missing native save file {}. Select the complete original dedicated-server save tree and match Configuration.",
                path.as_ref().display()
            ),
        ))
    }
}

fn require_directory(
    tree: &io::Inventory,
    source: &Path,
    path: impl AsRef<Path>,
) -> Result<PathBuf, StorageError> {
    actual(tree, path.as_ref())
        .filter(|path| tree.entries.get(*path).is_some_and(Option::is_none))
        .cloned()
        .ok_or_else(|| {
            invalid(
                source,
                format!("Missing native save directory {}.", path.as_ref().display()),
            )
        })
}

fn setting<'a>(context: &'a Context, source: &Path, key: &str) -> Result<&'a str, StorageError> {
    context
        .settings
        .get(key)
        .and_then(Value::as_str)
        .filter(|name| slots::is_native_name(name))
        .ok_or_else(|| {
            invalid(
                source,
                format!(
                    "Set {key} to the original single native world/map name in Configuration first."
                ),
            )
        })
}

fn continuing(context: &Context, source: &Path, key: &str) -> Result<(), StorageError> {
    if context
        .settings
        .get(key)
        .and_then(Value::as_str)
        .is_some_and(|mode| mode.eq_ignore_ascii_case("Continue"))
    {
        Ok(())
    } else {
        Err(invalid(
            source,
            format!(
                "Set {key} to Continue in Configuration before importing; New would overwrite the imported world."
            ),
        ))
    }
}

fn slot(context: &Context, source: &Path, maximum: Option<u64>) -> Result<u64, StorageError> {
    slots::configured_save_slot(&context.settings, maximum).ok_or_else(|| {
        invalid(
            source,
            "Set a valid native Save Slot in Configuration first.",
        )
    })
}

fn read_source_json(
    tree: &io::Inventory,
    source: &Path,
    path: &Path,
) -> Result<Value, StorageError> {
    let path =
        actual(tree, path).ok_or_else(|| invalid(source, "Missing native metadata file."))?;
    let bytes = io::read_prefix(&source.join(path), 1024 * 1024 + 1)?;
    if bytes.len() > 1024 * 1024
        || !tree
            .entries
            .get(path)
            .and_then(Option::as_ref)
            .is_some_and(|(size, hash)| *size == bytes.len() as u64 && *hash == io::digest(&bytes))
    {
        return Err(invalid(
            &source.join(path),
            "Native metadata is oversized or changed while checking.",
        ));
    }
    let document: Value = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(&source.join(path), error.to_string()))?;
    if !document.is_object() {
        return Err(invalid(
            &source.join(path),
            "Native world metadata must be a JSON object.",
        ));
    }
    Ok(document)
}

#[cfg(test)]
#[path = "save_import_server_worlds_tests.rs"]
mod tests;
