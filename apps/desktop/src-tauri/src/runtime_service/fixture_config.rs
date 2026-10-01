//! Explicit, disposable namespace for the opt-in native service acceptance.
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::MetadataExt;
use std::path::{Component, Path, PathBuf, Prefix};

use serde::{Deserialize, Serialize};

pub(super) const MARKER: &str = ".runtime-service-fixture.json";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    pub root: PathBuf,
    pub nonce: String,
    #[serde(default)]
    pub scenario: Scenario,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scenario {
    #[default]
    Normal,
    TrayExit,
    TrayExitHangSave,
}

impl Scenario {
    pub(super) fn is_tray_exit(self) -> bool {
        self != Self::Normal
    }

    pub(super) fn exit_grace(self) -> Option<std::time::Duration> {
        match self {
            Self::Normal => None,
            Self::TrayExit => Some(std::time::Duration::from_secs(30)),
            Self::TrayExitHangSave => Some(std::time::Duration::from_secs(4)),
        }
    }
}

pub(super) fn checked_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute()
        || !matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)))
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Fixture paths must be absolute local drive paths without traversal".into());
    }
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_attributes() & 0x400 != 0 => {
                return Err(format!(
                    "Fixture path contains a reparse point: {}",
                    ancestor.display()
                ));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}

pub(super) fn same_path(actual: &Path, expected: &Path) -> Result<(), String> {
    checked_path(actual)?;
    checked_path(expected)?;
    if dunce::canonicalize(actual).map_err(|e| e.to_string())?
        != dunce::canonicalize(expected).map_err(|e| e.to_string())?
    {
        return Err("Synthetic fixture path escaped its owned namespace".into());
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        checked_path(path)?;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 {
            return Err("Fixture config exceeds 16 KiB".into());
        }
        let config: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        checked_path(&config.root)?;
        if config.root.parent() != path.parent() {
            return Err("Fixture root must be a new sibling of its config".into());
        }
        super::super::security::Endpoint::isolated(&config.nonce).map_err(|e| e.to_string())?;
        Ok(config)
    }

    pub fn create(&self) -> Result<(), String> {
        fs::create_dir(&self.root).map_err(|e| format!("Create new fixture root: {e}"))?;
        checked_path(&self.root)?;
        write_new(&self.root.join(MARKER), self)?;
        for path in [
            "logs",
            "data",
            "localappdata",
            "runtime/games/necesse/jre/bin",
            "runtime/instances",
            "runtime/steamcmd",
        ] {
            fs::create_dir_all(self.root.join(path)).map_err(|e| e.to_string())?;
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        fs::copy(
            exe,
            self.root.join("runtime/games/necesse/jre/bin/java.exe"),
        )
        .map_err(|e| e.to_string())?;
        fs::write(
            self.root.join("runtime/games/necesse/Server.jar"),
            b"SYNTHETIC FIXTURE - NOT A GAME PACKAGE",
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn verify(&self) -> Result<(), String> {
        checked_path(&self.root)?;
        let marker: Self = read_json(&self.root.join(MARKER))?;
        if marker.root != self.root
            || marker.nonce != self.nonce
            || marker.scenario != self.scenario
        {
            return Err("Fixture marker mismatch".into());
        }
        Ok(())
    }

    /// Must run at the first entry in main, before any Tauri/reader threads.
    pub fn initialize_environment(&self) -> Result<(), String> {
        self.verify()?;
        unsafe {
            std::env::set_var("LOCALAPPDATA", self.root.join("localappdata"));
            std::env::set_var("LANGAME_RUNTIME_SERVICE_FIXTURE_ROOT", &self.root);
            std::env::set_var("LANGAME_RUNTIME_SERVICE_FIXTURE_NONCE", &self.nonce);
            std::env::remove_var("LANGAME_LAN_HOST");
        }
        let settings = app_core::AppSettings {
            archives_root: String::new(),
            servers_root: self
                .root
                .join("runtime/instances")
                .to_string_lossy()
                .into_owned(),
            games_root: self
                .root
                .join("runtime/games")
                .to_string_lossy()
                .into_owned(),
            modules_root: app_storage::StoragePaths::default()
                .modules_root
                .to_string_lossy()
                .into_owned(),
            steamcmd_root: self
                .root
                .join("runtime/steamcmd")
                .to_string_lossy()
                .into_owned(),
        };
        app_storage::save_app_settings(settings).map_err(|e| e.to_string())?;
        let paths = app_storage::bootstrap_storage()
            .map_err(|e| e.to_string())?
            .paths;
        for path in [
            &paths.app_data_root,
            &paths.instances_root,
            &paths.games_root,
            &paths.steamcmd_root,
        ] {
            if !path.starts_with(&self.root) {
                return Err("Fixture storage escaped its exclusive root".into());
            }
        }
        Ok(())
    }
}

pub(super) fn write_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
    checked_path(path)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("Create {}: {e}", path.display()))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| e.to_string())
}

pub(super) fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    checked_path(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Fixture evidence exceeds 1 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
