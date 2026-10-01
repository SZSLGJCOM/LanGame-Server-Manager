from __future__ import annotations

import argparse
import json
import tomllib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MODULE_ROOT = ROOT / "modules" / "scum"
FIXTURE_PATH = MODULE_ROOT / "config-fixtures" / "2026-09-28-steamcmd_anonymous_app_3792580.json"


def qualified(row: dict[str, Any]) -> str:
    return f"{row['source']}.{row['key']}"


def classifications(ledger: dict[str, Any]) -> dict[str, list[str]]:
    items = ledger.get("items", [])
    generated_key = "generated_current_server_settings.scum.ServerSettingsVersion"
    editable_key = "launch_args.extra_launch_args"
    return {
        "editable": [editable_key],
        "specialized": [qualified(row) for row in items
                        if qualified(row) not in {generated_key, editable_key}],
        "derived": [],
        "generated": [generated_key],
        "excluded": [qualified(row) for row in ledger.get("exclusions", [])],
    }


def build_fixture() -> dict[str, Any]:
    ledger = tomllib.loads((MODULE_ROOT / "config-sources.toml").read_text(encoding="utf-8-sig"))
    classified = classifications(ledger)
    server_path = "SCUM/Saved/Config/WindowsServer"
    return {
        "fixture_version": 1,
        "module_id": "scum",
        "evidence": {
            "status": ledger["status"],
            "verified_at": "2026-09-28",
            "build": "Generated ServerSettings v7 baseline plus official 1.3.3.0 August additions; existing build 25187656 INI compared read-only",
            "source_ids": [source["id"] for source in ledger["sources"]],
        },
        "initial": {"files": [
            {
                "root": "install",
                "path": f"{server_path}/ServerSettings.ini",
                "content": (
                    "; server-owned comment\n[General]\nscum.ServerSettingsVersion=7\n"
                    "scum.ServerName=Before\nscum.FutureGeneral=keep\nscum.ServerName=duplicate\n\n"
                    "[World]\nscum.FutureWorld=keep\n\n[FutureSection]\nFutureKey=keep\n"
                ),
            },
            {
                "root": "install",
                "path": f"{server_path}/EconomyOverride.json",
                "content": {
                    "economy-override": {
                        "economy-reset-time-hours": "1.0",
                        "future-global": "keep",
                        "traders": {
                            "A_0_Armory": [{"tradeable-code": "Old", "future-row": "replaced-with-array"}],
                            "future-trader": [{"future": True}],
                        },
                    },
                    "future-root": {"keep": True},
                },
            },
            {
                "root": "install",
                "path": f"{server_path}/RaidTimes.json",
                "content": {"raiding-times": [{"day": "Old"}], "future-root": "keep"},
            },
            {
                "root": "install",
                "path": f"{server_path}/Notifications.json",
                "content": {"Notifications": [{"message": "Old"}], "future-root": "keep"},
            },
            {
                "root": "install",
                "path": f"{server_path}/ServerSettingsAdminUsers.ini",
                "content": "76561198000000000\n",
            },
        ]},
        "lifecycle": {
            "save_stage": "Render typed SCUM files into the isolated instance configuration directory.",
            "pre_start": "Prevalidate and atomically merge INI, JSON, and Player Access roster files as one batch.",
        },
        "coverage": {category: len(keys) for category, keys in classified.items()},
        "classifications": classified,
        "settings": {
            "server_general": {"server_name": "LanGame SCUM", "max_players": 32},
            "server_world": {"animal_global_density_multiplier": 0.75,
                             "max_allowed_apex_facility_keycards": 8,
                             "max_allowed_apex_facility_keycards_police_station": 6,
                             "max_allowed_apex_facility_keycards_radiation_zone": 2},
            "server_features": {"number_of_allowed_flags_per_player": 8},
            "server_respawn": {"random_respawn_price": 325, "cloning_sickness_enabled": False},
            "server_vehicles": {"fuel_drain_from_engine_multiplier": 0.8},
            "server_damage": {"human_to_human_damage_multiplier": 0.9},
            "economy_override": {
                "economy-reset-time-hours": "12.0",
                "traders": {"A_0_Armory": [{
                    "tradeable-code": "Weapon_AK47",
                    "base-purchase-price": "2500",
                    "base-sell-price": "900",
                    "delta-price": "0.0",
                    "can-be-purchased": "true",
                    "required-famepoints": "25",
                    "available-after-sale-only": "false",
                }]},
            },
            "raid_times": [{
                "day": "Weekend", "time": "12:00-15:00,20:00-21:30",
                "start-announcement-time": "30", "end-announcement-time": "15",
            }],
            "notifications": [{
                "day": "Everyday", "time": ["15:12", "20:00-21:00"],
                "duration": "10", "color": "255-200-0", "wait": "15",
                "message": "Players online: #NumPlayers",
            }],
            "admin_steam_ids": "76561198000000001\n76561198000000002",
            "extra_launch_args": "-log\n-NoSound",
        },
        "expected": {
            "files": [
                {"root": "install", "path": f"{server_path}/ServerSettings.ini", "format": "ini", "keys": {
                    "General.scum.ServerSettingsVersion": "7",
                    "General.scum.ServerName": "LanGame SCUM",
                    "General.scum.MaxPlayers": "32",
                    "General.scum.FutureGeneral": "keep",
                    "World.scum.AnimalGlobalDensityMultiplier": "0.75",
                    "World.scum.FutureWorld": "keep",
                    "World.scum.MaxAllowedApexFacilityKeycards": "8",
                    "World.scum.MaxAllowedApexFacilityKeycards_PoliceStation": "6",
                    "World.scum.MaxAllowedApexFacilityKeycards_RadiationZone": "2",
                    "Features.scum.NumberOfAllowedFlagsPerPlayer": "8",
                    "Respawn.scum.RandomRespawnPrice": "325",
                    "Respawn.scum.CloningSicknessEnabled": "False",
                    "Vehicles.scum.FuelDrainFromEngineMultiplier": "0.8",
                    "Damage.scum.HumanToHumanDamageMultiplier": "0.9",
                    "FutureSection.FutureKey": "keep",
                }},
                {"root": "install", "path": f"{server_path}/EconomyOverride.json", "format": "json", "keys": {
                    "/economy-override/economy-reset-time-hours": "12.0",
                    "/economy-override/future-global": "keep",
                    "/economy-override/traders/A_0_Armory/0/tradeable-code": "Weapon_AK47",
                    "/economy-override/traders/future-trader/0/future": True,
                    "/future-root/keep": True,
                }},
                {"root": "install", "path": f"{server_path}/RaidTimes.json", "format": "json", "keys": {
                    "/raiding-times/0/day": "Weekend", "/future-root": "keep",
                }},
                {"root": "install", "path": f"{server_path}/Notifications.json", "format": "json", "keys": {
                    "/Notifications/0/time/1": "20:00-21:00", "/future-root": "keep",
                }},
                {"root": "install", "path": f"{server_path}/ServerSettingsAdminUsers.ini", "format": "text", "fragments": [
                    "76561198000000001", "76561198000000002",
                ]},
            ],
            "launch": {
                "executable_suffix": "SCUM/Binaries/Win64/SCUMServer.exe",
                "arguments": ["-MULTIHOME=0.0.0.0", "-Port=7777", "-QueryPort=27015", "-log", "-NoSound"],
            },
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate the exact SCUM acceptance fixture.")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = json.dumps(build_fixture(), ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if not FIXTURE_PATH.is_file() or FIXTURE_PATH.read_text(encoding="utf-8") != rendered:
            raise SystemExit("SCUM acceptance fixture is stale")
        return 0
    FIXTURE_PATH.parent.mkdir(parents=True, exist_ok=True)
    FIXTURE_PATH.write_text(rendered, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
