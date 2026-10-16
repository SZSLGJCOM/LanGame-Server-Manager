use super::*;

pub(super) fn support(context: &Context) -> InstanceSaveImportSupport {
    let settings = &context.settings;
    let hint = match context.record.summary.module_id.as_str() {
        "minecraft" => Some("Select the complete Java world directory containing level.dat and region/. The current level_name selects the destination; use matching game and Mod versions.".into()),
        "terraria" => Some(format!("Select a directory containing the configured world_file: {}. Change World Save File in Configuration first if needed. Include its .wld.bak; tModLoader worlds are not accepted by this vanilla importer.", text(settings, "world_file"))),
        "valheim" => Some(format!("Select a save root containing worlds_local/{}.fwl + .db, or worlds_local/{}/ with complete committed .fwl2 + .db2 + .ok generations. Match world_name in Configuration first; keep the entire native world tree.", text(settings, "world_name"), text(settings, "world_name"))),
        "corekeeper" => Some(format!("Select the complete native worlds directory containing the configured slot {}.world.gzip and all associated files. Match World Slot in Configuration first; player saves remain managed by the game.", text(settings, "world_index"))),
        "enshrouded" => Some("Select the complete dedicated-server savegame directory containing world0.ksd, or the complete first-slot legacy 3ad85aea file family. The official game performs any format migration when started; other single-player slots must first be prepared using the game's supported procedure.".into()),
        "astroneer" => Some(format!("Select SaveGames containing {}$YYYY.MM.DD-HH.MM.SS.savegame. Match Active Save Name and enable Load Autosave in Configuration first. Keep the native filename.", text(settings, "active_save_file_name"))),
        "abioticfactor" => Some(format!("Select the complete Steam world directory containing WorldSave_Facility.sav and WorldSave_MetaData.sav, including Players/. The destination world_save_name is {}. Cross-platform save conversion is not performed.", text(settings, "world_save_name"))),
        "returntomoria" => Some(format!("Select the directory containing the original MW_*.sav file selected by Optional World Filename (world_file_name = {}). Set that exact filename in Configuration before importing; do not rename the native save.", text(settings, "world_file_name"))),
        "palworld" => Some("Select a complete dedicated-server world directory containing Level.sav and LevelMeta.sav, including Players/ and all associated files. Co-op host GUID conversion and cross-platform player conversion are not performed.".into()),
        "romestead" => Some(format!("Select saved_worlds containing {}/game_state and {}/world_desc, including the complete matching world directory. Match auto_start_world_name in Configuration before importing.", text(settings, "auto_start_world_name"), text(settings, "auto_start_world_name"))),
        "runescapedragonwilds" => Some(format!("Select SaveGames containing the native world whose internal WorldName matches default_world_name = {}. Keep original .sav names and associated files; selection uses native TimeOfSave, not file modification time. DedicatedServer.ini is not imported and world rules remain in the original save bytes.", text(settings, "default_world_name"))),
        "vrising" => Some(format!("Select the complete Saves root containing v4/{}/AutoSave_<number>.save or .save.gz and all matching world metadata. This importer targets the repository's verified PC v4 save generation; other generations require preparation. Match save_name first; do not include source Settings or administrator/ban lists.", text(settings, "save_name"))),
        "necesse" => Some(format!("Select the native saves root containing {}.zip when zip_saves is enabled, or {}/world.dat and worldSettings.cfg otherwise. Keep native ZIP containers unchanged and include all world data. Match world_name and zip_saves in Configuration first; exclude the parent data/cfg server configuration.", text(settings, "world_name"), text(settings, "world_name"))),
        "barotrauma" => Some("Select Multiplayer containing paired <campaign>.save and <campaign>_CharacterData.xml. Matching .<campaign> backups may be included as complete pairs. Import does not select or start a campaign; load it through the game's campaign interface afterwards.".into()),
        _ => None,
    };
    InstanceSaveImportSupport {
        module_id: context.record.summary.module_id.clone(),
        supported: hint.is_some(),
        mode: if hint.is_some() { "native_directory" } else { "guide" }.into(),
        source_hint: hint.unwrap_or_else(|| "Use the game-specific migration instructions. Automatic native save-directory import is not available for this game.".into()),
    }
}

