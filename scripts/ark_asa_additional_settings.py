"""ASA writer mappings verified independently of the pinned July wiki crosswalk.

The historical crosswalk remains unchanged. Current support and its evidence are
recorded under a separate module source and materialization fixture.
"""

from __future__ import annotations

from typing import Any


ASA_ADDITIONAL_SOURCE = "asa_advanced_game_ini"
ASA_ADDITIONAL_ARRAY = "ARK_ASA_ADVANCED_GAME_INI"
ASA_ADDITIONAL_SECTION = "/script/shootergame.shootergamemode"
ASA_ADDITIONAL_ROWS = [
    ("SupplyCrateLootQualityMultiplier", "supply_crate_loot_quality_multiplier", False, False, False),
    ("OverrideMaxExperiencePointsPlayer", "override_max_experience_points_player", False, False, False),
    ("OverrideMaxExperiencePointsDino", "override_max_experience_points_dino", False, False, False),
    ("bAutoUnlockAllEngrams", "auto_unlock_all_engrams", False, False, False),
    ("ConfigOverrideSupplyCrateItems", "config_override_supply_crate_items", True, False, False),
    ("ConfigOverrideItemCraftingCosts", "config_override_item_crafting_costs", True, False, False),
    ("ConfigOverrideItemMaxQuantity", "config_override_item_max_quantity", True, False, False),
    ("ConfigOverrideNPCSpawnEntriesContainer", "config_override_npc_spawn_entries_container", True, False, False),
    ("ConfigSubtractNPCSpawnEntriesContainer", "config_subtract_npc_spawn_entries_container", True, False, False),
    ("DinoSpawnWeightMultipliers", "dino_spawn_weight_multipliers", True, False, False),
    ("NPCReplacements", "npc_replacements", True, False, False),
    ("OverridePlayerLevelEngramPoints", "override_player_level_engram_points", True, False, False),
    ("EngramEntryAutoUnlocks", "engram_entry_auto_unlocks", True, False, False),
    ("PerLevelStatsMultiplier_DinoTamed", "per_level_stats_multiplier_dino_tamed_type_integer", True, True, False),
    ("PerLevelStatsMultiplier_DinoWild", "per_level_stats_multiplier_dino_wild_integer", True, True, False),
]
ASA_ADDITIONAL_KEYS = {row[1] for row in ASA_ADDITIONAL_ROWS}
ASA_INDEXED_SOURCE_SUFFIXES = {
    "PerLevelStatsMultiplier_DinoTamed": "<_type>[<integer>]",
    "PerLevelStatsMultiplier_DinoWild": "[<integer>]",
}


def additional_rust_tables() -> dict[str, list[str]]:
    import json

    return {ASA_ADDITIONAL_ARRAY: [
        "    (" + ", ".join(json.dumps(value) for value in row) + "),"
        for row in ASA_ADDITIONAL_ROWS
    ]}


def verify_asa_additional_settings(contract: dict[str, Any], failures: list[str]) -> None:
    """Require exact schema, provenance, writer and output coverage for additions."""
    label = f"asa:{ASA_ADDITIONAL_SOURCE}"
    ledger = contract["ledger"]
    sources = [row for row in ledger.get("sources", []) if row.get("id") == ASA_ADDITIONAL_SOURCE]
    if len(sources) != 1 or not all(sources[0].get(key) for key in ("authority", "url", "description")):
        failures.append(f"{label}: expected one independently documented evidence source")
    arrays = contract["ini_arrays"]
    if arrays.get(ASA_ADDITIONAL_ARRAY) != ASA_ADDITIONAL_ROWS:
        failures.append(f"{label}: exact supplemental writer mismatch")
    if ASA_ADDITIONAL_ARRAY not in contract["storage_logic"]:
        failures.append(f"{label}: supplemental writer is not reachable from storage dispatcher")
    if "{{arksa.additional_game_ini}}" not in contract["templates"]:
        failures.append(f"{label}: Game.ini aggregate token is missing")

    schema = contract["schema"]["properties"]
    actual_keys = {key for key, prop in schema.items() if prop.get("x-lsgm-source") == ASA_ADDITIONAL_SOURCE}
    if actual_keys != ASA_ADDITIONAL_KEYS:
        failures.append(f"{label}: supplemental schema inventory differs from verified keys")
    mappings = [row for row in ledger.get("items", []) if row.get("source") == ASA_ADDITIONAL_SOURCE]
    if len(mappings) != len(ASA_ADDITIONAL_ROWS):
        failures.append(f"{label}: supplemental ledger inventory differs from verified keys")
    fixtures = [fixture for fixture in contract["fixtures"]
                if ASA_ADDITIONAL_SOURCE in fixture.get("evidence", {}).get("source_ids", [])]
    if not fixtures:
        failures.append(f"{label}: independent materialization fixture is missing")

    for native, key, repeated, indexed, _quoted in ASA_ADDITIONAL_ROWS:
        prop = schema.get(key, {})
        source_key = str(prop.get("x-lsgm-source-key", ""))
        expected_source_key = f"{ASA_ADDITIONAL_SECTION}.{native}{ASA_INDEXED_SOURCE_SUFFIXES.get(native, '')}"
        if (prop.get("x-lsgm-source") != ASA_ADDITIONAL_SOURCE
                or prop.get("x-lsgm-source-surface") != "config_file"
                or source_key != expected_source_key
                or (repeated and (prop.get("type") != "string" or prop.get("format") != "textarea"))):
            failures.append(f"{label}:{key}: schema source, surface or repeated-line editor is incorrect")
        matches = [row for row in mappings if row.get("schema_key") == key
                   and row.get("key") == source_key and row.get("surface") == "config_file"]
        if len(matches) != 1:
            failures.append(f"{label}:{key}: expected one matching supplemental ledger mapping")
        writers = [(array, row) for array, rows in arrays.items() for row in rows if row[1] == key]
        if len(writers) != 1 or writers[0][0] != ASA_ADDITIONAL_ARRAY:
            failures.append(f"{label}:{key}: duplicate or misplaced writer")
        prefixes = (f"{native}=", f"{native}[", f"{native}_") if indexed else (f"{native}=",)
        exercised = any(
            key in fixture.get("settings", {})
            and any(line.startswith(prefixes)
                    for file in fixture.get("expected", {}).get("files", [])
                    if file.get("root") == "config" and file.get("path") == "Game.ini"
                    for fragment in file.get("fragments", []) for line in fragment.splitlines())
            for fixture in fixtures
        )
        if not exercised:
            failures.append(f"{label}:{key}: independent fixture omits native Game.ini output")
