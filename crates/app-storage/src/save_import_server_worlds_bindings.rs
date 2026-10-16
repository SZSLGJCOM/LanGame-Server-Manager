use super::*;

fn rust_payload(path: &Path) -> bool {
    let Some(name) = path.to_str().filter(|_| path.components().count() == 1) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    // Facepunch's server.identity file inventory distinguishes persistent
    // gameplay databases from server identity and Rust+ pairing credentials.
    // https://wiki.facepunch.com/rust/Getting-Started_w-Server
    if name.starts_with("player.tokens") || name == "companion.id" {
        return false;
    }
    name == "loadouts"
        || name == "serveremoji"
        || name.starts_with("player.")
        || name.starts_with("sv.files")
        || name.ends_with(".map")
        || name.ends_with(".sav")
        || name.contains(".sav.")
}

pub(super) fn rust(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let level = setting(context, source, "level")?;
    let custom = context
        .settings
        .get("level_url")
        .and_then(Value::as_str)
        .is_some_and(|url| !url.trim().is_empty());
    let prefix = if custom {
        String::new()
    } else if matches!(level, "Procedural Map" | "Barren") {
        let number = |key| {
            context
                .settings
                .get(key)
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    invalid(
                        source,
                        format!("Set {key} before importing the matching Rust map."),
                    )
                })
        };
        format!(
            "{}.{}.{}.",
            level.replace(' ', "").to_ascii_lowercase(),
            number("world_size")?,
            number("seed")?
        )
    } else {
        format!("{}.", level.to_ascii_lowercase())
    };
    let saves: Vec<_> = tree
        .entries
        .keys()
        .filter(|path| {
            path.components().count() == 1
                && file(tree, path)
                && path
                    .to_str()
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".sav"))
        })
        .collect();
    if !saves.iter().any(|path| {
        path.to_string_lossy()
            .to_ascii_lowercase()
            .starts_with(&prefix)
    }) || !tree.entries.keys().any(|path| {
        path.components().count() == 1
            && file(tree, path)
            && path.to_str().is_some_and(|name| {
                name.to_ascii_lowercase().ends_with(".map")
                    && (custom || name.to_ascii_lowercase().starts_with(&prefix))
            })
    }) {
        return Err(invalid(
            source,
            "No native .sav/.map set matches the configured Rust map, seed and size. Include the complete identity save payload and match Configuration first.",
        ));
    }
    let mappings = tree
        .entries
        .keys()
        .filter(|path| rust_payload(path))
        .map(|path| mapping(path, path, true))
        .collect();
    let mut scope = ServerScope::new(context.target.clone(), mappings);
    if let Some(current) = io::current_inventory(&context.target)? {
        scope.clear = current
            .entries
            .keys()
            .filter(|path| rust_payload(path))
            .cloned()
            .collect();
    }
    Ok(scope)
}

pub(super) fn satisfactory(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let server = actual(tree, Path::new("server"))
        .filter(|path| tree.entries.get(*path).is_some_and(Option::is_none));
    let mut mappings = Vec::new();
    for path in tree.entries.keys() {
        let parent = path.parent().unwrap_or(Path::new(""));
        let allowed_parent = server
            .is_some_and(|directory| io::same_relative_path(parent, directory))
            || parent.as_os_str().is_empty();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if allowed_parent
            && file(tree, path)
            && name.to_ascii_lowercase().ends_with(".sav")
            && !name.to_ascii_lowercase().starts_with("serversettings")
        {
            // Native dedicated sessions live in SaveGames/server, including
            // when the user directly selects that source subdirectory.
            let target = Path::new("server").join(name);
            if mappings
                .iter()
                .any(|existing: &ServerMapping| io::same_relative_path(&existing.target, &target))
            {
                return Err(invalid(
                    source,
                    "Ambiguous Satisfactory save names exist at both the root and server/. Select only the original server subdirectory.",
                ));
            }
            mappings.push(mapping(path, target, false));
        }
    }
    if mappings.is_empty() {
        return Err(invalid(
            source,
            "No nonempty dedicated-server world .sav files were found. ServerSettings files are host identity and are excluded.",
        ));
    }
    Ok(ServerScope::new(context.target.clone(), mappings))
}