fn text(settings: &Map<String, Value>, key: &str) -> String {
    settings
        .get(key)
        .map(|value| match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn selected(
    settings: &Map<String, Value>,
    key: &str,
    source: &Path,
) -> Result<String, StorageError> {
    let name = text(settings, key);
    if name.is_empty()
        || name.contains(['/', '\\', ':'])
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name.chars().any(char::is_control)
    {
        return Err(invalid(
            source,
            format!(
                "Set {key} to a single native world/save filename in Configuration before importing."
            ),
        ));
    }
    Ok(name)
}

fn entry<'a>(tree: &'a io::Inventory, path: &Path) -> Option<&'a Option<(u64, String)>> {
    tree.entries
        .iter()
        .find(|(candidate, _)| io::same_relative_path(candidate, path))
        .map(|(_, value)| value)
}

fn has_file(tree: &io::Inventory, path: impl AsRef<Path>) -> bool {
    entry(tree, path.as_ref())
        .is_some_and(|entry| entry.as_ref().is_some_and(|(bytes, _)| *bytes > 0))
}

pub(super) fn preserved_paths(context: &Context) -> &'static [&'static str] {
    match context.record.summary.module_id.as_str() {
        "abioticfactor" => &["SandboxSettings.ini"],
        "valheim" => &["adminlist.txt", "bannedlist.txt", "permittedlist.txt"],
        _ => &[],
    }
}

fn require_file(
    tree: &io::Inventory,
    source: &Path,
    path: impl AsRef<Path>,
) -> Result<(), StorageError> {
    if !has_file(tree, &path) {
        return Err(invalid(
            source,
            format!(
                "The selected native save directory is missing {}. Match the selected world in Configuration and include its complete save tree.",
                path.as_ref().display()
            ),
        ));
    }
    Ok(())
}

