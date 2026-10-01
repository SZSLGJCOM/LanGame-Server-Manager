from __future__ import annotations

import json
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def expected_file(
    root: str,
    path: str,
    format_name: str,
    *,
    keys: dict[str, Any] | None = None,
    fragments: list[str] | None = None,
) -> dict[str, Any]:
    result: dict[str, Any] = {"root": root, "path": path, "format": format_name}
    if keys is not None:
        result["keys"] = keys
    elif fragments is not None:
        result["fragments"] = fragments
    else:
        raise ValueError("an expected file needs keys or fragments")
    return result


SQUAD_RAW_FILES = {
    "custom_options": ("CustomOptions.cfg", "CustomOption=Acceptance"),
    "excluded_factions": ("ExcludedFactions.cfg", "CAF"),
    "excluded_layers": ("ExcludedLayers.cfg", "AlBasrah_RAAS_v1"),
    "excluded_levels": ("ExcludedLevels.cfg", "AlBasrah"),
    "layer_rotation": ("LayerRotation.cfg", "AlBasrah_RAAS_v1"),
    "layer_voting": ("LayerVoting.cfg", "AlBasrah_RAAS_v1"),
    "layer_voting_low_players": ("LayerVotingLowPlayers.cfg", "Sumari_AAS_v1"),
    "layer_voting_night": ("LayerVotingNight.cfg", "Mutaha_RAAS_v1"),
    "level_rotation": ("LevelRotation.cfg", "AlBasrah"),
    "map_rotation": ("MapRotation.cfg", "AlBasrah_RAAS_v1"),
    "motd_cfg": ("MOTD.cfg", "Acceptance Squad rules"),
    "remote_admin_hosts": (
        "RemoteAdminListHosts.cfg",
        "https://config.example.invalid/admins.cfg",
    ),
    "remote_ban_hosts": (
        "RemoteBanListHosts.cfg",
        "https://config.example.invalid/bans.cfg",
    ),
    "vote_config": ("VoteConfig.cfg", "Mode=LayerList_Vote"),
}


