#!/usr/bin/env python3
"""Mechanically apply the pinned ARK crosswalk to schema, ledgers, UI and writers."""

from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path
from typing import Any

try:
    from .ark_asa_additional_settings import ASA_ADDITIONAL_KEYS, additional_rust_tables
    from .ark_official_apply_support import (
        apply_project_descriptions,
        canonical,
        indexed_repeated,
        ledger_identity,
        property_for,
        prune_group_keys,
        prune_ledger_items,
        repeated,
        source_parts,
        writer_native_key,
    )
except ImportError:
    from ark_asa_additional_settings import ASA_ADDITIONAL_KEYS, additional_rust_tables
    from ark_official_apply_support import (
        apply_project_descriptions,
        canonical,
        indexed_repeated,
        ledger_identity,
        property_for,
        prune_group_keys,
        prune_ledger_items,
        repeated,
        source_parts,
        writer_native_key,
    )

ROOT = Path(__file__).resolve().parents[1]
CROSSWALK = ROOT / "docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json"
MODULES = {"ase": "arksurvivalevolved", "asa": "arksurvivalascended"}
ARK_SERVER_URL_SAFE_PATTERN = r"^[^?\r\n]*$"
ARK_SERVER_URL_STRING_KEYS = {
    "ase": ("map_name",),
    "asa": ("map_name", "server_name"),
}
ARK_INI_PASSWORD_PATTERN = r"^[^\r\n]*$"
ARK_ASA_INI_PASSWORD_KEYS = ("server_password", "admin_password")
ARK_CLUSTER_ARGUMENT_PATTERN = r'^[^"\u0000-\u001f\u007f-\u009f\u2028\u2029]*$'
TS_FILES = {
    "ase": ROOT / "apps/desktop/src/views/settings/modules/ark-ase.ts",
    "asa": ROOT / "apps/desktop/src/views/settings/modules/ark-asa.ts",
}
INVENTORY_FILES = {
    "ase": ROOT / "apps/desktop/src/views/settings/modules/ark-ase-official-inventory.ts",
    "asa": ROOT / "apps/desktop/src/views/settings/modules/ark-asa-official-inventory.ts",
}
STORAGE_DATA_FILES = {
    edition: ROOT / f"crates/app-storage/src/templates_render_ark_{edition}.rs"
    for edition in MODULES
}
RUNTIME_DATA_FILES = {
    edition: ROOT / f"crates/app-runtime/src/launch_templates_ark_{edition}_data.rs"
    for edition in MODULES
}

KEY_OVERRIDES = {
    "serveradminpassword": "admin_password",
    "serverpassword": "server_password",
    "serverpve": "server_pve",
    "sessionname": "server_name",
    "maxplayers": "max_players",
    "nobattleye": "battleye_enabled",
    "exclusivejoin": "exclusive_join_enabled",
    "serverrconoutputtribelogs": "server_game_log_include_tribe_logs",
    "allowflyerspeedleveling": "b_allow_flyer_speed_leveling",
    "activemods": "active_mod_ids",
    "mods": "mod_ids_csv",
    "modids": "auto_managed_mod_ids",
    "gamemodids": "active_mod_ids",
    "mapplayerlocation": "show_map_player_location",
    "pvedisallowtribewar": "b_pv_eallow_tribe_war",
    "pveallowtribewar": "b_pv_eallow_tribe_war",
}

QUOTED_INI_NAMES = {
    "banlisturl",
    "customdynamicconfigurl",
    "customlivetuningurl",
}

EDITION_SCHEMA_EXTRAS = {
    "ase": {
        "admin_account_ids",
        "cluster_directory",
        "custom_launch_flags",
        "exclusive_join_list",
        "game_ini_extra",
        "game_user_settings_extra",
        "map_name",
        "priority_join_list",
    },
    "asa": {
        "admin_account_ids",
        "cluster_directory",
        "anchored_vessel_check_radius",
        "custom_launch_flags",
        "exclusive_join_list",
        "game_ini_extra",
        "game_user_settings_extra",
        "map_name",
        "max_anchored_vessels_in_range",
        "max_players",
        "needs_power_to_activate_aquatic_compartments",
        "prevent_template_on_saddle",
        "priority_join_list",
        "rcon_enabled",
        "use_astraeos_traversal_buff",
    },
}


