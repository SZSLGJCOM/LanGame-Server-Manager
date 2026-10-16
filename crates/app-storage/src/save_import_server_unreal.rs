//! Prepared dedicated-server data only. These adapters select native payloads
//! inside the module's effective save scope; host configuration remains managed.
use super::*;

pub(super) fn hint(context: &Context) -> Option<String> {
    let module = context.record.summary.module_id.as_str();
    Some(match module {
        "arksurvivalevolved" | "arksurvivalascended" => format!(
            "Select the existing main-map save directory or Saved root containing {}.ark, with its player, tribe and backup files. Other maps and cluster transfers are kept separate.",
            setting(context, "map_name")
        ),
        "conanexiles" => "Select ConanSandbox/Saved or a prepared database directory containing game_0.db, or the original Game.db for the game's native upgrade.".into(),
        "humanitz" => format!("Select SaveGames or Saved containing SaveList/Default/Save_{}.sav and its matching world directory, including the shared save files.", setting(context, "save_name")),
        "nightingale" => "Select NWX/Saved containing Offline/ and Config/WindowsServer/ServerState.ini from the same stopped dedicated server.".into(),
        "projectzomboid" => "Select the complete multiplayer world directory containing map_ver.bin and players.db, or a root containing exactly one such world.".into(),
        "scum" => "Select SaveFiles or Saved containing the original dedicated-server SCUM.db and its associated save files.".into(),
        "soulmask" => "Select the existing dedicated Level01_Main world directory, or Saved/Worlds containing Level01_Main/world.db and its associated world data.".into(),
        _ => return None,
    })
}

pub(super) fn plan(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<Option<ServerPlan>, StorageError> {
    let module = context.record.summary.module_id.as_str();
    let (scope, warning, native_config) = match module {
        "arksurvivalevolved" | "arksurvivalascended" => (
            ark(context, source, tree)?,
            "Only the configured main map and its native player/tribe files are imported. Other maps, cluster uploads and server configuration stay separate; single-player host identities are not converted.",
            None,
        ),
        "conanexiles" => (
            conan(context, source, tree)?,
            "The game loads or upgrades the original database on startup. Match the Enhanced version and Mods; source server passwords, settings and server UID are not imported.",
            None,
        ),
        "humanitz" => (
            humanitz(context, source, tree)?,
            "The complete SaveGames tree is imported for the configured SaveName. Local Host/Offline player identity conversion is not performed.",
            None,
        ),
        "nightingale" => {
            let (scope, state) = nightingale(context, source, tree)?;
            (
                scope,
                "Realm data, player Profiles and the native CurrentDeploy selection are imported together. Target connection/admin settings remain managed; in-game single-player migration is separate.",
                Some(state),
            )
        }
        "projectzomboid" => (
            zomboid(context, source, tree)?,
            "World and character records are copied unchanged. Rejoin with the original username/SteamID; login accounts, passwords and server configuration remain managed by the destination. Match the game build and Mods.",
            None,
        ),
        "scum" => (
            scum(context, source, tree)?,
            "Only original dedicated-server SaveFiles are imported. Match the game version and review target wipe settings before starting; single-player Sandbox conversion is not performed.",
            None,
        ),
        "soulmask" => (
            soulmask(context, source, tree)?,
            "The existing Level01_Main dedicated world is imported unchanged. Target gameplay/server settings remain managed; other maps, DLC transfers and single-player identity conversion are separate.",
            None,
        ),
        _ => return Ok(None),
    };
    validate_payload(source, tree, &scope.mappings, native_config)?;
    Ok(Some(ServerPlan::new(vec![scope], vec![warning.into()])))
}

fn setting(context: &Context, key: &str) -> String {
    context
        .settings
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .into()
}

fn native_name(value: &str, source: &Path) -> Result<String, StorageError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|c| c.is_control() || "<>\"/\\:|?*".contains(c))
    {
        return Err(invalid(
            source,
            "The configured native save name must be one filename component.",
        ));
    }
    Ok(value.into())
}

fn inside(path: &Path, prefix: &Path) -> bool {
    let leading: PathBuf = path
        .components()
        .take(prefix.components().count())
        .collect();
    io::same_relative_path(&leading, prefix)
}

fn item(tree: &io::Inventory, name: impl AsRef<Path>, file: bool) -> Option<PathBuf> {
    tree.entries.iter().find_map(|(path, value)| {
        (io::same_relative_path(path, name.as_ref())
            && if file {
                value.as_ref().is_some_and(|(size, _)| *size > 0)
            } else {
                value.is_none()
            })
        .then(|| path.clone())
    })
}

fn required(
    tree: &io::Inventory,
    source: &Path,
    name: impl AsRef<Path>,
) -> Result<PathBuf, StorageError> {
    item(tree, &name, true).ok_or_else(|| {
        invalid(
            source,
            format!(
                "The original dedicated-server save is missing {}.",
                name.as_ref().display()
            ),
        )
    })
}