SPECS: dict[str, dict[str, Any]] = {
    "barotrauma": {
        "filename": "2026-07-13-server_settings.json",
        "verified_at": "2026-07-13",
        "build": "Steam public build 23799137 / DedicatedServer 1.13.4.0",
        "settings": {
            "server_name": '深海 & "船员" <一号>',
            "server_message": "欢迎 <潜航员> & 保持冷静",
            "server_password": "fixture-generated-valid-value",
            "max_players": 16,
            "auto_restart": True,
            "respawn_interval": 2.5,
            "campaign_world_hostility": "High",
            "campaign_max_mission_count": 4,
            "campaign_oxygen_multiplier": 1.25,
            "campaign_crew_vitality_multiplier": 3.25,
            "admin_entries": '76561198077777777, Host <Lead> & "One"',
            "mod_workshop_ids": "",
            "extra_launch_args": "",
        },
        "files": [
            expected_file(
                "config",
                "serversettings.xml",
                "xml",
                fragments=[
                    'ServerName="深海 &amp; &quot;船员&quot; &lt;一号&gt;"',
                    'ServerMessageText="欢迎 &lt;潜航员&gt; &amp; 保持冷静"',
                    'password="fixture-generated-valid-value"',
                    'AutoRestart="true"',
                    'RespawnInterval="2.5"',
                    'WorldHostility="High"',
                    'MaxMissionCount="4"',
                    'OxygenMultiplier="1.25"',
                    'CrewVitalityMultiplier="3.25"',
                ],
            ),
            expected_file(
                "config",
                "Data/clientpermissions.xml",
                "xml",
                fragments=[
                    '<Client name="Host &lt;Lead&gt; &amp; &quot;One&quot;" '
                    'accountid="STEAM_1:1:58756024" permissions="All" />'
                ],
            ),
            expected_file(
                "config",
                "config_player.xml",
                "xml",
                fragments=["<regularpackages />"],
            ),
        ],
        "executable": "DedicatedServer.exe",
        "arguments": [],
    },
    "rust": {
        "filename": "2026-07-13-steam_public_build_24090743.json",
        "verified_at": "2026-07-13",
        "build": "Steam public build 24090743",
        "settings": {
            "server_name": "LanGame Rust 验收",
            "server_description": "原生配置覆盖验收",
            "max_players": 250,
            "rcon_password": "acceptance-rust-rcon",
            "rcon_web": True,
            "world_config_json": json.dumps(
                {"MainRoads": False, "FutureNative": {"keep": True}},
                ensure_ascii=False,
                indent=2,
            ),
            "owner_entries": "76561198077777777|Host|Created by LanGame",
            "moderator_entries": "76561198011111111|Moderator|Weekend",
            "skip_queue_entries": "76561198000000000|Priority Friend",
            "banned_entries": "76561198022222222|Griefing|Repeated",
            "custom_launch_flags": "",
        },
        "files": [
            expected_file(
                "install",
                "server/acceptance-rust/cfg/server.cfg",
                "text",
                fragments=[
                    'server.hostname "LanGame Rust 验收"',
                    "server.maxplayers 250",
                    'rcon.password "acceptance-rust-rcon"',
                    "server.writecfg",
                ],
            ),
            expected_file(
                "install",
                "server/acceptance-rust/cfg/users.cfg",
                "text",
                fragments=[
                    'ownerid 76561198077777777 "Host" "Created by LanGame"',
                    'moderatorid 76561198011111111 "Moderator" "Weekend"',
                    'global.skipqueueid 76561198000000000 "Priority Friend"',
                ],
            ),
            expected_file(
                "install",
                "server/acceptance-rust/cfg/bans.cfg",
                "text",
                fragments=[
                    'banid 76561198022222222 "LanGame" "Griefing|Repeated"'
                ],
            ),
            expected_file(
                "install",
                "server/acceptance-rust/world-config.json",
                "json",
                keys={"MainRoads": False, "FutureNative.keep": True},
            ),
        ],
        "executable": "RustDedicated.exe",
        "arguments": [
            "-batchmode",
            "-nographics",
            "-logfile",
            "{{paths.logs_dir}}/rust.log",
            "+server.identity",
            "acceptance-rust",
            "+world.configfile",
            "world-config.json",
            "+server.port",
            "28015",
            "+server.queryport",
            "28017",
            "+rcon.port",
            "28016",
            "+rcon.password",
            "acceptance-rust-rcon",
            "+rcon.web",
            "true",
        ],
    },
    "sevendaystodie": {
        "filename": "2026-08-10-steamcmd_anonymous_app_294420.json",
        "verified_at": "2026-08-10",
        "build": "SteamCMD app 294420 build 24392395 / V2.6 b14",
        "settings": {
            "server_name": '七日 & "家园" <A>',
            "server_description": "生存 & 建造",
            "server_password": "fixture-generated-valid-value",
            "max_players": 32,
            "game_world": "Navezgane",
            "world_name": "AcceptanceWorld",
            "world_seed": "种子&Seed",
            "sandbox_code": "AAAJABJACJADJARFBNC",
            "admin_users": [{
                "platform": "Steam",
                "userid": "76561198077777777",
                "name": 'Ops "One"',
                "permission_level": 0,
            }],
            "admin_groups": [{
                "steam_id": "103582791434672565",
                "name": "Steam Universe",
                "permission_level_default": 1000,
                "permission_level_mod": 0,
            }],
            "whitelist_users": [{
                "platform": "EOS",
                "userid": "0002604bc42244e099c1bf05145fb71f",
                "name": "Friend",
            }],
            "whitelist_groups": [{
                "steam_id": "103582791434672566",
                "name": "Friends Group",
            }],
            "blacklist_entries": [{
                "platform": "PSN",
                "userid": "Raider-One_42",
                "name": "Raider",
                "unbandate": "2030-01-01",
                "reason": "Griefing & spam",
            }],
            "command_permissions": [{"cmd": "help", "permission_level": 1000}],
        },
        "files": [
            expected_file(
                "config",
                "serverconfig.xml",
                "xml",
                fragments=[
                    'name="ServerName" value="七日 &amp; &quot;家园&quot; &lt;A&gt;"',
                    'name="ServerDescription" value="生存 &amp; 建造"',
                    'name="WorldGenSeed" value="种子&amp;Seed"',
                ],
            ),
            expected_file(
                "saves",
                "serveradmin.xml",
                "xml",
                fragments=[
                    '<user platform="Steam" userid="76561198077777777" '
                    'name="Ops &quot;One&quot;" permission_level="0" />',
                    '<group steamID="103582791434672565" name="Steam Universe"',
                    '<user platform="EOS" userid="0002604bc42244e099c1bf05145fb71f"',
                    '<blacklisted platform="PSN" userid="Raider-One_42" '
                    'name="Raider" unbandate="2030-01-01" reason="Griefing &amp; spam" />',
                    '<permission cmd="help" permission_level="1000" />',
                ],
            ),
        ],
        "executable": "7DaysToDieServer.exe",
        "arguments": [
            "-quit",
            "-batchmode",
            "-nographics",
            "-configfile={{paths.config_dir}}/serverconfig.xml",
            "-dedicated",
        ],
    },
    "squad": {
        "filename": "2026-07-13-steamcmd_anonymous_app_403240.json",
        "verified_at": "2026-07-13",
        "build": "SteamCMD app 403240 build 23797339",
        "settings": {
            "server_name": "LanGame Squad 验收",
            "server_message": "Welcome to the acceptance server.",
            "max_players": 96,
            "rcon_password": "acceptance-squad-rcon",
            "admin_steam_ids": "76561198077777777",
            "priority_join_steam_ids": "76561198000000000",
            "admins_cfg": "Group=EventAdmin:kick,ban",
            "extra_launch_args": "",
            **{key: value for key, (_, value) in SQUAD_RAW_FILES.items()},
        },
        "files": [
            expected_file(
                "install",
                "SquadGame/ServerConfig/Server.cfg",
                "text",
                fragments=['ServerName="LanGame Squad 验收"', "MaxPlayers=96"],
            ),
            expected_file(
                "install",
                "SquadGame/ServerConfig/Rcon.cfg",
                "text",
                fragments=["Port=21114", "Password=acceptance-squad-rcon"],
            ),
            expected_file(
                "install",
                "SquadGame/ServerConfig/Admins.cfg",
                "text",
                fragments=[
                    "Admin=76561198077777777:LanGameAdmin",
                    "Admin=76561198000000000:LanGameReserved",
                    "Group=EventAdmin:kick,ban",
                ],
            ),
            expected_file(
                "install",
                "SquadGame/ServerConfig/ServerMessages.cfg",
                "text",
                fragments=["Welcome to the acceptance server."],
            ),
            *[
                expected_file(
                    "install",
                    f"SquadGame/ServerConfig/{file_name}",
                    "text",
                    fragments=[value],
                )
                for file_name, value in SQUAD_RAW_FILES.values()
            ],
        ],
        "executable": "SquadGameServer.exe",
        "arguments": [
            "MULTIHOME=0.0.0.0",
            "PORT=7787",
            "QUERYPORT=27165",
            "RCONPORT=21114",
            "FIXEDMAXPLAYERS=96",
        ],
    },
    "valheim": {
        "filename": "2026-07-13-steamcmd_anonymous_app_896660.json",
        "verified_at": "2026-07-13",
        "build": "SteamCMD app 896660 build 21981590",
        "settings": {
            "server_name": "Valheim 验收服",
            "world_name": "AcceptanceWorld",
            "server_password": "acceptance-valheim",
            "public_server": 1,
            "crossplay_enabled": True,
            "instance_id": "acceptance-shard",
            "save_interval_seconds": 900,
            "backup_count": 6,
            "backup_short_seconds": 3600,
            "backup_long_seconds": 21600,
            "world_preset": "normal",
            "world_modifiers": "combat hard\ndeathpenalty casual",
            "world_set_keys": "playerevents\nnomap",
            "admin_list": "76561198077777777\nSteam_ABC123",
            "banned_list": "blocked-player\nCrossPlay-User",
            "permitted_list": "friend_one\nfriend-three",
            "log_file": "D:\\ValheimLogs\\server.log",
            "custom_launch_flags": "",
        },
        "files": [
            expected_file(
                "saves",
                "adminlist.txt",
                "text",
                fragments=["76561198077777777\nSteam_ABC123"],
            ),
            expected_file(
                "saves",
                "bannedlist.txt",
                "text",
                fragments=["blocked-player\nCrossPlay-User"],
            ),
            expected_file(
                "saves",
                "permittedlist.txt",
                "text",
                fragments=["friend_one\nfriend-three"],
            ),
        ],
        "executable": "valheim_server.exe",
        "arguments": [
            "-nographics",
            "-batchmode",
            "-name",
            "Valheim 验收服",
            "-port",
            "2456",
            "-world",
            "AcceptanceWorld",
            "-preset",
            "normal",
            "-modifier",
            "combat",
            "hard",
            "-modifier",
            "deathpenalty",
            "casual",
            "-setkey",
            "playerevents",
            "-setkey",
            "nomap",
            "-password",
            "acceptance-valheim",
            "-savedir",
            "{{paths.saves_dir}}",
            "-public",
            "1",
            "-saveinterval",
            "900",
            "-backups",
            "6",
            "-backupshort",
            "3600",
            "-backuplong",
            "21600",
            "-crossplay",
            "-instanceid",
            "acceptance-shard",
            "-logFile",
            "D:\\ValheimLogs\\server.log",
        ],
    },
}


