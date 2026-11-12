//! Reviewed Nexus payload locations; archive filenames never establish a runtime root.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Profile {
    AbioticFactor,
    HumanitZ,
    Enshrouded,
}

impl Profile {
    pub(super) fn for_module(module: &str) -> Option<Self> {
        match module {
            "abioticfactor" => Some(Self::AbioticFactor),
            "humanitz" => Some(Self::HumanitZ),
            "enshrouded" => Some(Self::Enshrouded),
            _ => None,
        }
    }

    pub(super) fn module(self) -> &'static str {
        match self {
            Self::AbioticFactor => "abioticfactor",
            Self::HumanitZ => "humanitz",
            Self::Enshrouded => "enshrouded",
        }
    }

    fn project(self) -> &'static str {
        match self {
            Self::AbioticFactor => "AbioticFactor",
            Self::HumanitZ => "HumanitZServer",
            Self::Enshrouded => "Enshrouded",
        }
    }

    pub(super) fn target(self) -> &'static str {
        match self {
            Self::AbioticFactor => "AbioticFactor/Binaries/Win64/ue4ss/Mods",
            Self::HumanitZ => "HumanitZServer/Content/Paks/~mods",
            Self::Enshrouded => "mods",
        }
    }

    fn paks(self) -> String {
        format!("{}/Content/Paks/", self.project())
    }

    fn lua_mods(self) -> String {
        format!("{}/Binaries/Win64/ue4ss/Mods/", self.project())
    }

    pub(super) fn runtime_root(self, target: &Path) -> Result<&Path, String> {
        let root = target
            .ancestors()
            .nth(Path::new(self.target()).components().count())
            .ok_or("Nexus target has no verified runtime root")?;
        if target != root.join(self.target()) {
            return Err("Nexus routes require this game's declared Mod target".into());
        }
        validate_manual_mod_target_path(root)?;
        Ok(root)
    }

    pub(super) fn allowed(self, path: &str) -> bool {
        if path.split('/').any(|part| profile_for_key(part).is_some()) {
            return false;
        }
        if self == Self::Enshrouded {
            return matches!(
                path,
                "powrprof.dll"
                    | "xpshare.ini"
                    | "dinput8.dll"
                    | "iphlpapi.dll"
                    | "livemap.ini"
                    | "AutolootCoop.ini"
                    | "plugins/AutolootCoop.dll"
            ) || emf::allowed_runtime(path)
                || path
                    .strip_prefix("BuildingTools-Languages/")
                    .is_some_and(building_tools_language)
                || path.strip_prefix("livemap/").is_some_and(livemap_asset)
                || path
                    .strip_prefix("shroudlink_mods/")
                    .is_some_and(shroudlink_plugin)
                || path.strip_prefix("mods/").is_some_and(|relative| {
                    !relative.is_empty()
                        && !relative.split('/').any(|part| part.starts_with('.'))
                        && !matches!(
                            Path::new(relative)
                                .extension()
                                .and_then(|value| value.to_str())
                                .unwrap_or_default()
                                .to_ascii_lowercase()
                                .as_str(),
                            "exe" | "bat" | "cmd" | "ps1" | "vbs" | "msi" | "com"
                        )
                });
        }
        if let Some(relative) = path.strip_prefix(&self.paks()) {
            return !relative.is_empty() && pak_extension(relative);
        }
        if let Some(relative) = path.strip_prefix(&self.lua_mods()) {
            let Some((name, inner)) = relative.split_once('/') else {
                return false;
            };
            return safe_mod_name(name) && safe_lua_file(inner);
        }
        false
    }

    pub(super) fn container(self, path: &str) -> Option<String> {
        if self == Self::Enshrouded {
            return self.allowed(path).then(|| {
                if path.starts_with("mods/") {
                    "mods/".into()
                } else if path.starts_with("shroudlink_mods/") {
                    "shroudlink_mods/".into()
                } else if path.starts_with("emf/") {
                    "emf/".into()
                } else if path.starts_with("livemap/") {
                    "livemap/".into()
                } else if path.starts_with("BuildingTools-Languages/") {
                    "BuildingTools-Languages/".into()
                } else if path.starts_with("plugins/") {
                    "plugins/".into()
                } else {
                    String::new()
                }
            });
        }
        [self.paks(), self.lua_mods()]
            .into_iter()
            .find(|prefix| path.starts_with(prefix))
    }

    pub(super) fn lua_marker(self, path: &str) -> Option<String> {
        if self == Self::Enshrouded {
            return None;
        }
        let relative = path.strip_prefix(&self.lua_mods())?;
        let (name, inner) = relative.split_once('/')?;
        inner
            .eq_ignore_ascii_case("Scripts/main.lua")
            .then(|| format!("{}{name}/enabled.txt", self.lua_mods()))
    }

    pub(super) fn is_lua_marker(self, path: &str) -> bool {
        if self == Self::Enshrouded {
            return false;
        }
        path.strip_prefix(&self.lua_mods())
            .and_then(|relative| relative.split_once('/'))
            .is_some_and(|(name, inner)| {
                safe_mod_name(name) && inner.eq_ignore_ascii_case("enabled.txt")
            })
    }

    pub(super) fn is_lua_dot_data(self, path: &str) -> bool {
        self != Self::Enshrouded
            && path
                .strip_prefix(&self.lua_mods())
                .and_then(|relative| relative.split_once('/'))
                .is_some_and(|(name, inner)| {
                    safe_mod_name(name)
                        && safe_lua_file(inner)
                        && Path::new(inner)
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                        && !Path::new(inner).extension().is_some_and(|extension| {
                            extension.eq_ignore_ascii_case("lua")
                                || extension.eq_ignore_ascii_case("luac")
                        })
                })
    }

    pub(super) fn mod_list(self) -> String {
        format!("{}mods.txt", self.lua_mods())
    }

    pub(super) fn is_eml_metadata(self, path: &str) -> bool {
        self == Self::Enshrouded
            && path
                .strip_prefix("mods/")
                .and_then(|relative| relative.split_once('/'))
                .is_some_and(|(name, inner)| {
                    safe_mod_name(name) && inner.eq_ignore_ascii_case("mod.json")
                })
    }
}

