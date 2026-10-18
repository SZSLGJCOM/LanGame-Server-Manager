use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::commands::OnlineModIdentity;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::commands) struct BepInExProfile {
    pub(in crate::commands) project: String,
    pub(in crate::commands) version: String,
    #[serde(default)]
    pub(in crate::commands) download_url: String,
    pub(in crate::commands) prefix: String,
    pub(in crate::commands) archive_bytes: u64,
    pub(super) archive_sha256: String,
    pub(super) files: BTreeMap<String, String>,
    #[serde(skip)]
    pub(super) expected_assembly: &'static str,
    #[serde(skip)]
    pub(super) receipt: &'static str,
    #[serde(skip)]
    pub(super) il2cpp: bool,
    #[serde(skip)]
    pub(super) ignored_payload: &'static [&'static str],
}

impl BepInExProfile {
    /// Each reviewed loader can satisfy older requests within its own build
    /// family. Another project's loader never supplies this dependency.
    pub(in crate::commands) fn supports_dependency(&self, project: &str, version: &str) -> bool {
        fn parts(version: &str) -> Option<[u32; 3]> {
            version
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?
                .try_into()
                .ok()
        }
        project == self.project
            && parts(version)
                .zip(parts(&self.version))
                .is_some_and(|(required, provided)| {
                    required[..2] == provided[..2] && required <= provided
                })
    }

    pub(in crate::commands) fn identity(&self) -> OnlineModIdentity {
        OnlineModIdentity {
            thunderstore_intent: None,
            provider: "thunderstore".into(),
            project: self.project.clone(),
            historical_names: vec![self.project.clone()],
            version: self.version.clone(),
            dependencies: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub(in crate::commands) struct ThunderstoreGameProfile {
    pub(in crate::commands) module_id: &'static str,
    pub(in crate::commands) community: &'static str,
    pub(in crate::commands) executable: &'static str,
    pub(in crate::commands) target_relative: &'static str,
    pub(in crate::commands) loader: Option<&'static BepInExProfile>,
}

pub(in crate::commands) fn reviewed_loader_satisfies(
    identity: &OnlineModIdentity,
    project: &str,
    version: &str,
) -> bool {
    identity.provider == "thunderstore"
        && identity.dependencies.is_empty()
        && ["valheim", "vrising"].into_iter().any(|module_id| {
            thunderstore_game_profile(module_id)
                .ok()
                .and_then(|game| game.loader)
                .is_some_and(|loader| {
                    identity.project == loader.project
                        && identity.version == loader.version
                        && loader.supports_dependency(project, version)
                })
        })
}

fn parse_loader(
    json: &str,
    expected_assembly: &'static str,
    receipt: &'static str,
    il2cpp: bool,
    ignored_payload: &'static [&'static str],
) -> Result<BepInExProfile, String> {
    let mut profile: BepInExProfile = serde_json::from_str(json)
        .map_err(|error| format!("Invalid bundled Mod loader profile: {error}"))?;
    if profile.archive_bytes == 0
        || profile.archive_bytes.checked_mul(200).is_none()
        || profile.archive_sha256.len() != 64
        || !profile
            .archive_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || !profile.files.contains_key(expected_assembly)
        || !profile.files.contains_key("winhttp.dll")
        || (il2cpp && !profile.files.contains_key("dotnet/coreclr.dll"))
        || profile.files.contains_key("doorstop_config.ini")
        || profile.files.contains_key("BepInEx/config/BepInEx.cfg")
        || profile.files.is_empty()
    {
        return Err("Bundled Mod loader profile lacks verified release evidence".into());
    }
    if profile.prefix.is_empty()
        || matches!(profile.prefix.as_str(), "." | "..")
        || profile.prefix.contains(['/', '\\', ':'])
    {
        return Err("Bundled Mod loader profile has an invalid archive prefix".into());
    }
    for (path, hash) in &profile.files {
        if path.is_empty()
            || path.contains(['\\', ':'])
            || path
                .split('/')
                .any(|part| part.is_empty() || matches!(part, "." | ".."))
            || hash.len() != 64
            || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("Bundled Mod loader profile contains an invalid immutable file".into());
        }
    }
    let (namespace, package) = profile
        .project
        .split_once('-')
        .ok_or("Bundled Mod loader profile has an invalid project")?;
    if profile.download_url.is_empty() {
        profile.download_url = format!(
            "https://thunderstore.io/package/download/{namespace}/{package}/{}/",
            profile.version
        );
    }
    profile.expected_assembly = expected_assembly;
    profile.receipt = receipt;
    profile.il2cpp = il2cpp;
    profile.ignored_payload = ignored_payload;
    Ok(profile)
}

pub(in crate::commands) fn thunderstore_game_profile(
    module_id: &str,
) -> Result<&'static ThunderstoreGameProfile, String> {
    static VALHEIM: OnceLock<Result<BepInExProfile, String>> = OnceLock::new();
    static VRISING: OnceLock<Result<BepInExProfile, String>> = OnceLock::new();
    static GAMES: OnceLock<Result<[ThunderstoreGameProfile; 3], String>> = OnceLock::new();
    let games = GAMES
        .get_or_init(|| {
            let valheim = VALHEIM
                .get_or_init(|| {
                    parse_loader(
                        include_str!("../assets/mod-loaders/valheim.json"),
                        "BepInEx/core/BepInEx.Preloader.dll",
                        ".langame-valheim-loader.json",
                        false,
                        &[
                            "changelog.txt",
                            "start_game_bepinex.sh",
                            "start_server_bepinex.sh",
                            "doorstop_libs/libdoorstop_x64.so",
                            "doorstop_libs/libdoorstop_x64.dylib",
                        ],
                    )
                })
                .as_ref()
                .map_err(Clone::clone)?;
            let vrising = VRISING
                .get_or_init(|| {
                    parse_loader(
                        include_str!("../assets/mod-loaders/vrising.json"),
                        "BepInEx/core/BepInEx.Unity.IL2CPP.dll",
                        ".langame-vrising-loader.json",
                        true,
                        &[],
                    )
                })
                .as_ref()
                .map_err(Clone::clone)?;
            Ok([
                ThunderstoreGameProfile {
                    module_id: "valheim",
                    community: "valheim",
                    executable: "valheim_server.exe",
                    target_relative: "BepInEx/plugins",
                    loader: Some(valheim),
                },
                ThunderstoreGameProfile {
                    module_id: "vrising",
                    community: "v-rising",
                    executable: "VRisingServer.exe",
                    target_relative: "BepInEx/plugins",
                    loader: Some(vrising),
                },
                ThunderstoreGameProfile {
                    module_id: "corekeeper",
                    community: "core-keeper",
                    executable: "CoreKeeperServer.exe",
                    target_relative: "CoreKeeperServer_Data/StreamingAssets/Mods",
                    loader: None,
                },
            ])
        })
        .as_ref()
        .map_err(Clone::clone)?;
    games
        .iter()
        .find(|profile| profile.module_id == module_id)
        .ok_or_else(|| format!("No verified Thunderstore runtime profile for {module_id}"))
}
