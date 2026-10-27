use super::*;

pub(super) const VTABLE_LAYOUT: &str = "ue4ss/VTableLayout.ini";

#[derive(Clone, Copy)]
pub(super) struct Profile {
    pub id: &'static str,
    pub project: &'static str,
    pub executable: &'static str,
    pub game_hash: &'static str,
    pub proxy: &'static str,
}

impl Profile {
    // Both loader preparation and save installation must admit the same image
    // and, for Conan, the provider shipped with that exact executable.
    pub fn verify_game_image(self, actual: &str) -> Result<Option<&'static str>, String> {
        if actual == self.game_hash {
            return Ok((self.id == "conanexiles")
                .then_some("d076d5d223360f133e365ef37901ff9789eb68186a8f6d66494038f4864684b2"));
        }
        match (self.id, actual) {
            ("abioticfactor", "2563f8d798c8277400acd28eefde604c0ac7d77601627296211af48b30dba25f") => Ok(None),
            ("conanexiles", "d0ea317e7e037e68454414f12828d5995cc9e519b9c34eda264be1954d352b14") => {
                Ok(Some("2eec9aa9aa443fb476db583cfdc5044285ccc246df0a60e93155fb37e14324b6"))
            }
            ("scum", "070fcad918ba04eb9797db29674cadbd6691427b936fe354bca23b008c8c9971") => Ok(None),
            _ => Err("This game build is outside the verified native save contract; no files were changed.".into()),
        }
    }
    pub fn get(id: &str) -> Result<Self, String> {
        let (id, project, executable, game_hash, proxy) = match id {
            "abioticfactor" => (
                "abioticfactor",
                "AbioticFactor",
                "AbioticFactorServer-Win64-Shipping.exe",
                "3ee1c8d170138fb3842dbde128813dabe9ec3cbabf5865b00d2fe45670b6be91",
                "version.dll",
            ),
            "returntomoria" => (
                "returntomoria",
                "Moria",
                "MoriaServer-Win64-Shipping.exe",
                "4513fb58ca31a3c7c7640ebeff9717ef68776b922986036f0c5b325dd28255fb",
                "dwmapi.dll",
            ),
            "nightingale" => (
                "nightingale",
                "NWX",
                "NWXServer-Win64-Shipping.exe",
                "8e06c60524066655aee66da2e06b52c702a951afc3c28b59a2923f9dd51bd7eb",
                "dwmapi.dll",
            ),
            "conanexiles" => (
                "conanexiles",
                "ConanSandbox",
                "ConanSandboxServer-Win64-Shipping.exe",
                "2c3fef162b159bd16124b7ea8030884aa1f66e7b1d966e63542407e590d04fa1",
                "dsound.dll",
            ),
            "scum" => (
                "scum",
                "SCUM",
                "SCUMServer.exe",
                "4ca11449c4c8d3a243c797c719f66e33192eefad53a9c64cca9aac925c9e39ad",
                "dwmapi.dll",
            ),
            "windrose" => (
                "windrose",
                "R5",
                "WindroseServer-Win64-Shipping.exe",
                "441acd04428aa5bd6535bc6565ed659a343001615d5322e8c65141b08b356854",
                "dwmapi.dll",
            ),
            _ => return Err("This game has no reviewed automatic UE4SS runtime.".into()),
        };
        Ok(Self {
            id,
            project,
            executable,
            game_hash,
            proxy,
        })
    }
    pub fn win64(self, root: &Path) -> std::path::PathBuf {
        root.join(self.project).join("Binaries/Win64")
    }
    pub fn settings(self) -> &'static str {
        match self.id {
            "conanexiles" => {
                include_str!("../../../../../modules/conanexiles/control/UE4SS-settings.ini")
            }
            "scum" => include_str!("../../../../../modules/scum/extensions/UE4SS-settings.ini"),
            "windrose" => {
                include_str!("../../../../../modules/windrose/extensions/UE4SS-settings.ini")
            }
            _ => include_str!("../../../../../modules/abioticfactor/control/UE4SS-settings.ini"),
        }
    }
    pub fn vtable_layout(self) -> Option<&'static [u8]> {
        (self.id == "conanexiles").then_some(include_bytes!(
            "../../../../../modules/conanexiles/control/VTableLayout.ini"
        ))
    }
    pub fn proxy_bytes(self) -> Option<&'static [u8]> {
        #[cfg(windows)]
        match self.proxy {
            "version.dll" => Some(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/unreal-loader-proxies/version.dll"
            ))),
            "dsound.dll" => Some(include_bytes!(concat!(
                env!("OUT_DIR"),
                "/unreal-loader-proxies/dsound.dll"
            ))),
            _ => None,
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

pub(super) fn licenses() -> Vec<(&'static str, &'static [u8])> {
    #[cfg(windows)]
    {
        vec![
            (
                "ue4ss/LICENSE-LanGameProxy-UE4SS.txt",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/unreal-loader-proxies/LICENSE-UE4SS.txt"
                )),
            ),
            (
                "ue4ss/LICENSE-LanGameProxy-fmt.txt",
                include_bytes!(concat!(
                    env!("OUT_DIR"),
                    "/unreal-loader-proxies/LICENSE-fmt.txt"
                )),
            ),
        ]
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

