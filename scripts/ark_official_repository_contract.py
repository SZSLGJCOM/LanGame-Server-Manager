"""Mechanical repository contract for the pinned ARK official crosswalk."""

from __future__ import annotations

import copy
import json
import re
import tomllib
from pathlib import Path
from typing import Any, Callable

try:
    from .ark_asa_additional_settings import verify_asa_additional_settings
except ImportError:
    from ark_asa_additional_settings import verify_asa_additional_settings


MODULES = {"ase": "arksurvivalevolved", "asa": "arksurvivalascended"}
NAMESPACES = {"ase": "arkse", "asa": "arksa"}
ARK_SERVER_URL_SAFE_PATTERN = r"^[^?\r\n]*$"
ARK_SERVER_URL_STRING_KEYS = {
    "ase": ("map_name",),
    "asa": ("map_name", "server_name"),
}
ARK_INI_PASSWORD_PATTERN = r"^[^\r\n]*$"
ARK_ASA_INI_PASSWORD_KEYS = ("server_password", "admin_password")
INI_ARRAYS = {
    "GameUserSettings.ini:[ServerSettings]": "GUS_SERVER_SETTINGS",
    "GameUserSettings.ini:[SessionSettings]": "GUS_SESSION_SETTINGS",
    "GameUserSettings.ini:[/Script/Engine.GameSession]": "GUS_ENGINE_SESSION",
    "GameUserSettings.ini:[Ragnarok]": "GUS_RAGNAROK",
    "GameUserSettings.ini:[MessageOfTheDay]": "GUS_MOTD",
    "Game.ini:[/script/shootergame.shootergamemode]": "GAME_INI",
}
INI_TOKENS = {
    "GameUserSettings.ini:[ServerSettings]": "additional_gus_server_settings",
    "GameUserSettings.ini:[SessionSettings]": "additional_gus_session_settings",
    "GameUserSettings.ini:[/Script/Engine.GameSession]": "additional_gus_engine_session",
    "GameUserSettings.ini:[Ragnarok]": "additional_gus_ragnarok",
    "GameUserSettings.ini:[MessageOfTheDay]": "additional_gus_motd",
    "Game.ini:[/script/shootergame.shootergamemode]": "additional_game_ini",
}
ASA_PATCH_GUS_ROWS = [
    ("AnchoredVesselCheckRadius", "anchored_vessel_check_radius", False, False, False),
    ("MaxAnchoredVesselsInRange", "max_anchored_vessels_in_range", False, False, False),
    (
        "NeedsPowerToActivateAquaticCompartments",
        "needs_power_to_activate_aquatic_compartments",
        False,
        False,
        False,
    ),
    ("PreventTemplateOnSaddle", "prevent_template_on_saddle", False, False, False),
    ("UseAstraeosTraversalBuff", "use_astraeos_traversal_buff", False, False, False),
]


Mutation = Callable[[str, dict[str, Any]], None]


def normalized(value: str) -> str:
    return re.sub(r"[^a-z0-9]+", "", value.split(".")[-1].lstrip("-?").lower())


def ledger_row_matches(row: dict[str, Any], item: dict[str, Any], source: str, native_key: str) -> bool:
    if row.get("source") != source:
        return False
    candidate = str(row.get("key", ""))
    surface = str(item["native_surface"])
    if surface == "launch_arg":
        return candidate.lstrip().startswith("-") and normalized(candidate) == normalized(native_key)
    if surface == "server_url":
        is_url_key = candidate.startswith("?") or candidate.lower().startswith("server_url.")
        return is_url_key and normalized(candidate) == normalized(native_key)
    if surface == "external_dynamic_config":
        return normalized(candidate) == normalized(native_key)
    full = lambda value: re.sub(r"[^a-z0-9]+", "", value.lower())
    return full(candidate) == full(native_key)


def writer_native_name(item: dict[str, Any]) -> str:
    name = str(item["native_name"])
    name = re.sub(r"\[<[^>]+>\]", "", name)
    return name.replace("<_type>", "")


