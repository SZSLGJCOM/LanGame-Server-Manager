from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

from verify_module_setting_coverage_constants import *
from verify_module_setting_coverage_dst import (
    dst_aggregate_token_settings,
    dst_override_inventory_settings,
    rust_executable_source,
    rust_function_body_from_source,
    validate_dst_native_settings_contract,
)
from verify_module_setting_coverage_ark import ark_aggregate_token_settings
from verify_module_setting_coverage_scum import (
    SCUM_TEMPLATE_CONTRACT,
    scum_native_section_ids_from_source,
    validate_scum_native_settings_contract,
)
from verify_module_setting_coverage_server_tabs import validate_server_detail_tool_tabs
from verify_module_setting_coverage_console import validate_runtime_console_boundary
from verify_module_setting_coverage_typescript import (
    extract_ts_balanced_block,
    extract_ts_braced_block,
    extract_ts_bracket_block,
    extract_ts_object_block,
    module_settings_section_ids_from_source,
    settings_definition_object_block,
    ts_executable_source,
    ts_const_string_array,
    ts_group_key_literal_entries,
    ts_group_key_literals,
    ts_object_has_property,
    ts_object_has_string_property,
    ts_object_keys,
    ts_object_string_values,
    ts_section_ids_from_array_block,
    ts_string_literals,
    ts_string_sequence,
)

ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
DESKTOP_SRC_DIR = ROOT / "apps" / "desktop" / "src"
DESKTOP_TAURI_COMMANDS_RS = ROOT / "apps" / "desktop" / "src-tauri" / "src" / "commands.rs"
APP_CORE_LIB_RS = ROOT / "crates" / "app-core" / "src" / "lib.rs"
APP_MODULES_LIB_RS = ROOT / "crates" / "app-modules" / "src" / "lib.rs"
APP_RUNTIME_LAUNCH_TEMPLATES_RS = ROOT / "crates" / "app-runtime" / "src" / "launch_templates.rs"
APP_STORAGE_INSTANCES_RS = ROOT / "crates" / "app-storage" / "src" / "instances.rs"
APP_STORAGE_TEMPLATES_RS = ROOT / "crates" / "app-storage" / "src" / "templates.rs"

PLAYER_CENTER_FRONTEND_FILES = (
    "views/servers/PlayerCenterWorkbench.tsx",
    "views/servers/player-center/use-player-access.tsx",
    "views/servers/player-center/PlayerAccessRosterEditor.tsx",
    "views/servers/player-center/PlayerAccessRosterList.tsx",
    "views/servers/player-center/SelectedPlayerRosterActions.tsx",
    "views/servers/player-center/player-access-roster-model.ts",
    "views/servers/player-center/player-access-roster-validation.ts",
    "views/servers/player-center/manual-player-action-model.ts",
    "views/servers/player-center/ManualPlayerActions.tsx",
    "views/servers/player-center/OnlinePlayersView.tsx",
    "views/servers/player-center/LivePlayerActionPanel.tsx",
    "views/servers/player-center/LivePlayerTable.tsx",
    "views/servers/player-center/LivePlayerState.tsx",
    "views/servers/player-center/use-live-players.ts",
)
PLAYER_CENTER_PUBLIC_CONTRACTS = {
    "views/servers/PlayerCenterWorkbench.tsx": ("export function PlayerCenterWorkbench",),
    "views/servers/player-center/use-player-access.tsx": ("export function usePlayerAccess",),
    "views/servers/player-center/PlayerAccessRosterEditor.tsx": ("export function PlayerAccessRosterEditor",),
    "views/servers/player-center/PlayerAccessRosterList.tsx": ("export function PlayerAccessRosterList",),
    "views/servers/player-center/SelectedPlayerRosterActions.tsx": ("export function SelectedPlayerRosterActions",),
    "views/servers/player-center/player-access-roster-model.ts": (
        "export function readPlayerAccessRosterCapabilities",
        "export function buildRosterFields",
    ),
    "views/servers/player-center/player-access-roster-validation.ts": ("export function validateRosterEntryInput",),
    "views/servers/player-center/manual-player-action-model.ts": ("export function readManualPlayerActions",),
    "views/servers/player-center/ManualPlayerActions.tsx": ("export function ManualPlayerActions",),
    "views/servers/player-center/OnlinePlayersView.tsx": ("export function OnlinePlayersView",),
    "views/servers/player-center/LivePlayerActionPanel.tsx": ("export function LivePlayerActionPanel",),
    "views/servers/player-center/LivePlayerTable.tsx": ("export function LivePlayerTable",),
    "views/servers/player-center/LivePlayerState.tsx": ("export function LivePlayerState",),
    "views/servers/player-center/use-live-players.ts": ("export function useLivePlayers",),
}

_APP_STORAGE_TEMPLATES_TEXT: str | None = None
_APP_RUNTIME_LAUNCH_TEMPLATES_TEXT: str | None = None
_DESKTOP_TAURI_COMMANDS_TEXT: str | None = None

SUPPORTED_PLAYER_ACCESS_CODECS = {
    "humanitz_net_id",
    "plain",
    "steam64",
    "uint64",
    "pipe_steam64",
    "csv_uuid_name",
    "minecraft_ip_csv",
    "terraria_banlist",
    "object_identity",
    "ark_account_id",
    "barotrauma_account",
    "dst_klei_id",
    "valheim_platform_id",
}
SUPPORTED_PLAYER_ACCESS_SYNC_MODES = {"direct", "reload", "restart"}
DELIMITED_PLAYER_ACCESS_IDENTITY_FORMATS = {
    "pipe_steam64": "steam64",
    "csv_uuid_name": "minecraft_uuid",
    "minecraft_ip_csv": "ip",
    "barotrauma_account": "barotrauma_account",
}
SUPPORTED_PLAYER_ACCESS_SEGMENT_FORMATS = {
    "steam64", "minecraft_uuid", "minecraft_name", "ip", "barotrauma_account",
    "text", "integer", "boolean",
}


def read_source_glob(directory: Path, pattern: str) -> str:
    return "\n".join(
        path.read_text(encoding="utf-8-sig")
        for path in sorted(directory.glob(pattern))
        if path.is_file()
    )


def read_module_toml(module_id: str) -> dict:
    return tomllib.loads((MODULES_DIR / module_id / "module.toml").read_text(encoding="utf-8-sig"))


def read_schema_properties(module_id: str) -> set[str]:
    schema_path = MODULES_DIR / module_id / "schema.json"
    schema = json.loads(schema_path.read_text(encoding="utf-8-sig"))
    properties = schema.get("properties", {})
    if not isinstance(properties, dict):
        return set()
    return set(properties)


def schema_property_is_type(module_id: str, property_key: str, expected_type: str) -> bool:
    return schema_has_type(read_schema_property_defs(module_id).get(property_key, {}), expected_type)


def read_schema_property_defs(module_id: str) -> dict[str, dict]:
    schema_path = MODULES_DIR / module_id / "schema.json"
    schema = json.loads(schema_path.read_text(encoding="utf-8-sig"))
    properties = schema.get("properties", {})
    if not isinstance(properties, dict):
        return {}
    return {
        key: value
        for key, value in properties.items()
        if isinstance(key, str) and isinstance(value, dict)
    }


def read_app_storage_templates_rs() -> str:
    global _APP_STORAGE_TEMPLATES_TEXT
    if _APP_STORAGE_TEMPLATES_TEXT is None:
        _APP_STORAGE_TEMPLATES_TEXT = "\n".join((
            read_source_glob(APP_STORAGE_TEMPLATES_RS.parent, "templates*.rs"),
            read_source_glob(APP_STORAGE_TEMPLATES_RS.parent / "templates_materialize", "*.rs"),
            (APP_STORAGE_TEMPLATES_RS.parent / "player_access_normalization.rs").read_text(
                encoding="utf-8-sig"
            ),
            (APP_STORAGE_TEMPLATES_RS.parent / "player_access_uint64.rs").read_text(
                encoding="utf-8-sig"
            ),
        ))
    return _APP_STORAGE_TEMPLATES_TEXT


def read_app_runtime_launch_templates_rs() -> str:
    global _APP_RUNTIME_LAUNCH_TEMPLATES_TEXT
    if _APP_RUNTIME_LAUNCH_TEMPLATES_TEXT is None:
        _APP_RUNTIME_LAUNCH_TEMPLATES_TEXT = read_source_glob(
            APP_RUNTIME_LAUNCH_TEMPLATES_RS.parent, "launch_templates*.rs"
        )
    return _APP_RUNTIME_LAUNCH_TEMPLATES_TEXT


def read_desktop_tauri_commands_rs() -> str:
    global _DESKTOP_TAURI_COMMANDS_TEXT
    if _DESKTOP_TAURI_COMMANDS_TEXT is None:
        sources: list[str] = []
        visited: set[Path] = set()

        def read_command_source(path: Path) -> None:
            path = path.resolve()
            if path in visited:
                return
            visited.add(path)
            source = path.read_text(encoding="utf-8-sig")
            sources.append(source)
            # Follow the split implementations, not unrelated files in their directories.
            for relative_path in re.findall(
                r'(?m)^\s*include!\s*\(\s*"([^"\r\n]+\.rs)"\s*\)\s*;', source
            ):
                read_command_source(path.parent / relative_path)

        for path in sorted(DESKTOP_TAURI_COMMANDS_RS.parent.glob("commands*.rs")):
            if path.is_file():
                read_command_source(path)
        _DESKTOP_TAURI_COMMANDS_TEXT = "\n".join(sources)
    return _DESKTOP_TAURI_COMMANDS_TEXT


def validate_roster_settings_frontend_save_chain(source: str) -> list[str]:
    source = ts_executable_source(source)
    declaration = re.search(r"\basync\s+function\s+handleSaveSettings\s*\(", source)
    body = ""
    if declaration:
        parameters = extract_ts_balanced_block(source, declaration.end() - 1, "(", ")")
        if parameters:
            body = extract_ts_braced_block(
                source, source.find("{", declaration.end() - 1 + len(parameters))
            )
    if re.search(
        r"\bawait\s+updateInstance\s*\(\s*input\s*,\s*expectedSettingsJson\s*,\s*"
        r"saveOptions\.collectionRemoval\s*,?\s*\)",
        body,
    ):
        return []
    return [
        "frontend: roster/settings save must await updateInstance with its input, expected baseline, and collection removal options"
    ]


def validate_roster_settings_backend_save_chain(commands_source: str) -> list[str]:
    failures: list[str] = []
    public_body = rust_function_body_from_source(
        commands_source, "update_instance_record_if_current"
    )
    if not public_body:
        return [
            "backend: roster/settings conditional update command is missing"
        ]

    public_precondition_call = re.search(
        r"\bupdate_instance_record_with_precondition\s*\(\s*"
        r"state\s*,\s*input\s*,\s*Some\s*\(\s*"
        r"expected_settings_json\.as_str\s*\(\s*\)\s*\)\s*,\s*None\s*,?\s*\)\s*\.await\b",
        public_body,
        re.DOTALL,
    )
    if not public_precondition_call:
        failures.append(
            "backend: roster/settings conditional update must forward the expected settings baseline"
        )

    precondition_body = rust_function_body_from_source(
        commands_source, "update_instance_record_with_precondition"
    )
    if not precondition_body:
        failures.append(
            "backend: roster/settings precondition dispatcher is missing"
        )
        return failures

    if not re.search(
        r"\bmatch\s+expected_settings_json\s*\{",
        precondition_body,
    ):
        failures.append(
            "backend: roster/settings precondition dispatcher must branch on the expected settings baseline"
        )

    conditional_update = re.search(
        r"\bSome\s*\(\s*(?P<expected>[A-Za-z_][A-Za-z0-9_]*)\s*\)\s*=>\s*"
        r"update_instance_if_current\s*\(\s*[^,]+,\s*input\s*,\s*&?\s*"
        r"(?P=expected)\s*\)\s*\.await\b",
        precondition_body,
        re.DOTALL,
    )
    if not conditional_update:
        failures.append(
            "backend: roster/settings expected-baseline branch must use conditional instance update"
        )

    unconditional_update = re.search(
        r"\bNone\s*=>\s*update_instance\s*\(\s*[^,]+,\s*input\s*\)\s*\.await\b",
        precondition_body,
        re.DOTALL,
    )
    if not unconditional_update:
        failures.append(
            "backend: roster/settings update without a baseline must retain the unconditional update branch"
        )
    return failures


def validate_instance_settings_lock_contract(source: str) -> list[str]:
    source = rust_executable_source(source)
    failures: list[str] = []
    body = re.sub(r"\s+", "", rust_function_body_from_source(source, "acquire_instance_settings_mutation_lock"))
    if "acquire_lock_at_path(instance_settings_mutation_lock_path(paths,instance_id))" not in body:
        failures.append("backend: settings mutations must acquire the per-instance lock path")
    dispatch = re.sub(r"\s+", "", rust_function_body_from_source(source, "acquire_lock_at_path"))
    acquire = re.sub(r"\s+", "", rust_function_body_from_source(source, "acquire_lock_at_path_with_mode"))
    attempt = re.search(
        r"let(\w+)=map_try_lock_result\(matchmode\{"
        r"LockMode::Shared=>file\.try_lock_shared\(\),"
        r"LockMode::Exclusive=>file\.try_lock\(\),?\}\);",
        acquire,
    )
    if (
        dispatch != "acquire_lock_at_path_with_mode(lock_path,LockMode::Exclusive)"
        or not attempt
        or not re.search(rf"match{re.escape(attempt[1])}\{{", acquire[attempt.end():])
    ):
        failures.append("backend: instance settings lock must acquire an exclusive nonblocking lease")
    mapper = re.sub(r"\s+", "", rust_function_body_from_source(source, "map_try_lock_result"))
    mapping = re.fullmatch(r"(?:return)?result\.map_err\(\|(?P<error>\w+)\|match(?P=error)\{(?P<arms>[^{}]*)\}\);?", mapper)
    if not mapping or "TryLockError::WouldBlock=>LockAcquireError::Contended" not in mapping["arms"] or not re.search(
        r"Err\(LockAcquireError::Contended\)=>\{Err\(StorageError::InstanceSettingsLocked\{[^{}]*\}\)\}",
        acquire,
    ):
        failures.append("backend: instance settings lock must report contention without granting a lease")
    return failures


def rust_function_body(function_name: str) -> str:
    return rust_function_body_from_source(read_app_storage_templates_rs(), function_name)


def rust_string_const_values_in_body(body: str) -> set[str]:
    if not body:
        return set()

    source = read_app_storage_templates_rs()
    values: set[str] = set()
    const_names = set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", body))
    for const_name in const_names:
        match = re.search(
            rf'\bconst\s+{re.escape(const_name)}\s*:\s*&str\s*=\s*"((?:\\.|[^"\\])*)"',
            source,
        )
        if match:
            values.add(match.group(1).replace('\\"', '"').replace("\\\\", "\\"))
    return values


