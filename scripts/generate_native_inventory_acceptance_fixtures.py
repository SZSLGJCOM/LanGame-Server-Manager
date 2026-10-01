from __future__ import annotations

import json
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]

SPECIALIZED_SCHEMA_KEYS: dict[str, set[str]] = {
    "corekeeper": {"admin_list", "ban_list"},
    "enshrouded": {"banned_player_ids"},
    "minecraft": {
        "operator_entries",
        "whitelist_entries",
        "banned_player_entries",
        "banned_ip_entries",
    },
    "necesse": {"owner_name"},
    "projectzomboid": set(),
    "sonsoftheforest": {"owner_whitelist_steam_ids"},
    "vrising": {"admin_list", "ban_list"},
}


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


SPECS: dict[str, dict[str, Any]] = {
    "corekeeper": {
        "filename": "2026-07-13-steamcmd_anonymous_probe.json",
        "build": "SteamCMD app 1963720 public build 23543502",
        "settings": {
            "server_name": "地心工坊 Δ",
            "world_index": 29,
            "world_seed": "seed-α",
            "hashed_world_seed": 4294967295,
            "world_mode": 4,
            "season_override": 7,
            "max_players": 100,
            "max_packets_per_frame": 8,
            "network_send_rate": 64,
            "direct_connection_enabled": True,
            "join_password": "acceptance-corekeeper",
            "allowed_platform_code": 2,
            "admin_list": "76561198000000001\n76561198000000002",
            "ban_list": "76561198000000003",
        },
        "initial": {
            "files": [{
                "root": "instance",
                "path": "data/ServerConfig.json",
                "content": {"futurePackageOption": {"keep": True}},
            }]
        },
        "files": [
            expected_file("instance", "data/ServerConfig.json", "json", keys={
                "worldName": "地心工坊 Δ",
                "world": 29,
                "worldSeed": "seed-α",
                "hashedWorldSeed": 4294967295,
                "maxNumberPlayers": 100,
                "futurePackageOption.keep": True,
            }),
            expected_file("instance", "data/Admins.json", "json", keys={
                "/0/steamId": 76561198000000001,
                "/1/steamId": 76561198000000002,
            }),
            expected_file("instance", "data/PlayerBans.json", "json", keys={
                "/banList/0/steamId": 76561198000000003,
            }),
        ],
        "executable": "CoreKeeperServer.exe",
        "arguments": [
            "-batchmode", "-logfile", "{{paths.logs_dir}}/CoreKeeperServer.log",
            "-datapath", "{{paths.data_dir}}", "-ip", "0.0.0.0",
            "-port", "27015", "-password", "acceptance-corekeeper",
            "-allowonlyplatform", "Epic",
        ],
    },
    "enshrouded": {
        "filename": "2026-07-13-steamcmd_anonymous_probe_2278520.json",
        "build": "SteamCMD anonymous probe for app 2278520",
        "settings": {
            "server_name": "余烬国度 Δ",
            "max_players": 16,
            "server_tags": "English,Chinese",
            "game_settings_preset": "Custom",
            "player_health_factor": 4,
            "hunger_to_starving_minutes": 20,
            "day_time_minutes": 60,
            "night_time_minutes": 2,
            "mining_damage_factor": 0.5,
            "enemy_damage_factor": 5,
            "pacify_all_enemies": True,
            "admin_password": "acceptance-enshrouded-admin",
            "banned_player_ids": "76561198000000004\n76561198000000005",
        },
        "initial": {
            "files": [{
                "root": "install",
                "path": "enshrouded_server.json",
                "content": {
                    "futureRoot": {"keep": True},
                    "gameSettings": {"futureSetting": "keep"},
                },
            }]
        },
        "files": [expected_file("install", "enshrouded_server.json", "json", keys={
            "name": "余烬国度 Δ",
            "slotCount": 16,
            "/tags/0": "English",
            "/tags/1": "Chinese",
            "gameSettings.playerHealthFactor": 4,
            "gameSettings.fromHungerToStarving": 1200000000000,
            "gameSettings.dayTimeDuration": 3600000000000,
            "gameSettings.nightTimeDuration": 120000000000,
            "gameSettings.futureSetting": "keep",
            "futureRoot.keep": True,
        })],
        "executable": "enshrouded_server.exe",
        "arguments": [],
    },
    "minecraft": {
        "filename": "2026-07-13-mojang_manifest_release_26_2.json",
        "build": "Mojang Java release 26.2",
        "settings": {
            "eula_accepted": True,
            "motd": "方块世界 Δ = ready",
            "max_players": 200,
            "level_name": "acceptance-world",
            "level_seed": "seed=Δ",
            "gamemode": "creative",
            "difficulty": "hard",
            "hardcore": True,
            "enable_query": True,
            "enable_whitelist": False,
            "enable_rcon": True,
            "rcon_password": "acceptance-minecraft-rcon",
            "memory_min_mb": 1536,
            "memory_max_mb": 6144,
            "operator_entries": "00000000-0000-0000-0000-000000000001,Builder,4,true",
            "whitelist_entries": "00000000-0000-0000-0000-000000000002,Friend",
            "extra_properties": "future-key=keep\nunicode-extra=值",
        },
        "files": [
            expected_file("instance", "server.properties", "properties", keys={
                "motd": "方块世界 Δ = ready",
                "max-players": "200",
                "level-name": "acceptance-world",
                "level-seed": "seed=Δ",
                "gamemode": "creative",
                "difficulty": "hard",
                "enable-rcon": "true",
                "white-list": "false",
                "rcon.password": "acceptance-minecraft-rcon",
                "future-key": "keep",
                "unicode-extra": "值",
            }),
            expected_file("instance", "eula.txt", "properties", keys={"eula": "true"}),
            expected_file("instance", "ops.json", "json", keys={
                "/0/name": "Builder", "/0/level": 4,
            }),
            expected_file("instance", "whitelist.json", "json", keys={
                "/0/name": "Friend",
            }),
        ],
        "executable": "jre/bin/java.exe",
        "arguments": [
            "-Xms1536M", "-Xmx6144M", "-jar",
            "{{paths.install_root}}/server.jar", "nogui",
        ],
    },
    "necesse": {
        "filename": "2026-09-28-native_server_cfg_build_24926481.json",
        "build": "Read-only installed Necesse 1.3.3 build 24926481 native config metadata",
        "verified_at": "2026-09-28",
        "settings": {
            "world_name": "Acceptance World Δ",
            "max_slots": 250,
            "motd": "欢迎来到 Necesse",
            "owner_name": "OwnerOne",
            "password": "acceptance-necesse",
            "pause_when_empty": True,
            "strict_server_authority": True,
            "logging_enabled": False,
            "zip_saves": False,
            "language": "zh",
            "ignore_seasons": True,
            "max_client_latency_seconds": 45, "unload_levels_cooldown": 60,
            "dropped_items_life_minutes": 90, "unload_settlements": True,
            "max_settlements_per_player": 3, "max_settlers_per_settlement": 20,
            "world_border_size": 600,
        },
        "initial": {"files": [{"root": "instance", "path": "data/cfg/server.cfg",
            "content": "SERVER = {\n maxClientLatencySeconds = 30, // keep\n future = 55,\n}\n"}]},
        "files": [expected_file("instance", "data/cfg/server.cfg", "text", fragments=[
            "maxClientLatencySeconds = 45, // keep", "unloadLevelsCooldown = 60,",
            "droppedItemsLifeMinutes = 90,", "unloadSettlements = true,",
            "maxSettlementsPerPlayer = 3,", "maxSettlersPerSettlement = 20,",
            "worldBorderSize = 600,", "future = 55,"
        ])],
        "executable": "jre/bin/java.exe",
        "arguments": [
            "-XX:+UnlockExperimentalVMOptions", "-XX:+UseG1GC",
            "-XX:+ExplicitGCInvokesConcurrent", "-XX:G1NewSizePercent=20",
            "-XX:G1ReservePercent=20", "-XX:MaxGCPauseMillis=50",
            "-XX:G1HeapRegionSize=32M", "-jar", "Server.jar", "-nogui", "-settings", "{{paths.data_dir}}/cfg/server.cfg",
            "-world", "Acceptance World Δ", "-port", "14159", "-slots", "250",
            "-owner", "OwnerOne", "-motd", "欢迎来到 Necesse",
            "-password", "acceptance-necesse", "-pausewhenempty", "1",
            "-strictserverauthority", "1", "-logging", "0", "-logs", "..\\logs",
            "-zipsaves", "0", "-language", "zh", "-ip", "0.0.0.0",
            "-datadir", "{{paths.data_dir}}", "-ignoreseasons",
        ],
    },
    "projectzomboid": {
        "filename": "2026-09-28-native_server_options_build_24909836.json",
        "build": "Read-only installed build 24909836 native inventory; historical launch evidence retained separately",
        "verified_at": "2026-09-28",
        "settings": {
            "server_name": "僵尸世界 Δ",
            "server_description": "Acceptance server",
            "welcome_message": "欢迎幸存者",
            "max_players": 100,
            "public_server": True,
            "allow_coop": False,
            "server_password": "acceptance-pz",
            "admin_username": "acceptadmin",
            "admin_password": "acceptance-pz-admin",
            "rcon_password": "acceptance-pz-rcon",
            "memory_gb": 6,
            "war": True, "war_start_delay": 120, "anti_cheat_speed": 3,
            "chat_message_character_limit": 512, "disable_vehicle_towing": True,
        },
        "files": [expected_file(
            "config",
            "runtime-home/Zomboid/Server/acceptance-projectzomboid.ini",
            "properties",
            keys={
                "PublicName": "僵尸世界 Δ",
                "PublicDescription": "Acceptance server",
                "ServerWelcomeMessage": "欢迎幸存者",
                "MaxPlayers": "100",
                "Public": "true",
                "AllowCoop": "false",
                "Password": "acceptance-pz",
                "RCONPassword": "acceptance-pz-rcon",
                "War": "true", "WarStartDelay": "120", "AntiCheatSpeed": "3",
                "ChatMessageCharacterLimit": "512", "DisableVehicleTowing": "true",
            },
        )],
        "executable": "jre64/bin/java.exe",
        "arguments": [
            "-Djava.awt.headless=true", "-Dzomboid.steam=1", "-Dzomboid.znetlog=1",
            "-XX:+UseZGC", "-XX:-CreateCoredumpOnCrash",
            "-XX:-OmitStackTraceInFastThrow", "-Xms6g", "-Xmx6g",
            "-Duser.home={{paths.config_dir}}/runtime-home",
            "-Djava.library.path=natives/;natives/win64/;.", "-cp", "java/",
            "zombie.network.GameServer", "-statistic", "0", "-servername",
            "acceptance-projectzomboid", "-adminusername", "acceptadmin",
            "-adminpassword", "acceptance-pz-admin", "-port", "16261",
            "-udpport", "16262",
        ],
    },
    "sonsoftheforest": {
        "filename": "2026-07-13-steamcmd_anonymous_app_2465200.json",
        "build": "SteamCMD anonymous app 2465200",
        "settings": {
            "server_name": "森林之子 Δ",
            "max_players": 8,
            "server_password": "acceptance-sotf",
            "owner_whitelist_steam_ids": "76561198000000001\n76561198000000002",
            "game_mode": "Custom",
            "save_slot": 3,
            "save_mode": "New",
            "save_interval": 60,
            "idle_day_cycle_speed": 1,
            "custom_pvp_damage": "Hard",
            "custom_starting_season": "Winter",
            "custom_one_hit_tree_cutting": True,
        },
        "files": [
            expected_file("config", "dedicatedserver.cfg", "json", keys={
                "ServerName": "森林之子 Δ",
                "MaxPlayers": 8,
                "Password": "acceptance-sotf",
                "GameMode": "Custom",
                "SaveSlot": 3,
                "SaveInterval": 60,
                "/CustomGameModeSettings/GameSetting.Multiplayer.PvpDamage": "Hard",
                "/CustomGameModeSettings/GameSetting.Environment.StartingSeason": "Winter",
                "/CustomGameModeSettings/GameSetting.Survival.OneHitToCutTrees": True,
            }),
            expected_file(
                "instance", "data/ownerswhitelist.txt", "text",
                fragments=["76561198000000001\n76561198000000002"],
            ),
        ],
        "executable": "SonsOfTheForestDS.exe",
        "arguments": [
            "-batchmode", "-nographics", "-userdatapath", "{{paths.data_dir}}",
            "-configfilepath", "{{paths.config_dir}}/dedicatedserver.cfg",
        ],
    },
    "vrising": {
        "filename": "2026-07-13-steamcmd_anonymous_app_1829350.json",
        "build": "SteamCMD anonymous app 1829350",
        "settings": {
            "server_name": "夜之城 \"Acceptance\"",
            "server_description": "Unicode Δ 与 JSON escaping",
            "max_players": 60,
            "server_password": "acceptance-vrising",
            "save_name": "acceptance-world",
            "rcon_enabled": True,
            "rcon_password": "acceptance-vrising-rcon",
            "admin_list": "76561198000000001\n76561198000000002",
            "ban_list": "76561198000000003",
            "vampire_max_health_modifier": 2.5,
            "global_unit_level_increase": 10,
            "vblood_unit_power_modifier": 1.75,
            "castle_heart_level_5_floor_limit": 777,
            "server_game_settings_json": json.dumps({
                "FutureNative": {"Keep": True},
                "UnlockedAchievements": [-1599762431],
                "VampireStatModifiers": {"FutureNested": 7},
            }, ensure_ascii=False, indent=2),
        },
        "initial": {
            "files": [{
                "root": "instance",
                "path": "Settings/ServerGameSettings.json",
                "content": {
                    "FutureDestination": "keep",
                    "VampireStatModifiers": {"DestinationNested": True},
                },
            }]
        },
        "files": [
            expected_file("instance", "Settings/ServerHostSettings.json", "json", keys={
                "Name": "夜之城 \"Acceptance\"",
                "Description": "Unicode Δ 与 JSON escaping",
                "MaxConnectedUsers": 60,
                "Password": "acceptance-vrising",
                "SaveName": "acceptance-world",
                "Rcon.Enabled": True,
                "Rcon.Password": "acceptance-vrising-rcon",
            }),
            expected_file("instance", "Settings/ServerGameSettings.json", "json", keys={
                "FutureNative.Keep": True,
                "FutureDestination": "keep",
                "/UnlockedAchievements/0": -1599762431,
                "VampireStatModifiers.FutureNested": 7,
                "VampireStatModifiers.DestinationNested": True,
                "VampireStatModifiers.MaxHealthModifier": 2.5,
                "UnitStatModifiers_Global.LevelIncrease": 10,
                "UnitStatModifiers_VBlood.PowerModifier": 1.75,
                "CastleStatModifiers_Global.HeartLimits.Level5.FloorLimit": 777,
            }),
            expected_file(
                "instance", "Settings/adminlist.txt", "text",
                fragments=["76561198000000001\n76561198000000002"],
            ),
            expected_file(
                "instance", "Settings/banlist.txt", "text",
                fragments=["76561198000000003"],
            ),
        ],
        "executable": "VRisingServer.exe",
        "arguments": ["-persistentDataPath", "{{paths.instance_root}}"],
    },
}


