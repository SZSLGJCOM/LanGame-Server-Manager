from __future__ import annotations

import json
import re


ARK_AGGREGATE_TOKEN_INVENTORIES: dict[tuple[str, str], tuple[str, str] | set[str]] = {
    ("arkse", "active_mods_ini_line"): {"active_mod_ids"},
    ("arkse", "additional_gus_server_settings"): ("storage", "ARK_ASE_GUS_SERVER_SETTINGS"),
    ("arkse", "additional_gus_session_settings"): ("storage", "ARK_ASE_GUS_SESSION_SETTINGS"),
    ("arkse", "additional_gus_engine_session"): ("storage", "ARK_ASE_GUS_ENGINE_SESSION"),
    ("arkse", "additional_gus_ragnarok"): ("storage", "ARK_ASE_GUS_RAGNAROK"),
    ("arkse", "additional_gus_motd"): ("storage", "ARK_ASE_GUS_MOTD"),
    ("arkse", "additional_game_ini"): ("storage", "ARK_ASE_GAME_INI"),
    ("arkse", "mod_installer_section"): {"auto_managed_mod_ids"},
    ("arkse", "multihome_ini_line"): set(),
    ("arkse", "official_launch_flags"): ("runtime", "ARK_ASE_ADDITIONAL_LAUNCH_SETTINGS"),
    ("arksa", "active_mods_ini_line"): {"mod_ids_csv"},
    ("arksa", "additional_gus_server_settings"): ("storage", "ARK_ASA_GUS_SERVER_SETTINGS"),
    ("arksa", "additional_gus_session_settings"): ("storage", "ARK_ASA_GUS_SESSION_SETTINGS"),
    ("arksa", "additional_gus_engine_session"): set(),
    ("arksa", "additional_gus_motd"): ("storage", "ARK_ASA_GUS_MOTD"),
    ("arksa", "additional_game_ini"): ("storage", "ARK_ASA_GAME_INI"),
    ("arksa", "multihome_ini_line"): set(),
    ("arksa", "official_launch_flags"): ("runtime", "ARK_ASA_ADDITIONAL_LAUNCH_SETTINGS"),
}


def rust_tuple_array_setting_keys(source: str, const_name: str) -> set[str] | None:
    match = re.search(
        rf"\bconst\s+{re.escape(const_name)}\s*:[^=]+?=\s*&\[(.*?)\n\];",
        source,
        re.DOTALL,
    )
    if match is None:
        return None

    keys: set[str] = set()
    for row in re.finditer(
        r'^\s*\(\s*"((?:\\.|[^"\\])*)"\s*,\s*"((?:\\.|[^"\\])*)"\s*,',
        match.group(1),
        re.MULTILINE,
    ):
        keys.add(json.loads(f'"{row.group(2)}"'))
    return keys


def ark_aggregate_token_settings(
    prefix: str,
    token: str,
    storage_source: str,
    runtime_source: str,
) -> set[str] | None:
    inventory = ARK_AGGREGATE_TOKEN_INVENTORIES.get((prefix, token))
    if inventory is None:
        return None
    if isinstance(inventory, set):
        return set(inventory)

    source_kind, const_name = inventory
    source = storage_source if source_kind == "storage" else runtime_source
    keys = rust_tuple_array_setting_keys(source, const_name)
    if keys is not None and (prefix, token) == ("arksa", "additional_gus_server_settings"):
        patch_keys = rust_tuple_array_setting_keys(
            storage_source, "ARK_ASA_PATCH_GUS_SERVER_SETTINGS"
        )
        if patch_keys is None:
            return None
        keys.update(patch_keys)
    if keys is not None and (prefix, token) == ("arksa", "additional_game_ini"):
        additional_keys = rust_tuple_array_setting_keys(storage_source, "ARK_ASA_ADVANCED_GAME_INI")
        if additional_keys is None:
            return None
        keys.update(additional_keys)
    return keys
