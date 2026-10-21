//! Fixed native persistence families and reload observations for disposable tests.
//! The caller owns fixture containment, identity pins and fresh log cursors.
use app_core::InstanceDetails;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(super) fn roots(module: &str, saves: &Path) -> Result<Vec<PathBuf>, String> {
    if !saves.is_absolute()
        || saves
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Checkpoint saves require an ordinary absolute fixture path".into());
    }
    match module {
        // Core's world metadata is in its world payload. Dragon's SaveGames and
        // Enshrouded's savegame roots contain their native primary/recovery family.
        // Return children: the owner must retain the saves directory itself.
        "corekeeper" | "runescapedragonwilds" | "enshrouded" => children(saves),
        // Preserve complete generations: .chunk plus .chunks/.db2/.fwl2/.ok,
        // rather than treating any one leaf as a standalone Valheim checkpoint.
        "valheim" => Ok(vec![saves.join("worlds_local")]),
        // record.json and state must stay together. ServerState selects the same
        // dedicated deploy; logs, crash output and IPC are not persistence input.
        "nightingale" => Ok(vec![
            saves.join("Offline"),
            saves.join("Config/WindowsServer/ServerState.ini"),
        ]),
        // The complete slot includes __RESUME__, guid, info and recovery leaves.
        // A reload-specific native observation is still required below.
        "theforest" => Ok(vec![saves.join("Multiplayer")]),
        // These disposable fixtures use the native default database selection.
        // The SQLite backup API, rather than raw main/WAL copies, owns capture.
        "conanexiles" => Ok(vec![saves.join("game_0.db"), saves.join("serveruid.txt")]),
        "scum" => Ok(vec![saves.join("SaveFiles/SCUM.db")]),
        _ => Err(format!("{module}: no verified native checkpoint profile")),
    }
}

/// Only the verified default SQLite databases use the transactional backup API.
/// A Valheim .db/.db2 is a native binary and must never enter that API.
pub(super) fn is_sqlite(module: &str, relative: &Path) -> bool {
    match module {
        "conanexiles" => relative == Path::new("game_0.db"),
        "scum" => relative == Path::new("SaveFiles/SCUM.db"),
        _ => false,
    }
}

fn children(root: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(root).map_err(|_| "Cannot enumerate native checkpoint family")?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "Cannot inspect native checkpoint family")?;
        let path = entry.path();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| "Cannot inspect native checkpoint member")?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = metadata.file_type().is_symlink();
        if reparse || (!metadata.is_file() && !metadata.is_dir()) {
            return Err("Native checkpoint family contains a redirected or special entry".into());
        }
        paths.push(path);
    }
    paths.sort();
    if paths.is_empty() {
        return Err("Native checkpoint family is empty after the formal save".into());
    }
    Ok(paths)
}

pub(super) fn native_logs(module: &str, program: &Path, details: &InstanceDetails) -> Vec<PathBuf> {
    let mut logs = Vec::new();
    if let Some(run) = &details.active_run {
        for process in &run.processes {
            if process.is_primary
                && process.status == "running"
                && let Some(path) = &process.log_path
            {
                logs.push(PathBuf::from(path));
            }
        }
    }
    let native = match module {
        "corekeeper" | "enshrouded" | "conanexiles" => Path::new(&details.config_file_path)
            .parent()
            .and_then(Path::parent)
            .map(|root| {
                root.join(match module {
                    "corekeeper" => "logs/CoreKeeperServer.log",
                    "enshrouded" => "logs/enshrouded_server.log",
                    _ => "logs/conan-server.log",
                })
            }),
        "valheim" => Some(program.join("BepInEx/LogOutput.log")),
        "nightingale" => Some(program.join("NWX/Saved/Logs/NWX.log")),
        "runescapedragonwilds" => Some(program.join("RSDragonwilds/Saved/Logs/RSDragonwilds.log")),
        "scum" => Some(program.join("SCUM/Saved/Logs/SCUM.log")),
        _ => None,
    };
    if let Some(path) = native {
        logs.push(path);
    }
    logs.sort();
    logs.dedup();
    logs
}

