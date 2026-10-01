"""Extract option facts from an owned DST package without executing game/mod Lua."""
from __future__ import annotations

import hashlib
from pathlib import Path
import re
import zipfile

from dst_world_options_lua import Reference, assigned_table, assignment_tables, parse_table, registrations, remove_comments

LOCATIONS = ("forest", "cave")
CATEGORIES = {"worldgen": "WORLDGEN_GROUP", "settings": "WORLDSETTINGS_GROUP"}
DYNAMIC = {"tasksets.GetGenTaskLists", "startlocations.GetGenStartLocations"}
MAX_SOURCE_BYTES = 4 * 1024 * 1024


def _read(bundle: zipfile.ZipFile, path: str) -> str:
    entries = [entry for entry in bundle.infolist() if entry.filename == path]
    if len(entries) != 1:
        raise ValueError(f"Missing or duplicate package source: {path}")
    if entries[0].file_size > MAX_SOURCE_BYTES:
        raise ValueError(f"Package Lua source exceeds inspection limit: {path}")
    return remove_comments(bundle.read(entries[0]).decode("utf-8-sig"))


def _pc_table(source: str, name: str) -> str:
    if name in ("frequency_descriptions", "worldgen_frequency_descriptions"):
        branch = re.search(r"if IsNotConsole\(\) then(.*?)\belse\b(.*?)\bend\b", source, re.S)
        selected = branch.group(1) if branch else ""
        expected_count = 2
    elif name == "size_descriptions":
        branch = re.search(r"if IsPS4\(\) then(.*?)\belse\b(.*?)\bend\b", source, re.S)
        selected = branch.group(2) if branch else ""
        expected_count = 2
    else:
        selected, expected_count = source, 1
    tables = assignment_tables(selected, name)
    if len(tables) != 1 or len(assignment_tables(source, name)) != expected_count:
        raise ValueError(f"Unsupported Lua description/PC branch: {name}")
    return tables[0]


def _description(source: str, name: str) -> list[str]:
    for call in re.finditer(r"\b([A-Za-z_][\w.]*)\s*\(\s*" + re.escape(name) + r"\b", source):
        if call.group(1) not in ("ipairs", "pairs"):
            raise ValueError(f"Unsupported dynamic description operation: {name}")
    if name == "ocean_worldgen_frequency_descriptions":
        pattern = r'ocean_worldgen_frequency_descriptions\[i\]\s*=\s*\{\s*text\s*=\s*data.text,\s*data\s*=\s*"ocean_"\s*\.\.\s*data.data\s*\}'
        if not re.search(pattern, source) or len(re.findall(r"\bocean_worldgen_frequency_descriptions\s*\[", source)) != 1:
            raise ValueError("Unsupported ocean worldgen description mapping")
        return ["ocean_" + value for value in _description(source, "worldgen_frequency_descriptions")]
    if re.search(r"\b" + re.escape(name) + r"\s*(?:\[|\.)", source):
        raise ValueError(f"Unsupported indexed description operation: {name}")
    table = parse_table(_pc_table(source, name), text_expressions=True)
    if list(table) != list(range(1, len(table) + 1)):
        raise ValueError(f"Unsupported non-array description table: {name}")
    result = []
    for entry in table.values():
        if not isinstance(entry, dict) or set(entry) != {"text", "data"} or not isinstance(entry["data"], str):
            raise ValueError(f"Unsupported description entry: {name}")
        result.append(entry["data"])
    if not result or len(result) != len(set(result)):
        raise ValueError(f"Empty or duplicate description values: {name}")
    return result


def _dynamic_descriptions(bundle: zipfile.ZipFile) -> dict[str, dict[str, list[str]]]:
    task_source = _read(bundle, "scripts/map/tasksets.lua")
    start_source = _read(bundle, "scripts/map/startlocations.lua")
    if "if not v.hideinfrontend and world == nil or v.location == world then" not in task_source:
        raise ValueError("Unsupported task set frontend filter")
    if "if world == nil or v.location == world then" not in start_source:
        raise ValueError("Unsupported start location frontend filter")
    modules = re.findall(r'require\("(map/tasksets/[^"\n]+)"\)', task_source)
    if not modules or len(modules) != len(set(modules)):
        raise ValueError("Missing or duplicate task set modules")
    tasks: dict[str, dict] = {}
    for module in modules:
        if not re.fullmatch(r"map/tasksets/[a-z_]+", module):
            raise ValueError(f"Unsupported task set module: {module}")
        source = _read(bundle, "scripts/" + module + ".lua")
        entries = registrations(source, "AddTaskSet", standalone=True)
        if len(re.findall(r"\bAddTaskSet\s*\(", source)) != len(entries):
            raise ValueError(f"Unsupported dynamic task set registration: {module}")
        if tasks.keys() & entries.keys():
            raise ValueError(f"Duplicate task set registration: {module}")
        tasks.update(entries)
    first_start = re.search(r'\bAddStartLocation\("', start_source)
    if first_start is None:
        raise ValueError("Missing literal start location registrations")
    preceding_ends = list(re.finditer(r"\bend\b", start_source[:first_start.start()]))
    block = start_source[preceding_ends[-1].end() if preceding_ends else 0:]
    export = re.search(r"\breturn\s*\{", block)
    if export:
        block = block[:export.start()]
    starts = registrations(block, "AddStartLocation", standalone=True)
    if len(re.findall(r"\bAddStartLocation\s*\(", start_source)) != len(starts) + 1:
        raise ValueError("Unsupported dynamic start location registration")
    # pairs() order is unspecified, so function-produced enum values use stable key order.
    return {
        location: {
            "tasksets.GetGenTaskLists": sorted(key for key, value in tasks.items() if value.get("location", "forest") == location),
            "startlocations.GetGenStartLocations": sorted(key for key, value in starts.items() if value.get("location") == location),
        }
        for location in LOCATIONS
    }