pub(super) fn sevendays(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<Vec<ServerScope>, StorageError> {
    let world = setting(context, source, "game_world")?;
    let game = setting(context, source, "world_name")?;
    if world.eq_ignore_ascii_case("RWG") {
        return Err(invalid(
            source,
            "Set World Type / Name to the actual generated world folder name, rather than RWG, so the imported map is loaded without generating another world.",
        ));
    }
    // serverconfig.xml sets UserDataFolder to instance_root; saves and generated
    // map data therefore have separate owned roots, not one replaceable tree.
    let destination = Path::new(world).join(game);
    let original = if actual(tree, &Path::new("Saves").join(&destination)).is_some() {
        Path::new("Saves").join(&destination)
    } else {
        destination.clone()
    };
    let original = require_directory(tree, source, original)?;
    require_file(tree, source, original.join("main.ttw"))?;
    require_directory(tree, source, original.join("Region"))?;
    let mut scopes = vec![ServerScope::new(
        context.target.clone(),
        vec![mapping(&original, &destination, true)],
    )];
    let generated = Path::new("GeneratedWorlds").join(world);
    if actual(tree, &generated).is_some() {
        let generated = require_directory(tree, source, generated)?;
        for marker in ["map_info.xml", "dtm.raw", "biomes.png", "prefabs.xml"] {
            require_file(tree, source, generated.join(marker))?;
        }
        scopes.push(ServerScope::new(
            instance_root(&context.record).join("GeneratedWorlds"),
            vec![mapping(generated, world, true)],
        ));
    } else {
        // Installed map data (including custom maps explicitly installed there)
        // is authoritative only when an actual complete map is present.
        let installed = context.install.join("Data/Worlds").join(world);
        let installed_tree = io::current_inventory(&installed)?.ok_or_else(|| invalid(source, "This map is not installed. Select the original UserDataFolder with its matching GeneratedWorlds directory as well as Saves."))?;
        for marker in ["map_info.xml", "dtm.raw", "biomes.png", "prefabs.xml"] {
            require_file(&installed_tree, &installed, marker)?;
        }
    }
    Ok(scopes)
}

pub(super) fn unturned(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let map = setting(context, source, "map")?;
    let level = require_directory(tree, source, "Level")?;
    let selected = level.join(map);
    require_directory(tree, source, &selected)?;
    for marker in ["Barricades.dat", "Structures.dat"] {
        require_file(tree, source, selected.join(marker))?;
    }
    // Official U3-SDK LevelSavedata and PlayerSavedata keep both trees under
    // ServerSavedata's server ID; Server/ holds host commands and access lists.
    let players = require_directory(tree, source, "Players")?;
    Ok(ServerScope::new(
        context.target.clone(),
        vec![
            mapping(&level, "Level", true),
            mapping(&players, "Players", true),
        ],
    ))
}

pub(super) fn windrose(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<ServerScope, StorageError> {
    let selected = setting(context, source, "world_island_id")?;
    let candidates: Vec<_> = tree
        .entries
        .keys()
        .filter(|path| {
            let parts: Vec<_> = path.iter().filter_map(|part| part.to_str()).collect();
            parts.len() == 5
                && parts[0] == "RocksDB_v2"
                && parts[2] == "Worlds"
                && parts[3] == selected
                && parts[4] == "WorldDescription.json"
                && file(tree, path)
        })
        .collect();
    if candidates.len() != 1 {
        return Err(invalid(
            source,
            "Select the complete stopped Default profile with exactly one current RocksDB_v2 world matching World Island ID. Older source generations must first complete native migration; duplicate version matches are ambiguous.",
        ));
    }
    let description = candidates[0];
    let document = read_source_json(tree, source, description)?;
    // The installed/native reader uses case-sensitive lowercase islandId; the
    // public guide's example capitalizes it differently. Reuse native spelling.
    if document
        .pointer("/WorldDescription/islandId")
        .and_then(Value::as_str)
        != Some(selected)
    {
        return Err(invalid(
            source,
            "The native WorldDescription islandId does not exactly match its folder and configured World Island ID.",
        ));
    }
    let directory = description
        .parent()
        .ok_or_else(|| invalid(source, "Invalid native world description path."))?;
    if !tree
        .entries
        .keys()
        .any(|path| path != description && within(path, directory) && file(tree, path))
    {
        return Err(invalid(
            source,
            "WorldDescription.json alone is not a complete native world. Include the original database and all associated files.",
        ));
    }
    let server = context.install.join("R5/ServerDescription.json");
    let bytes = io::read_prefix(&server, 1024 * 1024 + 1)?;
    if bytes.len() > 1024 * 1024 {
        return Err(invalid(&server, "Native server description is oversized."));
    }
    let document: Value =
        serde_json::from_slice(&bytes).map_err(|error| invalid(&server, error.to_string()))?;
    if document
        .pointer("/ServerDescription_Persistent/WorldIslandId")
        .and_then(Value::as_str)
        != Some(selected)
    {
        return Err(invalid(
            &server,
            "Match the target's native WorldIslandId and Configuration to the original source world ID before importing. Source server identity is never imported.",
        ));
    }
    Ok(ServerScope::new(
        context.target.clone(),
        vec![mapping("", "", true)],
    ))
}