def resolve_keys(items: list[dict[str, Any]], schema: dict[str, Any]) -> None:
    index: dict[str, set[str]] = {}
    for key, prop in schema["properties"].items():
        for candidate in (key, str(prop.get("x-lsgm-source-key", ""))):
            if candidate:
                index.setdefault(canonical(candidate), set()).add(key)
    for item in items:
        name = canonical(item["native_name"])
        override = KEY_OVERRIDES.get(name)
        matches = index.get(name, set())
        if override:
            item["schema_key"] = override
        elif len(matches) == 1:
            item["schema_key"] = next(iter(matches))


def rust_ini_tables(edition: str, items: list[dict[str, Any]]) -> dict[str, list[str]]:
    prefix = "ARK_ASE" if edition == "ase" else "ARK_ASA"
    tables: dict[str, list[str]] = {}
    suffixes = {
        "GameUserSettings.ini:[ServerSettings]": "GUS_SERVER_SETTINGS",
        "GameUserSettings.ini:[SessionSettings]": "GUS_SESSION_SETTINGS",
        "GameUserSettings.ini:[/Script/Engine.GameSession]": "GUS_ENGINE_SESSION",
        "GameUserSettings.ini:[Ragnarok]": "GUS_RAGNAROK",
        "GameUserSettings.ini:[MessageOfTheDay]": "GUS_MOTD",
        "Game.ini:[/script/shootergame.shootergamemode]": "GAME_INI",
    }
    for surface, suffix in suffixes.items():
        rows = [item for item in items if item.get("native_surface") == surface]
        tables[f"{prefix}_{suffix}"] = [
            "    ("
            f'{json.dumps(writer_native_key(item))}, {json.dumps(item["schema_key"])}, '
            f'{str(repeated(item)).lower()}, {str(indexed_repeated(item)).lower()}, '
            f'{str(canonical(item["native_name"]) in QUOTED_INI_NAMES).lower()}'
            "),"
            for item in rows
        ]
    return tables


def rust_extra_ini_tables(
    edition: str, schema: dict[str, Any]
) -> dict[str, list[str]]:
    prefix = f"ARK_{edition.upper()}"
    rows: list[str] = []
    for key in sorted(EDITION_SCHEMA_EXTRAS[edition]):
        prop = schema["properties"].get(key, {})
        if prop.get("x-lsgm-source") not in {"game_user_settings", "asa_server_patch_notes_v93_19"}:
            continue
        source_key = str(prop.get("x-lsgm-source-key", ""))
        if not source_key.startswith("ServerSettings."):
            continue
        native = source_key.split(".", 1)[1]
        rows.append(
            f"    ({json.dumps(native)}, {json.dumps(key)}, false, false, false),"
        )
    if not rows:
        return {}
    return {f"{prefix}_PATCH_GUS_SERVER_SETTINGS": rows}


