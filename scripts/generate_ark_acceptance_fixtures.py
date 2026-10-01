#!/usr/bin/env python3
"""Generate exhaustive ARK materialization and launch acceptance fixtures."""

from __future__ import annotations

import json
import re
import tomllib
from pathlib import Path
from typing import Any

from ark_official_repository_contract import expected_repeated, writer_native_name


ROOT = Path(__file__).resolve().parents[1]
REPORT = json.loads(
    (ROOT / "docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json").read_text(
        encoding="utf-8"
    )
)
EDITIONS = {
    "ase": (
        "arksurvivalevolved",
        "2026-07-13-steamcmd_anonymous_app_376030.json",
        "ShooterGame/Binaries/Win64/ShooterGameServer.exe",
    ),
    "asa": (
        "arksurvivalascended",
        "2026-07-13-steamcmd_anonymous_app_2430930.json",
        "ShooterGame/Binaries/Win64/ArkAscendedServer.exe",
    ),
}
QUOTED = {"banlisturl", "customdynamicconfigurl", "customlivetuningurl"}
ASA_PATCH_GUS_SETTINGS = {
    "anchored_vessel_check_radius": "AnchoredVesselCheckRadius",
    "max_anchored_vessels_in_range": "MaxAnchoredVesselsInRange",
    "needs_power_to_activate_aquatic_compartments": "NeedsPowerToActivateAquaticCompartments",
    "use_astraeos_traversal_buff": "UseAstraeosTraversalBuff",
}