def ledger_identity(item: dict[str, Any]) -> tuple[str, str]:
    surface = str(item["native_surface"])
    native = str(item["native_name"])
    if surface in {"launch_arg", "server_url"}:
        return "launch_args", native
    if surface.startswith("GameUserSettings.ini"):
        section = surface.split(":", 1)[1].strip("[]")
        return "game_user_settings", f"{section}.{native}"
    if surface.startswith("Game.ini"):
        section = surface.split(":", 1)[1].strip("[]")
        return "game_ini", f"{section}.{native}"
    if surface == "external_dynamic_config":
        return "dynamic_config", native
    raise ValueError(surface)


def parse_ini_rows(source: str) -> dict[str, list[tuple[str, str, bool, bool, bool]]]:
    arrays: dict[str, list[tuple[str, str, bool, bool, bool]]] = {}
    for name, body in re.findall(
        r"pub\((?:super|crate)\) const (ARK_[A-Z_]+): &\[ArkIniSetting\] = &\[(.*?)\n\];",
        source,
        re.DOTALL,
    ):
        arrays[name] = [
            (json.loads(native), json.loads(key), repeated == "true", indexed == "true", quoted == "true")
            for native, key, repeated, indexed, quoted in re.findall(
                r"\(\s*(\"(?:\\.|[^\"])*\")\s*,\s*(\"(?:\\.|[^\"])*\")\s*,\s*(true|false)\s*,\s*(true|false)\s*,\s*(true|false)\s*\)",
                body,
            )
        ]
    return arrays


def parse_launch_rows(source: str) -> dict[str, list[tuple[str, str, str]]]:
    arrays: dict[str, list[tuple[str, str, str]]] = {}
    for name, body in re.findall(
        r"pub\((?:super|crate)\) const (ARK_[A-Z_]+): &\[ArkLaunchSetting\] = &\[(.*?)\n\];",
        source,
        re.DOTALL,
    ):
        arrays[name] = [
            (json.loads(flag), json.loads(key), kind)
            for flag, key, kind in re.findall(
                r"\(\s*(\"(?:\\.|[^\"])*\")\s*,\s*(\"(?:\\.|[^\"])*\")\s*,\s*ArkLaunchSettingKind::(Flag|InvertedFlag|Value|ModIds)\s*\)",
                body,
            )
        ]
    return arrays


def load_contract(root: Path, edition: str) -> dict[str, Any]:
    module_root = root / "modules" / MODULES[edition]
    storage_data = root / f"crates/app-storage/src/templates_render_ark_{edition}.rs"
    runtime_data = root / f"crates/app-runtime/src/launch_templates_ark_{edition}_data.rs"
    runtime_logic = root / f"crates/app-runtime/src/launch_templates_ark_{edition}.rs"
    fixtures = sorted((module_root / "config-fixtures").glob("*.json"))
    return {
        "schema": json.loads((module_root / "schema.json").read_text(encoding="utf-8-sig")),
        "ledger": tomllib.loads((module_root / "config-sources.toml").read_text(encoding="utf-8-sig")),
        "manifest": (module_root / "module.toml").read_text(encoding="utf-8-sig"),
        "templates": "\n".join(
            path.read_text(encoding="utf-8-sig")
            for path in sorted((module_root / "templates").glob("*.hbs"))
        ),
        "ini_arrays": parse_ini_rows(storage_data.read_text(encoding="utf-8")),
        "launch_arrays": parse_launch_rows(runtime_data.read_text(encoding="utf-8")),
        "runtime_logic": "\n".join(
            (
                runtime_logic.read_text(encoding="utf-8"),
                (root / "crates/app-runtime/src/launch_templates_ark.rs").read_text(
                    encoding="utf-8"
                ),
            )
        ),
        "storage_logic": "\n".join(
            (root / relative).read_text(encoding="utf-8")
            for relative in (
                "crates/app-storage/src/templates.rs",
                "crates/app-storage/src/templates_render_ark.rs",
            )
        ),
        "fixture": json.loads(fixtures[0].read_text(encoding="utf-8-sig")),
        "fixtures": [json.loads(path.read_text(encoding="utf-8-sig")) for path in fixtures],
    }


def expected_repeated(item: dict[str, Any]) -> tuple[bool, bool]:
    name = str(item["native_name"])
    canonical = normalized(name)
    repeated = (
        item["type"] in {"(...)", '"<string>"'}
        or "[<" in name
        or "<_type>" in name
        or name.startswith("CheatTeleportLocations")
        or canonical
        in {"excludeitemindices", "modids", "overrideplayerlevelengrampoints"}
    )
    return repeated, "[<" in name or "<_type>" in name