pub(super) fn require_loaded(module: &str, before: &str, fresh: &str) -> Result<(), String> {
    let loaded = match module {
        // The native spelling "finishied" is intentional. A new-world seed
        // without deserialization is insufficient.
        "corekeeper" => {
            fresh.contains("Decompressed brotli compressed world file:")
                && fresh.contains("Deserialize world finishied")
                && fresh.contains("successfully loaded world file into SerializeWorld")
                && !fresh.contains("no existing save file, starting with empty world")
        }
        // SubsequentLaunch distinguishes a restored world from FirstLaunch.
        "nightingale" => {
            fresh.contains("E_WorldPersistenceLoadState::SubsequentLaunch")
                && fresh.contains(
                    "finished restoring world state and building navigation. Going ready.",
                )
                && !fresh.contains("E_WorldPersistenceLoadState::FirstLaunch")
        }
        "runescapedragonwilds" => dragon_loaded(before, fresh)?,
        // Run alone is also reached by a new world; require deserialization.
        "enshrouded" => {
            fresh.contains("Found file 'world0.ksd'")
                && fresh.contains("Savegame deserialization took ")
                && fresh.contains("Switching state from LoadEcsScene to Run")
                && !fresh
                    .to_ascii_lowercase()
                    .contains("failed to load server save game")
        }
        // Bind the reloaded world to the last completed native save number.
        "valheim" => valheim_loaded(before, fresh)?,
        "theforest" => {
            return Err("The Forest has no verified reload-specific native log observation; world_ready does not prove a checkpoint was loaded".into());
        }
        // The caller restores the exact database and server UID. An empty
        // player world can still reload its persisted native mod controllers.
        "conanexiles" => conan_loaded(before, fresh),
        // Readiness/global stats do not prove the restored DB was loaded.
        "scum" => fresh.lines().any(|line| {
            line.contains("LogEntitySystem: DbLoadState took ")
                && positive_count(line, " for ", " entities")
        }),
        _ => return Err(format!("{module}: no verified native reload observation")),
    };
    if !loaded {
        return Err(format!(
            "{module}: fresh native logs do not prove loading the captured checkpoint"
        ));
    }
    Ok(())
}

fn positive_count(line: &str, prefix: &str, suffix: &str) -> bool {
    line.split_once(prefix)
        .and_then(|(_, value)| value.split_once(suffix))
        .and_then(|(value, _)| value.parse::<u64>().ok())
        .is_some_and(|count| count > 0)
}

fn conan_loaded(before: &str, fresh: &str) -> bool {
    if !fresh.contains("Entered application state 'WorldPersistenceDone'") {
        return false;
    }
    if fresh
        .lines()
        .any(|line| positive_count(line, "WorldPersistence: Finished loading ", " actors in "))
    {
        return true;
    }
    let spawning = "Persistence: Spawning mod controller: ";
    let loading = "Persistence: Loading mod controller: ";
    let expected: BTreeSet<_> = before
        .lines()
        .filter_map(|line| {
            line.split_once(spawning)
                .or_else(|| line.split_once(loading))
        })
        .map(|(_, name)| name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    let loaded: BTreeSet<_> = fresh
        .lines()
        .filter_map(|line| line.split_once(loading))
        .map(|(_, name)| name.trim())
        .filter(|name| !name.is_empty())
        .collect();
    // Spawning means regeneration, whereas Loading reads actor_position rows
    // selected by mod_controllers in the restored native SQLite snapshot.
    !expected.is_empty() && expected == loaded && !fresh.contains(spawning)
}

fn valheim_loaded(before: &str, fresh: &str) -> Result<bool, String> {
    let mut worlds = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut pending = None;
    for line in before.lines() {
        let world = line
            .split_once("Load world: ")
            .map(|(_, world)| world)
            .or_else(|| {
                line.split_once("ZNet.LoadWorld: ")
                    .and_then(|(_, world)| world.split_once(", save number "))
                    .map(|(world, _)| world)
            });
        if let Some(world) = world.filter(|world| !world.trim().is_empty()) {
            worlds.insert(world.trim());
        }
        if line.contains("World save (1/5)") {
            pending = line
                .split_once("=> Save number ")
                .and_then(|(_, number)| number.trim().parse::<u32>().ok())
                .filter(|number| *number > 0);
        } else if line.contains("World save (5/5) done.")
            && let Some(number) = pending.take()
        {
            completed.insert(number);
        }
    }
    let Some(number) = completed.last() else {
        return Err("Valheim capture has no completed native save number".into());
    };
    if worlds.len() != 1 {
        return Err("Valheim capture has no unique native world identity".into());
    }
    let mut loaded = BTreeSet::new();
    for line in fresh.lines() {
        if let Some((world, saved)) = line
            .split_once("ZNet.LoadWorld: ")
            .and_then(|(_, value)| value.split_once(", save number "))
            && let Ok(saved) = saved.trim().parse::<u32>()
        {
            loaded.insert((world.trim(), saved));
        }
    }
    // Exact strings read from this build's ZNet.LoadWorld IL. A later success
    // cannot hide a failed first attempt or backup fallback in this restart.
    let failed = [
        "World load failed early.",
        "World load failed mid-file.",
        "Exception while loading world",
        "incompatible data version",
    ]
    .iter()
    .any(|message| fresh.contains(message));
    Ok(!failed
        && loaded.len() == 1
        && loaded.contains(&(
            *worlds.first().ok_or("Valheim world identity is absent")?,
            *number,
        ))
        && fresh.contains("ZoneSystem.Load => DB2: unpacking compressed data")
        && fresh.contains("ZDOMan.LoadChunks => Loading ZDOs done"))
}

fn bracketed<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    line.split_once(prefix)?
        .1
        .split_once(']')
        .map(|(value, _)| value)
}