def schema_string_literals_in_rust_function(
    schema_properties: set[str], function_name: str, allowed_prefixes: tuple[str, ...]
) -> set[str]:
    body = rust_function_body(function_name)
    if not body:
        return set()
    identifiers = set(re.findall(r'"([A-Za-z][A-Za-z0-9_]*)"', body))
    identifiers.update(rust_string_const_values_in_body(body))
    refs = identifiers & schema_properties
    if allowed_prefixes:
        refs = {ref for ref in refs if ref.startswith(allowed_prefixes)}
    return refs


def materialized_settings(module_id: str, schema_properties: set[str]) -> set[str]:
    refs = workshop_materialized_settings(module_id, schema_properties)
    for function_name, allowed_prefixes in MATERIALIZATION_RUST_FUNCTION_COVERAGE.get(module_id, ()):
        if module_id == "satisfactory":
            body = rust_function_body(function_name)
            executable = rust_executable_source(body)
            native_map = re.search(
                r"\bfor\s*\(\s*setting\s*,\s*native_key\s*\)\s*in\s*\[(.*?)\]\s*\{",
                executable,
                re.DOTALL,
            )
            if native_map:
                entries = re.compile(r'\(\s*"([a-z][a-z0-9_]*)"\s*,\s*"FG\.[^"]+"\s*,?\s*\)')
                refs.update(
                    entry[1]
                    for entry in entries.finditer(body, native_map.start(1), native_map.end(1))
                    if executable[entry.start()] == "(" and entry[1] in schema_properties
                )
            continue
        refs.update(
            schema_string_literals_in_rust_function(
                schema_properties, function_name, allowed_prefixes
            )
        )
    return refs


def workshop_materialized_settings(module_id: str, schema_properties: set[str]) -> set[str]:
    functions = {
        "barotrauma": ("materialize_barotrauma_workshop_mods", "prepare_barotrauma_workshop_mods"),
        "conanexiles": ("materialize_conan_modlist", "prepare_conan_modlist"),
    }.get(module_id)
    if not functions:
        return set()
    entrypoint, prepare = functions
    body = rust_executable_source(rust_function_body(entrypoint))
    if not re.search(rf"\b{prepare}\s*\(\s*context\s*\)", body):
        return set()
    if not re.search(r"\bfiles\s*\.\s*apply\s*\(", body):
        return set()
    prepared_body = rust_executable_source(rust_function_body(prepare))
    if not re.search(r"\bresolve_packages\s*\(\s*context\s*,", prepared_body):
        return set()
    resolver = rust_function_body("resolve_packages")
    executable = rust_executable_source(resolver)
    refs = set()
    for call in re.finditer(
        r'(?P<prefix>\bparse_workshop_id_list\s*\(\s*context\.settings\s*,\s*)'
        r'"(?P<key>[A-Za-z][A-Za-z0-9_]*)"(?P<suffix>\s*,?\s*\))',
        resolver,
    ):
        if all(
            executable[call.start(part):call.end(part)] == call[part]
            for part in ("prefix", "suffix")
        ):
            refs.add(call["key"])
    return refs & schema_properties


def read_rendered_surfaces(module_id: str) -> str:
    module_root = MODULES_DIR / module_id
    module_toml = read_module_toml(module_id)
    args = module_toml.get("process", {}).get("args_template", [])
    if not isinstance(args, list):
        args = []

    template_texts = []
    for template_path in sorted((module_root / "templates").glob("**/*.hbs")):
        template_texts.append(template_path.read_text(encoding="utf-8-sig"))
    for relative_path in MODULE_SCHEMA_USAGE_ADDITIONAL_SOURCES.get(module_id, ()):
        source_path = ROOT / relative_path
        if source_path.is_file():
            template_texts.append(source_path.read_text(encoding="utf-8-sig"))

    return "\n".join([*(str(arg) for arg in args), *template_texts])


def referenced_settings(
    module_id: str, surface_text: str, schema_properties: set[str]
) -> tuple[set[str], set[str]]:
    template_tokens = TEMPLATE_TOKEN_RE.findall(surface_text)
    direct_refs = {
        token.rsplit(".", 1)[-1]
        for token in template_tokens
        if token.startswith(("settings.", "json.settings.", "xml.settings."))
    }
    direct_refs.update(token for token in template_tokens if token in schema_properties)
    direct_refs.update(SETTING_TOKEN_RE.findall(surface_text))
    direct_refs.update(set(re.findall(r'"([A-Za-z][A-Za-z0-9_]*)"', surface_text)) & schema_properties)
    derived_refs: set[str] = set()
    unknown_module_tokens: set[str] = set()
    allowed_module_prefixes = {module_id, *MODULE_TOKEN_PREFIX_ALIASES.get(module_id, set())}

    for full_token in template_tokens:
        parts = full_token.split(".")
        if len(parts) != 2:
            continue
        prefix, token = parts
        if prefix == "launch":
            runtime_source = read_app_runtime_launch_templates_rs()
            resolve_body = rust_function_body_from_source(runtime_source, "resolve_token")
            extra_args_body = rust_function_body_from_source(
                runtime_source, "lookup_extra_launch_args_token"
            )
            if (
                token == "extra_args"
                and 'token.strip_prefix("launch.")' in resolve_body
                and '"extra_args" | "extra_launch_args"' in extra_args_body
                and "render_split_launch_flags" in extra_args_body
            ):
                derived_refs.add("extra_launch_args")
            else:
                unknown_module_tokens.add(full_token)
            continue
        if prefix in {"settings", "paths", "ports", "instance", "module"}:
            continue
        if prefix not in allowed_module_prefixes:
            if not full_token.startswith("json.settings."):
                unknown_module_tokens.add(full_token)
            continue
        if prefix == "abioticfactor" and token.startswith("ini_value "):
            segments = token.split()
            if len(segments) >= 2:
                derived_refs.add(segments[1])
                continue
        coverage = None
        if prefix == "unturned" and token == "native_config":
            coverage = {
                key for key, prop in read_schema_property_defs(module_id).items()
                if prop.get("x-lsgm-native-type")
            }
        if prefix == "dst":
            coverage = dst_aggregate_token_settings(
                token, read_app_storage_templates_rs(), schema_properties
            )
        if coverage is None and prefix in {"arkse", "arksa"}:
            coverage = ark_aggregate_token_settings(
                prefix,
                token,
                read_app_storage_templates_rs(),
                read_app_runtime_launch_templates_rs(),
            )
        if coverage is None:
            coverage = DERIVED_TOKEN_SETTING_COVERAGE.get((prefix, token))
        if coverage is None and (prefix, token) in DERIVED_TOKEN_SCHEMA_REMAINDER_EXCLUSIONS:
            excluded = DERIVED_TOKEN_SCHEMA_REMAINDER_EXCLUSIONS[(prefix, token)]
            coverage = schema_properties - excluded
        if coverage is None and (prefix, token) in DERIVED_TOKEN_RUST_FUNCTION_COVERAGE:
            function_name, allowed_prefixes = DERIVED_TOKEN_RUST_FUNCTION_COVERAGE[(prefix, token)]
            coverage = schema_string_literals_in_rust_function(
                schema_properties, function_name, allowed_prefixes
            )
        if coverage is None:
            unknown_module_tokens.add(full_token)
            continue
        derived_refs.update(coverage)

    return direct_refs | derived_refs | materialized_settings(module_id, schema_properties), unknown_module_tokens


def referenced_ports(module_id: str, surface_text: str) -> set[str]:
    ports = set(PORT_TOKEN_RE.findall(surface_text))
    for token in TEMPLATE_TOKEN_RE.findall(surface_text):
        parts = token.split(".")
        if len(parts) == 3 and parts[0] == "ports" and parts[2] == "port":
            ports.add(parts[1])
            continue
        if len(parts) != 2:
            continue
        prefix, derived_token = parts
        allowed_module_prefixes = {module_id, *MODULE_TOKEN_PREFIX_ALIASES.get(module_id, set())}
        if prefix in allowed_module_prefixes:
            ports.update(DERIVED_TOKEN_PORT_COVERAGE.get((prefix, derived_token), set()))
    return ports


def declared_ports(module_toml: dict) -> set[str]:
    ports = set()
    for index, port in enumerate(module_toml.get("default_ports", []), start=1):
        if not isinstance(port, dict):
            continue
        ports.add(str(port.get("name") or f"port{index}"))
    return ports


def declared_port_number(module_toml: dict, port_name: str) -> int | None:
    for port in module_toml.get("default_ports", []):
        if isinstance(port, dict) and str(port.get("name") or "") == port_name:
            port_number = port.get("port")
            return port_number if isinstance(port_number, int) else None
    return None


def declared_port_protocol(module_toml: dict, port_name: str) -> str | None:
    for port in module_toml.get("default_ports", []):
        if isinstance(port, dict) and str(port.get("name") or "") == port_name:
            return str(port.get("protocol") or "").lower()
    return None


def port_alias_is_materialized_by_number(module_toml: dict, referenced_port_names: set[str], port_name: str) -> bool:
    port_number = declared_port_number(module_toml, port_name)
    if port_number is None:
        return False
    return any(
        referenced_name != port_name
        and declared_port_number(module_toml, referenced_name) == port_number
        for referenced_name in referenced_port_names
    )


def expected_stdin_process_keys(module_id: str) -> set[str]:
    if module_id == "dontstarve":
        return {"master", "caves", "islands", "volcano"}
    return {"main"}


def player_action_allowed_transport_metadata_keys(transport: str) -> set[str]:
    if transport == "stdin":
        return {"process_key"}
    if transport in PLAYER_ACTION_REMOTE_TRANSPORTS:
        return {"port_name", "password_setting_key", "enabled_setting_key"}
    return set()


def runtime_player_action_is_read_only(action_id: str) -> bool:
    return action_id in PLAYER_ACTION_NON_MUTATING_IDS or action_id.startswith(PLAYER_ACTION_READ_PREFIXES)


def runtime_player_action_can_verify_state(action_id: str) -> bool:
    return action_id in PLAYER_ACTION_READ_IDS or action_id.startswith(PLAYER_ACTION_READ_PREFIXES)


def runtime_player_action_requires_destructive(action_id: str) -> bool:
    if runtime_player_action_is_read_only(action_id):
        return False
    return action_id.startswith(PLAYER_ACTION_MUTATING_PREFIXES)


def runtime_player_action_targetless_read_source_key(action: dict) -> tuple[str, str] | None:
    action_id = str(action.get("id") or "").strip()
    command_template = str(action.get("command_template") or "")
    if "{{target}}" in command_template or not runtime_player_action_can_verify_state(action_id):
        return None
    return runtime_player_action_dispatch_source_key(action)


def runtime_player_action_dispatch_source_key(action: dict) -> tuple[str, str]:
    transport = str(action.get("transport") or "stdin")
    if transport == "stdin":
        return transport, str(action.get("process_key") or "main")
    if transport == "telnet":
        return transport, str(action.get("port_name") or "telnet")
    if transport in PLAYER_ACTION_RCON_TRANSPORTS:
        return transport, str(action.get("port_name") or "rcon")
    return transport, ""


def runtime_player_action_target_is_clear(action: dict) -> bool:
    text = " ".join(
        str(action.get(key) or "")
        for key in (
            "target_label",
            "target_label_zh_cn",
            "target_placeholder",
            "target_placeholder_zh_cn",
        )
    )
    return bool(PLAYER_ACTION_TARGET_IDENTITY_RE.search(text))


def remote_action_setting_description_is_clear(property_def: dict) -> bool:
    description = str(property_def.get("description") or "").strip()
    return bool(
        description
        and REMOTE_ACTION_SETTING_MATERIALIZATION_RE.search(description)
        and REMOTE_ACTION_SETTING_EFFECT_RE.search(description)
    )