pub(super) fn validate(
    context: &Context,
    source: &Path,
    tree: &io::Inventory,
) -> Result<Vec<String>, StorageError> {
    let module = context.record.summary.module_id.as_str();
    if !support(context).supported {
        return Err(invalid(
            source,
            "Native directory import is not available for this game. Follow its migration guide.",
        ));
    }
    let settings = &context.settings;
    let mut native_files: Vec<PathBuf> =
        preserved_paths(context).iter().map(PathBuf::from).collect();
    if module == "necesse" && settings.get("zip_saves").and_then(Value::as_bool) == Some(false) {
        native_files
            .push(Path::new(&selected(settings, "world_name", source)?).join("worldSettings.cfg"));
    }
    io::reject_non_save_entries(tree, source, &native_files)?;
    match module {
        "necesse" => {
            let world = selected(settings, "world_name", source)?;
            match settings.get("zip_saves").and_then(Value::as_bool) {
                Some(true) => {
                    let filename = format!("{world}.zip");
                    require_file(tree, source, &filename)?;
                    if !io::read_prefix(&source.join(filename), 4)?.starts_with(b"PK\x03\x04") {
                        return Err(invalid(
                            source,
                            "The selected Necesse world is not a native ZIP save. Preserve the original game container without extracting it.",
                        ));
                    }
                }
                Some(false) => {
                    for file in ["world.dat", "worldSettings.cfg"] {
                        require_file(tree, source, Path::new(&world).join(file))?;
                    }
                }
                None => {
                    return Err(invalid(
                        source,
                        "Set Compress World Saves in Configuration before importing.",
                    ));
                }
            }
        }
        "barotrauma" => validate_barotrauma(source, tree)?,
        "minecraft" => {
            require_file(tree, source, "level.dat")?;
            if !entry(tree, Path::new("region")).is_some_and(Option::is_none) {
                return Err(invalid(
                    source,
                    "Select a complete Java world containing level.dat and region/. Bedrock and separated Bukkit dimensions require preparation first.",
                ));
            }
        }
        "terraria" => {
            let world = selected(settings, "world_file", source)?;
            require_file(tree, source, &world)?;
            if !world.ends_with(".wld")
                || tree.entries.keys().any(|path| {
                    path.extension().is_some_and(|extension| {
                        extension.to_string_lossy().eq_ignore_ascii_case("twld")
                    })
                })
            {
                return Err(invalid(
                    source,
                    "This import requires a vanilla Terraria .wld world. A tModLoader .twld companion requires a matching dedicated-server setup.",
                ));
            }
        }
        "valheim" => {
            let world = selected(settings, "world_name", source)?;
            let directory = Path::new("worlds_local").join(&world);
            if tree.entries.contains_key(&directory) {
                let latest = tree
                    .entries
                    .iter()
                    .filter(|(_, entry)| entry.is_some())
                    .map(|(path, _)| path)
                    .filter(|path| path.parent() == Some(directory.as_path()))
                    .filter_map(|path| {
                        path.file_name()?
                            .to_str()?
                            .strip_prefix("_main.")?
                            .strip_suffix(".ok")?
                            .parse::<u32>()
                            .ok()
                    })
                    .max();
                let number = latest.ok_or_else(|| {
                    invalid(
                        source,
                        "The selected Valheim world has no committed .ok generation.",
                    )
                })?;
                for extension in ["fwl2", "db2"] {
                    require_file(
                        tree,
                        source,
                        directory.join(format!("_main.{number}.{extension}")),
                    )?;
                }
            } else {
                for extension in ["fwl", "db"] {
                    require_file(
                        tree,
                        source,
                        Path::new("worlds_local").join(format!("{world}.{extension}")),
                    )?;
                }
            }
        }
        "corekeeper" => require_file(
            tree,
            source,
            format!("{}.world.gzip", text(settings, "world_index")),
        )?,
        "enshrouded" => {
            if !has_file(tree, "world0.ksd") {
                require_file(tree, source, "3ad85aea")?;
                require_file(tree, source, "3ad85aea-index")?;
            }
        }
        "astroneer" => {
            let name = selected(settings, "active_save_file_name", source)?;
            if settings.get("load_auto_save").and_then(Value::as_bool) != Some(true) {
                return Err(invalid(
                    source,
                    "Enable Load Autosave in Configuration before importing the active ASTRONEER save.",
                ));
            }
            let found = tree
                .entries
                .keys()
                .filter(|path| path.components().count() == 1)
                .filter(|path| has_file(tree, path))
                .filter_map(|path| {
                    path.to_str()
                        .and_then(crate::astroneer_saves::parse_save_filename)
                })
                .any(|(slot, _)| slot == name);
            if !found {
                return Err(invalid(
                    source,
                    format!(
                        "No native {name}$YYYY.MM.DD-HH.MM.SS.savegame file matches Active Save Name."
                    ),
                ));
            }
        }
        "abioticfactor" => {
            require_file(tree, source, "WorldSave_Facility.sav")?;
            require_file(tree, source, "WorldSave_MetaData.sav")?;
        }
        "returntomoria" => {
            let filename = selected(settings, "world_file_name", source)?;
            if !filename.starts_with("MW_") || !filename.ends_with(".sav") {
                return Err(invalid(
                    source,
                    "Set Optional World Filename to the original MW_*.sav filename before importing.",
                ));
            }
            require_file(tree, source, filename)?;
        }
        "palworld" => {
            require_file(tree, source, "Level.sav")?;
            require_file(tree, source, "LevelMeta.sav")?;
            if entry(tree, Path::new("WorldOption.sav")).is_some()
                || entry(
                    tree,
                    Path::new("Players/00000000000000000000000000000001.sav"),
                )
                .is_some()
            {
                return Err(invalid(
                    source,
                    "This appears to be a co-op world. Prepare a dedicated-server save and player GUID mapping with the game's supported migration procedure first.",
                ));
            }
        }
        "romestead" => {
            let world = selected(settings, "auto_start_world_name", source)?;
            for file in ["game_state", "world_desc"] {
                require_file(tree, source, Path::new(&world).join(file))?;
            }
        }
        "runescapedragonwilds" => {
            // This is native metadata text, not a filename. Do not apply path
            // component restrictions to the world's display name.
            let world = settings
                .get("default_world_name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    invalid(
                        source,
                        "Set World to Load in Configuration before importing.",
                    )
                })?;
            let file = crate::dragonwilds_world_settings::io::latest_world(source, world)?
                .ok_or_else(|| invalid(source, format!(
                    "No native Dragonwilds world matches DefaultWorldName {world}. Match World to Load in Configuration before importing."
                )))?;
            let relative = file
                .strip_prefix(source)
                .map_err(|_| invalid(&file, "Selected world escaped its source directory."))?;
            let bytes = crate::dragonwilds_world_settings::io::read_world(&file)?;
            let expected = entry(tree, relative).and_then(Option::as_ref);
            if !expected.is_some_and(|(length, hash)| {
                *length == bytes.len() as u64 && *hash == io::digest(&bytes)
            }) {
                return Err(invalid(
                    &file,
                    "The selected native world changed while checking. Close the source game and inspect again.",
                ));
            }
        }
        "vrising" => {
            let world = selected(settings, "save_name", source)?;
            let directory = Path::new("v4").join(world);
            if !entry(tree, &directory).is_some_and(Option::is_none) {
                return Err(invalid(
                    source,
                    "Select the complete Saves root with the configured v4/<SaveName> world. Older PC save generations and PS5 saves are not converted.",
                ));
            }
            for relative in tree.entries.keys() {
                let name = relative
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                if ["ServerHostSettings.json", "adminlist.txt", "banlist.txt"]
                    .iter()
                    .any(|reserved| name.eq_ignore_ascii_case(reserved))
                {
                    return Err(invalid(
                        &source.join(relative),
                        "Source V Rising server settings or access lists are not imported. Select a prepared dedicated-server Saves root and configure the target separately.",
                    ));
                }
            }
            let found = tree.entries.keys().any(|path| {
                path.parent()
                    .is_some_and(|parent| io::same_relative_path(parent, &directory))
                    && has_file(tree, path)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| {
                            let name = name.to_ascii_lowercase();
                            name.strip_prefix("autosave_")
                                .and_then(|name| {
                                    name.strip_suffix(".save.gz")
                                        .or_else(|| name.strip_suffix(".save"))
                                })
                                .is_some_and(|number| {
                                    !number.is_empty()
                                        && number.bytes().all(|byte| byte.is_ascii_digit())
                                        && number.parse::<u32>().is_ok()
                                })
                        })
            });
            if !found {
                return Err(invalid(
                    source,
                    "The configured PC v4 world has no nonempty native AutoSave_<number>.save or .save.gz. Include its complete world directory and metadata.",
                ));
            }
        }
        _ => return Err(invalid(source, "No native save profile is available.")),
    }
    let mut warnings = vec!["Import replaces the destination save directory after creating a safeguard backup. The source stays unchanged. Game version, Mods, player ownership and successful in-game loading must be verified after starting the server.".into()];
    if module == "abioticfactor" {
        warnings.push("Source SandboxSettings.ini is excluded. The existing destination SandboxSettings.ini and LGSM Configuration remain authoritative.".into());
    } else if module == "valheim" {
        warnings.push("Source adminlist.txt, bannedlist.txt and permittedlist.txt are excluded. Existing destination access lists and LGSM Configuration are preserved.".into());
    } else if module == "runescapedragonwilds" {
        warnings.push("Dragonwilds world rules travel inside the original .sav and are not rewritten during import. Native world-name/timestamp metadata selects the world; successful full-world loading and player identity still require startup verification. LGSM ports, passwords, owner permissions and DedicatedServer.ini remain separate.".into());
    } else if module == "vrising" {
        warnings.push("Only the known PC v4 save-directory layout is checked; autosave payload compatibility and player continuity require the native game to load it. World metadata and any world-local gameplay rules are copied unchanged. Source host/network settings and access lists are not taken over; review target gameplay overrides and Mods separately.".into());
    } else if module == "necesse" {
        warnings.push("Native Necesse world rules and progress travel together. ZIP saves are copied as containers without extraction. Match Mods and the target's outside-character policy before joining; server passwords and owner configuration are not imported.".into());
    } else if module == "barotrauma" {
        warnings.push("After import, select and load the campaign in the game's campaign interface. Import preserves paired campaign and character files but does not convert a single-player character or choose a campaign automatically.".into());
    }
    Ok(warnings)
}

