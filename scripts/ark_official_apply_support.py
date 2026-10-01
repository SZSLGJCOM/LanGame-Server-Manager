"""Shared schema and ledger helpers for the pinned ARK crosswalk applicator."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

PROJECT_FIELD_DESCRIPTIONS: dict[str, dict[str, dict[str, str]]] = json.loads(
    Path(__file__).with_name("ark_field_descriptions.json").read_text(encoding="utf-8")
)

SECTION_MAP = {
    "Command line options": "operations",
    "[ServerSettings]": "gameplay",
    "[SessionSettings]": "session",
    "[/Script/Engine.GameSession]": "operations",
    "[Ragnarok]": "world",
    "[MessageOfTheDay]": "session",
    "Game.ini": "advanced",
    "[ModInstaller]": "mods",
}

REPEATED_VALUE_NAMES = {
    "excludeitemindices",
    "modids",
    "overrideplayerlevelengrampoints",
}

CONFLICTS = {
    "forceuseperfthreads": ["noperfthreads", "onethread"],
    "noperfthreads": ["forceuseperfthreads", "onethread"],
    "onethread": ["forceuseperfthreads", "noperfthreads"],
    "nodinos": [
        "no_dinos_except_forced_spawn",
        "no_dinos_except_streaming_spawn",
        "no_dinos_except_manual_spawn",
        "no_dinos_except_water_spawn",
    ],
}


def canonical(value: str) -> str:
    value = value.split(".")[-1].lstrip("-?")
    return re.sub(r"[^a-z0-9]+", "", value.lower())


def ledger_identity(source: str, key: str) -> tuple[str, str]:
    if source == "launch_args":
        kind = "url" if key.startswith("?") or key.lower().startswith("server_url.") else "arg"
        return source, f"{kind}:{canonical(key)}"
    return source, re.sub(r"[^a-z0-9]+", "", key.lower())


def prune_group_keys(text: str, removed_keys: set[str]) -> str:
    def prune(match: re.Match[str]) -> str:
        block = match.group(1)
        for key in removed_keys:
            quoted = re.escape(json.dumps(key))
            block = re.sub(rf"{quoted}\s*,\s*", "", block)
            block = re.sub(rf",\s*{quoted}", "", block)
            block = re.sub(quoted, "", block)
        return f"keys: [{block}]"

    return re.sub(r"keys:\s*\[(.*?)\]", prune, text, flags=re.DOTALL)


def prune_ledger_items(
    text: str,
    removed_keys: set[str],
    rejected_identities: set[tuple[str, str]],
) -> str:
    blocks = re.split(r"(?=^\[\[(?:sources|items|exclusions)\]\])", text, flags=re.MULTILINE)
    kept: list[str] = []
    for block in blocks:
        if block.startswith("[[items]]"):
            match = re.search(r'^schema_key\s*=\s*"([^"]+)"', block, re.MULTILINE)
            if match and match.group(1) in removed_keys:
                continue
            source = re.search(r'^source\s*=\s*"([^"]+)"', block, re.MULTILINE)
            key = re.search(r'^key\s*=\s*"([^"]+)"', block, re.MULTILINE)
            if source and key and ledger_identity(source.group(1), key.group(1)) in rejected_identities:
                continue
        kept.append(block)
    return "".join(kept)


def source_parts(item: dict[str, Any]) -> tuple[str, str, str]:
    surface = item["native_surface"]
    native = item["native_name"]
    if surface in {"launch_arg", "server_url"}:
        return "launch_args", native, "launch_arg"
    if surface.startswith("GameUserSettings.ini"):
        section = surface.split(":", 1)[1]
        return "game_user_settings", f"{section.strip('[]')}.{native}", "config_file"
    if surface.startswith("Game.ini"):
        section = surface.split(":", 1)[1]
        materializer = "materializer" if repeated(item) else "config_file"
        return "game_ini", f"{section.strip('[]')}.{native}", materializer
    if surface == "external_dynamic_config":
        # DynamicConfig aliases reuse a schema field whose actual managed
        # output is the preferred local INI surface. `derived` is an audit
        # classification, not a writable provenance surface.
        return "dynamic_config", native, "config_file"
    raise ValueError(surface)


def repeated(item: dict[str, Any]) -> bool:
    raw_type = item["type"]
    name = item["native_name"]
    return (
        raw_type in {"(...)", '"<string>"'}
        or "[<" in name
        or "<_type>" in name
        or name.startswith("CheatTeleportLocations")
        or canonical(name) in REPEATED_VALUE_NAMES
    )


def indexed_repeated(item: dict[str, Any]) -> bool:
    name = item["native_name"]
    return "[<" in name or "<_type>" in name


def writer_native_key(item: dict[str, Any]) -> str:
    name = item["native_name"]
    name = re.sub(r"\[<[^>]+>\]", "", name)
    return name.replace("<_type>", "")


def scalar_default(item: dict[str, Any]) -> Any | None:
    raw_type = item["type"]
    raw = item["default"]
    if raw_type == "boolean":
        return raw.lower() == "true" if raw.lower() in {"true", "false"} else None
    if raw_type == "integer":
        try:
            return int(float(raw))
        except ValueError:
            return None
    if raw_type in {"float", "seconds"}:
        try:
            return float(raw)
        except ValueError:
            return None
    return ""


def inferred_launch_type(item: dict[str, Any]) -> str | None:
    raw = item["raw_name"].lower()
    if "=<" not in raw:
        return None
    if any(marker in raw for marker in ("float", "multiplier")):
        return "number"
    if any(marker in raw for marker in ("integer", "seconds", "epoch time")):
        return "integer"
    return "string"


def apply_project_descriptions(schema: dict[str, Any], module_id: str) -> None:
    for key, descriptions in PROJECT_FIELD_DESCRIPTIONS[module_id].items():
        # A stale help entry signals a changed contract, not a field to recreate.
        schema["properties"][key]["description"] = descriptions["en"]


def property_for(
    item: dict[str, Any], order: int, *, section: str | None = None,
    description: str | None = None, module_id: str | None = None,
) -> dict[str, Any]:
    raw_type = item["type"]
    is_repeated = repeated(item)
    is_launch_flag = item["native_surface"] == "launch_arg" and "=" not in item["raw_name"]
    launch_type = inferred_launch_type(item)
    if is_repeated:
        prop: dict[str, Any] = {"type": "string", "format": "textarea"}
    elif raw_type == "boolean" or is_launch_flag:
        prop = {"type": "boolean"}
    elif launch_type:
        prop = {"type": launch_type}
    elif raw_type == "integer":
        prop = {"type": "integer"}
    elif raw_type in {"float", "seconds"}:
        prop = {"type": "number"}
    else:
        prop = {"type": "string"}
    if is_repeated:
        default: Any | None = ""
    elif is_launch_flag:
        default = False
    elif launch_type in {"integer", "number"}:
        raw_default = item["default"].strip()
        if not raw_default:
            default = None
        else:
            try:
                default = int(float(raw_default)) if launch_type == "integer" else float(raw_default)
            except ValueError:
                default = None
    else:
        default = scalar_default(item)
    prop.update(
        {
            "title": item["native_name"],
            "x-lsgm-section": section or SECTION_MAP[item["section"].split(" > ")[-1]],
            "x-lsgm-order": order,
        }
    )
    # Keep reviewed project help separate from upstream metadata and prose.
    curated = PROJECT_FIELD_DESCRIPTIONS.get(module_id or "", {}).get(item.get("schema_key", ""))
    if curated:
        description = curated["en"]
    if module_id and not description:
        raise ValueError(f"Missing project-authored help for {module_id}:{item.get('schema_key')}")
    if description:
        prop["description"] = description
    if default is not None:
        prop["default"] = default
    else:
        prop["x-lsgm-default-source"] = "official_unspecified"
    if canonical(item["native_name"]) == "eventcolorschanceoverride":
        prop.update({"type": "number", "minimum": 0, "maximum": 1})
        prop.pop("default", None)
    if canonical(item["native_name"]) in {
        "fishinglootqualitymultiplier",
        "supplycratelootqualitymultiplier",
    }:
        prop.update({"minimum": 1, "maximum": 5})
    conflicts = CONFLICTS.get(canonical(item["native_name"]))
    if conflicts:
        prop["x-lsgm-conflicts-with"] = conflicts
    source, key, surface = source_parts(item)
    prop.update(
        {
            "x-lsgm-source": source,
            "x-lsgm-source-key": key,
            "x-lsgm-source-surface": surface,
        }
    )
    return prop