def validate_runtime_player_actions(module_id: str, module_toml: dict, surface_text: str = "") -> list[str]:
    failures: list[str] = []
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return failures

    actions = runtime.get("player_actions", [])
    if actions in (None, []):
        return failures
    if not isinstance(actions, list):
        return [f"{module_id}: runtime.player_actions must be an array of tables"]

    seen_ids: set[str] = set()
    referenced_port_names = referenced_ports(module_id, surface_text)
    property_defs = read_schema_property_defs(module_id)
    targetless_read_sources = {
        source_key
        for action in actions
        if isinstance(action, dict)
        for source_key in [runtime_player_action_targetless_read_source_key(action)]
        if source_key is not None
    }
    for index, action in enumerate(actions, start=1):
        if not isinstance(action, dict):
            failures.append(f"{module_id}: runtime.player_actions[{index}] must be a table")
            continue

        action_id = str(action.get("id") or "").strip()
        action_kind = str(action.get("kind") or "").strip()
        is_broadcast_action = action_kind == "broadcast"
        if not re.fullmatch(r"[A-Za-z0-9_]+", action_id):
            failures.append(f"{module_id}: runtime.player_actions[{index}] has invalid id {action_id!r}")
        elif action_id in seen_ids:
            failures.append(f"{module_id}: duplicate runtime.player_actions id {action_id!r}")
        seen_ids.add(action_id)

        if not str(action.get("label") or "").strip():
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] is missing label")
        if runtime_player_action_requires_destructive(action_id) and action.get("destructive") is not True:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] changes player access state and must set destructive = true"
            )
        if action.get("destructive") is True and not (
            runtime_player_action_requires_destructive(action_id)
            or runtime_player_action_is_read_only(action_id)
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] is marked destructive but its id does not classify the player-management effect"
            )
        if runtime_player_action_is_read_only(action_id) and action.get("destructive") is True:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] is read-only and must not set destructive = true"
            )

        transport = str(action.get("transport") or "stdin")
        if transport not in {"stdin"} | PLAYER_ACTION_REMOTE_TRANSPORTS:
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] has unsupported transport {transport!r}")
        allowed_transport_metadata = player_action_allowed_transport_metadata_keys(transport)
        stray_transport_metadata = [
            key
            for key in PLAYER_ACTION_TRANSPORT_METADATA_KEYS
            if action.get(key) is not None and key not in allowed_transport_metadata
        ]
        if stray_transport_metadata:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] declares transport metadata {stray_transport_metadata} that is not valid for transport {transport!r}"
            )
        blank_transport_metadata = [
            key
            for key in PLAYER_ACTION_TRANSPORT_METADATA_KEYS
            if action.get(key) is not None and not str(action.get(key) or "").strip()
        ]
        if blank_transport_metadata:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] declares blank transport metadata {blank_transport_metadata}"
            )

        command_template = str(action.get("command_template") or "")
        if not command_template.strip():
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] is missing command_template")
        if "\n" in command_template or "\r" in command_template:
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] command_template must be one line")
        template_tokens = re.findall(r"\{\{\s*([^}]+?)\s*\}\}", command_template)
        unknown_template_tokens = sorted(
            {
                token.strip()
                for token in template_tokens
                if token.strip() not in PLAYER_ACTION_TEMPLATE_TOKENS | {"request_id"}
            }
        )
        if unknown_template_tokens:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses unsupported command_template tokens {unknown_template_tokens}"
            )
        if "target" in {token.strip() for token in template_tokens} and "{{target}}" not in command_template:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] must use exact {{target}} token without spaces"
            )
        if "role" in {token.strip() for token in template_tokens} and "{{role}}" not in command_template:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] must use exact {{role}} token without spaces"
            )
        if command_template.count("{{target}}") > 1:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] must not use {{target}} more than once"
            )
        if command_template.count("{{role}}") > 1:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] must not use {{role}} more than once"
            )

        uses_target = "{{target}}" in command_template
        uses_role = "{{role}}" in command_template
        target_descriptor = " ".join(
            str(action.get(key) or "")
            for key in (
                "target_label",
                "target_label_zh_cn",
                "target_placeholder",
                "target_placeholder_zh_cn",
            )
        )
        if uses_role and not uses_target:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses {{role}} but has no explicit {{target}}"
            )
        if not uses_target:
            target_metadata_keys = (
                "target_required",
                "target_label",
                "target_label_zh_cn",
                "target_placeholder",
                "target_placeholder_zh_cn",
                "target_encoding",
            )
            stray_target_metadata = [
                key for key in target_metadata_keys if action.get(key) is not None
            ]
            if stray_target_metadata:
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] declares target metadata {stray_target_metadata} but command_template has no {{target}}"
                )
        if (
            uses_target
            and action_id
            and not is_broadcast_action
            and not runtime_player_action_is_read_only(action_id)
            and action.get("destructive") is not True
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] targets a player and is not read-only, so it must set destructive = true"
            )
        if (
            not uses_target
            and action_id
            and not is_broadcast_action
            and not runtime_player_action_is_read_only(action_id)
            and action_id not in PLAYER_ACTION_TARGETLESS_MUTATING_IDS
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] has no {{target}} and must be classified as a read-only action"
            )
        if runtime_player_action_requires_destructive(action_id) and not uses_target:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] changes player access state and must require an explicit {{target}}"
            )
        if runtime_player_action_requires_destructive(action_id) and uses_target and not runtime_player_action_target_is_clear(action):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] changes player access state but target_label/target_placeholder do not describe the player identifier type"
            )
        if runtime_player_action_requires_destructive(action_id) and uses_target:
            dispatch_source_key = runtime_player_action_dispatch_source_key(action)
            if dispatch_source_key not in targetless_read_sources:
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] changes player access state but has no targetless read/list action on the same command source {dispatch_source_key!r}"
                )
        if uses_target and action.get("target_required") is not True:
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] uses {{target}} but target_required is not true")
        if uses_target and not str(action.get("target_label") or "").strip():
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] uses {{target}} but has no target_label")
        if uses_target and not str(action.get("target_placeholder") or "").strip():
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] uses {{target}} but has no target_placeholder")

        target_encoding = action.get("target_encoding")
        if target_encoding is not None and str(target_encoding) not in PLAYER_ACTION_TARGET_ENCODINGS:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] has invalid target_encoding {target_encoding!r}"
            )
        if str(target_encoding) == "quoted_string" and re.search(r'["\']\{\{target\}\}|\{\{target\}\}["\']', command_template):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses target_encoding quoted_string, so command_template must not add quotes around {{target}}"
            )
        if (
            uses_target
            and str(target_encoding) == "quoted_string"
            and not is_broadcast_action
            and not PLAYER_ACTION_QUOTED_TEXT_TARGET_RE.search(target_descriptor)
            and "({{target}})" not in command_template
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses target_encoding quoted_string but target_label/target_placeholder describe a pure ID token; keep it raw unless the command syntax requires a quoted literal"
            )
        if (
            uses_target
            and str(target_encoding or "raw") == "raw"
            and re.search(r"(character name|player name|\bname\b)", target_descriptor, re.I)
            and not PLAYER_ACTION_RAW_NAME_TARGET_SOURCE_RE.search(target_descriptor)
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses a raw name-like target; target label/placeholder must point to a single-token username/ID or a list/status output source"
            )
        if (
            uses_target
            and str(target_encoding or "raw") == "raw"
            and PLAYER_ACTION_RAW_NAME_TARGET_RE.search(target_descriptor)
            and not PLAYER_ACTION_SINGLE_TOKEN_TARGET_RE.search(target_descriptor)
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] exposes a raw player-name target; target_label/target_placeholder must explicitly say single-token or username because the UI rejects whitespace for raw targets"
            )
        localized_target_placeholder = str(action.get("target_placeholder_zh_cn") or "").strip()
        if (
            uses_target
            and str(target_encoding or "raw") == "raw"
            and localized_target_placeholder
            and PLAYER_ACTION_ZH_NAME_PLACEHOLDER_RE.search(localized_target_placeholder)
            and not PLAYER_ACTION_SINGLE_TOKEN_TARGET_RE.search(localized_target_placeholder)
        ):
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] has a Chinese raw player-name placeholder; target_placeholder_zh_cn must explicitly say single-token, no-whitespace, or username because the UI rejects whitespace for raw targets"
            )

        if uses_role:
            role_values = action.get("role_values", [])
            if not isinstance(role_values, list) or not all(str(value).strip() for value in role_values):
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] uses {{role}} but role_values is empty or invalid")
            elif len({str(value) for value in role_values}) != len(role_values):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] role_values must be unique"
                )
            else:
                unsafe_role_values = [
                    str(value)
                    for value in role_values
                    if not PLAYER_ACTION_ROLE_VALUE_RE.fullmatch(str(value))
                ]
                if unsafe_role_values:
                    failures.append(
                        f"{module_id}: runtime.player_actions[{action_id or index}] role_values must be shell-safe single tokens, got {unsafe_role_values}"
                    )
        elif action.get("role_values") is not None:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] declares role_values but command_template does not use {{role}}"
            )

        process_key = action.get("process_key")
        if process_key is not None and not re.fullmatch(r"[A-Za-z0-9_-]+", str(process_key)):
            failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] has invalid process_key {process_key!r}")
        if transport == "stdin" and not str(process_key or "").strip():
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses stdin and must declare process_key"
            )
        if transport == "stdin" and str(process_key or "").strip():
            expected_process_keys = expected_stdin_process_keys(module_id)
            if str(process_key) not in expected_process_keys:
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] stdin process_key {process_key!r} must be one of {sorted(expected_process_keys)}"
                )
        if transport in PLAYER_ACTION_REMOTE_TRANSPORTS and process_key is not None:
            failures.append(
                f"{module_id}: runtime.player_actions[{action_id or index}] uses remote transport {transport!r} and must not declare process_key"
            )

        if transport == "palworld_rest":
            expected_templates = {
                "broadcast": "announce {{target}}", "show_players": "players",
                "save_world": "save", "kick_player": "kick {{target}}",
                "ban_player": "ban {{target}}", "unban_player": "unban {{target}}",
            }
            expected_metadata = {
                "port_name": "rest_api", "password_setting_key": "admin_password",
                "enabled_setting_key": "rest_api_enabled",
            }
            if module_id != "palworld" or command_template != expected_templates.get(action_id):
                failures.append(f"{module_id}: runtime.player_actions[{action_id}] must use the declared Palworld REST operation")
            if any(action.get(key) != value for key, value in expected_metadata.items()):
                failures.append(f"{module_id}: runtime.player_actions[{action_id}] must use the authenticated instance REST API configuration")
            if declared_port_protocol(module_toml, "rest_api") != "tcp" or "rest_api" not in referenced_port_names:
                failures.append(f"{module_id}: Palworld REST management requires a materialized TCP REST API port")
            if not schema_property_is_type(module_id, "admin_password", "string") or not schema_property_is_type(module_id, "rest_api_enabled", "boolean"):
                failures.append(f"{module_id}: Palworld REST management requires administrator credentials and an API enable setting")

        if transport in PLAYER_ACTION_RCON_TRANSPORTS:
            port_name = str(action.get("port_name") or "rcon")
            if port_name not in declared_ports(module_toml):
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing RCON port {port_name!r}")
            elif port_name not in referenced_port_names and not port_alias_is_materialized_by_number(module_toml, referenced_port_names, port_name):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] uses RCON port {port_name!r}, but that port is not materialized by launch args, templates, Rust derived tokens, or a same-number materialized port alias"
                )
            port_protocol = declared_port_protocol(module_toml, port_name)
            if transport in {"source_rcon", "websocket_rcon", "humanitz_rcon"} and port_protocol is not None and port_protocol != "tcp":
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] {transport} port {port_name!r} must be TCP"
                )
            if transport == "battleye_rcon" and port_protocol is not None and port_protocol != "udp":
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] BattlEye RCON port {port_name!r} must be UDP")
            password_key = str(action.get("password_setting_key") or "rcon_password")
            properties = set(property_defs)
            if password_key not in properties:
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing RCON password setting {password_key!r}")
            if password_key in properties and not schema_property_is_type(module_id, password_key, "string"):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] RCON password setting {password_key!r} must be a string schema field"
                )
            if (
                password_key in properties
                and not str(property_defs.get(password_key, {}).get("default") or "").strip()
                and property_defs.get(password_key, {}).get("x-lsgm-default-source") != "generated_secret"
            ):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] RCON password setting {password_key!r} must have a non-empty or generated-secret default because native RCON dispatch rejects empty passwords"
                )
            if password_key in properties and not remote_action_setting_description_is_clear(property_defs.get(password_key, {})):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] RCON password setting {password_key!r} description must explain the config/launch write target and remote player-command effect"
                )
            enabled_key = action.get("enabled_setting_key")
            if enabled_key is not None and str(enabled_key) not in properties:
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing RCON enabled setting {enabled_key!r}")
            if enabled_key is not None and str(enabled_key) in properties and not schema_property_is_type(module_id, str(enabled_key), "boolean"):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] RCON enabled setting {enabled_key!r} must be a boolean schema field"
                )
            if enabled_key is not None and str(enabled_key) in properties and not remote_action_setting_description_is_clear(property_defs.get(str(enabled_key), {})):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] RCON enabled setting {enabled_key!r} description must explain the config/launch write target and remote player-command effect"
                )
        if transport == "telnet":
            port_name = str(action.get("port_name") or "telnet")
            if port_name not in declared_ports(module_toml):
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing Telnet port {port_name!r}")
            elif port_name not in referenced_port_names and not port_alias_is_materialized_by_number(module_toml, referenced_port_names, port_name):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] uses Telnet port {port_name!r}, but that port is not materialized by launch args, templates, Rust derived tokens, or a same-number materialized port alias"
                )
            port_protocol = declared_port_protocol(module_toml, port_name)
            if port_protocol is not None and port_protocol != "tcp":
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet port {port_name!r} must be TCP"
                )
            properties = set(property_defs)
            password_key = str(action.get("password_setting_key") or "telnet_password")
            if password_key not in properties:
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing Telnet password setting {password_key!r}")
            if password_key in properties and not schema_property_is_type(module_id, password_key, "string"):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet password setting {password_key!r} must be a string schema field"
                )
            if (
                password_key in properties
                and not str(property_defs.get(password_key, {}).get("default") or "").strip()
                and property_defs.get(password_key, {}).get("x-lsgm-default-source") != "generated_secret"
            ):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet password setting {password_key!r} must have a non-empty or generated-secret default because player-management Telnet dispatch requires authentication"
                )
            if password_key in properties and not remote_action_setting_description_is_clear(property_defs.get(password_key, {})):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet password setting {password_key!r} description must explain the config/launch write target and remote player-command effect"
                )
            enabled_key = action.get("enabled_setting_key")
            if enabled_key is not None and str(enabled_key) not in properties:
                failures.append(f"{module_id}: runtime.player_actions[{action_id or index}] references missing Telnet enabled setting {enabled_key!r}")
            if enabled_key is not None and str(enabled_key) in properties and not schema_property_is_type(module_id, str(enabled_key), "boolean"):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet enabled setting {enabled_key!r} must be a boolean schema field"
                )
            if enabled_key is not None and str(enabled_key) in properties and not remote_action_setting_description_is_clear(property_defs.get(str(enabled_key), {})):
                failures.append(
                    f"{module_id}: runtime.player_actions[{action_id or index}] Telnet enabled setting {enabled_key!r} description must explain the config/launch write target and remote player-command effect"
                )

    return failures


def module_has_player_roster_fields(properties: dict[str, dict]) -> bool:
    return any(
        property_def.get("x-lsgm-player-access-kind") in PLAYER_ACCESS_KINDS
        for property_def in properties.values()
    )


def module_has_player_roster_management_fields(properties: dict[str, dict]) -> bool:
    return any(
        property_def.get("x-lsgm-player-access-kind") in PLAYER_ACCESS_MANAGEMENT_KINDS
        for property_def in properties.values()
    )


def player_roster_field_keys(properties: dict[str, dict]) -> set[str]:
    return {
        key
        for key, property_def in properties.items()
        if property_def.get("x-lsgm-player-access-kind") in PLAYER_ACCESS_KINDS
    }


def module_has_runtime_player_actions(module_toml: dict) -> bool:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return False
    actions = runtime.get("player_actions", [])
    return isinstance(actions, list) and any(
        isinstance(action, dict) and str(action.get("kind") or "").strip().lower() != "broadcast"
        for action in actions
    )


def module_runtime_player_query_protocol(module_toml: dict) -> str:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return ""
    player_query = runtime.get("player_query")
    if not isinstance(player_query, dict):
        return ""
    return str(player_query.get("protocol") or "").strip().lower()


def runtime_player_action_count(module_toml: dict) -> int:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return 0
    actions = runtime.get("player_actions", [])
    return len(actions) if isinstance(actions, list) else 0


def runtime_player_action_transports(module_toml: dict) -> set[str]:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return set()
    actions = runtime.get("player_actions", [])
    if not isinstance(actions, list):
        return set()
    return {
        str(action.get("transport") or "stdin")
        for action in actions
        if isinstance(action, dict)
    }


def validate_player_management_surface_transports(
    module_id: str,
    player_management: dict,
    module_toml: dict,
) -> list[str]:
    surface = str(player_management.get("planned_surface") or "").strip().lower()
    transports = runtime_player_action_transports(module_toml)
    failures: list[str] = []

    for transport in sorted(transports):
        tokens = PLAYER_ACTION_TRANSPORT_SURFACE_TOKENS.get(transport)
        if not tokens:
            continue
        if not any(token in surface for token in tokens):
            failures.append(
                f"{module_id}: player_management.planned_surface {surface!r} does not mention runtime player action transport {transport!r}"
            )

    for transport, tokens in PLAYER_ACTION_TRANSPORT_SURFACE_TOKENS.items():
        if transport in transports:
            continue
        if any(token in surface for token in tokens):
            failures.append(
                f"{module_id}: player_management.planned_surface mentions {transport!r} but runtime.player_actions use {sorted(transports)}"
            )

    return failures