pub(super) fn profile_for_key(key: &str) -> Option<Profile> {
    let rest = key.strip_prefix("nexus-")?;
    let (module, project) = rest.rsplit_once('-')?;
    if !valid_project_id(project) {
        return None;
    }
    Profile::for_module(module)
}

pub(super) fn valid_project_id(value: &str) -> bool {
    value
        .parse::<u64>()
        .is_ok_and(|id| id > 0 && id.to_string() == value)
}

pub(super) fn safe_mod_name(name: &str) -> bool {
    !name.eq_ignore_ascii_case("LgsmWorldSave")
        && profile_for_key(name).is_none()
        && !management::protected_root(name)
        && management::validate_component(name).is_ok()
}

fn safe_lua_file(relative: &str) -> bool {
    // This path is inside a named UE4SS Mod, never the loader/framework directory.
    // Public RandomDay ships .randomday_mode as mutable Mod data, not a directory.
    !relative
        .rsplit_once('/')
        .is_some_and(|(parent, _)| parent.split('/').any(|part| part.starts_with('.')))
        && !matches!(
            Path::new(relative)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "exe" | "dll" | "bat" | "cmd" | "ps1" | "vbs" | "msi" | "com"
        )
}

fn shroudlink_plugin(relative: &str) -> bool {
    !relative.contains(['/', '\\'])
        && management::validate_component(relative).is_ok()
        && Path::new(relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
}

fn building_tools_language(relative: &str) -> bool {
    // The standalone author's package keeps its text translations beside dinput8.dll.
    // https://www.nexusmods.com/enshrouded/mods/221?tab=description
    !relative.starts_with('.')
        && !relative.contains(['/', '\\'])
        && management::validate_component(relative).is_ok()
        && Path::new(relative)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
}

fn livemap_asset(relative: &str) -> bool {
    !relative.is_empty()
        && !relative.split('/').any(|part| part.starts_with('.'))
        && !matches!(
            Path::new(relative)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "dll" | "exe" | "bat" | "cmd" | "ps1" | "vbs" | "msi" | "com" | "py"
        )
}

pub(super) fn nexus_metadata(profile: Profile, path: &str) -> bool {
    if profile == Profile::Enshrouded
        && [
            "AutolootCoop-LICENSE.txt",
            "AutolootCoop-LootList.txt",
            "AutolootCoop-README.txt",
        ]
        .iter()
        .any(|name| path.eq_ignore_ascii_case(name))
    {
        return true;
    }
    if routed::is_metadata(path) {
        return true;
    }
    if path.contains(['/', '\\']) {
        return false;
    }
    let name = path.to_ascii_lowercase();
    let Some(stem) = name
        .strip_suffix(".txt")
        .or_else(|| name.strip_suffix(".md"))
    else {
        return false;
    };
    // Public HumanitZ 180 ships a Portuguese README and the font's OFL notice.
    // Preserve these as project metadata instead of rejecting the Pak payload.
    matches!(stem, "leia-me" | "third_party_notices")
        || ["readme-", "readme_", "ofl_"]
            .iter()
            .any(|prefix| stem.starts_with(prefix))
        || ["_readme", "-readme", "_license", "-license"]
            .iter()
            .any(|suffix| stem.ends_with(suffix))
}

fn pak_extension(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "pak" | "utoc" | "ucas" | "sig"
            )
        })
}