/// Only a bounded, unambiguous native marker establishes the source root. An
/// existing source server ID is never assumed to equal the destination ID.
fn locate(tree: &io::Inventory, source: &Path, suffix: &Path) -> Result<PathBuf, StorageError> {
    let length = suffix.components().count();
    let mut roots = Vec::new();
    for (path, value) in &tree.entries {
        let parts: Vec<_> = path.components().collect();
        if !value.as_ref().is_some_and(|(bytes, _)| *bytes > 0)
            || parts.len() < length
            || parts.len() > length + 4
        {
            continue;
        }
        let tail: PathBuf = parts[parts.len() - length..].iter().collect();
        if io::same_relative_path(&tail, suffix) {
            roots.push(parts[..parts.len() - length].iter().collect::<PathBuf>());
        }
    }
    roots.sort();
    roots.dedup();
    if roots.len() != 1 {
        return Err(invalid(
            source,
            format!(
                "Select one complete dedicated-server save containing {}; {} matching roots were found.",
                suffix.display(),
                roots.len()
            ),
        ));
    }
    Ok(roots.remove(0))
}

fn mapping(source: PathBuf, target: impl Into<PathBuf>, replace: bool) -> ServerMapping {
    ServerMapping {
        source,
        target: target.into(),
        replace,
    }
}

fn scope(context: &Context, mappings: Vec<ServerMapping>) -> ServerScope {
    ServerScope::new(context.target.clone(), mappings)
}

fn validate_payload(
    source: &Path,
    tree: &io::Inventory,
    mappings: &[ServerMapping],
    native_config: Option<PathBuf>,
) -> Result<(), StorageError> {
    let mut payload = tree.clone();
    payload
        .entries
        .retain(|path, _| mappings.iter().any(|mapping| inside(path, &mapping.source)));
    let allowed: Vec<_> = native_config.into_iter().collect();
    io::reject_non_save_entries(&payload, source, &allowed)
}

fn sqlite(tree: &io::Inventory, source: &Path, path: &Path) -> Result<PathBuf, StorageError> {
    let path = required(tree, source, path)?;
    if io::read_prefix(&source.join(&path), 16)? != b"SQLite format 3\0" {
        return Err(invalid(
            &source.join(&path),
            "Expected an original SQLite game database.",
        ));
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.to_string_lossy()));
        if item(tree, &sidecar, true).is_some() {
            return Err(invalid(
                &source.join(sidecar),
                "The source database has an active journal. Stop the original server and obtain a clean, consistent database backup.",
            ));
        }
    }
    Ok(path)
}

fn ark_world_file(name: &str, map: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let map = map.to_ascii_lowercase();
    if name == format!("{map}.ark")
        || name == format!("{map}_anticorruptionbackup.bak")
        || name == format!("{map}_newlaunchbackup.bak")
    {
        return true;
    }
    let Some(timestamp) = name.strip_prefix(&format!("{map}_")).and_then(|name| {
        name.strip_suffix(".arkrbf")
            .or_else(|| name.strip_suffix(".ark"))
    }) else {
        return false;
    };
    // Native snapshots use DD.MM.YYYY_HH.MM.SS. Match its separators as well
    // as digits so another map sharing this map's prefix is never selected.
    timestamp.len() == 19
        && timestamp
            .bytes()
            .enumerate()
            .all(|(index, byte)| match index {
                2 | 5 | 13 | 16 => byte == b'.',
                10 => byte == b'_',
                _ => byte.is_ascii_digit(),
            })
}

fn ark_character_file(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        ".arkprofile",
        ".arktribe",
        ".arktributetribe",
        ".profilebak",
        ".tribebak",
        ".arkprofilebak",
        ".arktribebak",
    ]
    .iter()
    .any(|suffix| name.ends_with(suffix))
}