def module_has_runtime_player_read_action(module_toml: dict) -> bool:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return False
    actions = runtime.get("player_actions", [])
    if not isinstance(actions, list):
        return False
    for action in actions:
        if not isinstance(action, dict):
            continue
        action_id = str(action.get("id") or "").strip()
        if runtime_player_action_can_verify_state(action_id):
            return True
    return False


def module_has_runtime_player_targetless_read_action(module_toml: dict) -> bool:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return False
    actions = runtime.get("player_actions", [])
    if not isinstance(actions, list):
        return False
    for action in actions:
        if not isinstance(action, dict):
            continue
        action_id = str(action.get("id") or "").strip()
        command_template = str(action.get("command_template") or "")
        if "{{target}}" not in command_template and runtime_player_action_can_verify_state(action_id):
            return True
    return False


def module_has_runtime_player_mutating_action(module_toml: dict) -> bool:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return False
    actions = runtime.get("player_actions", [])
    if not isinstance(actions, list):
        return False
    for action in actions:
        if not isinstance(action, dict):
            continue
        action_id = str(action.get("id") or "").strip()
        if runtime_player_action_requires_destructive(action_id):
            return True
    return False


def module_has_online_player_list(module_toml: dict) -> bool:
    runtime = module_toml.get("runtime", {})
    if not isinstance(runtime, dict):
        return False
    player_list = runtime.get("player_list")
    if not isinstance(player_list, dict) or player_list.get("scope", "online") != "online":
        return False
    if not player_list.get("response_codec") or not player_list.get("identity_kind"):
        return False
    source = player_list.get("source")
    if source in {"http_api", "server_query"}:
        return True
    if source not in {"runtime_action", "structured_log", "console_log"}:
        return False
    actions = runtime.get("player_actions", [])
    if not isinstance(actions, list):
        return False
    return any(
        isinstance(action, dict)
        and action.get("id") == player_list.get("action_id")
        and not action.get("target_required")
        and bool(action.get("command_template"))
        and "{{target}}" not in str(action.get("command_template") or "")
        for action in actions
    )


def validate_player_management_contract(
    module_id: str,
    module_toml: dict,
    properties: dict[str, dict],
) -> list[str]:
    failures: list[str] = []
    has_roster_fields = module_has_player_roster_fields(properties)
    has_runtime_actions = module_has_runtime_player_actions(module_toml)
    has_online_list = module_has_online_player_list(module_toml)
    has_mutating_actions = module_has_runtime_player_mutating_action(module_toml)
    player_management = module_toml.get("player_management")

    if player_management is None and module_id not in TARGET_MODULE_IDS:
        return failures

    if not isinstance(player_management, dict):
        return [
            f"{module_id}: missing player management contract; declare schema roster fields, runtime.player_actions, or [player_management] pending_adapter"
        ]

    status = str(player_management.get("status") or "").strip()
    if status not in PLAYER_MANAGEMENT_STATUSES:
        failures.append(f"{module_id}: player_management.status must be one of {sorted(PLAYER_MANAGEMENT_STATUSES)}")
    if status == "runtime_actions" and not module_has_runtime_player_actions(module_toml):
        failures.append(f"{module_id}: player_management.status runtime_actions requires runtime.player_actions")
    if status == "runtime_actions" and not module_has_runtime_player_read_action(module_toml):
        failures.append(
            f"{module_id}: player_management.status runtime_actions requires at least one read/list player action"
        )
    if status == "runtime_actions" and not module_has_runtime_player_targetless_read_action(module_toml):
        failures.append(
            f"{module_id}: player_management.status runtime_actions requires at least one targetless player list/read action so the operator can discover online players or roster state"
        )
    if status == "runtime_actions" and not has_mutating_actions and not has_online_list:
        failures.append(
            f"{module_id}: player_management.status runtime_actions requires moderation actions or a declared online player-list adapter"
        )
    if status == "runtime_actions":
        failures.extend(
            validate_player_management_surface_transports(
                module_id, player_management, module_toml
            )
        )
        runtime_contract_text = " ".join(
            str(player_management.get(key) or "")
            for key in ("reason", "verification")
        )
        if not PLAYER_MANAGEMENT_RUNTIME_SURFACE_RE.search(runtime_contract_text):
            failures.append(
                f"{module_id}: runtime player_management reason/verification must name the runtime command surface or transport"
            )
        if not PLAYER_MANAGEMENT_RUNTIME_READ_RE.search(runtime_contract_text):
            failures.append(
                f"{module_id}: runtime player_management reason/verification must name the player list/read command or output used for validation"
            )
        if has_mutating_actions and not PLAYER_MANAGEMENT_RUNTIME_EFFECT_RE.search(runtime_contract_text):
            failures.append(
                f"{module_id}: runtime player_management reason/verification must name the moderation/access side effect being verified"
            )
    if has_roster_fields:
        planned_surface = str(player_management.get("planned_surface") or "")
        reason_and_verification = " ".join(
            str(player_management.get(key) or "")
            for key in ("reason", "verification")
        )
        if not PLAYER_MANAGEMENT_ROSTER_SURFACE_RE.search(planned_surface):
            failures.append(
                f"{module_id}: player_management.planned_surface must mention the persistent roster/config/launch surface because schema player-access fields are declared"
            )
        if not PLAYER_MANAGEMENT_ROSTER_MATERIALIZATION_RE.search(reason_and_verification):
            failures.append(
                f"{module_id}: player_management.reason/verification must explain how schema player-access fields are materialized or read by the server"
            )
    if status == "persistent_roster" and not module_has_player_roster_fields(properties):
        failures.append(f"{module_id}: player_management.status persistent_roster requires schema roster fields")
    if status == "persistent_roster" and not module_has_player_roster_management_fields(properties):
        failures.append(
            f"{module_id}: player_management.status persistent_roster requires at least one admin/allow/block roster field; priority-only rosters are not enough"
        )
    if status == "persistent_roster":
        persistent_contract_text = " ".join(
            str(player_management.get(key) or "")
            for key in ("planned_surface", "reason", "verification")
        )
        if not PLAYER_MANAGEMENT_PERSISTENT_FILE_RE.search(persistent_contract_text):
            failures.append(
                f"{module_id}: persistent player_management contract must name the server-read roster/config file"
            )
        if not PLAYER_MANAGEMENT_PERSISTENT_MATERIALIZATION_RE.search(persistent_contract_text):
            failures.append(
                f"{module_id}: persistent player_management contract must explain materialization or readback verification"
            )
        if not PLAYER_MANAGEMENT_PERSISTENT_EFFECT_RE.search(persistent_contract_text):
            failures.append(
                f"{module_id}: persistent player_management contract must name the admin/allow/block effect being verified"
            )
    if status == "persistent_roster" and has_runtime_actions:
        failures.append(
            f"{module_id}: player_management.status persistent_roster must not hide runtime.player_actions; use runtime_actions"
        )
    if status == "pending_adapter":
        # This status gates runtime moderation, while native roster files and read-only
        # player queries have independent contracts. A missing command transport must
        # not require deleting functioning persistent access controls or player counts.
        reason = str(player_management.get("reason") or "")
        verification = str(player_management.get("verification") or "")
        if reason.strip() and not PLAYER_MANAGEMENT_PENDING_GAP_RE.search(reason):
            failures.append(
                f"{module_id}: pending player_management.reason must name the missing or unverified player-management evidence"
            )
        if verification.strip() and not PLAYER_MANAGEMENT_PENDING_GATE_RE.search(verification):
            failures.append(
                f"{module_id}: pending player_management.verification must say runtime commands stay hidden until their transport is proven"
            )

    for key in ("planned_surface", "reason", "verification"):
        if not str(player_management.get(key) or "").strip():
            failures.append(f"{module_id}: player_management.{key} is required for {status or 'unknown'} player-management contract")

    return failures


def player_management_bucket(module_toml: dict, properties: dict[str, dict]) -> str:
    player_management = module_toml.get("player_management")
    status = ""
    if isinstance(player_management, dict):
        status = str(player_management.get("status") or "").strip()
    if status == "pending_adapter":
        return "pending_adapter"
    if module_has_runtime_player_actions(module_toml):
        return "runtime_actions"
    if status == "persistent_roster" or module_has_player_roster_fields(properties):
        return "persistent_roster"
    return "unclassified"


def pending_player_management_modules(module_manifests: dict[str, dict]) -> set[str]:
    pending: set[str] = set()
    for module_id, module_toml in module_manifests.items():
        player_management = module_toml.get("player_management")
        if isinstance(player_management, dict) and player_management.get("status") == "pending_adapter":
            pending.add(module_id)
    return pending


def read_frontend_file(relative_path: str) -> str:
    return (DESKTOP_SRC_DIR / relative_path).read_text(encoding="utf-8-sig")


def read_player_center_frontend_sources(source_dir: Path = DESKTOP_SRC_DIR) -> dict[str, str]:
    return {
        relative_path: (source_dir / relative_path).read_text(encoding="utf-8-sig")
        for relative_path in PLAYER_CENTER_FRONTEND_FILES
    }


def validate_player_center_frontend_contracts(source_dir: Path = DESKTOP_SRC_DIR) -> list[str]:
    failures: list[str] = []
    missing = [
        relative_path
        for relative_path in PLAYER_CENTER_FRONTEND_FILES
        if not (source_dir / relative_path).is_file()
    ]
    failures.extend(f"frontend: missing Player Center source {relative_path}" for relative_path in missing)
    if missing:
        return failures

    for relative_path, source in read_player_center_frontend_sources(source_dir).items():
        for contract in PLAYER_CENTER_PUBLIC_CONTRACTS[relative_path]:
            if contract not in source:
                failures.append(
                    f"frontend: {relative_path} is missing Player Center public contract {contract!r}"
                )
    return failures


def read_module_settings_source(module_id: str) -> str:
    file_stem = MODULE_SETTINGS_FILE_NAMES.get(module_id, module_id)
    module_path = DESKTOP_SRC_DIR / "views" / "settings" / "modules" / f"{file_stem}.ts"
    if not module_path.exists():
        return ""
    return module_path.read_text(encoding="utf-8-sig")


def module_settings_file_stem(module_id: str) -> str:
    return MODULE_SETTINGS_FILE_NAMES.get(module_id, module_id)


def module_settings_definition_path(module_id: str) -> Path:
    return (
        DESKTOP_SRC_DIR
        / "views"
        / "settings"
        / "modules"
        / f"{module_settings_file_stem(module_id)}.ts"
    )


def module_id_for_settings_file_stem(file_stem: str) -> str:
    for module_id, mapped_file_stem in MODULE_SETTINGS_FILE_NAMES.items():
        if mapped_file_stem == file_stem:
            return module_id
    return file_stem


def validate_settings_module_explicit_schema_keys(module_id: str, source: str) -> list[str]:
    schema_properties = {
        key
        for key, property_def in read_schema_property_defs(module_id).items()
        if property_def.get("x-lsgm-player-access-kind") not in PLAYER_ACCESS_KINDS
    }
    grouped_keys = ts_group_key_literals(source)
    missing_keys = sorted(schema_properties - grouped_keys)
    if not missing_keys:
        return []

    return [
        f"{module_id}: dedicated settings definition field groups must explicitly place schema field keys: {', '.join(missing_keys)}"
    ]


def validate_settings_module_group_schema_keys(module_id: str, source: str) -> list[str]:
    schema_properties = read_schema_properties(module_id)
    group_keys = ts_group_key_literal_entries(source)
    failures: list[str] = []

    unknown_group_keys = sorted(set(group_keys) - schema_properties)
    if unknown_group_keys:
        failures.append(
            f"{module_id}: dedicated settings definition groups reference unknown schema field keys: {', '.join(unknown_group_keys)}"
        )

    seen_keys: set[str] = set()
    duplicate_keys: set[str] = set()
    for key in group_keys:
        if key in seen_keys:
            duplicate_keys.add(key)
        seen_keys.add(key)
    if duplicate_keys:
        failures.append(
            f"{module_id}: dedicated settings definition groups contain duplicate schema field keys: {', '.join(sorted(duplicate_keys))}"
        )

    return failures


def validate_settings_module_definition_registration(
    module_id: str, registry_source: str
) -> list[str]:
    module_path = module_settings_definition_path(module_id)
    if not module_path.exists():
        return [
            f"{module_id}: missing dedicated frontend settings definition {module_path.relative_to(ROOT).as_posix()}"
        ]

    source = module_path.read_text(encoding="utf-8-sig")
    failures: list[str] = []
    export_match = re.search(
        r"export\s+const\s+([A-Za-z0-9_]+SettingsDefinition)\s*:\s*SettingsModuleDefinition\s*=",
        source,
    )
    if not export_match:
        failures.append(
            f"{module_id}: dedicated settings definition must export a typed *SettingsDefinition constant"
        )
        return failures

    export_name = export_match.group(1)
    definition_block = settings_definition_object_block(source, export_match)
    if not definition_block:
        failures.append(
            f"{module_id}: dedicated settings definition {export_name} must export an object literal"
        )
        return failures

    for marker in ("getSections", "buildFieldGroups", "getFieldCopy"):
        if not ts_object_has_property(definition_block, marker):
            failures.append(
                f"{module_id}: dedicated settings definition {export_name} must export {marker}"
            )
    for import_path in re.findall(r"from\s+[\"']\./([^\"']+)[\"']", source):
        imported_stem = Path(import_path).name
        imported_module_id = module_id_for_settings_file_stem(imported_stem)
        if imported_module_id in TARGET_MODULE_IDS and imported_module_id != module_id:
            failures.append(
                f"{module_id}: dedicated settings definition {export_name} must not import sibling game settings module {import_path!r}; keep game-specific sections/groups in its own file"
            )
    if not ts_object_has_string_property(definition_block, "id", module_id):
        failures.append(
            f"{module_id}: dedicated settings definition {export_name} does not declare id {module_id!r}"
        )
    validation_source = source
    for relative_path in MODULE_SETTINGS_ADDITIONAL_SOURCES.get(module_id, ()):
        validation_source = f"{validation_source}\n{read_frontend_file(relative_path)}"
    failures.extend(validate_settings_module_explicit_schema_keys(module_id, validation_source))
    failures.extend(validate_settings_module_group_schema_keys(module_id, validation_source))

    file_stem = module_settings_file_stem(module_id)
    import_marker = f'import {{ {export_name} }} from "./modules/{file_stem}";'
    if import_marker not in registry_source:
        failures.append(
            f"{module_id}: module-registry.ts must import {export_name} from ./modules/{file_stem}"
        )

    registry_marker = f"[{export_name}.id]: {export_name}"
    if registry_marker not in registry_source:
        failures.append(
            f"{module_id}: module-registry.ts must register {export_name} by its id"
        )

    return failures