fn engine_override_compatible(
    current: &BTreeMap<(String, String), String>,
    required: &BTreeMap<(String, String), String>,
) -> bool {
    let version = ["MajorVersion", "MinorVersion"].into_iter().all(|key| {
        let key = ("EngineVersionOverride".into(), key.into());
        current.get(&key).is_none_or(|value| {
            value.is_empty() || value == "-1" || required.get(&key) == Some(value)
        })
    });
    version
        && ["DebugBuild", "Stats"].into_iter().all(|key| {
            let key = ("EngineVersionOverride".into(), key.into());
            required.contains_key(&key) || current.get(&key).is_none_or(String::is_empty)
        })
}

fn mod_paths_compatible(current: &BTreeMap<(String, String), String>) -> bool {
    // Fixed upstream SettingsManager applies these paths before mod discovery.
    // Additional directories can shadow a mod with the same name, and removals
    // can remove the owned Mods root. Retain such operator configuration intact.
    ["ModsFolderPath", "+ModsFolderPaths", "-ModsFolderPaths"]
        .into_iter()
        .all(|key| {
            current
                .get(&("Overrides".into(), key.into()))
                .is_none_or(String::is_empty)
        })
}

/// Preserve operator additions while retaining the tested native profile's
/// engine-version contract, including automatic detection where supported.
pub(crate) fn compatible_settings(current: &str, required: &str) -> bool {
    use crate::live_players::file_ipc::unreal_world_save::installation::profile_entries;
    let Some(current) = profile_entries(current) else {
        return false;
    };
    let Some(required) = profile_entries(required) else {
        return false;
    };
    engine_override_compatible(&current, &required)
        && mod_paths_compatible(&current)
        && required
            .iter()
            .all(|(key, value)| current.get(key) == Some(value))
}

/// Only required key/value spans change. Unknown settings, comments, line
/// endings and mod lists remain intact. Ambiguous documents are not rewritten.
pub(super) fn merge_settings(current: &[u8], required: &str) -> Result<Vec<u8>, String> {
    use crate::live_players::file_ipc::unreal_world_save::installation::profile_entries;
    let text = std::str::from_utf8(current)
        .map_err(|_| "The UE4SS settings are not UTF-8; they were preserved.")?;
    let existing =
        profile_entries(text).ok_or("The UE4SS settings are ambiguous; they were preserved.")?;
    let wanted = profile_entries(required).ok_or("The bundled UE4SS profile is invalid.")?;
    if !mod_paths_compatible(&existing) {
        return Err("The existing UE4SS mod-directory override conflicts with the owned save extension path; the settings were preserved.".into());
    }
    if !engine_override_compatible(&existing, &wanted) {
        return Err("The existing UE4SS engine-version override conflicts with the verified native profile; the settings were preserved.".into());
    }
    let mut missing = wanted.clone();
    let mut section = String::new();
    let mut output = String::new();
    for line in text.split_inclusive('\n') {
        let clean = line.trim().trim_start_matches('\u{feff}');
        if let Some(name) = clean
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            section = name.into();
        }
        if !clean.starts_with([';', '#'])
            && let Some((key, value)) = line.split_once('=')
        {
            let entry = (section.clone(), key.trim().to_owned());
            if let Some(expected) = wanted.get(&entry) {
                missing.remove(&entry);
                if value.trim() != expected {
                    let suffix = if line.ends_with("\r\n") {
                        "\r\n"
                    } else if line.ends_with('\n') {
                        "\n"
                    } else {
                        ""
                    };
                    output.push_str(key);
                    output.push_str("= ");
                    output.push_str(expected);
                    output.push_str(suffix);
                    continue;
                }
            }
        }
        output.push_str(line);
    }
    // Missing keys in existing sections must be inserted within that section,
    // rather than creating duplicate INI section headers.
    for ((section, key), value) in missing {
        let header = format!("[{section}]");
        let lines = output.split_inclusive('\n').collect::<Vec<_>>();
        let header_index = lines
            .iter()
            .position(|line| line.trim().trim_start_matches('\u{feff}') == header);
        if let Some(index) = header_index {
            let end = lines
                .iter()
                .enumerate()
                .skip(index + 1)
                .find(|(_, line)| line.trim_start().starts_with('['))
                .map_or(lines.len(), |(index, _)| index);
            let offset = lines[..end].iter().map(|line| line.len()).sum::<usize>();
            let newline = if output.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let prefix = if offset > 0 && !output[..offset].ends_with('\n') {
                newline
            } else {
                ""
            };
            output.insert_str(offset, &format!("{prefix}{key} = {value}{newline}"));
        } else {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&format!("[{section}]\n{key} = {value}\n"));
        }
    }
    let merged = profile_entries(&output).ok_or("The merged UE4SS settings are ambiguous.")?;
    if !wanted
        .iter()
        .all(|(key, value)| merged.get(key) == Some(value))
        || !existing
            .iter()
            .filter(|(key, _)| !wanted.contains_key(*key))
            .all(|(key, value)| merged.get(key) == Some(value))
    {
        return Err("The UE4SS settings could not be merged without losing custom fields.".into());
    }
    Ok(output.into_bytes())
}