def canonical(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", value.lower())


def repeated_value(item: dict[str, Any]) -> str:
    name = canonical(item["native_name"])
    if name == "overrideplayerlevelengrampoints":
        return "8\n12"
    if name == "levelexperiencerampoverrides":
        return (
            "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=10)\n"
            "(ExperiencePointsForLevel[0]=0,ExperiencePointsForLevel[1]=20)"
        )
    if name == "excludeitemindices":
        return "1\n2"
    if name == "modids":
        return "731604991\n895711211"
    if "classname" in name:
        return "FixtureClass_A_C\nFixtureClass_B_C"
    if name == "cheatteleportlocations":
        return '(TeleportName="FixtureA",TeleportLocation=(X=1,Y=2,Z=3))\n(TeleportName="FixtureB",TeleportLocation=(X=4,Y=5,Z=6))'
    if expected_repeated(item)[1]:
        return "[0]=1.25\n[1]=2.5"
    if item["type"] == '"<string>"':
        return "FixtureValueA\nFixtureValueB"
    return "(FixtureIndex=1)\n(FixtureIndex=2)"


def number_value(prop: dict[str, Any], integer: bool) -> int | float:
    minimum = prop.get("minimum")
    maximum = prop.get("maximum")
    if isinstance(minimum, (int, float)) and isinstance(maximum, (int, float)):
        value = (minimum + maximum) / 2
    elif isinstance(minimum, (int, float)):
        value = minimum if minimum > 0 else 1
    elif isinstance(maximum, (int, float)):
        value = min(maximum, 1)
    else:
        value = 2
    return int(value) if integer else float(value)


def setting_value(
    edition: str,
    item: dict[str, Any],
    prop: dict[str, Any],
    existing: dict[str, Any],
) -> Any:
    key = item["schema_key"]
    name = canonical(item["native_name"])
    if expected_repeated(item)[0]:
        return repeated_value(item)
    special: dict[str, Any] = {
        "admin_password": "fixture-admin-password",
        "server_password": "fixture-server-password",
        "server_name": f"LanGame {edition.upper()} Official Coverage",
        "active_event": "WinterWonderland",
        "cluster_id": f"fixture-{edition}-cluster",
        "culture": "en",
        "event_colors_chance_override": 0.5,
        "new_year1_utc": 1893456000,
        "new_year2_utc": 1893484800,
        "map_mod_id": "731604991",
        "total_conversion_mod": "731604991",
        "public_ipfor_epic": "203.0.113.10",
        "server_platform": "Steam+Xbox",
        "passive_mod_ids_csv": "927083,940786",
    }
    if key in special:
        return special[key]
    if prop.get("enum"):
        values = [value for value in prop["enum"] if value not in {"", None}]
        if values:
            return values[0]
    value_type = prop.get("type")
    if value_type == "boolean":
        return False if name == "nobattleye" else True
    if value_type == "integer":
        return number_value(prop, True)
    if value_type == "number":
        return number_value(prop, False)
    if "url" in name:
        return f"https://example.invalid/{edition}/{name}"
    if "password" in name:
        return "fixture-password"
    if "modid" in name:
        return "731604991"
    if key in existing and isinstance(existing[key], str) and existing[key].strip():
        return existing[key]
    return f"fixture-{name or key}"


def rendered_ini_fragment(item: dict[str, Any], value: Any) -> str:
    native = writer_native_name(item)
    repeated, indexed = expected_repeated(item)
    if repeated:
        lines = []
        for line in str(value).splitlines():
            if indexed:
                lines.append(line if line.startswith(native) else f"{native}{line}")
            else:
                lines.append(line if line.startswith(f"{native}=") else f"{native}={line}")
        return "\n".join(lines) + "\n"
    if isinstance(value, bool):
        text = str(value).lower()
    else:
        text = str(value)
    if canonical(item["native_name"]) in QUOTED:
        text = f'"{text}"'
    return f"{native}={text}"


def build_launch_arguments(
    edition: str,
    settings: dict[str, Any],
    items: list[dict[str, Any]],
) -> list[str]:
    max_players = settings["max_players"]
    if edition == "ase":
        url = (
            f"{settings['map_name']}?AltSaveDirectoryName=acceptance-arksurvivalevolved"
            f"?Port=7777?QueryPort=27015?MaxPlayers={max_players}"
            f"?SessionName={settings['server_name']}?RCONEnabled=true?RCONPort=27020"
        )
        for key, native in [
            ("event_colors_chance_override", "EventColorsChanceOverride"),
            ("new_year1_utc", "NewYear1UTC"),
            ("new_year2_utc", "NewYear2UTC"),
        ]:
            url += f"?{native}={settings[key]}"
        args = [
            url,
            "-NullRHI",
            "-Unattended",
            "-NoSplash",
            "-abslog={{paths.logs_dir}}/ark-evolved-server.log",
            "-ClusterDirOverride={{paths.saves_dir}}\\acceptance-arksurvivalevolved\\cluster",
        ]
    else:
        url = (
            f"{settings['map_name']}?AltSaveDirectoryName=acceptance-arksurvivalascended"
            f"?QueryPort=27015?MaxPlayers={max_players}"
            f"?SessionName={settings['server_name']}"
            "?RCONEnabled=true?RCONPort=27020"
        )
        args = [
            url,
            "-port=7777",
            f"-WinLiveMaxPlayers={max_players}",
            "-NullRHI",
            "-Unattended",
            "-NoSplash",
            "-abslog={{paths.logs_dir}}/ark-ascended-server.log",
            "-ClusterDirOverride={{paths.saves_dir}}\\acceptance-arksurvivalascended\\cluster",
            f"-mods={settings['mod_ids_csv']}",
        ]
    for item in items:
        if item["classification"] != "schema_native" or item["native_surface"] != "launch_arg":
            continue
        key = item["schema_key"]
        raw = item["raw_name"]
        value = settings[key]
        if item["native_name"] == "-NoBattlEye":
            if not value:
                args.append(item["native_name"])
        elif "=" not in raw:
            if value:
                args.append(item["native_name"])
        elif str(value).strip():
            args.append(f"{item['native_name']}={value}")
    return args


def update_classifications(fixture: dict[str, Any], ledger: dict[str, Any]) -> None:
    existing = {
        key: category
        for category, keys in fixture.get("classifications", {}).items()
        for key in keys
    }
    source_ids = set(fixture["evidence"]["source_ids"])
    fixture["evidence"]["status"] = ledger["status"]
    fixture["evidence"]["verified_at"] = "2026-07-13"
    source_ids.add("dynamic_config")
    source_ids.add("ark_official_server_configuration_2026_07_13")
    if fixture.get("module_id") == "arksurvivalascended":
        source_ids.discard("asa_server_patch_notes_v91_8")
        source_ids.add("asa_server_patch_notes_v92_36")
    fixture["evidence"]["source_ids"] = sorted(source_ids)
    rows: dict[str, str] = {}
    for row in ledger.get("items", []):
        if row["source"] in source_ids:
            qualified = f"{row['source']}.{row['key']}"
            category = existing.get(qualified, "editable")
            rows[qualified] = "specialized" if row.get("schema_key") == "cluster_directory" else category
    for row in ledger.get("exclusions", []):
        if row["source"] in source_ids:
            rows[f"{row['source']}.{row['key']}"] = "excluded"
    classifications = {
        category: sorted(key for key, value in rows.items() if value == category)
        for category in ("editable", "specialized", "derived", "generated", "excluded")
    }
    fixture["classifications"] = classifications
    fixture["coverage"] = {key: len(value) for key, value in classifications.items()}


def generate(edition: str, module_id: str, fixture_name: str, executable: str) -> None:
    module_root = ROOT / "modules" / module_id
    fixture_path = module_root / "config-fixtures" / fixture_name
    fixture = json.loads(fixture_path.read_text(encoding="utf-8-sig"))
    schema = json.loads((module_root / "schema.json").read_text(encoding="utf-8-sig"))
    ledger = tomllib.loads((module_root / "config-sources.toml").read_text(encoding="utf-8-sig"))
    items = REPORT["editions"][edition]["items"]
    settings = {
        key: value
        for key, value in fixture["settings"].items()
        if key in schema["properties"]
    }
    settings.update(
        {
            "server_name": f"LanGame {edition.upper()} Official Coverage",
            "map_name": "TheIsland" if edition == "ase" else "TheIsland_WP",
            "max_players": 70 if edition == "ase" else 30,
            "admin_password": "fixture-admin-password",
            "server_password": "fixture-server-password",
            "rcon_enabled": True,
            "cluster_id": f"fixture-{edition}-cluster",
            "cluster_directory": "",
            "custom_launch_flags": "",
            "game_user_settings_extra": "",
            "game_ini_extra": "",
        }
    )
    if edition == "ase":
        settings.update(
            {
                "active_mod_ids": "731604991,895711211",
                "auto_managed_mod_ids": "731604991\n895711211",
            }
        )
    else:
        settings.update(
            {
                "mod_ids_csv": "927083,940786",
                "use_astraeos_traversal_buff": True,
            }
        )
    for item in items:
        if item["classification"] != "schema_native":
            continue
        prop = schema["properties"][item["schema_key"]]
        settings[item["schema_key"]] = setting_value(edition, item, prop, settings)
    fixture["settings"] = settings

    gus_fragments: list[str] = []
    game_fragments: list[str] = []
    for item in items:
        if item["classification"] != "schema_native":
            continue
        surface = item["native_surface"]
        if surface.startswith("GameUserSettings.ini"):
            gus_fragments.append(rendered_ini_fragment(item, settings[item["schema_key"]]))
        elif surface == "Game.ini:[/script/shootergame.shootergamemode]":
            game_fragments.append(rendered_ini_fragment(item, settings[item["schema_key"]]))
    if edition == "ase":
        gus_fragments.append("[/Script/ShooterGame.ShooterGameUserSettings]\nVersion=5")
        gus_fragments.append("ActiveMods=731604991,895711211")
        game_fragments.append("[ModInstaller]\nModIDS=731604991\nModIDS=895711211\n")
    else:
        gus_fragments.append("ActiveMods=927083,940786")
        for key, native in ASA_PATCH_GUS_SETTINGS.items():
            value = settings[key]
            text = str(value).lower() if isinstance(value, bool) else str(value)
            gus_fragments.append(f"{native}={text}")
    gus_fragments.extend(["RCONPort=27020", "Port=7777", "QueryPort=27015"])
    fixture["expected"]["files"] = [
        {
            "root": "config",
            "path": "GameUserSettings.ini",
            "format": "ini",
            "fragments": gus_fragments,
        },
        {
            "root": "config",
            "path": "Game.ini",
            "format": "ini",
            "fragments": game_fragments,
        },
    ]
    native_gus_fragments = [
        f"ServerAdminPassword={settings['admin_password']}",
        f"ServerPassword={settings['server_password']}",
    ]
    if edition == "ase":
        native_gus_fragments.extend([
            "RCONEnabled=true",
            "RCONPort=27020",
            "[/Script/ShooterGame.ShooterGameUserSettings]\nVersion=5",
        ])
    fixture["expected"]["files"].append(
        {
            "root": "install",
            "path": "ShooterGame/Saved/Config/WindowsServer/GameUserSettings.ini",
            "format": "ini",
            "fragments": native_gus_fragments,
        }
    )
    fixture["expected"]["launch"] = {
        "executable_suffix": executable,
        "arguments": build_launch_arguments(edition, settings, items),
    }
    update_classifications(fixture, ledger)
    fixture_path.write_text(
        json.dumps(fixture, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    for edition, args in EDITIONS.items():
        generate(edition, *args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