def build_classifications(module_id: str, ledger: dict[str, Any]) -> dict[str, list[str]]:
    classifications = {
        "editable": [],
        "specialized": [],
        "derived": [],
        "generated": [],
        "excluded": [],
    }
    specialized = SPECIALIZED_SCHEMA_KEYS[module_id]
    for item in ledger.get("items", []):
        category = "specialized" if item["schema_key"] in specialized else "editable"
        classifications[category].append(f"{item['source']}.{item['key']}")
    for exclusion in ledger.get("exclusions", []):
        classifications["excluded"].append(
            f"{exclusion['source']}.{exclusion['key']}"
        )
    return classifications


def build_fixture(module_id: str, spec: dict[str, Any]) -> dict[str, Any]:
    module_root = ROOT / "modules" / module_id
    ledger = tomllib.loads(
        (module_root / "config-sources.toml").read_text(encoding="utf-8-sig")
    )
    classifications = build_classifications(module_id, ledger)
    fixture: dict[str, Any] = {
        "fixture_version": 1,
        "module_id": module_id,
        "evidence": {
            "status": ledger["status"],
            "verified_at": spec.get("verified_at", "2026-07-13"),
            "build": spec["build"],
            "source_ids": [source["id"] for source in ledger.get("sources", [])],
        },
        "coverage": {key: len(value) for key, value in classifications.items()},
        "classifications": classifications,
        "settings": spec["settings"],
        "expected": {
            "files": spec["files"],
            "launch": {
                "executable_suffix": spec["executable"],
                "arguments": spec["arguments"],
            },
        },
    }
    if "initial" in spec:
        fixture["initial"] = spec["initial"]
    return fixture


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