def verify_repository(
    report: dict[str, Any],
    root: Path,
    mutation: Mutation | None = None,
) -> list[str]:
    failures: list[str] = []
    for edition in MODULES:
        contract = load_contract(root, edition)
        if mutation:
            mutation(edition, contract)
        verify_edition(report, edition, contract, failures)
    return failures


def verify_edition(
    report: dict[str, Any],
    edition: str,
    contract: dict[str, Any],
    failures: list[str],
) -> None:
    prefix = f"ARK_{edition.upper()}"
    namespace = NAMESPACES[edition]
    schema = contract["schema"].get("properties", {})
    ledger_items = contract["ledger"].get("items", [])
    ledger_exclusions = contract["ledger"].get("exclusions", [])
    expected_by_array: dict[str, list[tuple[Any, ...]]] = {}
    fixture = contract["fixture"]
    fixture_settings = fixture.get("settings", {})
    expected_files = fixture.get("expected", {}).get("files", [])
    fixture_fragments = {
        file.get("path"): file.get("fragments", [])
        for file in expected_files
        if file.get("root") == "config"
    }
    launch_arguments = fixture.get("expected", {}).get("launch", {}).get("arguments", [])
    preferred_schema_keys: set[str] = set()
    server_name = schema.get("server_name", {})
    if server_name.get("x-lsgm-default-source") != "instance_name":
        failures.append(
            f"{edition}:server_name: expected x-lsgm-default-source=instance_name"
        )
    for key in ARK_SERVER_URL_STRING_KEYS[edition]:
        prop = schema.get(key, {})
        if prop.get("type") != "string" or prop.get("pattern") != ARK_SERVER_URL_SAFE_PATTERN:
            failures.append(
                f"{edition}:{key}: unsafe server URL pattern; "
                f"expected exact {ARK_SERVER_URL_SAFE_PATTERN!r}"
            )
    if edition == "asa":
        for key in ARK_ASA_INI_PASSWORD_KEYS:
            prop = schema.get(key, {})
            if prop.get("type") != "string" or prop.get("pattern") != ARK_INI_PASSWORD_PATTERN:
                failures.append(
                    f"{edition}:{key}: unsafe INI password pattern; "
                    f"expected exact {ARK_INI_PASSWORD_PATTERN!r}"
                )
    for preferred in report["editions"][edition]["items"]:
        if preferred["classification"] not in {"schema_native", "specialized"}:
            continue
        preferred_source, preferred_native = ledger_identity(preferred)
        preferred_rows = [
            row
            for row in ledger_items
            if ledger_row_matches(row, preferred, preferred_source, preferred_native)
        ]
        if len(preferred_rows) == 1 and preferred_rows[0].get("schema_key"):
            preferred_schema_keys.add(str(preferred_rows[0]["schema_key"]))
    for item in report["editions"][edition]["items"]:
        source, native_key = ledger_identity(item)
        classification = str(item["classification"])
        target_rows = (
            ledger_items
            if classification in {"schema_native", "specialized", "derived"}
            else ledger_exclusions
        )
        matches = [
            row
            for row in target_rows
            if ledger_row_matches(row, item, source, native_key)
        ]
        label = f"{edition}:{item['native_name']}"
        if len(matches) != 1:
            failures.append(
                f"{label}: expected one live {classification} ledger row, found {len(matches)}"
            )
            continue
        if classification == "manager_derived":
            if not str(matches[0].get("reason", "")).strip():
                failures.append(f"{label}: manager-derived row has no ownership reason")
            surface = str(item["native_surface"])
            native = str(item["native_name"])
            if surface == "server_url":
                query = f"?{native.lstrip('?')}="
                if not launch_arguments or query not in launch_arguments[0]:
                    failures.append(f"{label}: manager-derived URL value is not exercised")
            elif surface == "launch_arg":
                emitted = any(
                    argument == native or argument.startswith(f"{native}=")
                    for argument in launch_arguments
                )
                conditional_multihome = (
                    native == "-MULTIHOME"
                    and f"{{{{{namespace}.multihome_flag}}}}" in contract["manifest"]
                    and '"multihome_flag"' in contract["runtime_logic"]
                )
                if not emitted and not conditional_multihome:
                    failures.append(f"{label}: manager-derived launch value is unreachable")
            elif surface.startswith("GameUserSettings.ini"):
                fragments = fixture_fragments.get("GameUserSettings.ini", [])
                emitted = any(fragment.startswith(f"{native}=") for fragment in fragments)
                conditional_multihome = (
                    native == "MultiHome"
                    and f"{{{{{namespace}.multihome_ini_line}}}}" in contract["templates"]
                    and "render_ark_multihome_ini_line" in contract["storage_logic"]
                )
                if not emitted and not conditional_multihome:
                    failures.append(f"{label}: manager-derived INI value is not exercised")
            continue
        if classification not in {"schema_native", "specialized", "derived"}:
            if not str(matches[0].get("reason", "")).strip():
                failures.append(f"{label}: exclusion has no evidence-backed reason")
            continue
        schema_key = str(matches[0].get("schema_key", ""))
        prop = schema.get(schema_key)
        if not isinstance(prop, dict):
            failures.append(f"{label}: missing schema key {schema_key!r}")
            continue
        if classification == "derived":
            if schema_key not in preferred_schema_keys:
                failures.append(
                    f"{label}: derived alias has no schema-native preferred writer"
                )
            continue
        if classification == "specialized":
            surface = str(item["native_surface"])
            if surface == "launch_arg":
                token_name = "cluster_dir_override_flag" if schema_key == "cluster_directory" else "mod_ids_flag"
                token = "{{" + namespace + "." + token_name + "}}"
                emitted = any(
                    argument.startswith(f"{item['native_name']}=")
                    for argument in launch_arguments
                )
                if token not in contract["manifest"] or not emitted:
                    failures.append(f"{label}: specialized launch writer is unreachable")
            else:
                token = f"{{{{{namespace}.active_mods_ini_line}}}}"
                fragments = fixture_fragments.get("GameUserSettings.ini", [])
                if token not in contract["templates"] or not any(
                    fragment.startswith("ActiveMods=") for fragment in fragments
                ):
                    failures.append(f"{label}: specialized ActiveMods writer is unreachable")
            continue
        if schema_key not in fixture_settings:
            failures.append(f"{label}: exhaustive fixture does not set {schema_key!r}")
        surface = str(item["native_surface"])
        if surface == "launch_arg":
            array = f"{prefix}_ADDITIONAL_LAUNCH_SETTINGS"
            raw = str(item["raw_name"])
            kind = "InvertedFlag" if item["native_name"] == "-NoBattlEye" else "Value" if "=" in raw else "Flag"
            if edition == "asa" and schema_key == "passive_mod_ids_csv":
                kind = "ModIds"
            expected_by_array.setdefault(array, []).append((item["native_name"], schema_key, kind))
            if prop.get("type") != "boolean" and kind in {"Flag", "InvertedFlag"}:
                failures.append(f"{label}: flag schema type is not boolean")
            if kind == "ModIds" and prop.get("type") != "string":
                failures.append(f"{label}: Mod ID list schema type is not string")
            if not any(
                argument == item["native_name"]
                or argument.startswith(f"{item['native_name']}=")
                for argument in launch_arguments
            ):
                failures.append(f"{label}: exhaustive launch fixture does not emit the flag")
        elif surface == "server_url":
            pair = f'("{schema_key}", "{str(item["native_name"]).lstrip("?")}")'
            if pair not in contract["runtime_logic"]:
                failures.append(f"{label}: exact server URL writer pair is missing")
            query = f"?{str(item['native_name']).lstrip('?')}="
            if not launch_arguments or query not in launch_arguments[0]:
                failures.append(f"{label}: exhaustive launch fixture omits the URL setting")
        elif surface == "Game.ini:[ModInstaller]":
            token = f"{{{{{namespace}.mod_installer_section}}}}"
            if token not in contract["templates"] or 'get("auto_managed_mod_ids")' not in contract["storage_logic"]:
                failures.append(f"{label}: ModInstaller writer is unreachable")
            if not any("ModIDS=" in fragment for fragment in fixture_fragments.get("Game.ini", [])):
                failures.append(f"{label}: exhaustive materialization fixture omits ModInstaller")
        else:
            suffix = INI_ARRAYS[surface]
            array = f"{prefix}_{suffix}"
            repeated, indexed = expected_repeated(item)
            quoted = normalized(str(item["native_name"])) in {
                "banlisturl", "customdynamicconfigurl", "customlivetuningurl"
            }
            expected_by_array.setdefault(array, []).append(
                (writer_native_name(item), schema_key, repeated, indexed, quoted)
            )
            token = f"{{{{{namespace}.{INI_TOKENS[surface]}}}}}"
            if token not in contract["templates"]:
                failures.append(f"{label}: native template token is missing")
            if repeated and (prop.get("type") != "string" or prop.get("format") != "textarea"):
                failures.append(f"{label}: repeated setting is not a string textarea")
            fixture_file = "GameUserSettings.ini" if surface.startswith("GameUserSettings.ini") else "Game.ini"
            native = writer_native_name(item)
            if not any(
                any(
                    line.startswith(f"{native}=") or line.startswith(f"{native}[") or line.startswith(f"{native}_")
                    for line in fragment.splitlines()
                )
                for fragment in fixture_fragments.get(fixture_file, [])
            ):
                failures.append(f"{label}: exhaustive materialization fixture omits the native line")

    if f"{{{{{namespace}.official_launch_flags}}}}" not in contract["manifest"]:
        failures.append(f"{edition}: official launch aggregate is missing from module.toml")
    for array, expected in expected_by_array.items():
        actual = (
            contract["launch_arrays"].get(array, [])
            if array.endswith("LAUNCH_SETTINGS")
            else contract["ini_arrays"].get(array, [])
        )
        if actual != expected:
            missing = [row for row in expected if row not in actual]
            extra = [row for row in actual if row not in expected]
            failures.append(
                f"{edition}:{array}: exact writer mismatch; missing={missing[:2]!r}, extra={extra[:2]!r}, expected={len(expected)}, actual={len(actual)}"
            )

    for item in report["editions"][edition]["edition_no_unknown_items"]:
        source, native_key = ledger_identity(item)
        label = f"{edition}:{item['native_name']}"
        matches = [
            row
            for row in ledger_exclusions
            if ledger_row_matches(row, item, source, native_key)
        ]
        if len(matches) != 1:
            availability = str(item["classification"]).removeprefix("excluded_edition_")
            failures.append(
                f"{label}: expected one edition-{availability} exclusion row, found {len(matches)}"
            )
        elif not str(matches[0].get("reason", "")).strip():
            failures.append(f"{label}: edition No/Unknown exclusion has no reason")
        for schema_key, prop in schema.items():
            if not isinstance(prop, dict):
                continue
            candidate = {
                "source": prop.get("x-lsgm-source"),
                "key": prop.get("x-lsgm-source-key"),
            }
            if ledger_row_matches(candidate, item, source, native_key):
                failures.append(
                    f"{label}: edition No/Unknown row leaks into schema key {schema_key!r}"
                )

    if edition == "asa":
        verify_asa_additional_settings(contract, failures)
        patch_array = "ARK_ASA_PATCH_GUS_SERVER_SETTINGS"
        actual_patch_rows = contract["ini_arrays"].get(patch_array, [])
        if actual_patch_rows != ASA_PATCH_GUS_ROWS:
            failures.append(
                f"asa:{patch_array}: patch writer mismatch; "
                f"expected={ASA_PATCH_GUS_ROWS!r}, actual={actual_patch_rows!r}"
            )
        if patch_array not in contract["storage_logic"]:
            failures.append("asa: patch GUS inventory is not reachable from the storage dispatcher")
        patch_settings = {
            key
            for entry in contract["fixtures"]
            for key in entry.get("settings", {})
        }
        gus_fragments = [
            fragment
            for entry in contract["fixtures"]
            for output in entry.get("expected", {}).get("files", [])
            if output.get("root") == "config" and output.get("path") == "GameUserSettings.ini"
            for fragment in output.get("fragments", [])
        ]
        for native, schema_key, *_flags in ASA_PATCH_GUS_ROWS:
            if schema_key not in patch_settings:
                failures.append(f"asa:{native}: patch fixture omits {schema_key!r}")
            if not any(fragment.startswith(f"{native}=") for fragment in gus_fragments):
                failures.append(f"asa:{native}: patch materialization fragment is missing")
