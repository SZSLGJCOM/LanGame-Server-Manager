use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

pub const TML_RELEASE: &str = "v2026.09.3.1";
/// Terraria.Main.curRelease in this stable build (Terraria 1.4.4.9).
/// https://docs.tmodloader.net/docs/stable/class_main.html
pub const TML_MAX_WORLD_VERSION: i32 = 279;

/// This is the exact, case-sensitive list published to Mods/enabled.json.
/// Workshop IDs record downloads; they do not decide which program starts.
pub fn enabled_mod_names(settings: &Map<String, Value>) -> impl Iterator<Item = &str> {
    let mut seen = HashSet::new();
    settings
        .get("tmodloader_enabled_mod_names")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("--"))
        .filter(move |name| seen.insert(*name))
}

pub fn uses_tmodloader(settings: &Value) -> bool {
    settings
        .as_object()
        .is_some_and(|values| enabled_mod_names(values).next().is_some())
        || settings["server_runtime"]
            .as_str()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("tmodloader"))
}

pub enum TmodLoaderInstallation {
    Managed(PathBuf),
    Custom(PathBuf),
}

impl AsRef<Path> for TmodLoaderInstallation {
    fn as_ref(&self) -> &Path {
        match self {
            Self::Managed(path) | Self::Custom(path) => path,
        }
    }
}

pub fn managed_runtime_dir(instance_root: &Path) -> PathBuf {
    instance_root
        .join("tmodloader")
        .join("runtimes")
        .join(TML_RELEASE)
}

pub fn installation(instance_root: &Path, settings: &Value) -> TmodLoaderInstallation {
    let root = instance_root.join("tmodloader");
    if let Some(path) = settings["tmodloader_runtime_dir"]
        .as_str()
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        // Path::join preserves the old meaning of both relative and absolute overrides.
        return TmodLoaderInstallation::Custom(root.join(path));
    }
    // Before automatic installation, operators could place a distribution directly
    // here without setting an override. Keep complete installations and leave
    // incomplete directories untouched when preparing the managed sibling.
    let previous = root.join("runtime");
    let modern = previous.join("LaunchUtils").is_dir() || previous.join("tModLoader.dll").is_file();
    let complete = if modern {
        [
            "LaunchUtils/busybox64.exe",
            "LaunchUtils/ScriptCaller.sh",
            "tModLoader.dll",
            "tModLoader.runtimeconfig.json",
        ]
        .iter()
        .all(|file| previous.join(file).is_file())
    } else {
        ["tModLoaderServer.exe", "start-tModLoaderServer.bat"]
            .iter()
            .any(|file| previous.join(file).is_file())
    };
    if complete {
        TmodLoaderInstallation::Custom(previous)
    } else {
        TmodLoaderInstallation::Managed(managed_runtime_dir(instance_root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn actual_enabled_names_override_vanilla_but_downloads_and_comments_do_not() {
        let mut settings = json!({"server_runtime": "vanilla", "tmodloader_workshop_item_ids": "3831007901",
            "tmodloader_enabled_mod_names": "  # cached\r\n -- disabled\n"});
        assert!(!uses_tmodloader(&settings));
        settings["tmodloader_enabled_mod_names"] =
            json!("BarnacleMod\r\n# kept\nBarnacleMod\n第二个");
        assert!(uses_tmodloader(&settings));
        assert_eq!(
            enabled_mod_names(settings.as_object().unwrap()).collect::<Vec<_>>(),
            ["BarnacleMod", "第二个"]
        );
        assert!(uses_tmodloader(&json!({"server_runtime": "tmodloader"})));
    }
}