def module_schema_sections(module_id: str) -> set[str]:
    source = read_module_settings_source(module_id)
    for relative_path in MODULE_SETTINGS_ADDITIONAL_SOURCES.get(module_id, []):
        source = f"{source}\n{read_frontend_file(relative_path)}"
    section_ids = module_settings_section_ids_from_source(source)
    if module_id == "scum":
        section_ids.update(scum_native_section_ids_from_source(source))
    return GENERIC_SCHEMA_SECTIONS | section_ids


def store_copy_has_key(source: str, object_markers: list[str], module_id: str) -> bool:
    key_pattern = re.compile(rf"^\s*{re.escape(module_id)}\s*:\s*\{{", re.MULTILINE)
    return any(key_pattern.search(extract_ts_object_block(source, marker)) for marker in object_markers)


def validate_schema_ui_metadata(module_id: str, properties: dict[str, dict]) -> list[str]:
    failures: list[str] = []
    seen_orders: set[tuple[str, float]] = set()
    allowed_sections = module_schema_sections(module_id)

    for key, property_def in sorted(properties.items()):
        if not str(property_def.get("title") or "").strip():
            failures.append(f"{module_id}: schema field {key!r} is missing title")
        if not str(property_def.get("description") or "").strip():
            failures.append(f"{module_id}: schema field {key!r} is missing description")
        if "default" not in property_def and "x-lsgm-default-source" not in property_def:
            failures.append(f"{module_id}: schema field {key!r} is missing default or x-lsgm-default-source")

        section = property_def.get("x-lsgm-section")
        if section not in allowed_sections:
            failures.append(
                f"{module_id}: schema field {key!r} has invalid x-lsgm-section {section!r}"
            )

        order = property_def.get("x-lsgm-order")
        if not isinstance(order, (int, float)):
            failures.append(f"{module_id}: schema field {key!r} is missing numeric x-lsgm-order")
            continue

        order_key = (str(section), float(order))
        if order_key in seen_orders:
            failures.append(
                f"{module_id}: duplicate x-lsgm-order {order} in section {section!r}"
            )
        seen_orders.add(order_key)

    return failures


def schema_has_type(property_def: dict, expected_type: str) -> bool:
    raw_type = property_def.get("type")
    if isinstance(raw_type, str):
        return raw_type == expected_type
    if isinstance(raw_type, list):
        return expected_type in raw_type
    return False


def is_blocked_player_access_key(key: str) -> bool:
    normalized = key.lower()
    return (
        normalized == "cosmetic_whitelist_override"
        or
        re.search(r"(password|token|secret|credential|rcon|gslt|url|endpoint|webhook|api_key)", normalized)
        or re.search(r"(^|_)permission(_|$)|permission_level|journeypermission|reserved_slots|whitelist_slots|max_admins", normalized)
        or re.search(r"(enable_|_enabled$|enforce_|notify_|hide_|can_|online_mode|steam_group_only|steam_group_admins)", normalized)
        or re.search(r"(_json$|_cfg$|_cfg_extra$|_extra$|extra_launch_args|users_cfg_extra|bans_cfg_extra)", normalized)
        or normalized == "command_permissions"
        or normalized.startswith("bans_server_")
    )


def inferred_player_access_kind(key: str) -> str | None:
    if is_blocked_player_access_key(key):
        return None
    normalized = key.lower()
    if re.search(r"(blacklist|blocklist|banned|banlist|ban_list|ban_user|ban_steam|banned_player|banned_ip|banned_steam)", normalized):
        return "block"
    if re.search(r"(skip_queue|priority_join|exclusive_join|priority)", normalized):
        return "priority"
    if re.search(r"(whitelist|allowlist|permitted|allow_list|white_list)", normalized):
        return "allow"
    if re.search(r"(admin|administrator|moderator|operator|owner|ops)", normalized):
        return "admin"
    return None


def is_player_access_value_shape(key: str, property_def: dict) -> bool:
    if schema_has_type(property_def, "array"):
        return True
    if not schema_has_type(property_def, "string"):
        return False
    if re.search(r"(entries|ids|list|users|players|names|administrators|moderators|operators|banned|whitelist|blacklist|blocklist)", key, re.I):
        return True
    return re.fullmatch(r"owner_name|owner_steam_id|owner_steam64_id", key, re.I) is not None


def is_player_access_single_value_key(key: str) -> bool:
    return re.fullmatch(r"owner_name|owner_steam_id|owner_steam64_id", key, re.I) is not None


def object_roster_identity_property(property_def: dict) -> tuple[str, dict] | None:
    if not schema_has_type(property_def, "array"):
        return None
    items = property_def.get("items")
    if not isinstance(items, dict) or not schema_has_type(items, "object"):
        return None
    item_properties = items.get("properties")
    if not isinstance(item_properties, dict):
        return None
    normalized_keys = {str(key).lower(): key for key in item_properties}
    for preferred_key in PLAYER_ROSTER_IDENTITY_PROPERTY_PRIORITY:
        actual_key = normalized_keys.get(preferred_key)
        if isinstance(actual_key, str) and isinstance(item_properties.get(actual_key), dict):
            return actual_key, item_properties[actual_key]
    for actual_key, item_property in item_properties.items():
        if isinstance(actual_key, str) and isinstance(item_property, dict) and schema_has_type(item_property, "string"):
            return actual_key, item_property
    return None


def validate_player_access_metadata(module_id: str, properties: dict[str, dict]) -> list[str]:
    failures: list[str] = []
    for key, property_def in sorted(properties.items()):
        declared_kind = property_def.get("x-lsgm-player-access-kind")
        inferred_kind = inferred_player_access_kind(key)
        has_roster_shape = is_player_access_value_shape(key, property_def)

        if declared_kind is not None:
            if declared_kind not in PLAYER_ACCESS_KINDS:
                failures.append(
                    f"{module_id}: schema field {key!r} has invalid x-lsgm-player-access-kind {declared_kind!r}"
                )
            if not has_roster_shape:
                failures.append(
                    f"{module_id}: schema field {key!r} declares player access but is not a supported string/list shape"
                )
            codec = property_def.get("x-lsgm-player-access-codec")
            if codec not in SUPPORTED_PLAYER_ACCESS_CODECS:
                failures.append(
                    f"{module_id}: schema field {key!r} must declare a supported x-lsgm-player-access-codec instead of relying on permissive inference"
                )
            if codec in DELIMITED_PLAYER_ACCESS_IDENTITY_FORMATS:
                fields = property_def.get("x-lsgm-player-access-delimited-fields")
                if not isinstance(fields, list) or not fields:
                    failures.append(
                        f"{module_id}: delimited player roster {key!r} must declare non-empty x-lsgm-player-access-delimited-fields"
                    )
                else:
                    for index, field in enumerate(fields):
                        if not isinstance(field, dict):
                            failures.append(
                                f"{module_id}: {key!r} delimited field {index} must be an object"
                            )
                            continue
                        if not str(field.get("name") or "").strip():
                            failures.append(
                                f"{module_id}: {key!r} delimited field {index} must name the segment"
                            )
                        if field.get("format") not in SUPPORTED_PLAYER_ACCESS_SEGMENT_FORMATS:
                            failures.append(
                                f"{module_id}: {key!r} delimited field {index} has unsupported format {field.get('format')!r}"
                            )
                        if not isinstance(field.get("required"), bool):
                            failures.append(
                                f"{module_id}: {key!r} delimited field {index} must declare boolean required"
                            )
                        if field.get("consumeRest") is True and (
                            index != len(fields) - 1 or field.get("format") != "text"
                        ):
                            failures.append(
                                f"{module_id}: {key!r} consumeRest is valid only on the final text segment"
                            )
                    first = fields[0] if isinstance(fields[0], dict) else {}
                    if first.get("format") != DELIMITED_PLAYER_ACCESS_IDENTITY_FORMATS[codec] or first.get("required") is not True:
                        failures.append(
                            f"{module_id}: {key!r} first delimited field must be required {DELIMITED_PLAYER_ACCESS_IDENTITY_FORMATS[codec]!r} identity"
                        )
            separators = property_def.get("x-lsgm-player-access-entry-separators")
            if separators is not None:
                if codec not in {"steam64", "uint64"} or not isinstance(separators, list) or set(separators) != {"newline", "comma"}:
                    failures.append(
                        f"{module_id}: {key!r} entry separators require explicit newline+comma metadata for steam64 or uint64 rosters"
                    )
            conflicts = property_def.get("x-lsgm-player-access-conflicts-with")
            if conflicts is not None:
                if not isinstance(conflicts, list) or not conflicts or any(not isinstance(field, str) or not field.strip() for field in conflicts):
                    failures.append(
                        f"{module_id}: {key!r} x-lsgm-player-access-conflicts-with must contain field names"
                    )
                else:
                    for conflict_key in conflicts:
                        conflict = properties.get(conflict_key)
                        reverse = conflict.get("x-lsgm-player-access-conflicts-with", []) if isinstance(conflict, dict) else []
                        if not isinstance(conflict, dict) or conflict.get("x-lsgm-player-access-kind") not in PLAYER_ACCESS_KINDS:
                            failures.append(
                                f"{module_id}: {key!r} conflict metadata references non-roster field {conflict_key!r}"
                            )
                        elif key not in reverse:
                            failures.append(
                                f"{module_id}: player-access conflict {key!r} -> {conflict_key!r} must be symmetric"
                            )
            sync = property_def.get("x-lsgm-player-access-sync")
            if not isinstance(sync, dict) or sync.get("mode") not in SUPPORTED_PLAYER_ACCESS_SYNC_MODES:
                failures.append(
                    f"{module_id}: schema field {key!r} must declare x-lsgm-player-access-sync with direct, reload, or restart mode"
                )
            if (
                has_roster_shape
                and schema_has_type(property_def, "string")
                and not is_player_access_single_value_key(key)
                and property_def.get("format") != "textarea"
            ):
                failures.append(
                    f"{module_id}: schema field {key!r} declares player access list text and must use format = textarea"
                )
            description = str(property_def.get("description") or "").strip()
            if not description:
                failures.append(
                    f"{module_id}: schema field {key!r} declares player access but has no description for the player-management UI"
                )
            elif not PLAYER_ROSTER_MATERIALIZATION_DESCRIPTION_RE.search(description):
                failures.append(
                    f"{module_id}: schema field {key!r} player-access description must explain the server-read file or materialization target"
                )
            elif not PLAYER_ROSTER_TARGET_DESCRIPTION_RE.search(description):
                failures.append(
                    f"{module_id}: schema field {key!r} player-access description must name the concrete server-read file, launch token, or native roster record"
                )
            identity_property = object_roster_identity_property(property_def)
            if schema_has_type(property_def, "array") and isinstance(property_def.get("items"), dict) and schema_has_type(property_def["items"], "object") and identity_property is None:
                failures.append(
                    f"{module_id}: schema field {key!r} object roster must declare at least one writable string identity property"
                )
            if identity_property is not None:
                identity_key, identity_def = identity_property
                if identity_key.lower() in PLAYER_ROSTER_STEAM_IDENTITY_KEYS:
                    expected_pattern = "^[0-9]{17,20}$" if "group" in key.lower() else "^[0-9]{17}$"
                    if identity_def.get("pattern") != expected_pattern:
                        failures.append(
                            f"{module_id}: schema field {key!r} object roster Steam identity {identity_key!r} must declare pattern {expected_pattern!r} so the player-management UI rejects entries the server-read file will ignore"
                        )
            continue

        if inferred_kind and has_roster_shape:
            failures.append(
                f"{module_id}: schema field {key!r} looks like player access; declare x-lsgm-player-access-kind or rename it out of the roster surface"
            )
    return failures


def validate_player_access_i18n_descriptions(module_id: str, properties: dict[str, dict]) -> list[str]:
    player_access_keys = player_roster_field_keys(properties)
    if not player_access_keys:
        return []

    failures: list[str] = []
    i18n_dir = DESKTOP_SRC_DIR / "i18n" / "games"
    for source_path in sorted(i18n_dir.glob(f"{module_id}.*.ts")):
        messages = ts_object_string_values(source_path.read_text(encoding="utf-8-sig"))
        for key in sorted(player_access_keys):
            message_key = f"settings.schema.{module_id}.{key}.description"
            description = str(messages.get(message_key) or "").strip()
            if not description:
                continue
            if not PLAYER_ROSTER_MATERIALIZATION_DESCRIPTION_RE.search(description):
                failures.append(
                    f"{module_id}: {source_path.name} translation for player-access field {key!r} must explain the server-read file or materialization target"
                )
            elif not PLAYER_ROSTER_TARGET_DESCRIPTION_RE.search(description):
                failures.append(
                    f"{module_id}: {source_path.name} translation for player-access field {key!r} must name the concrete server-read file, launch token, or native roster record"
                )
    return failures


def validate_player_roster_materialization(
    module_id: str,
    properties: dict[str, dict],
    referenced_property_keys: set[str],
) -> list[str]:
    missing = sorted(player_roster_field_keys(properties) - referenced_property_keys)
    if not missing:
        return []
    return [
        (
            f"{module_id}: player roster schema fields are not materialized by launch args, "
            f"templates, or Rust derived tokens: {missing}"
        )
    ]


def validate_no_raw_player_roster_template_references(
    module_id: str,
    properties: dict[str, dict],
) -> list[str]:
    roster_keys = sorted(player_roster_field_keys(properties))
    if not roster_keys:
        return []

    templates_dir = MODULES_DIR / module_id / "templates"
    if not templates_dir.exists():
        return []

    failures: list[str] = []
    module_root = MODULES_DIR / module_id
    for template_path in sorted(templates_dir.glob("**/*.hbs")):
        template_text = template_path.read_text(encoding="utf-8-sig")
        relative_template = str(template_path.relative_to(module_root)).replace("\\", "/")
        for key in roster_keys:
            for raw_token in (f"settings.{key}", f"json.settings.{key}", key):
                pattern = re.compile(r"{{\s*" + re.escape(raw_token) + r"\s*}}")
                if pattern.search(template_text):
                    failures.append(
                        f"{module_id}: player roster field {key!r} is rendered directly as {{{{{raw_token}}}}} in {relative_template}; route it through a module-derived token or Rust renderer"
                    )

    return failures