fn strip_ascii_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.get(..prefix.len())
        .filter(|start| start.eq_ignore_ascii_case(prefix))
        .map(|_| &path[prefix.len()..])
}

fn lua_relative(profile: Profile, path: &str) -> Option<&str> {
    // HumanitZ 61 ships named UE4SS Mods beneath the game's client folder.
    // Rebase this named Mods container without adopting the client's loader.
    // https://www.nexusmods.com/humanitz/mods/61?tab=description
    if profile == Profile::HumanitZ
        && let Some(relative) = strip_ascii_prefix(path, "HumanitZ/Binaries/Win64/ue4ss/Mods/")
    {
        return Some(relative);
    }
    [
        profile.lua_mods(),
        "Binaries/Win64/ue4ss/Mods/".into(),
        "ue4ss/Mods/".into(),
        "Mods/".into(),
    ]
    .iter()
    .find_map(|prefix| strip_ascii_prefix(path, prefix))
}

fn pak_relative(profile: Profile, path: &str) -> Option<&str> {
    // HumanitZ 46 packages its Pak under the game's client Content/Paks tree.
    // Rebase only this explicit container, retaining its author-defined subfolders.
    // https://www.nexusmods.com/humanitz/mods/46?tab=description
    if profile == Profile::HumanitZ
        && let Some(relative) = strip_ascii_prefix(path, "HumanitZ/Content/Paks/")
    {
        return Some(relative);
    }
    [profile.paks(), "Content/Paks/".into()]
        .iter()
        .find_map(|prefix| strip_ascii_prefix(path, prefix))
}

fn raw_route(profile: Profile, path: &str, named_roots: &HashSet<String>) -> Option<String> {
    if profile == Profile::Enshrouded {
        // XHL's published dedicated-server layout puts dinput8.dll beside the
        // server executable, alone or with its explicit mods/ payload.
        // https://www.nexusmods.com/enshrouded/mods/236?tab=description
        for name in [
            "powrprof.dll",
            "xpshare.ini",
            "dinput8.dll",
            "iphlpapi.dll",
            "livemap.ini",
            "AutolootCoop.ini",
            "plugins/AutolootCoop.dll",
        ] {
            if path.eq_ignore_ascii_case(name) {
                return Some(name.into());
            }
        }
        if let Some(relative) = strip_ascii_prefix(path, "livemap/") {
            return Some(format!("livemap/{relative}"));
        }
        if let Some(relative) = strip_ascii_prefix(path, "BuildingTools-Languages/") {
            return Some(format!("BuildingTools-Languages/{relative}"));
        }
        if let Some(relative) = strip_ascii_prefix(path, "shroudlink_mods/") {
            return shroudlink_plugin(relative).then(|| format!("shroudlink_mods/{relative}"));
        }
        if let Some(relative) = strip_ascii_prefix(path, "mods/") {
            return Some(format!("mods/{relative}"));
        }
        let (name, _) = path.split_once('/')?;
        return named_roots
            .contains(&name.to_ascii_lowercase())
            .then(|| format!("mods/{path}"));
    }
    let pak = pak_relative(profile, path);
    if let Some(relative) = pak.filter(|relative| pak_extension(relative)) {
        return Some(format!("{}{relative}", profile.paks()));
    }
    if pak_extension(path) {
        if path.starts_with("~mods/") || path.starts_with("LogicMods/") {
            return Some(format!("{}{path}", profile.paks()));
        }
        if !path.contains('/') {
            let prefix = if profile == Profile::HumanitZ {
                "~mods/"
            } else {
                ""
            };
            return Some(format!("{}{prefix}{path}", profile.paks()));
        }
    }
    let relative = lua_relative(profile, path).unwrap_or(path);
    let (name, inner) = relative.split_once('/')?;
    if named_roots.contains(&name.to_ascii_lowercase()) {
        // Release/Scripts plus Release/Content/Paks is a shared package wrapper.
        // Do not bury explicit Pak containers inside the named Lua root: leave
        // this round incomplete so the single-wrapper pass can separate them.
        if pak_extension(inner)
            && (pak_relative(profile, inner).is_some()
                || ["~mods/", "LogicMods/"]
                    .iter()
                    .any(|prefix| strip_ascii_prefix(inner, prefix).is_some()))
        {
            return None;
        }
        return Some(format!("{}{relative}", profile.lua_mods()));
    }
    None
}