def _extract(bundle: zipfile.ZipFile) -> dict:
    source = _read(bundle, "scripts/map/customize.lua")
    tables = {category: assigned_table(source, name, local_only=True) for category, name in CATEGORIES.items()}
    # This reader intentionally models the current unmodded client filtering contract.
    for predicate in (
        "if location == nil or item.world == nil or table.contains(item.world, location) then",
        "if is_master_world or not item.master_controlled then",
    ):
        if predicate not in source:
            raise ValueError(f"Unsupported customization visibility predicate: {predicate}")
    for name in ("MOD_WORLDGEN_GROUP", "MOD_WORLDSETTINGS_GROUP"):
        if assigned_table(source, name, local_only=True):
            raise ValueError(f"Unsupported nonempty built-in mod group: {name}")
    dynamic = _dynamic_descriptions(bundle)
    descriptions: dict[str, list[str]] = {}
    groups: dict[str, list[dict]] = {}
    options = []
    for category, table in tables.items():
        groups[category] = []
        for group_id, group in table.items():
            if not isinstance(group, dict) or set(group) != {"order", "text", "desc", "atlas", "items"}:
                raise ValueError(f"Unsupported customization group: {category}.{group_id}")
            if not isinstance(group["text"], Reference):
                raise ValueError(f"Unsupported group text reference: {group_id}")
            groups[category].append({"id": group_id, "order": group["order"], "textKey": group["text"].name})
            for key, item in group["items"].items():
                allowed = {"value", "image", "world", "desc", "order", "options_remap", "master_controlled", "masteroption", "master_sync"}
                if not isinstance(item, dict) or not {"value", "image"} <= item.keys() or item.keys() - allowed:
                    raise ValueError(f"Unsupported customization option properties: {key}")
                desc = item.get("desc") or group["desc"]
                if not isinstance(desc, Reference):
                    raise ValueError(f"Unsupported description reference: {key}")
                if desc.name not in DYNAMIC and desc.name not in descriptions:
                    descriptions[desc.name] = _description(source, desc.name)
                worlds = list(item["world"].values()) if "world" in item else list(LOCATIONS)
                if not worlds or set(worlds) - set(LOCATIONS) or len(worlds) != len(set(worlds)):
                    raise ValueError(f"Unsupported option locations: {key}")
                locations = [location for location in LOCATIONS if location in worlds]
                values = {location: dynamic[location][desc.name] if desc.name in DYNAMIC else descriptions[desc.name] for location in locations}
                options.append({"key": key, "category": category, "group": group_id, "order": item.get("order"),
                                "locations": locations, "masterControlled": item.get("master_controlled", False), "values": values})
        groups[category].sort(key=lambda group: (group["order"], group["id"]))
    # Independent declaration scan catches items missed by a changed table layout.
    declared = re.findall(r'\["([^"\n]+)"\]\s*=\s*\{\s*value\s*=', source)
    keys = [item["key"] for item in options]
    if len(declared) != len(keys) or set(declared) != set(keys) or len(keys) != len(set(keys)):
        raise ValueError("Unsupported customization declaration layout or duplicate option keys")
    order = {(category, group["id"]): group["order"] for category, entries in groups.items() for group in entries}
    options.sort(key=lambda item: (list(CATEGORIES).index(item["category"]), order[item["category"], item["group"]], item["order"] if item["order"] is not None else float("inf"), item["key"]))
    return {"groups": groups, "options": options}


def extract_inventory(install_root: Path) -> dict:
    version_path = install_root / "version.txt"
    version = version_path.read_text(encoding="utf-8-sig").strip()
    if not re.fullmatch(r"[0-9]+", version):
        raise ValueError("Unsupported game version.txt: expected numeric game version")
    archive = install_root / "data/databundles/scripts.zip"
    before = archive.stat()
    with archive.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
        stream.seek(0)
        with zipfile.ZipFile(stream) as bundle:
            result = _extract(bundle)
    after = archive.stat()
    if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns) or version_path.read_text(encoding="utf-8-sig").strip() != version:
        raise ValueError("Game package changed during extraction; retry after its update completes")
    return {"gameVersion": version, "scriptsSha256": digest, **result}