def classifications(module_id: str, ledger: dict[str, Any]) -> dict[str, list[str]]:
    schema = json.loads((ROOT / "modules" / module_id / "schema.json").read_text(encoding="utf-8-sig"))
    properties = schema["properties"]
    result = {key: [] for key in ("editable", "specialized", "derived", "generated", "excluded")}
    for item in ledger.get("items", []):
        schema_key = item["schema_key"]
        property_schema = properties[schema_key]
        category = "specialized" if (
            property_schema.get("x-lsgm-player-access-kind") is not None
            or (module_id == "sevendaystodie" and schema_key == "command_permissions")
        ) else "editable"
        result[category].append(f"{item['source']}.{item['key']}")
    for exclusion in ledger.get("exclusions", []):
        result["excluded"].append(f"{exclusion['source']}.{exclusion['key']}")
    return result


def build_fixture(module_id: str, spec: dict[str, Any]) -> dict[str, Any]:
    module_root = ROOT / "modules" / module_id
    ledger = tomllib.loads((module_root / "config-sources.toml").read_text(encoding="utf-8-sig"))
    classified = classifications(module_id, ledger)
    return {
        "fixture_version": 1,
        "module_id": module_id,
        "evidence": {
            "status": ledger["status"],
            "verified_at": spec["verified_at"],
            "build": spec["build"],
            "source_ids": [source["id"] for source in ledger.get("sources", [])],
        },
        "coverage": {key: len(value) for key, value in classified.items()},
        "classifications": classified,
        "settings": spec["settings"],
        "expected": {
            "files": spec["files"],
            "launch": {
                "executable_suffix": spec["executable"],
                "arguments": spec["arguments"],
            },
        },
    }


def main() -> int:
    for module_id, spec in SPECS.items():
        target = ROOT / "modules" / module_id / "config-fixtures" / spec["filename"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(
            json.dumps(build_fixture(module_id, spec), ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
            newline="\n",
        )
        print(target.relative_to(ROOT).as_posix())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