fn ark(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let map = native_name(&setting(context, "map_name"), source)?;
    if map.len() > 128
        || !map
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(invalid(
            source,
            "The current ARK map must be a native package identifier.",
        ));
    }
    let root = locate(tree, source, Path::new(&format!("{map}.ark")))?;
    if tree.entries.keys().any(|path| {
        path.parent()
            .is_some_and(|parent| io::same_relative_path(parent, &root))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.to_ascii_lowercase().ends_with(".ark") && !ark_world_file(name, &map)
                })
    }) {
        return Err(invalid(
            source,
            "Multiple main maps share this source save directory. Prepare the selected map and its matching player/tribe files separately.",
        ));
    }
    let id = native_name(&context.record.summary.id, source)?;
    if tree.entries.keys().any(|path| {
        inside(path, &root)
            && path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("LocalPlayer.arkprofile")
            })
    }) {
        return Err(invalid(
            source,
            "Prepare a dedicated-server save first; LocalPlayer.arkprofile host identity conversion is not performed.",
        ));
    }
    let ascended = context.record.summary.module_id == "arksurvivalascended";
    let target_root = if ascended {
        Path::new(&id).join(&map)
    } else {
        PathBuf::from(&id)
    };
    let mut mappings = Vec::new();
    for (path, value) in &tree.entries {
        if value.is_some()
            && path
                .parent()
                .is_some_and(|parent| io::same_relative_path(parent, &root))
        {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| invalid(source, "Invalid native ARK filename."))?;
            if ark_world_file(name, &map) || ark_character_file(name) {
                mappings.push(mapping(path.clone(), target_root.join(name), false));
            }
        }
    }
    // Character/tribe companions supplied beside an ASA map directory retain
    // their original relative position when they are supplied;
    // never read outside the operator's selected source root.
    let companions = ascended
        && root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(&map));
    if companions && let Some(parent) = root.parent() {
        for (path, value) in &tree.entries {
            if value.is_some()
                && path
                    .parent()
                    .is_some_and(|candidate| io::same_relative_path(candidate, parent))
                && let Some(name) = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| ark_character_file(name))
            {
                if name.eq_ignore_ascii_case("LocalPlayer.arkprofile") {
                    return Err(invalid(
                        source,
                        "LocalPlayer.arkprofile requires host identity conversion before import.",
                    ));
                }
                mappings.push(mapping(path.clone(), Path::new(&id).join(name), false));
            }
        }
    }
    let mut result = scope(context, mappings);
    if let Some(current) = io::current_inventory(&context.target)? {
        for (path, value) in current.entries {
            let map_file = path
                .parent()
                .is_some_and(|parent| io::same_relative_path(parent, &target_root))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| ark_world_file(name, &map) || ark_character_file(name));
            let character = ascended
                && path
                    .parent()
                    .is_some_and(|parent| io::same_relative_path(parent, Path::new(&id)))
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(ark_character_file);
            if value.is_some() && (map_file || character) {
                result.clear.push(path);
            }
        }
    }
    Ok(result)
}

fn conan(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let current = locate(tree, source, Path::new("game_0.db"));
    let (root, main, enhanced) = match current {
        Ok(root) => (root, "game_0.db", true),
        Err(_)
            if !tree.entries.keys().any(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("game_0.db"))
            }) =>
        {
            (
                locate(tree, source, Path::new("Game.db"))?,
                "Game.db",
                false,
            )
        }
        Err(error) => return Err(error),
    };
    sqlite(tree, source, &root.join(main))?;
    let mut mappings = Vec::new();
    for (path, value) in &tree.entries {
        if value.is_some()
            && path
                .parent()
                .is_some_and(|parent| io::same_relative_path(parent, &root))
        {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let lower = name.to_ascii_lowercase();
            if lower == main.to_ascii_lowercase()
                || (enhanced && lower.starts_with("game_0_") && lower.ends_with(".db"))
            {
                sqlite(tree, source, path)?;
                mappings.push(mapping(path.clone(), PathBuf::from(name), false));
            }
        }
    }
    let mut result = scope(context, mappings);
    // An old destination game_0.db would hide an imported UE4 Game.db during
    // the engine's documented migration. Journal files must not be reused.
    for base in ["game_0.db", "Game.db"] {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            result.clear.push(PathBuf::from(format!("{base}{suffix}")));
        }
    }
    Ok(result)
}

fn humanitz(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let name = native_name(&setting(context, "save_name"), source)?;
    let marker = PathBuf::from(format!("SaveList/Default/Save_{name}.sav"));
    let root = locate(tree, source, &marker)?;
    for file in [
        marker,
        PathBuf::from("SaveList/SaveCache.sav"),
        PathBuf::from("DedSave_ResGlobal.sav"),
    ] {
        let path = required(tree, source, root.join(file))?;
        if io::read_prefix(&source.join(path), 4)? != b"GVAS" {
            return Err(invalid(
                source,
                "Expected original HumanitZ GVAS save files.",
            ));
        }
    }
    if item(tree, root.join("SaveList/Default").join(&name), false).is_none() {
        return Err(invalid(
            source,
            "Include the complete directory matching the configured HumanitZ SaveName.",
        ));
    }
    if tree.entries.keys().any(|path| {
        inside(path, &root)
            && path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .eq_ignore_ascii_case("Host_Offline.sav")
            })
    }) {
        return Err(invalid(
            source,
            "Prepare a dedicated-server world first; Host_Offline identity conversion is not performed.",
        ));
    }
    Ok(scope(context, vec![mapping(root, "SaveGames", true)]))
}