def validate_sevendaystodie_serveradmin_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    required_markers = (
        'const SEVENDAYSTODIE_SERVER_ADMIN_FILE: &str = "serveradmin.xml";',
        "fn materialize_sevendaystodie_support_files(",
        "&context.config_dir.join(SEVENDAYSTODIE_SERVER_ADMIN_FILE)",
        "&context.saves_dir.join(SEVENDAYSTODIE_SERVER_ADMIN_FILE)",
        "fn normalize_sevendaystodie_steam_id(value: Option<&Value>) -> Option<String>",
        "fn normalize_sevendaystodie_user_identity(",
        '"steam" => "Steam"',
        '"eos" => "EOS"',
        '"xbl" => "XBL"',
        '"psn" => "PSN"',
        "(17..=20).contains(&text.len())",
        "text.chars().all(|character| character.is_ascii_digit())",
        "fn sevendaystodie_serveradmin_lists_skip_invalid_identity_rows()",
    )
    for marker in required_markers:
        if marker not in source:
            failures.append(
                f"sevendaystodie: serveradmin.xml roster materialization guard is missing marker {marker!r}"
            )

    for field, function_name in SEVENDAYSTODIE_SERVERADMIN_OBJECT_ROSTER_RENDERERS.items():
        body = rust_function_body(function_name)
        if not body:
            failures.append(
                f"sevendaystodie: missing Rust renderer {function_name} for serveradmin.xml roster field {field}"
            )
            continue
        identity_markers = (
            (
                'normalize_sevendaystodie_steam_id(entry.get("steam_id"))',
                "seen.insert(steam_id.to_ascii_lowercase())",
            )
            if "group" in field
            else (
                "normalize_sevendaystodie_user_identity(entry)",
                "platform.to_ascii_lowercase()",
                "userid.to_ascii_lowercase()",
            )
        )
        renderer_markers = (
            f'lookup_setting_array(settings, "{field}")',
            *identity_markers,
            "continue;",
            "lines.push(format!(",
        )
        for marker in renderer_markers:
            if marker not in body:
                failures.append(
                    f"sevendaystodie: renderer {function_name} no longer filters/deduplicates {field} through its native identity marker {marker!r}"
                )

    return failures


def validate_minecraft_native_roster_json_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    required_markers = (
        "fn split_delimited_entry(line: &str, delimiter: char, fields: usize) -> Vec<String>",
        "fn normalize_minecraft_uuid(raw: &str) -> Option<String>",
        "fn normalize_minecraft_player_name(raw: &str) -> Option<String>",
        "fn normalize_minecraft_banned_ip(raw: &str) -> Option<String>",
        "fn minecraft_roster_json_filters_invalid_identity_rows()",
    )
    for marker in required_markers:
        if marker not in source:
            failures.append(
                f"minecraft: native roster JSON materialization guard is missing marker {marker!r}"
            )

    for field, (function_name, identity_normalizer) in MINECRAFT_NATIVE_JSON_ROSTER_RENDERERS.items():
        body = rust_function_body(function_name)
        if not body:
            failures.append(
                f"minecraft: missing Rust renderer {function_name} for native roster field {field}"
            )
            continue
        if field in {"operator_entries", "banned_player_entries"}:
            if f'parse_config_lines(settings, "{field}")' not in body:
                failures.append(
                    f"minecraft: renderer {function_name} no longer reads schema field {field}"
                )
        elif field == "whitelist_entries":
            call_marker = 'render_minecraft_named_uuid_json(\n            settings,\n            "whitelist_entries",'
            if call_marker not in source:
                failures.append(
                    "minecraft: whitelist_entries no longer routes through render_minecraft_named_uuid_json"
                )
        elif field == "banned_ip_entries" and f'parse_config_lines(settings, "{field}")' not in body:
            failures.append(
                f"minecraft: renderer {function_name} no longer reads schema field {field}"
            )

        renderer_markers = (
            "split_delimited_entry(&line, ','",
            identity_normalizer,
            "seen.insert(",
            "return None;",
            "serde_json::json!({",
        )
        for marker in renderer_markers:
            if marker not in body:
                failures.append(
                    f"minecraft: renderer {function_name} no longer filters/deduplicates {field} through identity marker {marker!r}"
                )
        if field != "banned_ip_entries" and "normalize_minecraft_player_name" not in body:
            failures.append(
                f"minecraft: renderer {function_name} no longer validates player names for {field}"
            )

    return failures


def validate_rust_cfg_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    required_markers = (
        "fn normalize_steam64_id(raw: &str) -> Option<String>",
        "fn rust_user_and_ban_lines_filter_invalid_steam64_entries()",
        "fn rust_skip_queue_lines_render_optional_name_and_note()",
    )
    for marker in required_markers:
        if marker not in source:
            failures.append(
                f"rust: users.cfg/bans.cfg roster materialization guard is missing marker {marker!r}"
            )

    for field, function_name in RUST_CFG_ROSTER_RENDERERS.items():
        body = rust_function_body(function_name)
        if not body:
            failures.append(
                f"rust: missing Rust renderer {function_name} for cfg roster field {field}"
            )
            continue
        if field == "owner_entries" and 'render_rust_user_lines(settings, "owner_entries", "ownerid")' not in source:
            failures.append("rust: owner_entries no longer routes through render_rust_user_lines")
        elif field == "moderator_entries" and 'render_rust_user_lines(\n            settings,\n            "moderator_entries",' not in source:
            failures.append("rust: moderator_entries no longer routes through render_rust_user_lines")
        elif field != "moderator_entries" and field != "owner_entries" and f'parse_config_lines(settings, "{field}")' not in body:
            failures.append(
                f"rust: renderer {function_name} no longer reads schema field {field}"
            )

        renderer_markers = (
            "normalize_steam64_id",
            "seen.insert(steam_id.clone())",
            "return None;",
        )
        for marker in renderer_markers:
            if marker not in body:
                failures.append(
                    f"rust: renderer {function_name} no longer filters/deduplicates {field} through Steam64 marker {marker!r}"
                )

    return failures


def validate_dst_roster_text_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in DST_ROSTER_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"dontstarve: cluster roster materialization guard is missing marker {marker!r}"
            )

    for marker in (
        'starts_with("KU_")',
        'value.contains("{{")',
        'value.contains("}}")',
        "character.is_whitespace()",
        "character.is_control()",
    ):
        if marker not in source:
            failures.append(
                f"dontstarve: Klei user ID normalizer no longer preserves marker {marker!r}"
            )

    renderer_body = rust_function_body("render_dst_klei_user_id_lines")
    for marker in (
        "parse_config_lines(settings, key)",
        "normalize_dst_klei_id(&line)",
        "seen.insert(line.to_ascii_lowercase())",
        '.join("\\n")',
    ):
        if marker not in renderer_body:
            failures.append(
                f"dontstarve: cluster roster renderer no longer preserves marker {marker!r}"
            )

    lookup_body = rust_function_body("lookup_dst_template_token_with_instance")
    for field, token in DST_ROSTER_TOKEN_COVERAGE.items():
        template_path = MODULES_DIR / "dontstarve" / "templates" / "clusters" / "main" / {
            "admin_list": "adminlist.txt.hbs",
            "whitelist": "whitelist.txt.hbs",
            "blocklist": "blocklist.txt.hbs",
        }[field]
        template_text = template_path.read_text(encoding="utf-8-sig") if template_path.exists() else ""
        if f"{{{{dst.{token}}}}}" not in template_text:
            failures.append(
                f"dontstarve: {template_path.name} must render {field} through dst.{token}, not raw settings text"
            )
        if f'"{token}"' not in lookup_body or f'render_dst_klei_user_id_lines(settings, "{field}")' not in lookup_body:
            failures.append(
                f"dontstarve: lookup_dst_template_token_with_instance no longer maps {token} to schema field {field}"
            )

    return failures


def validate_valheim_roster_text_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    required_markers = (
        "fn lookup_valheim_template_token(settings: &Map<String, Value>, path: &str) -> Option<String>",
        "fn normalize_valheim_platform_id(raw: &str) -> Option<String>",
        "fn render_valheim_platform_id_lines(settings: &Map<String, Value>, key: &str) -> String",
        "fn valheim_roster_lists_filter_to_single_platform_id_tokens()",
    )
    for marker in required_markers:
        if marker not in source:
            failures.append(
                f"valheim: roster text materialization guard is missing marker {marker!r}"
            )

    renderer_body = rust_function_body("render_valheim_platform_id_lines")
    renderer_markers = (
        "parse_config_lines(settings, key)",
        "normalize_valheim_platform_id(&line)",
        "seen.insert(line.to_ascii_lowercase())",
        '.join("\\n")',
    )
    for marker in renderer_markers:
        if marker not in renderer_body:
            failures.append(
                f"valheim: platform roster renderer no longer filters/deduplicates through marker {marker!r}"
            )

    lookup_body = rust_function_body("lookup_valheim_template_token")
    for field, token in VALHEIM_ROSTER_TOKEN_COVERAGE.items():
        template_path = MODULES_DIR / "valheim" / "templates" / {
            "admin_list": "adminlist.txt.hbs",
            "banned_list": "bannedlist.txt.hbs",
            "permitted_list": "permittedlist.txt.hbs",
        }[field]
        template_text = template_path.read_text(encoding="utf-8-sig") if template_path.exists() else ""
        if f"{{{{valheim.{token}}}}}" not in template_text:
            failures.append(
                f"valheim: {template_path.name} must render {field} through valheim.{token}, not raw settings text"
            )
        if f'"{token}"' not in lookup_body or f'render_valheim_platform_id_lines(settings, "{field}")' not in lookup_body:
            failures.append(
                f"valheim: lookup_valheim_template_token no longer maps {token} to schema field {field}"
            )

    return failures


def validate_squad_admins_cfg_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in SQUAD_ADMINS_CFG_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"squad: Admins.cfg materialization guard is missing marker {marker!r}"
            )

    body = rust_function_body("render_squad_admins_cfg")
    required_body_markers = (
        "render_squad_admin_assignment_lines(",
        "parse_squad_extra_admin_lines(settings)",
        "Group={SQUAD_ADMIN_GROUP_NAME}:",
        "Group={SQUAD_RESERVED_GROUP_NAME}:reserve",
    )
    for marker in required_body_markers:
        if marker not in body:
            failures.append(
                f"squad: render_squad_admins_cfg no longer preserves structured Admins.cfg marker {marker!r}"
            )

    return failures


def validate_unturned_commands_dat_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in UNTURNED_COMMANDS_DAT_ROSTER_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"unturned: Commands.dat roster materialization guard is missing marker {marker!r}"
            )

    owner_body = rust_function_body("render_unturned_owner_line")
    for marker in (
        'lookup_materialized_setting_text(settings, "owner_steam_id")',
        "normalize_steam64_id(&raw)",
        'format!("Owner {steam_id}")',
    ):
        if marker not in owner_body:
            failures.append(
                f"unturned: owner Commands.dat renderer no longer preserves marker {marker!r}"
            )

    admin_body = rust_function_body("render_unturned_admin_lines")
    for marker in (
        'parse_steam64_lines(settings, "admin_steam_ids")',
        'format!("Admin {steam_id}")',
    ):
        if marker not in admin_body:
            failures.append(
                f"unturned: admin Commands.dat renderer no longer preserves marker {marker!r}"
            )

    return failures


def validate_steam64_text_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in STEAM64_TEXT_ROSTER_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"steam64 text roster materialization guard is missing marker {marker!r}"
            )

    scum_body = rust_function_body("render_scum_admin_steam_ids_lines")
    if 'parse_steam64_lines(settings, "admin_steam_ids").join("\\n")' not in scum_body:
        failures.append("scum: AdminUsers renderer must use the shared Steam64 parser")

    abiotic_body = rust_function_body("render_abioticfactor_moderator_lines")
    for marker in (
        "parse_steam64_values_from_text(&raw)",
        'format!("Moderator={steam_id}")',
    ):
        if marker not in abiotic_body:
            failures.append(
                f"abioticfactor: Admin.ini moderator renderer no longer preserves marker {marker!r}"
            )

    return failures


def validate_json_account_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in JSON_ACCOUNT_ROSTER_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"JSON account roster materialization guard is missing marker {marker!r}"
            )

    delimited_body = rust_function_body("parse_steam64_values_from_text")
    for marker in (
        ".split('\\n')",
        ".filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with(\"//\"))",
        ".flat_map(|line| line.split(','))",
        "filter_map(normalize_steam64_id)",
        "seen.insert(entry.clone())",
    ):
        if marker not in delimited_body:
            failures.append(
                f"shared Steam64 delimited parser no longer preserves marker {marker!r}"
            )

    corekeeper_body = rust_function_body("normalize_corekeeper_identifier_list")
    if "parse_steam64_values_from_text(raw)" not in corekeeper_body:
        failures.append("corekeeper: roster parser must use the shared Steam64 delimited parser")

    enshrouded_body = rust_function_body("render_enshrouded_banned_accounts_json")
    for marker in (
        "parse_enshrouded_banned_account_ids(settings)",
        "Value::Number(serde_json::Number::from(account_id))",
        '"accountId"',
        '"displayName"',
        '"characterName"',
        '"banDate"',
    ):
        if marker not in enshrouded_body:
            failures.append(
                f"enshrouded: native bans renderer no longer preserves marker {marker!r}"
            )

    uint64_body = rust_function_body("normalize_uint64_id")
    for marker in ("byte.is_ascii_digit()", "value.parse::<u64>().ok()?", "Some(value.to_string())"):
        if marker not in uint64_body:
            failures.append(f"enshrouded: exact uint64 validation is missing marker {marker!r}")
    render_body = rust_function_body("render_template_text")
    if "render_enshrouded_banned_accounts_json(context.settings)?" not in render_body:
        failures.append("enshrouded: native bans rendering must propagate invalid roster errors")

    return failures


def validate_terraria_vrising_text_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in TERRARIA_BANLIST_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"terraria: banlist materialization guard is missing marker {marker!r}"
            )
    for marker in VRISING_ROSTER_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"vrising: admin/ban roster materialization guard is missing marker {marker!r}"
            )

    terraria_template = MODULES_DIR / "terraria" / "templates" / "banlist.txt.hbs"
    if terraria_template.read_text(encoding="utf-8-sig").strip() != "{{terraria.banlist_lines}}":
        failures.append("terraria: banlist.txt.hbs must render through terraria.banlist_lines")

    vrising_admin_template = MODULES_DIR / "vrising" / "templates" / "Settings" / "adminlist.txt.hbs"
    vrising_ban_template = MODULES_DIR / "vrising" / "templates" / "Settings" / "banlist.txt.hbs"
    if vrising_admin_template.read_text(encoding="utf-8-sig").strip() != "{{vrising.admin_list_lines}}":
        failures.append("vrising: adminlist.txt.hbs must render through vrising.admin_list_lines")
    if vrising_ban_template.read_text(encoding="utf-8-sig").strip() != "{{vrising.ban_list_lines}}":
        failures.append("vrising: banlist.txt.hbs must render through vrising.ban_list_lines")

    terraria_body = rust_function_body("render_terraria_banlist_lines")
    for marker in (
        'parse_config_lines(settings, "banlist_entries")',
        "normalize_terraria_banlist_entry(&entry)",
        "seen.insert(entry.to_ascii_lowercase())",
    ):
        if marker not in terraria_body:
            failures.append(
                f"terraria: banlist renderer no longer preserves marker {marker!r}"
            )

    terraria_normalizer_body = rust_function_body("normalize_terraria_banlist_entry")
    for marker in ('value.contains("{{")', 'value.contains("}}")', "character.is_control()"):
        if marker not in terraria_normalizer_body:
            failures.append(
                f"terraria: banlist normalizer no longer preserves marker {marker!r}"
            )

    vrising_body = rust_function_body("render_vrising_steam64_lines")
    for marker in (
        "lookup_setting_text(settings, key)",
        "parse_steam64_values_from_text(&raw).join(\"\\n\")",
    ):
        if marker not in vrising_body:
            failures.append(
                f"vrising: roster renderer no longer preserves marker {marker!r}"
            )

    return failures