fn validate_barotrauma(source: &Path, tree: &io::Inventory) -> Result<(), StorageError> {
    let mut main_campaigns = 0;
    for path in tree.entries.keys() {
        if path.components().count() != 1 || !has_file(tree, path) {
            return Err(invalid(
                source,
                "Prepare only complete Barotrauma campaign/character pairs at the Multiplayer root, without subdirectories or empty files.",
            ));
        }
        let name = path
            .to_str()
            .ok_or_else(|| invalid(source, "Invalid campaign filename."))?;
        let (native, suffix, prefix) = match name.rsplit_once(".bk") {
            Some((stem, index))
                if stem.starts_with('.')
                    && !index.is_empty()
                    && index.bytes().all(|byte| byte.is_ascii_digit())
                    && index.parse::<u32>().is_ok() =>
            {
                (&stem[1..], format!(".bk{index}"), ".")
            }
            _ => (name, String::new(), ""),
        };
        if let Some(stem) = native.strip_suffix(".save").filter(|stem| !stem.is_empty()) {
            let characters = format!("{prefix}{stem}_CharacterData.xml{suffix}");
            require_file(tree, source, &characters)?;
            if !io::read_prefix(&source.join(path), 3)?.starts_with(b"\x1f\x8b\x08") {
                return Err(invalid(
                    &source.join(path),
                    "Expected a native gzip Barotrauma campaign container.",
                ));
            }
            let bytes = io::read_prefix(&source.join(&characters), 65536)?;
            let mut reader = quick_xml::Reader::from_reader(bytes.as_slice());
            loop {
                use quick_xml::events::Event;
                match reader
                    .read_event()
                    .map_err(|error| invalid(&source.join(&characters), error.to_string()))?
                {
                    Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
                    Event::Text(text)
                        if text.as_ref().bytes().all(|byte| byte.is_ascii_whitespace()) => {}
                    Event::Start(node) | Event::Empty(node)
                        if node.name().as_ref() == "CharacterData" =>
                    {
                        break;
                    }
                    _ => {
                        return Err(invalid(
                            &source.join(&characters),
                            "Expected a native CharacterData XML root within the first 64 KiB.",
                        ));
                    }
                }
            }
            if suffix.is_empty() {
                main_campaigns += 1;
            }
        } else if let Some(stem) = native
            .strip_suffix("_CharacterData.xml")
            .filter(|stem| !stem.is_empty())
        {
            require_file(tree, source, format!("{prefix}{stem}.save{suffix}"))?;
        } else {
            return Err(invalid(
                &source.join(path),
                "This directory contains unrelated files. Prepare only native Barotrauma campaign and matching character pairs, including paired numbered backups if needed.",
            ));
        }
    }
    if main_campaigns == 0 {
        return Err(invalid(
            source,
            "Include at least one main Barotrauma campaign and matching character file; standalone backup recovery is not performed.",
        ));
    }
    Ok(())
}