fn native_text(source: &Path, path: &Path) -> Result<String, StorageError> {
    let bytes = io::read_prefix(&source.join(path), 65537)?;
    if bytes.len() > 65536 {
        return Err(invalid(path, "Native selection metadata exceeds 64 KiB."));
    }
    if bytes.starts_with(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err(invalid(path, "Invalid native UTF-16 selection metadata."));
        }
        let units: Vec<_> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&units).map_err(|error| invalid(path, error.to_string()))
    } else {
        String::from_utf8(bytes)
            .map(|text| text.trim_start_matches('\u{feff}').into())
            .map_err(|error| invalid(path, error.to_string()))
    }
}

fn nightingale(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<(ServerScope, PathBuf), StorageError> {
    let selection = Path::new("Config/WindowsServer/ServerState.ini");
    let root = locate(tree, source, selection)?;
    let state = required(tree, source, root.join(selection))?;
    let mut section = false;
    let mut deploy = None;
    for line in native_text(source, &state)?.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if line.starts_with('[') {
            section = line.eq_ignore_ascii_case("[/Script/NWX.NWXServerState]");
            if section {
                continue;
            }
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid(&state, "Expected native CurrentDeploy selection."))?;
        if !section || !key.trim().eq_ignore_ascii_case("CurrentDeploy") || deploy.is_some() {
            return Err(invalid(
                &state,
                "Only the native CurrentDeploy state is imported; omit unrelated settings from ServerState.ini.",
            ));
        }
        let value = value.trim();
        let value = if value.starts_with('"') {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or_else(|| invalid(&state, "Invalid quoted CurrentDeploy selection."))?
        } else {
            value
        };
        deploy = Some(native_name(value, &state)?);
    }
    let deploy = deploy.ok_or_else(|| {
        invalid(
            &state,
            "The source ServerState.ini has no CurrentDeploy selection.",
        )
    })?;
    let realm = root.join("Offline/DedicatedServer/Deploy").join(&deploy);
    let record = required(tree, source, realm.join("record.json"))?;
    let data: Value = serde_json::from_str(&native_text(source, &record)?)
        .map_err(|error| invalid(&record, error.to_string()))?;
    if data.get("id").and_then(Value::as_str) != Some(deploy.as_str()) {
        return Err(invalid(
            &record,
            "The realm record does not match CurrentDeploy.",
        ));
    }
    let world = required(tree, source, realm.join("state"))?;
    if !io::read_prefix(&source.join(world), 3)?.starts_with(b"\x1f\x8b\x08") {
        return Err(invalid(
            source,
            "Expected an original Nightingale realm state container.",
        ));
    }
    let offline = item(tree, root.join("Offline"), false).ok_or_else(|| {
        invalid(
            source,
            "Include the complete Offline realm and character tree.",
        )
    })?;
    Ok((
        scope(
            context,
            vec![
                mapping(offline, "Offline", true),
                mapping(state.clone(), selection, true),
            ],
        ),
        state,
    ))
}

fn zomboid(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let root = locate(tree, source, Path::new("map_ver.bin"))?;
    sqlite(tree, source, &root.join("players.db"))?;
    if let Some(vehicles) = item(tree, root.join("vehicles.db"), true) {
        sqlite(tree, source, &vehicles)?;
    }
    Ok(scope(context, vec![mapping(root, PathBuf::new(), true)]))
}

fn scum(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let root = locate(tree, source, Path::new("SCUM.db"))?;
    sqlite(tree, source, &root.join("SCUM.db"))?;
    Ok(scope(context, vec![mapping(root, "SaveFiles", true)]))
}

fn soulmask(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let selected = Path::new("Level01_Main/world.db");
    let root = if tree.entries.keys().any(|path| {
        path.components().count() >= 2
            && io::same_relative_path(
                &path
                    .components()
                    .skip(path.components().count() - 2)
                    .collect::<PathBuf>(),
                selected,
            )
    }) {
        locate(tree, source, selected)?.join("Level01_Main")
    } else {
        locate(tree, source, Path::new("world.db"))?
    };
    if root.as_os_str().is_empty()
        && source
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_ascii_lowercase();
                name.starts_with("level") && name.ends_with("_main") && name != "level01_main"
            })
    {
        return Err(invalid(
            source,
            "This Soulmask module loads Level01_Main; the selected directory names another map.",
        ));
    }
    if !root.as_os_str().is_empty()
        && !root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("Level01_Main"))
    {
        return Err(invalid(
            source,
            "The current Soulmask module loads Level01_Main. Select the matching dedicated map; cross-map conversion is not performed.",
        ));
    }
    sqlite(tree, source, &root.join("world.db"))?;
    Ok(scope(
        context,
        vec![mapping(root, "Worlds/Dedicated/Level01_Main", true)],
    ))
}

#[cfg(test)]
#[path = "save_import_server_unreal_tests.rs"]
mod tests;
