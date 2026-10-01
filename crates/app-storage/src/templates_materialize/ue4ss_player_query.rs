use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::ManagedConfigMutation;
use crate::StorageError;

#[derive(Clone, Copy)]
pub(super) enum Game {
    Windrose,
    Dragonwilds,
    Scum,
}

impl Game {
    fn module_id(self) -> &'static str {
        match self {
            Self::Windrose => "windrose",
            Self::Dragonwilds => "runescapedragonwilds",
            Self::Scum => "scum",
        }
    }

    fn loader_path(self) -> &'static str {
        match self {
            Self::Windrose => "R5/Binaries/Win64/ue4ss",
            Self::Dragonwilds => "RSDragonwilds/Binaries/Win64/ue4ss",
            Self::Scum => "SCUM/Binaries/Win64/ue4ss",
        }
    }

    fn script(self) -> &'static [u8] {
        match self {
            Self::Windrose => include_bytes!(
                "../../../../modules/windrose/extensions/LgsmPlayerQuery/Scripts/main.lua"
            ),
            Self::Scum => include_bytes!(
                "../../../../modules/scum/extensions/LgsmPlayerQuery/Scripts/main.lua"
            ),
            Self::Dragonwilds => include_bytes!(
                "../../../../modules/runescapedragonwilds/extensions/LgsmPlayerQuery/Scripts/main.lua"
            ),
        }
    }

    fn profile(self) -> &'static str {
        match self {
            Self::Windrose => {
                include_str!("../../../../modules/windrose/extensions/UE4SS-settings.ini")
            }
            Self::Scum => include_str!("../../../../modules/scum/extensions/UE4SS-settings.ini"),
            Self::Dragonwilds => include_str!(
                "../../../../modules/runescapedragonwilds/extensions/UE4SS-settings.ini"
            ),
        }
    }
}

/// Prepare only our read-only script in an explicitly installed UE4SS runtime.
/// Loader binaries and the operator's loader configuration are never replaced.
pub(super) fn materialize(
    game: Game,
    install: &Path,
    running: bool,
    files: &mut ManagedConfigMutation,
) -> Result<(), StorageError> {
    if running {
        return Ok(());
    }
    let loader = install.join(game.loader_path());
    if !loader.join("UE4SS.dll").is_file() {
        return Ok(());
    }
    let fail_io = |path: &Path, error| failure(game, path, error);
    let root = fs::canonicalize(install).map_err(|error| fail_io(install, error))?;
    let loader = root.join(game.loader_path());
    check_components(game, &root, &loader)?;
    let settings = loader.join("UE4SS-settings.ini");
    if !settings.is_file() {
        return Ok(());
    }
    check_components(game, &root, &settings)?;
    if fs::metadata(&settings)
        .map_err(|error| fail_io(&settings, error))?
        .len()
        > 64 * 1024
    {
        return Err(failure(
            game,
            &settings,
            "The UE4SS configuration exceeds its read limit.",
        ));
    }
    let actual = fs::read_to_string(&settings).map_err(|error| fail_io(&settings, error))?;
    if !compatible_profile(&actual, game.profile()) {
        return Ok(());
    }
    let scripts = loader.join("Mods/LgsmPlayerQuery/Scripts");
    let exchange = root.join("langame_player_query");
    for directory in [&scripts, &exchange] {
        check_components(game, &root, directory)?;
        fs::create_dir_all(directory).map_err(|error| fail_io(directory, error))?;
    }
    for (path, content) in [
        (scripts.join("main.lua"), game.script()),
        (
            loader.join("Mods/LgsmPlayerQuery/enabled.txt"),
            b"".as_slice(),
        ),
    ] {
        check_components(game, &root, &path)?;
        if fs::read(&path).ok().as_deref() == Some(content) {
            continue;
        }
        files.write(&path, content)?;
    }
    Ok(())
}

fn compatible_profile(actual: &str, expected: &str) -> bool {
    fn entries(text: &str) -> Option<BTreeMap<(String, String), String>> {
        let mut section = String::new();
        let mut values = BTreeMap::new();
        for line in text.trim_start_matches('\u{feff}').lines().map(str::trim) {
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
            {
                section = name.to_owned();
            } else {
                let (key, value) = line.split_once('=')?;
                if values
                    .insert(
                        (section.clone(), key.trim().to_owned()),
                        value.trim().to_owned(),
                    )
                    .is_some()
                {
                    return None;
                }
            }
        }
        Some(values)
    }
    let Some(actual) = entries(actual) else {
        return false;
    };
    let Some(expected) = entries(expected) else {
        return false;
    };
    actual == expected
}