def validate_barotrauma_roster_rendering() -> list[str]:
    source = read_app_storage_templates_rs()
    failures: list[str] = []
    for marker in BAROTRAUMA_CLIENT_PERMISSIONS_GUARD_MARKERS:
        if marker not in source:
            failures.append(
                f"barotrauma: clientpermissions.xml materialization guard is missing marker {marker!r}"
            )

    barotrauma_template = (
        MODULES_DIR / "barotrauma" / "templates" / "Data" / "clientpermissions.xml.hbs"
    )
    if barotrauma_template.read_text(encoding="utf-8-sig").strip() != "{{barotrauma.client_permissions_xml}}":
        failures.append(
            "barotrauma: clientpermissions.xml.hbs must render through barotrauma.client_permissions_xml"
        )

    barotrauma_normalizer = rust_function_body("normalize_barotrauma_account")
    for marker in (
        'upper.strip_prefix("STEAM_")',
        'upper.starts_with("[U:1:")',
        "const STEAM64_BASE",
        'format!("STEAM_1:{}:{}", account_id % 2, account_id / 2)',
    ):
        if marker not in barotrauma_normalizer:
            failures.append(
                f"barotrauma: Steam identity normalizer no longer preserves marker {marker!r}"
            )

    barotrauma_renderer = rust_function_body("render_barotrauma_client_permissions_xml")
    for marker in (
        'parse_config_lines(settings, "admin_entries")',
        "normalize_barotrauma_account(part)",
        "escape_xml_attribute(display_name)",
        "permissions=\\\"All\\\"",
    ):
        if marker not in barotrauma_renderer:
            failures.append(
                f"barotrauma: client permissions renderer no longer preserves marker {marker!r}"
            )

    return failures


def validate_strict_frontend_schema_authority(module_id: str) -> list[str]:
    source = read_module_settings_source(module_id)
    for relative_path in MODULE_SETTINGS_ADDITIONAL_SOURCES.get(module_id, []):
        source = f"{source}\n{read_frontend_file(relative_path)}"
    if not source:
        return []

    return [
        (
            f"{module_id}: strict module frontend still contains {marker}; "
            "use schema x-lsgm-section/x-lsgm-order as the field grouping truth"
        )
        for marker in FRONTEND_SECTION_ORDER_OVERRIDE_MARKERS
        if marker in source
    ]


def validate_frontend_coverage() -> list[str]:
    failures: list[str] = []

    api_source = read_frontend_file("api.ts")
    types_source = read_frontend_file("types.ts")
    store_copy_source = read_frontend_file("store-copy.ts")
    servers_view_source = read_frontend_file("views/ServersView.tsx")
    runtime_surface_source = read_frontend_file("views/servers/RuntimeSurfaceWorkbench.tsx")
    guided_settings_source = read_frontend_file("views/settings/guided-settings.ts")
    module_registry_source = read_frontend_file("views/settings/module-registry.ts")
    desktop_actions_source = read_frontend_file("hooks/useDesktopActions.ts")
    api_mock_source = "\n".join(
        read_frontend_file(relative_path)
        for relative_path in (
            "api-mock.ts",
            "api-mock/module-details.ts",
            "api-mock/module-manifest.ts",
        )
    )
    player_access_actions_source = read_frontend_file("domain/player-access.ts")
    commands_source = read_desktop_tauri_commands_rs()
    storage_instances_source = (
        APP_STORAGE_INSTANCES_RS.read_text(encoding="utf-8-sig")
        if APP_STORAGE_INSTANCES_RS.exists()
        else ""
    )
    player_access_storage_path = APP_STORAGE_INSTANCES_RS.parent / "player_access.rs"
    player_access_storage_source = (
        player_access_storage_path.read_text(encoding="utf-8-sig")
        if player_access_storage_path.exists()
        else ""
    )
    instance_settings_lock_path = APP_STORAGE_INSTANCES_RS.parent / "instance_settings_lock.rs"
    instance_settings_lock_source = (
        instance_settings_lock_path.read_text(encoding="utf-8-sig")
        if instance_settings_lock_path.exists()
        else ""
    )
    failures.extend(validate_player_center_frontend_contracts())
    player_access_model_markers = (
        '"x-lsgm-player-access-codec"',
        '"x-lsgm-player-access-sync"',
        "export function decodePlayerAccessEntries(",
        "export function buildPlayerAccessBindings",
        "export function filterConsumedPlayerAccessActions",
    )
    for marker in player_access_model_markers:
        if marker not in player_access_actions_source:
            failures.append(
                f"frontend: player-access action model marker is missing {marker!r}"
            )
    api_dispatch_markers = (
        'invokeOrMock<InstanceRuntimeCommandResult>("send_instance_runtime_command"',
        "input: {",
        "instanceId,",
        "command,",
        "processKey,",
        "transport: options.transport",
        "portName: options.portName",
        "passwordSettingKey: options.passwordSettingKey",
        "enabledSettingKey: options.enabledSettingKey",
    )
    for marker in api_dispatch_markers:
        if marker not in api_source:
            failures.append(
                f"frontend: sendInstanceRuntimeCommand no longer forwards runtime command metadata marker {marker!r}"
            )
    roster_settings_chain_markers = (
        ("frontend", desktop_actions_source, "async function handleSaveSettings(input: UpdateInstanceInput, saveOptions: SaveInstanceSettingsOptions = {})"),
        ("frontend", desktop_actions_source, "saveOptions.expectedSettingsJson"),
        ("frontend", desktop_actions_source, "const panel = await refreshInstancePanelAfterMutation(input.id);"),
        ("frontend", desktop_actions_source, "if (saveOptions.throwOnError)"),
        ("frontend", api_source, 'invokeOrMock<InstanceDetails>("update_instance_record_if_current", { input, expectedSettingsJson })'),
        ("backend", commands_source, "pub async fn update_instance_record_if_current("),
        ("backend", storage_instances_source, "pub async fn update_instance_if_current("),
        ("backend", storage_instances_source, "fn settings_json_matches(current: &str, expected: &str) -> bool"),
        ("backend", storage_instances_source, "StorageError::InstanceSettingsPreconditionFailed"),
        ("backend", commands_source, '"config_file_path": details.config_file_path'),
    )
    for layer, source, marker in roster_settings_chain_markers:
        if marker not in source:
            failures.append(
                f"{layer}: roster/settings save chain no longer preserves materialization marker {marker!r}"
            )
    failures.extend(validate_roster_settings_frontend_save_chain(desktop_actions_source))
    failures.extend(validate_roster_settings_backend_save_chain(commands_source))
    player_access_mutation_chain_markers = (
        ("frontend", types_source, "export interface InstancePlayerAccessMutationInput"),
        ("frontend", api_source, 'invokeOrMock<InstancePlayerAccessMutationResult>("apply_instance_player_access_mutation"'),
        ("frontend", desktop_actions_source, "async function handleApplyPlayerAccessMutation("),
        ("frontend", desktop_actions_source, "result = await applyInstancePlayerAccessMutation(input);"),
        ("backend", commands_source, "pub async fn apply_instance_player_access_mutation("),
        ("backend", commands_source, "let persistent = persist_player_access_mutation(&storage.paths, input)"),
        ("backend", commands_source, "dispatch_player_access_action("),
        ("backend", player_access_storage_source, "let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;"),
        ("backend", player_access_storage_source, "let details = read_instance_details(paths, instance_id).await?;"),
        ("backend", player_access_storage_source, "let patched = patch_field_value("),
        ("backend", player_access_storage_source, "let updated = update_instance_locked("),
        ("backend", player_access_storage_source, '"expectedValue is required for scalar player-access fields"'),
        ("backend", player_access_storage_source, '"expectedValue is only valid for scalar player-access fields"'),
    )
    for layer, source, marker in player_access_mutation_chain_markers:
        if marker not in source:
            failures.append(
                f"{layer}: semantic player-access mutation chain marker is missing {marker!r}"
            )
    persistent_dispatch_index = commands_source.find(
        "let persistent = persist_player_access_mutation(&storage.paths, input)"
    )
    live_dispatch_index = commands_source.find(
        "dispatch_player_access_action(", persistent_dispatch_index
    )
    if not (0 <= persistent_dispatch_index < live_dispatch_index):
        failures.append(
            "backend: player-access orchestration must persist/materialize before dispatching any live action"
        )
    player_access_lock_index = player_access_storage_source.find(
        "let settings_lock = acquire_instance_settings_mutation_lock(paths, instance_id)?;"
    )
    player_access_read_index = player_access_storage_source.find(
        "let details = read_instance_details(paths, instance_id).await?;",
        player_access_lock_index,
    )
    player_access_patch_index = player_access_storage_source.find(
        "let patched = patch_field_value(", player_access_read_index
    )
    player_access_write_index = player_access_storage_source.find(
        "let updated = update_instance_locked(", player_access_patch_index
    )
    if not (
        0
        <= player_access_lock_index
        < player_access_read_index
        < player_access_patch_index
        < player_access_write_index
    ):
        failures.append(
            "backend: player-access read/patch/write must remain inside one per-instance storage mutation lock"
        )
    failures.extend(validate_instance_settings_lock_contract(instance_settings_lock_source))
    settings_cas_start = storage_instances_source.find(
        "pub async fn update_instance_if_current("
    )
    settings_cas_end = storage_instances_source.find(
        "fn settings_json_matches(", settings_cas_start
    )
    settings_cas_source = (
        storage_instances_source[settings_cas_start:settings_cas_end]
        if settings_cas_start >= 0 and settings_cas_end > settings_cas_start
        else ""
    )
    settings_cas_lock_index = settings_cas_source.find(
        "acquire_instance_settings_mutation_lock(paths, &input.id)?"
    )
    settings_cas_read_index = settings_cas_source.find(
        "read_instance_details(paths, &input.id).await?"
    )
    settings_cas_compare_index = settings_cas_source.find(
        "settings_json_matches(&current.settings_json, expected_settings_json)"
    )
    settings_cas_write = re.search(
        r"\bupdate_instance_with_baseline_locked\(paths, input, &settings_lock, Some\(current\.settings_json\)\)\s*\.await",
        settings_cas_source,
    )
    settings_cas_write_index = settings_cas_write.start() if settings_cas_write else -1
    if not (
        0
        <= settings_cas_lock_index
        < settings_cas_read_index
        < settings_cas_compare_index
        < settings_cas_write_index
    ):
        failures.append(
            "backend: ordinary settings precondition read/compare/write must remain inside one per-instance storage mutation lock"
        )
    lifecycle_mutation_lock_markers = (
        "start_instance_process_after_reconcile_reserved(",
        "stop_instance_process(",
        "delete_instance_record(",
        "state.acquire_instance_mutation",
    )
    for marker in lifecycle_mutation_lock_markers:
        if marker not in commands_source:
            failures.append(
                f"backend: instance lifecycle/player-access serialization marker is missing {marker!r}"
            )
    update_instance_start = storage_instances_source.find("pub async fn update_instance(")
    update_instance_end = storage_instances_source.find(
        "pub async fn materialize_instance_configuration(",
        update_instance_start if update_instance_start >= 0 else 0,
    )
    update_instance_source = (
        storage_instances_source[update_instance_start:update_instance_end]
        if update_instance_start >= 0 and update_instance_end > update_instance_start
        else ""
    )
    storage_update_markers = (
        "let settings = normalize_complete_instance_settings(",
        "parse_settings_object(&input.settings_json)?",
        "write_pending_instance_configuration_in_worker(",
        "settings: &settings,",
        "commit_instance_transaction(tx, config_mutation, settings_lock).await?;",
    )
    for marker in storage_update_markers:
        if marker not in update_instance_source:
            failures.append(
                f"backend: app-storage update_instance no longer materializes saved settings marker {marker!r}"
            )
    ordered_storage_markers = (
        "let settings = normalize_complete_instance_settings(",
        "write_pending_instance_configuration_in_worker(",
        "commit_instance_transaction(tx, config_mutation, settings_lock).await?;",
    )
    last_index = -1
    for marker in ordered_storage_markers:
        current_index = update_instance_source.find(marker)
        if current_index < 0:
            continue
        if current_index < last_index:
            failures.append(
                "backend: app-storage update_instance must normalize settings, coordinate native/config writes in its worker, then commit in that order"
            )
            break
        last_index = current_index
    normalization_markers = (
        "pub fn normalize_complete_instance_settings(",
        "let mut settings = merge_schema_defaults_with_settings(",
        'String::from("bind_ip")',
        "validate_settings_against_schema(descriptor, &settings, SettingsValidationPhase::Complete)?",
    )
    for marker in normalization_markers:
        if marker not in storage_instances_source:
            failures.append(
                f"backend: complete settings normalization no longer preserves marker {marker!r}"
            )
    failures.extend(validate_runtime_console_boundary(
        runtime_surface_source, read_frontend_file("runtime-console-transport.ts")
    ))
    types_player_management_markers = (
        "export interface ModulePlayerActionDetails {",
        "label_zh_cn?: string | null;",
        'transport?: "stdin" | "source_rcon" | "websocket_rcon" | "battleye_rcon" | "telnet" | string | null;',
        "command_template: string;",
        "target_label?: string | null;",
        "target_label_zh_cn?: string | null;",
        "target_placeholder?: string | null;",
        "target_placeholder_zh_cn?: string | null;",
        "target_required?: boolean;",
        'target_encoding?: "raw" | "quoted_string" | string | null;',
        "role_values?: string[];",
        "process_key?: string | null;",
        "port_name?: string | null;",
        "password_setting_key?: string | null;",
        "enabled_setting_key?: string | null;",
        "destructive?: boolean;",
        "export interface ModulePlayerManagementDetails {",
        "status: string;",
        "planned_surface: string;",
        "reason: string;",
        "verification: string;",
        "export interface ModuleRuntimeDetails {",
        "player_actions?: ModulePlayerActionDetails[];",
        "player_management?: ModulePlayerManagementDetails | null;",
    )
    for marker in types_player_management_markers:
        if marker not in types_source:
            failures.append(
                f"frontend: types.ts no longer preserves player-management payload marker {marker!r}"
            )
    tauri_dispatch_markers = (
        "pub async fn send_instance_runtime_command",
        "process_key: Option<String>",
        "transport: Option<String>",
        "port_name: Option<String>",
        "password_setting_key: Option<String>",
        "enabled_setting_key: Option<String>",
        "dispatch_source_rcon_command(",
        "dispatch_websocket_rcon_command(",
        "dispatch_battleye_rcon_command(",
        "dispatch_telnet_command(",
        "let ResolvedRuntimeCommand {",
    )
    for marker in tauri_dispatch_markers:
        if marker not in commands_source:
            failures.append(
                f"backend: send_instance_runtime_command no longer handles runtime command metadata marker {marker!r}"
            )
    tauri_remote_command_setting_markers = (
        "failed to parse instance settings for RCON",
        "RCON command is disabled because `{enabled_key}` is not enabled for this instance",
        'unwrap_or("rcon_password")',
        'unwrap_or("rcon")',
        "instance has no `{port_key}` port for RCON",
        "RCON port `{port_key}` must be TCP",
        "failed to parse instance settings for WebSocket RCON",
        "WebSocket RCON command is disabled because `{enabled_key}` is not enabled for this instance",
        "WebSocket RCON password setting `{password_key}` is empty",
        "WebSocket RCON port `{port_key}` must be TCP",
        "failed to parse instance settings for BattlEye RCON",
        "BattlEye RCON command is disabled because `{enabled_key}` is not enabled for this instance",
        "BattlEye RCON password setting `{password_key}` is empty",
        "BattlEye RCON port `{port_key}` must be UDP",
        "failed to parse instance settings for Telnet",
        "Telnet command is disabled because `{enabled_key}` is disabled for this instance",
        'unwrap_or("telnet_password")',
        'unwrap_or("telnet")',
        "instance has no `{port_key}` port for Telnet",
        "Telnet port `{port_key}` must be TCP",
        ".find(|port| port.name.eq_ignore_ascii_case(port_key))",
        "normalize_query_host(&details.summary.bind_ip)",
    )
    for marker in tauri_remote_command_setting_markers:
        if marker not in commands_source:
            failures.append(
                f"backend: remote runtime command dispatch no longer reads instance setting/port marker {marker!r}"
            )
    failures.extend(validate_server_detail_tool_tabs(
        servers_view_source,
        read_frontend_file("views/servers/server-detail-tab-specs.ts"),
    ))
    if "localizationModuleId" not in guided_settings_source:
        failures.append("frontend: generic schema localization does not use module id fallback")
    if "player_actions: []" not in api_mock_source:
        failures.append(
            "frontend: api-mock must not hard-code runtime.player_actions; module.toml is the player-action truth"
        )
    if "player_management: null" not in api_mock_source:
        failures.append(
            "frontend: api-mock must not hard-code runtime.player_management; module.toml is the player-management contract truth"
        )
    if "command_template:" in api_mock_source:
        failures.append(
            "frontend: api-mock contains player command templates; keep runtime.player_actions out of mock data"
        )
    for marker in (
        "target_required:",
        "target_encoding:",
        "role_values:",
        "password_setting_key:",
        "enabled_setting_key:",
    ):
        if marker in api_mock_source:
            failures.append(
                f"frontend: api-mock contains runtime player-action metadata marker {marker!r}; keep player actions out of mock data"
            )

    return failures