fn guid(value: &str) -> Option<String> {
    (value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| value.to_ascii_lowercase())
}

fn dragon_loaded(before: &str, fresh: &str) -> Result<bool, String> {
    let mut expected = BTreeSet::new();
    for line in before.lines() {
        let value = if line.contains("LogPersistence:") {
            bracketed(line, "Guid[")
        } else if line.contains("Setting [\"WorldSaveGuid\"]") {
            bracketed(line, "value[")
        } else {
            None
        };
        if let Some(value) = value.and_then(guid) {
            expected.insert(value);
        }
    }
    if expected.len() != 1 {
        return Err("Dragonwilds capture has no unique native world GUID".into());
    }
    let mut loaded = BTreeSet::new();
    for line in fresh
        .lines()
        .filter(|line| line.contains("PostLoadWorldState() : World load SUCCEEDED (slot:"))
    {
        if let Some(value) = bracketed(line, "Guid[").and_then(guid) {
            loaded.insert(value);
        }
    }
    Ok(loaded == expected && fresh.contains("LogSpudSubsystem: Load: Success"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_or_new_world_does_not_prove_reload() {
        assert!(
            require_loaded(
                "corekeeper",
                "",
                "no existing save file, starting with empty world\nUsing seed 123"
            )
            .is_err()
        );
        assert!(require_loaded("nightingale", "", "E_WorldPersistenceLoadState::FirstLaunch\nfinished restoring world state and building navigation. Going ready.").is_err());
        assert!(
            require_loaded("theforest", "", "[LanGame native control] world_ready=True").is_err()
        );
        assert!(
            require_loaded("enshrouded", "", "Switching state from LoadEcsScene to Run").is_err()
        );
        assert!(require_loaded("valheim", "", "Load world: n (n)\nOpened Steam server").is_err());
    }

    #[test]
    fn dragon_reload_binds_the_saved_world_guid() {
        let saved = "LogPersistence: SaveGameInternal() : Starting save (Guid[244CFF6E493547F66C9223BDB46D65A2] WorldName[Ashenfall])";
        let loaded = "LogPersistence: PostLoadWorldState() : World load SUCCEEDED (slot: Ashenfall) (Guid[244CFF6E493547F66C9223BDB46D65A2])\nLogSpudSubsystem: Load: Success";
        require_loaded("runescapedragonwilds", saved, loaded).unwrap();
        assert!(
            require_loaded(
                "runescapedragonwilds",
                saved,
                &loaded.replace(
                    "244CFF6E493547F66C9223BDB46D65A2",
                    "11111111111111111111111111111111"
                )
            )
            .is_err()
        );
        assert!(require_loaded("runescapedragonwilds", "", loaded).is_err());
    }

    #[test]
    fn observed_native_deserialization_chains_are_required() {
        let core = "Decompressed brotli compressed world file: 43864 bytes -> 486312 bytes\nDeserialize world finishied\nsuccessfully loaded world file into SerializeWorld";
        require_loaded("corekeeper", "", core).unwrap();
        assert!(
            require_loaded(
                "corekeeper",
                "",
                &format!("{core}\nno existing save file, starting with empty world")
            )
            .is_err()
        );
        let night = "finished restoring world state and building navigation. Going ready.\nE_WorldPersistenceLoadState::SubsequentLaunch";
        require_loaded("nightingale", "", night).unwrap();
        assert!(
            require_loaded(
                "nightingale",
                "",
                &format!("{night}\nE_WorldPersistenceLoadState::FirstLaunch")
            )
            .is_err()
        );
        let ensh = "Found file 'world0.ksd'\nSavegame deserialization took 930.07 ms\nSwitching state from LoadEcsScene to Run";
        require_loaded("enshrouded", "", ensh).unwrap();
        assert!(
            require_loaded(
                "enshrouded",
                "",
                &format!("{ensh}\nFailed to load server save game")
            )
            .is_err()
        );
    }

    #[test]
    fn valheim_reload_matches_completed_save_number_and_world() {
        let before = "Load world: n (n)\nWorld save (1/5) Cloud & Backup checks done [2ms] => Save number 2\nWorld save (5/5) done. Total time [33ms]";
        let fresh = "ZNet.LoadWorld: n (n), save number 2\nZoneSystem.Load => DB2: unpacking compressed data 144,682 => 206,874 bytes\nZDOMan.LoadChunks => Loading ZDOs done [4ms]";
        require_loaded("valheim", before, fresh).unwrap();
        for invalid in [
            fresh.replace("n (n)", "other (other)"),
            fresh.replace("save number 2", "save number 1"),
            format!("World load failed early. Exiting without save. Check backups!\n{fresh}"),
            format!("Exception while loading world n:\n{fresh}"),
            "Load world: n (n)\nLoading: Generating locations".into(),
        ] {
            assert!(require_loaded("valheim", before, &invalid).is_err());
        }
        assert!(require_loaded("valheim", "Load world: n (n)", fresh).is_err());
        assert!(
            require_loaded(
                "valheim",
                &before.replace("World save (5/5) done.", "incomplete"),
                fresh
            )
            .is_err()
        );
    }

    #[test]
    fn database_reload_requires_nonzero_native_loaded_records() {
        let conan = "WorldPersistence: Finished loading 89 actors in 0.1 seconds.\nEntered application state 'WorldPersistenceDone'";
        require_loaded("conanexiles", "", conan).unwrap();
        assert!(
            require_loaded("conanexiles", "", &conan.replace("89 actors", "0 actors")).is_err()
        );
        assert!(
            require_loaded(
                "conanexiles",
                "",
                "Entered application state 'WorldPersistenceDone'"
            )
            .is_err()
        );
        let scum = "LogEntitySystem: DbLoadState took 18.077ms for 3960 entities";
        require_loaded("scum", "", scum).unwrap();
        assert!(require_loaded("scum", "", &scum.replace("3960 entities", "0 entities")).is_err());
        assert!(require_loaded("scum", "", "GlobalStats").is_err());
    }

    #[test]
    fn conan_empty_player_world_reloads_its_persisted_controllers() {
        let before = "Persistence: Spawning mod controller: BP_ConanSword_DLC_ModController_C\nPersistence: Spawning mod controller: DLC_Turan_Modcontroller_C";
        let loaded = "Persistence: Loading mod controller: BP_ConanSword_DLC_ModController_C\nPersistence: Loading mod controller: DLC_Turan_Modcontroller_C\nWorldPersistence: Finished loading 0 actors in 0.0 seconds.\nEntered application state 'WorldPersistenceDone'";
        require_loaded("conanexiles", before, loaded).unwrap();
        require_loaded("conanexiles", loaded, loaded).unwrap();
        for invalid in [
            loaded.replace("DLC_Turan_Modcontroller_C", "Other_ModController_C"),
            loaded.replace("Persistence: Loading mod controller: DLC_Turan_Modcontroller_C", ""),
            format!("{loaded}\nPersistence: Loading mod controller: Other_ModController_C"),
            loaded.replace("Loading mod controller", "Spawning mod controller"),
            loaded.replace("WorldPersistenceDone", "WorldPersistence"),
            "Loading active mod controllers (#16 expected)\nEntered application state 'WorldPersistenceDone'".into(),
        ] {
            assert!(require_loaded("conanexiles", before, &invalid).is_err());
        }
        assert!(require_loaded("conanexiles", "", loaded).is_err());
        assert!(
            require_loaded(
                "conanexiles",
                "Persistence: Spawning mod controller: ",
                loaded
            )
            .is_err()
        );
    }

    #[test]
    fn conan_native_log_uses_the_instance_abslog_destination() {
        let details =
            crate::commands::tests::manual_runtime_tests::player_snapshot_fixture_instance(
                "conanexiles",
                app_core::InstanceStatus::Running,
            );
        assert_eq!(
            native_logs("conanexiles", Path::new("D:/fixture/runtime"), &details),
            [PathBuf::from("D:/fixture/logs/conan-server.log")]
        );
    }

    #[test]
    fn sqlite_classification_is_an_exact_native_database_allowlist() {
        assert!(is_sqlite("conanexiles", Path::new("game_0.db")));
        assert!(is_sqlite("scum", Path::new("SaveFiles/SCUM.db")));
        assert!(!is_sqlite("valheim", Path::new("worlds_local/n/n.db")));
        assert!(!is_sqlite("conanexiles", Path::new("game_0_backup_1.db")));
        assert!(!is_sqlite("scum", Path::new("SaveFiles/SCUM.db-wal")));
    }
}