def rust_data_file(tables: dict[str, list[str]], type_name: str) -> str:
    blocks = ["use super::*;", ""]
    for name, rows in tables.items():
        if not rows:
            continue
        blocks.extend(
            ["#[rustfmt::skip]", f"pub(crate) const {name}: &[{type_name}] = &[", *rows, "];", ""]
        )
    return "\n".join(blocks)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--descriptions-only", action="store_true",
                        help="Refresh project-authored schema help without changing native contracts or writers.")
    args = parser.parse_args()
    report = json.loads(CROSSWALK.read_text(encoding="utf-8"))
    all_tables: dict[str, list[str]] = {}
    launch_tables: dict[str, list[str]] = {}
    for edition, module_id in MODULES.items():
        module_root = ROOT / "modules" / module_id
        schema_path = module_root / "schema.json"
        schema = json.loads(schema_path.read_text(encoding="utf-8-sig"))
        if args.descriptions_only:
            apply_project_descriptions(schema, module_id)
            schema_path.write_text(json.dumps(schema, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            continue
        items = report["editions"][edition]["items"]
        resolve_keys(items, schema)
        if edition == "asa":
            for item in items:
                if canonical(item["native_name"]) == "activemods":
                    item["schema_key"] = "mod_ids_csv"
        for item in items:
            if item["classification"] != "schema_native":
                continue
            prop = schema["properties"].get(item.get("schema_key"))
            if not isinstance(prop, dict):
                continue
            source, key, surface = source_parts(item)
            prop.update(
                {
                    "x-lsgm-source": source,
                    "x-lsgm-source-key": key,
                    "x-lsgm-source-surface": surface,
                }
            )
        if edition == "asa":
            schema["properties"]["max_players"].update(
                {
                    "x-lsgm-source": "game_user_settings",
                    "x-lsgm-source-key": "SessionSettings.MaxPlayers",
                    "x-lsgm-source-surface": "config_file",
                }
            )
            schema["properties"]["rcon_enabled"].update(
                {
                    "x-lsgm-source": "launch_args",
                    "x-lsgm-source-key": "server_url.RCONEnabled",
                    "x-lsgm-source-surface": "launch_arg",
                }
            )
        official_keys = {
            str(item["schema_key"])
            for item in items
            if item.get("schema_key")
        }
        allowed_keys = official_keys | EDITION_SCHEMA_EXTRAS[edition]
        if edition == "asa":
            allowed_keys |= ASA_ADDITIONAL_KEYS
        removed_keys = set(schema["properties"]) - allowed_keys
        for key in removed_keys:
            schema["properties"].pop(key, None)
        ts_path = TS_FILES[edition]
        ts_text = ts_path.read_text(encoding="utf-8")
        constant_name = f"ARK_{edition.upper()}_OFFICIAL_SCHEMA_KEYS"
        inventory_path = INVENTORY_FILES[edition]
        inventory_text = inventory_path.read_text(encoding="utf-8") if inventory_path.exists() else ""
        existing_block = re.search(
            rf"(?:export )?const {constant_name} = \[(.*?)\] as const;",
            inventory_text or ts_text,
            re.DOTALL,
        )
        generated_keys = set(
            re.findall(r'"([^"]+)"', existing_block.group(1)) if existing_block else []
        )
        current_native_keys = {
            item["schema_key"]
            for item in items
            if item["classification"] == "schema_native"
        }
        stale_keys = generated_keys - current_native_keys
        generated_keys -= stale_keys
        for key in stale_keys:
            schema["properties"].pop(key, None)
        next_order = max(int(prop.get("x-lsgm-order", 0)) for prop in schema["properties"].values()) + 1
        added: list[str] = []
        for item in items:
            if item["classification"] != "schema_native":
                continue
            key = item["schema_key"]
            if key not in schema["properties"]:
                schema["properties"][key] = property_for(item, next_order, module_id=module_id)
                next_order += 1
                added.append(key)
                generated_keys.add(key)

        item_index = {
            canonical(item["native_name"]): item
            for item in items
            if item["classification"] == "schema_native"
        }
        for key in generated_keys:
            matching = [item for item in item_index.values() if item.get("schema_key") == key]
            if len(matching) != 1:
                raise RuntimeError(f"cannot restore generated {edition} schema key {key!r}")
            existing_property = schema["properties"].get(key, {})
            existing_order = int(existing_property.get("x-lsgm-order", next_order))
            # Native metadata refreshes must preserve the reviewed UI taxonomy.
            schema["properties"][key] = property_for(
                matching[0], existing_order, section=existing_property.get("x-lsgm-section"),
                description=existing_property.get("description"),
                module_id=module_id,
            )
            if existing_order == next_order:
                next_order += 1

        # The wiki leaves these value-taking launch options untyped. Keep their
        # established manager contracts instead of treating them as bare flags.
        active_event = schema["properties"]["active_event"]
        active_event["type"] = "string"
        active_event["default"] = ""
        active_event.pop("x-lsgm-default-source", None)
        battleye = schema["properties"]["battleye_enabled"]
        battleye["type"] = "boolean"
        battleye["default"] = True
        battleye.pop("x-lsgm-default-source", None)
        schema["properties"]["server_name"]["x-lsgm-default-source"] = "instance_name"
        schema["properties"]["cluster_id"].update(
            pattern=ARK_CLUSTER_ARGUMENT_PATTERN,
            maxLength=128,
            description="Maps in one cluster must use the same ID and shared cluster directory. Setting only the ID keeps each instance's uploads separate.",
        )
        for key in ARK_SERVER_URL_STRING_KEYS[edition]:
            schema["properties"][key]["pattern"] = ARK_SERVER_URL_SAFE_PATTERN
        if edition == "asa":
            for key in ARK_ASA_INI_PASSWORD_KEYS:
                schema["properties"][key]["pattern"] = ARK_INI_PASSWORD_PATTERN
            schema["properties"]["per_level_stats_multiplier_player_integer"]["x-lsgm-section"] = "leveling"
            mod_ids = schema["properties"]["mod_ids_csv"]
            mod_ids["type"] = "string"
            mod_ids["default"] = ""
            mod_ids.pop("x-lsgm-default-source", None)
            passive_mod_ids = schema["properties"]["passive_mod_ids_csv"]
            passive_mod_ids["type"] = "string"
            passive_mod_ids["format"] = "textarea"
            passive_mod_ids["default"] = ""
            passive_mod_ids.pop("x-lsgm-default-source", None)

        # These repeatable rows predate the generated inventory but still need
        # native repeated-line semantics in the aggregate writer.
        for item in items:
            if item["classification"] != "schema_native" or not repeated(item):
                continue
            key = item["schema_key"]
            prop = schema["properties"][key]
            prop["type"] = "string"
            prop["format"] = "textarea"
            prop["default"] = ""
        apply_project_descriptions(schema, module_id)
        schema_path.write_text(json.dumps(schema, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

        marker = f"const {constant_name} = ["
        block = f"export const {constant_name} = [\n" + "\n".join(f'  "{key}",' for key in sorted(generated_keys)) + "\n] as const;\n"
        inventory_path.write_text(block, encoding="utf-8")
        ts_text = re.sub(rf"{re.escape(marker)}.*?\] as const;\s*", "", ts_text, flags=re.DOTALL)
        import_line = f'import {{ {constant_name} }} from "./ark-{edition}-official-inventory";\n'
        # The inventory is an audit artifact; semantic groups own UI placement.
        ts_text = ts_text.replace(import_line, "")
        ts_path.write_text(ts_text, encoding="utf-8")
        for group_path in sorted(ts_path.parent.glob(f"ark-{edition}-groups-*.ts")):
            group_path.write_text(
                prune_group_keys(group_path.read_text(encoding="utf-8"), removed_keys),
                encoding="utf-8",
            )

        schema_native_items = [
            item for item in items if item["classification"] == "schema_native"
        ]
        all_tables.update(rust_ini_tables(edition, schema_native_items))
        all_tables.update(rust_extra_ini_tables(edition, schema))
        if edition == "asa":
            all_tables.update(additional_rust_tables())
        launch_rows = []
        for item in schema_native_items:
            if item.get("native_surface") != "launch_arg" or item["classification"] != "schema_native":
                continue
            if item["native_name"] == "-NoBattlEye":
                kind = "InvertedFlag"
            elif edition == "asa" and item["schema_key"] == "passive_mod_ids_csv":
                kind = "ModIds"
            else:
                kind = "Flag" if "=" not in item["raw_name"] else "Value"
            launch_rows.append(
                f'    ({json.dumps(item["native_name"])}, {json.dumps(item["schema_key"])}, ArkLaunchSettingKind::{kind}),'
            )
        launch_tables[f"ARK_{edition.upper()}_ADDITIONAL_LAUNCH_SETTINGS"] = launch_rows

        ledger_path = module_root / "config-sources.toml"
        rejected_identities = {
            ledger_identity(source_parts(item)[0], source_parts(item)[1])
            for item in report["editions"][edition]["edition_no_unknown_items"]
        }
        ledger_text = prune_ledger_items(
            ledger_path.read_text(encoding="utf-8-sig"),
            removed_keys,
            rejected_identities,
        )
        for stale_key in stale_keys:
            ledger_text = re.sub(
                rf'\n\[\[items\]\]\n(?:(?!\n\[\[).)*?schema_key = {re.escape(json.dumps(stale_key))}\n(?:(?!\n\[\[).)*',
                "\n",
                ledger_text,
                flags=re.DOTALL,
            )
        ledger = tomllib.loads(ledger_text)
        known = {
            ledger_identity(entry["source"], entry["key"])
            for entry in ledger.get("items", [])
        }
        known_exclusions = {
            ledger_identity(entry["source"], entry["key"])
            for entry in ledger.get("exclusions", [])
        }
        additions = []
        for item in items:
            if item["classification"] != "schema_native":
                continue
            source, key, surface = source_parts(item)
            identity = ledger_identity(source, key)
            if identity in known:
                continue
            known.add(identity)
            additions.append(
                f'\n[[items]]\nsource = {json.dumps(source)}\nkey = {json.dumps(key)}\nschema_key = {json.dumps(item["schema_key"])}\nsurface = {json.dumps(surface)}\n'
            )
        for item in items:
            classification = item["classification"]
            if classification == "schema_native":
                continue
            source, key, surface = source_parts(item)
            identity = ledger_identity(source, key)
            if classification in {"specialized", "derived"} and item.get("schema_key"):
                if identity in known:
                    continue
                known.add(identity)
                additions.append(
                    f'\n[[items]]\nsource = {json.dumps(source)}\nkey = {json.dumps(key)}\n'
                    f'schema_key = {json.dumps(item["schema_key"])}\nsurface = {json.dumps(surface)}\n'
                )
            elif identity not in known_exclusions:
                known_exclusions.add(identity)
                additions.append(
                    f'\n[[exclusions]]\nsource = {json.dumps(source)}\nkey = {json.dumps(key)}\n'
                    f'reason = {json.dumps(item["reason"])}\n'
                )
        for item in report["editions"][edition]["edition_no_unknown_items"]:
            source, key, _surface = source_parts(item)
            identity = ledger_identity(source, key)
            if identity in known_exclusions:
                continue
            known_exclusions.add(identity)
            additions.append(
                f'\n[[exclusions]]\nsource = {json.dumps(source)}\nkey = {json.dumps(key)}\n'
                f'reason = {json.dumps(item["reason"])}\n'
            )
        if 'id = "dynamic_config"' not in ledger_text:
            additions.insert(
                0,
                '\n[[sources]]\nid = "dynamic_config"\nkind = "documentation"\n'
                'path = "HTTP DynamicConfig document"\n'
                'authority = "ark_official_community_wiki_server_configuration"\n'
                'description = "Externally hosted optional DynamicConfig mirror; audited but not a stable local materialization target."\n'
                'url = "https://ark.wiki.gg/wiki/Server_configuration"\n',
            )
        ledger_path.write_text(ledger_text + "".join(additions), encoding="utf-8")

        edition_tables = {
            name: rows
            for name, rows in all_tables.items()
            if name.startswith(f"ARK_{edition.upper()}_")
        }
        STORAGE_DATA_FILES[edition].write_text(
            rust_data_file(edition_tables, "ArkIniSetting"), encoding="utf-8"
        )
        launch_table = {
            name: rows
            for name, rows in launch_tables.items()
            if name.startswith(f"ARK_{edition.upper()}_")
        }
        RUNTIME_DATA_FILES[edition].write_text(
            rust_data_file(launch_table, "ArkLaunchSetting"), encoding="utf-8"
        )

    if not args.descriptions_only:
        CROSSWALK.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
