"""DST native world-option inventory, schema contract, and reviewable serialization."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import tempfile

from dst_world_options_extract import extract_inventory
from dst_world_options_lua import parse_table

PAGES = {
    "mastergen": ("worldgen", "forest", True),
    "mastersettings": ("settings", "forest", True),
    "cavesgen": ("worldgen", "cave", False),
    "cavessettings": ("settings", "cave", False),
}
MODULE = Path("modules/dontstarve")


def validate_inventory(inventory: dict) -> None:
    if not isinstance(inventory, dict) or set(inventory) != {"gameVersion", "scriptsSha256", "groups", "options"}:
        raise ValueError("Invalid native inventory root")
    if not isinstance(inventory["gameVersion"], str) or not re.fullmatch(r"\d+", inventory["gameVersion"]):
        raise ValueError("Invalid inventory gameVersion")
    if not isinstance(inventory["scriptsSha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", inventory["scriptsSha256"]):
        raise ValueError("Invalid inventory scriptsSha256")
    groups = inventory["groups"]
    if not isinstance(groups, dict) or set(groups) != {"worldgen", "settings"}:
        raise ValueError("Invalid inventory categories")
    group_ids: dict[str, set[str]] = {}
    for category, entries in groups.items():
        if not isinstance(entries, list) or not entries:
            raise ValueError(f"Invalid inventory groups: {category}")
        ids: set[str] = set()
        for group in entries:
            if (not isinstance(group, dict) or set(group) != {"id", "order", "textKey"}
                    or not isinstance(group["id"], str) or group["id"] in ids
                    or type(group["order"]) is not int or not isinstance(group["textKey"], str)
                    or not group["textKey"].startswith("STRINGS.UI.")):
                raise ValueError(f"Invalid or duplicate inventory group: {category}")
            ids.add(group["id"])
        group_ids[category] = ids
    options = inventory["options"]
    if not isinstance(options, list) or not options:
        raise ValueError("Invalid inventory options")
    keys: set[str] = set()
    for option in options:
        if not isinstance(option, dict) or set(option) != {"key", "category", "group", "order", "locations", "masterControlled", "values"}:
            raise ValueError("Invalid native option properties")
        key = option["key"]
        if not isinstance(key, str) or not re.fullmatch(r"[a-z_][a-z_0-9]*", key) or key in keys:
            raise ValueError(f"Invalid or duplicate option key: {key}")
        keys.add(key)
        if option["category"] not in group_ids or option["group"] not in group_ids[option["category"]]:
            raise ValueError(f"{key}: unknown inventory category/group")
        if option["order"] is not None and type(option["order"]) is not int:
            raise ValueError(f"{key}: invalid order")
        if type(option["masterControlled"]) is not bool:
            raise ValueError(f"{key}: invalid masterControlled")
        locations, values = option["locations"], option["values"]
        if not isinstance(locations, list) or not locations or any(location not in ("forest", "cave") for location in locations) or len(set(locations)) != len(locations):
            raise ValueError(f"{key}: invalid locations")
        if not isinstance(values, dict) or set(values) != set(locations):
            raise ValueError(f"{key}: values must cover exactly its locations")
        for choices in values.values():
            if not isinstance(choices, list) or not choices or any(not isinstance(value, str) for value in choices) or len(set(choices)) != len(choices):
                raise ValueError(f"{key}: invalid enum values")


def compare_schema(inventory: dict, schema: dict) -> list[str]:
    validate_inventory(inventory)
    properties = schema.get("properties")
    if not isinstance(properties, dict):
        raise ValueError("DST schema has no properties object")
    errors: list[str] = []
    for section, (category, location, is_master) in PAGES.items():
        expected = {option["key"]: option for option in inventory["options"] if option["category"] == category
                    and location in option["locations"] and (is_master or not option["masterControlled"])}
        actual: dict[str, dict] = {}
        for field, prop in properties.items():
            source_key = prop.get("x-lsgm-source-key", "")
            if prop.get("x-lsgm-section") != section or not source_key.startswith("overrides."):
                continue
            key = source_key.removeprefix("overrides.")
            if key in actual:
                errors.append(f"{section}: duplicate {key} mapping ({field})")
            actual[key] = prop
        for key in sorted(expected.keys() - actual.keys()):
            errors.append(f"{section}: missing {key}")
        for key in sorted(actual.keys() - expected.keys()):
            errors.append(f"{section}: unexpected {key}")
        for key in sorted(actual.keys() & expected.keys()):
            wanted, found = expected[key]["values"][location], actual[key].get("enum")
            # Native task/start registries iterate with pairs(), which has no stable order.
            matches = sorted(found or []) == sorted(wanted) if key in ("task_set", "start_location") else found == wanted
            if not matches:
                errors.append(f"{section}.{key}: enum expected {wanted!r}; schema has {found!r}")
    return errors


def compare_inventory(tracked: dict, actual: dict) -> list[str]:
    errors: list[str] = []
    for metadata in ("gameVersion", "scriptsSha256"):
        if tracked.get(metadata) != actual.get(metadata):
            errors.append(f"{metadata}: tracked {tracked.get(metadata)!r}; package {actual.get(metadata)!r}")
    for category in ("worldgen", "settings"):
        before = {group["id"]: group for group in tracked["groups"][category]}
        after = {group["id"]: group for group in actual["groups"][category]}
        for key in sorted(before.keys() | after.keys()):
            if key not in before or key not in after:
                errors.append(f"{category}: {'new' if key in after else 'missing'} group {key}")
                continue
            for attribute in ("order", "textKey"):
                if before[key][attribute] != after[key][attribute]:
                    errors.append(f"{category}.{key}.{attribute}: tracked {before[key][attribute]!r}; package {after[key][attribute]!r}")
    before = {option["key"]: option for option in tracked["options"]}
    after = {option["key"]: option for option in actual["options"]}
    for key in sorted(before.keys() | after.keys()):
        if key not in before or key not in after:
            errors.append(f"options: {'new' if key in after else 'missing'} {key}")
            continue
        for attribute in ("category", "group", "order", "locations", "masterControlled", "values"):
            if before[key][attribute] != after[key][attribute]:
                errors.append(f"{key}.{attribute}: tracked {before[key][attribute]!r}; package {after[key][attribute]!r}")
    return errors


def write_inventory(path: Path, inventory: dict) -> None:
    validate_inventory(inventory)
    def encode(value: object) -> str:
        return json.dumps(value, ensure_ascii=False, separators=(",", ":"))
    lines = ["{", f'  "gameVersion":{encode(inventory["gameVersion"])},', f'  "scriptsSha256":{encode(inventory["scriptsSha256"])},', '  "groups":{']
    for index, (category, groups) in enumerate(inventory["groups"].items()):
        lines.append(f'    "{category}":[')
        lines.extend("      " + encode(group) + ("," if offset + 1 < len(groups) else "") for offset, group in enumerate(groups))
        lines.append("    ]" + ("," if index == 0 else ""))
    lines.extend(["  },", '  "options":['])
    lines.extend("    " + encode(option) + ("," if index + 1 < len(inventory["options"]) else "") for index, option in enumerate(inventory["options"]))
    lines.extend(["  ]", "}"])
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", newline="\n", dir=path.parent, prefix=".dst-options-", suffix=".json", delete=False) as stream:
            temporary = Path(stream.name)
            stream.write("\n".join(lines) + "\n")
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def verify_repository(root: Path) -> list[str]:
    try:
        inventory = json.loads((root / MODULE / "world-options.json").read_text(encoding="utf-8"))
        schema = json.loads((root / MODULE / "schema.json").read_text(encoding="utf-8"))
        return compare_schema(inventory, schema)
    except (OSError, ValueError, TypeError, KeyError) as error:
        return [f"DST world options: {error}"]