def validate_rust_player_management_mapping() -> list[str]:
    failures: list[str] = []
    app_modules_source = APP_MODULES_LIB_RS.read_text(encoding="utf-8-sig") if APP_MODULES_LIB_RS.exists() else ""
    app_core_source = APP_CORE_LIB_RS.read_text(encoding="utf-8-sig") if APP_CORE_LIB_RS.exists() else ""

    player_action_fields = (
        "id",
        "label",
        "label_zh_cn",
        "transport",
        "command_template",
        "target_label",
        "target_label_zh_cn",
        "target_placeholder",
        "target_placeholder_zh_cn",
        "target_required",
        "target_encoding",
        "role_values",
        "process_key",
        "port_name",
        "password_setting_key",
        "enabled_setting_key",
        "destructive",
    )
    for field in player_action_fields:
        if f"{field}:" not in app_modules_source:
            failures.append(
                f"backend: app-modules no longer deserializes runtime.player_actions field {field!r}"
            )
        if f"pub {field}:" not in app_core_source:
            failures.append(
                f"backend: app-core ModulePlayerActionSpec no longer serializes field {field!r}"
            )
        if f"{field}: action.{field}" not in app_modules_source and field not in {"transport", "target_required", "role_values", "destructive"}:
            failures.append(
                f"backend: runtime_spec_from_toml no longer maps runtime.player_actions field {field!r}"
            )

    mapping_markers = (
        'transport: action.transport.unwrap_or_else(|| String::from("stdin"))',
        "target_required: action.target_required.unwrap_or(false)",
        "role_values: action.role_values.unwrap_or_default()",
        "destructive: action.destructive.unwrap_or(false)",
        "player_management: player_management.map(|spec| ModulePlayerManagementSpec",
        "status: spec.status",
        "planned_surface: spec.planned_surface",
        "reason: spec.reason",
        "verification: spec.verification",
    )
    for marker in mapping_markers:
        if marker not in app_modules_source:
            failures.append(
                f"backend: app-modules runtime/player-management mapping marker is missing {marker!r}"
            )

    test_markers = (
        "fn discover_modules_reads_runtime_player_actions()",
        'transport = \\"source_rcon\\"',
        'target_encoding = \\"quoted_string\\"',
        'password_setting_key = \\"rcon_password\\"',
        'enabled_setting_key = \\"rcon_enabled\\"',
        'role_values = [\\"USER\\", \\"ADMIN\\"]',
        "assert_eq!(player_management.status",
        "assert_eq!(player_management.planned_surface",
        "assert_eq!(player_management.reason",
        "assert_eq!(player_management.verification",
    )
    for marker in test_markers:
        if marker not in app_modules_source:
            failures.append(
                f"backend: app-modules runtime player-action discovery test marker is missing {marker!r}"
            )

    return failures


def validate_target_module_manifest(module_id: str, module_toml: dict, surface_text: str) -> list[str]:
    failures: list[str] = []

    if module_toml.get("id") != module_id:
        failures.append(f"{module_id}: module.toml id does not match directory name")
    if not str(module_toml.get("name") or "").strip():
        failures.append(f"{module_id}: module.toml is missing name")
    if not str(module_toml.get("description") or "").strip():
        failures.append(f"{module_id}: module.toml is missing description")

    supported_platforms = module_toml.get("supported_platforms", [])
    if "windows" not in supported_platforms:
        failures.append(f"{module_id}: supported_platforms must include windows")

    install = module_toml.get("install", {})
    if not isinstance(install, dict) or not str(install.get("shared_game_dir") or "").strip():
        failures.append(f"{module_id}: install.shared_game_dir is required for stable Windows install roots")

    process = module_toml.get("process", {})
    if not isinstance(process, dict):
        failures.append(f"{module_id}: missing process section")
    else:
        if not str(process.get("executable") or "").strip():
            failures.append(f"{module_id}: process.executable is required")
        if process.get("window_policy") not in {"background", "external"}:
            failures.append(f"{module_id}: process.window_policy must be background or external")
        if process.get("host_surface") not in {"managed_terminal", "managed_pseudo_console", "managed_native_window", "external_window"}:
            failures.append(f"{module_id}: process.host_surface is missing or invalid")
        if not str(process.get("host_notes") or "").strip():
            failures.append(f"{module_id}: process.host_notes must explain the Windows hosting surface")

    storage = module_toml.get("storage", {})
    if not isinstance(storage, dict) or not str(storage.get("saves_path_template") or "").strip():
        failures.append(f"{module_id}: storage.saves_path_template is required for instance backup routing")

    ports_seen: set[str] = set()
    for index, port_def in enumerate(module_toml.get("default_ports", []), start=1):
        if not isinstance(port_def, dict):
            failures.append(f"{module_id}: default_ports[{index}] must be a table")
            continue

        port_name = str(port_def.get("name") or f"port{index}")
        if not re.fullmatch(r"[A-Za-z0-9_]+", port_name):
            failures.append(f"{module_id}: default port name {port_name!r} must be identifier-safe")
        if port_name in ports_seen:
            failures.append(f"{module_id}: duplicate default port name {port_name!r}")
        ports_seen.add(port_name)

        protocol = str(port_def.get("protocol") or "").lower()
        if protocol not in {"tcp", "udp"}:
            failures.append(f"{module_id}: default port {port_name!r} has invalid protocol {protocol!r}")

        port_number = port_def.get("port")
        if not isinstance(port_number, int) or not (1 <= port_number <= 65535):
            failures.append(f"{module_id}: default port {port_name!r} has invalid port {port_number!r}")

    port_refs = referenced_ports(module_id, surface_text)
    missing_ports = sorted(port_refs - ports_seen)
    if missing_ports:
        failures.append(f"{module_id}: ports referenced without default_ports entries: {missing_ports}")

    runtime = module_toml.get("runtime", {})
    port_roles = runtime.get("port_roles", []) if isinstance(runtime, dict) else []
    if not port_roles:
        failures.append(f"{module_id}: runtime.port_roles must classify every default port")
    else:
        roles_seen: set[str] = set()
        classified_ports: dict[str, str] = {}
        for index, role_spec in enumerate(port_roles, start=1):
            if not isinstance(role_spec, dict):
                failures.append(f"{module_id}: runtime.port_roles[{index}] must be a table")
                continue
            role = str(role_spec.get("role") or "")
            if role not in {"player", "service"}:
                failures.append(f"{module_id}: runtime.port_roles[{index}] has invalid role {role!r}")
            if role in roles_seen:
                failures.append(f"{module_id}: runtime.port_roles repeats role {role!r}")
            roles_seen.add(role)
            for port_name in role_spec.get("port_names", []):
                normalized = str(port_name)
                if not normalized:
                    failures.append(f"{module_id}: runtime.port_roles[{index}] contains an empty port name")
                elif normalized not in ports_seen:
                    failures.append(f"{module_id}: runtime.port_roles references unknown port {normalized!r}")
                elif normalized in classified_ports:
                    failures.append(
                        f"{module_id}: port {normalized!r} belongs to both {classified_ports[normalized]!r} and {role!r} roles"
                    )
                else:
                    classified_ports[normalized] = role
        unclassified_ports = sorted(ports_seen - classified_ports.keys())
        if unclassified_ports:
            failures.append(
                f"{module_id}: runtime.port_roles does not classify default ports {unclassified_ports}"
            )

    failures.extend(validate_runtime_player_actions(module_id, module_toml, surface_text))

    return failures


def main() -> int:
    failures: list[str] = []
    module_tomls: dict[str, dict] = {}
    player_management_counts = {
        "runtime_actions": 0,
        "persistent_roster": 0,
        "pending_adapter": 0,
        "unclassified": 0,
    }
    player_runtime_action_count = 0
    player_roster_field_count = 0
    failures.extend(validate_frontend_coverage())
    failures.extend(validate_rust_player_management_mapping())
    failures.extend(validate_sevendaystodie_serveradmin_roster_rendering())
    failures.extend(validate_minecraft_native_roster_json_rendering())
    failures.extend(validate_rust_cfg_roster_rendering())
    failures.extend(validate_dst_roster_text_rendering())
    failures.extend(validate_dst_native_settings_contract(ROOT))
    failures.extend(validate_scum_native_settings_contract(ROOT))
    failures.extend(validate_valheim_roster_text_rendering())
    failures.extend(validate_squad_admins_cfg_rendering())
    failures.extend(validate_unturned_commands_dat_roster_rendering())
    failures.extend(validate_steam64_text_roster_rendering())
    failures.extend(validate_json_account_roster_rendering())
    failures.extend(validate_terraria_vrising_text_roster_rendering())
    failures.extend(validate_barotrauma_roster_rendering())
    module_manifest_cache: dict[str, tuple[dict, str]] = {}
    for module_id in sorted(TARGET_MODULE_IDS):
        module_root = MODULES_DIR / module_id
        if not (module_root / "module.toml").exists():
            continue
        module_toml = read_module_toml(module_id)
        module_tomls[module_id] = module_toml
        surface_text = read_rendered_surfaces(module_id)
        module_manifest_cache[module_id] = (module_toml, surface_text)
        failures.extend(validate_target_module_manifest(module_id, module_toml, surface_text))

    for module_id in sorted(SCHEMA_COVERAGE_MODULE_IDS):
        module_root = MODULES_DIR / module_id
        if not (module_root / "module.toml").exists():
            failures.append(f"{module_id}: missing module.toml")
            continue

        module_toml, surface_text = module_manifest_cache.get(module_id) or (
            read_module_toml(module_id),
            read_rendered_surfaces(module_id),
        )
        module_tomls.setdefault(module_id, module_toml)
        property_defs = read_schema_property_defs(module_id)
        properties = set(property_defs)
        refs, unknown_module_tokens = referenced_settings(module_id, surface_text, properties)
        port_refs = referenced_ports(module_id, surface_text)
        port_names = declared_ports(module_toml)

        missing_schema = sorted(refs - properties)
        unreferenced_properties = sorted(properties - refs)
        missing_ports = sorted(port_refs - port_names)

        if module_id in STRICT_MODULE_IDS:
            failures.extend(validate_schema_ui_metadata(module_id, property_defs))
            failures.extend(validate_strict_frontend_schema_authority(module_id))
        failures.extend(validate_player_access_metadata(module_id, property_defs))
        failures.extend(validate_player_access_i18n_descriptions(module_id, property_defs))
        failures.extend(validate_player_roster_materialization(module_id, property_defs, refs))
        failures.extend(validate_no_raw_player_roster_template_references(module_id, property_defs))
        failures.extend(validate_player_management_contract(module_id, module_toml, property_defs))
        player_management_counts[player_management_bucket(module_toml, property_defs)] += 1
        player_runtime_action_count += runtime_player_action_count(module_toml)
        player_roster_field_count += len(player_roster_field_keys(property_defs))
        for token in sorted(unknown_module_tokens):
            failures.append(f"{module_id}: unknown module-specific template token {token}")
        if missing_schema:
            failures.append(f"{module_id}: settings referenced without schema fields: {missing_schema}")
        if missing_ports:
            failures.append(f"{module_id}: ports referenced without default_ports entries: {missing_ports}")
        if unreferenced_properties:
            failures.append(
                f"{module_id}: schema fields not used by launch args, templates, or derived tokens: {unreferenced_properties}"
            )

    if failures:
        print("module setting coverage verification failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    pending_modules = sorted(pending_player_management_modules(module_tomls))
    pending_suffix = f"; pending modules: {', '.join(pending_modules)}" if pending_modules else ""

    print(
        f"module setting coverage verified for {len(STRICT_MODULE_IDS)} strict modules; "
        f"schema coverage verified for {len(SCHEMA_COVERAGE_MODULE_IDS)} modules; "
        f"manifest and frontend coverage verified for {len(TARGET_MODULE_IDS)} target games; "
        f"player management coverage: {player_management_counts['runtime_actions']} runtime action modules, "
        f"{player_management_counts['persistent_roster']} persistent roster modules, "
        f"{player_management_counts['pending_adapter']} pending adapters; "
        f"{player_runtime_action_count} runtime player actions; "
        f"{player_roster_field_count} schema roster fields"
        f"{pending_suffix}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
