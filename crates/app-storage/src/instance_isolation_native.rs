use std::path::{Path, PathBuf};

/// Exclusive native configuration destinations used by templates_materialize.
/// Shared program files, DST's aggregated Workshop downloads and explicitly
/// selected ARK transfer directories are not configuration ownership claims.
pub(crate) fn configuration_paths(module_id: &str, runtime: &Path, id: &str) -> Vec<PathBuf> {
    let relative: &[&str] = match module_id {
        "arksurvivalevolved" | "arksurvivalascended" => &[
            "ShooterGame/Saved/Config/WindowsServer",
            "ShooterGame/Saved/AllowedCheaterSteamIDs.txt",
            "ShooterGame/Binaries/Win64/PlayersExclusiveJoinList.txt",
            "ShooterGame/Binaries/Win64/PlayersJoinNoCheckList.txt",
        ],
        "palworld" => &[
            "Pal/Saved/Config/WindowsServer",
            "Pal/Binaries/Win64/Mods/PalModSettings.ini",
        ],
        "conanexiles" => &[
            "ConanSandbox/Saved/Config/WindowsServer",
            "ConanSandbox/Mods/modlist.txt",
        ],
        "astroneer" => &["Astro/Saved/Config/WindowsServer"],
        "enshrouded" => &["enshrouded_server.json"],
        "humanitz" => &[
            "HumanitZServer/GameServerSettings.ini",
            "HumanitZServer/WelcomeMessage.txt",
            "HumanitZServer/AdminList.txt",
            "HumanitZServer/F_MVPAccess.txt",
            "HumanitZServer/F_ReservedSlots.txt",
            "HumanitZServer/F_BannedPlayers.txt",
        ],
        "nightingale" => &["NWX/Config/ServerSettings.ini"],
        "returntomoria" => &[
            "MoriaServerConfig.ini",
            "MoriaServerRules.txt",
            "MoriaServerPermissions.txt",
        ],
        "rimworld" => &["Configs"],
        "romestead" => &["config.json", "start-romestead.bat"],
        "runescapedragonwilds" => &["RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"],
        "scum" => &["SCUM/Saved/Config/WindowsServer"],
        "soulmask" => &["WS/Saved/GameplaySettings/GameXishu.json"],
        "squad" => &["SquadGame/ServerConfig"],
        "windrose" => &["R5/ServerDescription.json"],
        // All other built-ins keep config in their instance root, or use the
        // identities below. Do not treat their shared installation as writable config.
        _ => &[],
    };
    let mut result = relative
        .iter()
        .map(|relative| runtime.join(relative))
        .collect::<Vec<_>>();
    match module_id {
        "abioticfactor" => {
            result.push(runtime.join(format!(
                "AbioticFactor/Saved/Config/WindowsServer/LanGame/{id}-SandboxSettings.ini"
            )));
            result.push(runtime.join(format!(
                "AbioticFactor/Saved/SaveGames/Server/LanGame/{id}-Admin.ini"
            )));
        }
        "rust" => result.push(runtime.join("server").join(id).join("cfg")),
        "unturned" => result.push(runtime.join("Servers").join(id)),
        _ => {}
    }
    result
}