fn check_components(game: Game, root: &Path, path: &Path) -> Result<(), StorageError> {
    let relative = path.strip_prefix(root).map_err(|_| {
        failure(
            game,
            path,
            "The player-query target is outside its installation.",
        )
    })?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(failure(
                            game,
                            &current,
                            "Player-query paths must not use reparse points.",
                        ));
                    }
                }
                if metadata.file_type().is_symlink() {
                    return Err(failure(
                        game,
                        &current,
                        "Player-query paths must not use symbolic links.",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(game, &current, error)),
        }
    }
    Ok(())
}

fn failure(game: Game, path: &Path, message: impl std::fmt::Display) -> StorageError {
    StorageError::ModuleSupportMaterialization {
        module_id: game.module_id().into(),
        path: path.to_path_buf(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_profiles_require_exact_dispatcher_keys_and_disabled_unsafe_hooks() {
        for game in [Game::Windrose, Game::Dragonwilds, Game::Scum] {
            let profile = game.profile();
            assert!(compatible_profile(profile, profile));
            for changed in [
                profile.replace("HookEngineTick = 1", "HookEngineTick = 0"),
                profile.replace("HookLoadMap = 0", "HookLoadMap = 1"),
                format!("{profile}\n[Hooks]\nHookEngineTick=0"),
                "[General]\nDefaultExecuteInGameThreadMethod=EngineTick".into(),
                profile.replace("[Hooks]", "[hooks]"),
                profile.replace("HookLoadMap", "hookloadmap"),
                format!("{profile}\n[Overrides]\nModsFolderPath=../OtherMods"),
                format!("{profile}\n[EngineVersionOverride]\nDebugBuild=1"),
            ] {
                assert!(!compatible_profile(&changed, profile));
            }
        }
    }

    #[test]
    fn stopped_compatible_loaders_receive_only_their_owned_query_files() {
        for game in [Game::Windrose, Game::Dragonwilds, Game::Scum] {
            let root =
                std::env::temp_dir().join(format!("langame-ue4ss-query-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            materialize(
                game,
                &root,
                false,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            assert!(!root.join("langame_player_query").exists());
            let loader = root.join(game.loader_path());
            fs::create_dir_all(&loader).unwrap();
            fs::write(loader.join("UE4SS.dll"), b"fixture").unwrap();
            fs::write(
                loader.join("UE4SS-settings.ini"),
                "[Hooks]\nHookEngineTick=1",
            )
            .unwrap();
            materialize(
                game,
                &root,
                false,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            assert!(!loader.join("Mods").exists());
            fs::write(loader.join("UE4SS-settings.ini"), game.profile()).unwrap();
            materialize(
                game,
                &root,
                true,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            assert!(!loader.join("Mods").exists());
            materialize(
                game,
                &root,
                false,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            let script = loader.join("Mods/LgsmPlayerQuery/Scripts/main.lua");
            assert_eq!(fs::read(&script).unwrap(), game.script());
            assert!(loader.join("Mods/LgsmPlayerQuery/enabled.txt").is_file());
            assert!(root.join("langame_player_query").is_dir());
            assert_eq!(
                fs::read_to_string(loader.join("UE4SS-settings.ini")).unwrap(),
                game.profile()
            );
            assert_eq!(fs::read(loader.join("UE4SS.dll")).unwrap(), b"fixture");
            assert!(!loader.join("Mods/mods.txt").exists());
            fs::write(&script, b"already running").unwrap();
            materialize(
                game,
                &root,
                true,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            assert_eq!(fs::read(&script).unwrap(), b"already running");
            materialize(
                game,
                &root,
                false,
                &mut ManagedConfigMutation::new(game.module_id()),
            )
            .unwrap();
            assert_eq!(fs::read(&script).unwrap(), game.script());
            fs::remove_dir_all(root).unwrap();
        }
        assert_ne!(Game::Windrose.script(), Game::Dragonwilds.script());
    }
}