fn runtime_container(name: &str) -> bool {
    [
        "AbioticFactor",
        "HumanitZ",
        "HumanitZServer",
        "Content",
        "Binaries",
        "ue4ss",
        "Mods",
        "~mods",
        "LogicMods",
        "shroudlink_mods",
    ]
    .iter()
    .any(|container| name.eq_ignore_ascii_case(container))
}

#[derive(Debug)]
pub(super) struct Route {
    pub(super) source: String,
    pub(super) destination: Option<String>,
    pub(super) metadata_path: Option<String>,
}

/// Exact containers or Scripts/main.lua establish layout. Bare Lua packages use
/// a stable project folder; a single archive wrapper may be removed first.
pub(super) fn plan(
    profile: Profile,
    payload: &BTreeMap<String, String>,
    project_id: &str,
) -> Result<Vec<Route>, String> {
    if profile == Profile::Enshrouded
        && let Some(routes) = emf::plan(payload)?
    {
        return Ok(routes);
    }
    let mut logical = payload
        .keys()
        .map(|path| (path.clone(), path.clone()))
        .collect::<Vec<_>>();
    let mut unsupported = Vec::new();
    for _ in 0..MAX_PACKAGE_DEPTH {
        unsupported.clear();
        // UE4SS loads each Mod's actual scripts directory. AF39's published
        // package starts at Scripts/main.lua and uses local require("config").
        // https://github.com/PieKing1215/AF_ResourceMultiplier
        let bare_lua = profile != Profile::Enshrouded
            && logical
                .iter()
                .any(|(_, path)| path.eq_ignore_ascii_case("Scripts/main.lua"));
        let named_roots = logical
            .iter()
            .filter_map(|(_, path)| {
                let path = lua_relative(profile, path).unwrap_or(path);
                let (name, inner) = path.split_once('/')?;
                let entry = if profile == Profile::Enshrouded {
                    inner.eq_ignore_ascii_case("mod.json")
                        && logical.iter().any(|(_, candidate)| {
                            candidate.eq_ignore_ascii_case(&format!("{name}/src/mod.lua"))
                        })
                } else {
                    inner.eq_ignore_ascii_case("Scripts/main.lua")
                };
                (safe_mod_name(name) && entry).then(|| name.to_ascii_lowercase())
            })
            .collect::<HashSet<_>>();
        let mut routes = Vec::new();
        let mut registry = ManualModStagingRegistry::default();
        let mut complete = true;
        let mut optional_selftest = false;
        for (source, path) in &logical {
            if profile == Profile::Enshrouded && path.eq_ignore_ascii_case("dbghelp.dll") {
                return Err("The package's root dbghelp.dll conflicts with LanGame native control; existing files were preserved".into());
            }
            if profile == Profile::Enshrouded && path.eq_ignore_ascii_case("xpshare_selftest.exe") {
                optional_selftest = true;
                continue;
            }
            if nexus_metadata(profile, path) {
                routes.push(Route {
                    source: source.clone(),
                    destination: None,
                    metadata_path: None,
                });
                continue;
            }
            let destination = raw_route(profile, path, &named_roots).or_else(|| {
                let first = path.split('/').next()?;
                (bare_lua
                    && !runtime_container(first)
                    && !first.eq_ignore_ascii_case("LgsmWorldSave")
                    && profile_for_key(first).is_none()
                    && !pak_extension(path)
                    && safe_lua_file(path))
                .then(|| format!("{}Nexus_{project_id}/{path}", profile.lua_mods()))
            });
            let Some(destination) = destination else {
                if unsupported.len() < 4 {
                    unsupported.push(path.chars().take(160).collect::<String>());
                }
                complete = false;
                continue;
            };
            if !profile.allowed(&destination) {
                return Err(format!(
                    "Nexus package targets a protected or unsupported runtime file: {path}"
                ));
            }
            let normalized = normalize_manual_mod_relative_path(Path::new(&destination))?;
            registry.register_file(&normalized)?;
            routes.push(Route {
                source: source.clone(),
                destination: Some(destination),
                metadata_path: None,
            });
        }
        if complete {
            let destinations = routes
                .iter()
                .filter_map(|route| route.destination.as_deref())
                .collect::<Vec<_>>();
            if destinations.is_empty() {
                return Err("Nexus package contains no recognized server Mod payload".into());
            }
            if profile == Profile::Enshrouded {
                if destinations
                    .iter()
                    .any(|path| path.starts_with("BuildingTools-Languages/"))
                    && !destinations.contains(&"dinput8.dll")
                {
                    return Err("Building Tools language resources require their dinput8.dll loader in the same package; existing files were preserved".into());
                }
                // Live Map's published server package pairs this proxy with its
                // complete static site. Deploy only; never start a listener here.
                // https://github.com/oggii/enshrouded-livemap
                let map_proxy = destinations.contains(&"iphlpapi.dll");
                let map_files = destinations.iter().any(|path| path.starts_with("livemap/"));
                let map_config = destinations.contains(&"livemap.ini");
                if (map_proxy || map_files || map_config)
                    && (!map_proxy
                        || !["livemap/index.html", "livemap/maps.json"]
                            .iter()
                            .all(|entry| {
                                destinations
                                    .iter()
                                    .any(|path| path.eq_ignore_ascii_case(entry))
                            }))
                {
                    return Err("Live Map requires iphlpapi.dll with its livemap/index.html and livemap/maps.json assets; existing files were preserved".into());
                }
                // Author's dedicated-server contract: both files sit beside the
                // server exe. The optional self-test is never installed or run.
                // https://www.nexusmods.com/enshrouded/mods/178?tab=description
                let proxy = destinations.contains(&"powrprof.dll");
                let config = destinations.contains(&"xpshare.ini");
                // The author's current Autoloot distribution uses a plugin and
                // this exact INI, not the older standalone dinput8 layout.
                // https://www.nexusmods.com/enshrouded/mods/266?tab=files
                let autoloot_config = destinations.contains(&"AutolootCoop.ini");
                let autoloot_plugin = destinations.contains(&"plugins/AutolootCoop.dll");
                let autoloot = autoloot_config && autoloot_plugin;
                if (autoloot_config || autoloot_plugin) && !(proxy && autoloot) {
                    return Err("Autoloot requires powrprof.dll, plugins/AutolootCoop.dll and AutolootCoop.ini together; existing files were preserved".into());
                }
                // Creative Mode's server edition pairs powrprof.dll with mods/,
                // while XP Share uses xpshare.ini. Neither layout can overwrite
                // another project's or an unowned loader during publication.
                // https://www.nexusmods.com/enshrouded/mods/146?tab=description
                let mod_payload = destinations.iter().any(|path| path.starts_with("mods/"));
                if (config && !proxy)
                    || (proxy && !config && !mod_payload && !autoloot)
                    || (optional_selftest && !(proxy && config))
                {
                    return Err("Enshrouded root proxy installation requires the complete powrprof.dll and xpshare.ini pair".into());
                }
            }
            for destination in &destinations {
                if pak_extension(destination) && !destination.to_ascii_lowercase().ends_with(".pak")
                {
                    let companion = Path::new(destination)
                        .with_extension("pak")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if !destinations
                        .iter()
                        .any(|path| path.eq_ignore_ascii_case(&companion))
                    {
                        return Err("Nexus Pak sidecar has no corresponding .pak payload".into());
                    }
                }
            }
            return Ok(routes);
        }
        // Documentation outside a single payload wrapper is still metadata.
        // It must not prevent unwrapping the actual Mod files.
        let payload_paths = || {
            logical
                .iter()
                .filter(|(_, path)| !nexus_metadata(profile, path))
        };
        let Some((first, _)) = payload_paths()
            .next()
            .and_then(|(_, path)| path.split_once('/'))
        else {
            break;
        };
        // Never strip a recognized container to reinterpret framework/client files.
        if runtime_container(first)
            || (profile == Profile::Enshrouded && first.eq_ignore_ascii_case("emf"))
            || !payload_paths().all(|(_, path)| {
                path.split_once('/')
                    .is_some_and(|(name, _)| name.eq_ignore_ascii_case(first))
            })
        {
            break;
        }
        logical = logical
            .into_iter()
            .map(|(source, path)| {
                if nexus_metadata(profile, &path) {
                    return (source, path);
                }
                let (_, relative) = path.split_once('/').expect("shared wrapper was checked");
                (source, relative.to_owned())
            })
            .collect();
    }
    let kind = if profile == Profile::Enshrouded {
        "Enshrouded server"
    } else {
        "server Pak or named UE4SS"
    };
    let detail = if unsupported.is_empty() {
        String::new()
    } else {
        format!("; unrecognized entries {unsupported:?}")
    };
    Err(format!(
        "Nexus package does not contain a recognized {kind} Mod layout{detail}; existing files were preserved"
    ))
}
