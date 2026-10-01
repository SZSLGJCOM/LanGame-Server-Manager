from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
MODULE_ROOT = ROOT / "modules" / "scum"
INVENTORY_ROOT = MODULE_ROOT / "server-settings-v7"
SECTION_COUNTS = {
    "General": 66,
    "World": 142,
    "Features": 135,
    "Respawn": 29,
    "Vehicles": 46,
    "Damage": 19,
}
EXPECTED_SHA256 = "2a51702b2c516b026e61d013d7d4fa4d4cb6a54a14ad51ba860e501bb3642ba4"
EXPECTED_SERVER_SETTINGS_FILE_SHA256 = (
    "FAE30EDB50B77F09AE9CD318F31816379E7E116A8EF1204109FF1C513FC8BF0D"
)
EXPECTED_JSON_FILE_SHA256 = {
    "EconomyOverride.json": "7B5993EDB295F47FA43556C31DA58B585CC47707A061506C8E3D72711BF0ECD0",
    "RaidTimes.json": "99F62CCFCA6271271B4A4DD25C2381C8CB243CF66C807E62D343A13662491DDA",
    "Notifications.json": "CEE739C424D233F601F7E806717E0D3ECE5F1BEE45CF01274C260201054A3F95",
}
# The archived native baseline remains reproducible; the August release adds these
# four settings with explicit native names, sections and defaults.
OFFICIAL_AUGUST_ADDITIONS = (
    ("World", "scum.MaxAllowedApexFacilityKeycards", "4"),
    ("World", "scum.MaxAllowedApexFacilityKeycards_PoliceStation", "3"),
    ("World", "scum.MaxAllowedApexFacilityKeycards_RadiationZone", "1"),
    ("Respawn", "scum.CloningSicknessEnabled", "True"),
)
OFFICIAL_AUGUST_SOURCE = "official_august_update_1_3_3_0"
SECTION_SCHEMA_KEYS = {
    section: f"server_{section.lower()}" for section in SECTION_COUNTS
}
HIGH_RISK_KEYS = {
    "scum.PartialWipe",
    "scum.GoldWipe",
    "scum.FullWipe",
    "scum.MasterServerIsLocalTest",
}


@dataclass(frozen=True)
class NativeSetting:
    section: str
    native_key: str
    native_default: str


def parse_generated_ini(path: Path) -> list[NativeSetting]:
    settings: list[NativeSetting] = []
    section = ""
    for line_number, raw_line in enumerate(
        path.read_text(encoding="utf-8-sig").splitlines(), start=1
    ):
        line = raw_line.strip()
        if not line or line.startswith((";", "#")):
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            if section not in SECTION_COUNTS:
                raise ValueError(f"line {line_number}: unexpected section {section!r}")
            continue
        if "=" not in raw_line or not section:
            raise ValueError(f"line {line_number}: malformed SCUM generated INI")
        native_key, native_default = raw_line.split("=", 1)
        native_key = native_key.strip()
        if not native_key.startswith("scum."):
            raise ValueError(f"line {line_number}: unexpected key {native_key!r}")
        settings.append(NativeSetting(section, native_key, native_default.strip()))
    return settings


def schema_key(native_key: str) -> str:
    suffix = native_key.removeprefix("scum.")
    snake = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", suffix)
    snake = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", snake)
    return re.sub(r"[^A-Za-z0-9]+", "_", snake).strip("_").lower()


def value_contract(native_default: str) -> tuple[str, Any]:
    if native_default in {"True", "False"}:
        return "boolean", native_default == "True"
    if re.fullmatch(r"-?\d+", native_default):
        return "integer", int(native_default)
    if re.fullmatch(r"-?(?:\d+\.\d*|\d*\.\d+)", native_default):
        return "number", float(native_default)
    return "string", native_default


def title_for(native_key: str) -> str:
    suffix = native_key.removeprefix("scum.").replace(".", " ")
    words = re.sub(r"([a-z0-9])([A-Z])", r"\1 \2", suffix)
    words = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1 \2", words)
    return words.replace("NPC", "NPC").replace("BCU", "BCU")


def inventory_record(setting: NativeSetting) -> dict[str, Any]:
    value_type, default = value_contract(setting.native_default)
    record: dict[str, Any] = {
        "section": setting.section,
        "key": schema_key(setting.native_key),
        "nativeKey": setting.native_key,
        "title": title_for(setting.native_key),
        "type": value_type,
        "default": default,
        "nativeDefault": setting.native_default,
        "presentation": (
            "generated"
            if setting.native_key == "scum.ServerSettingsVersion"
            else "specialized"
        ),
    }
    if setting.native_key == "scum.MaxPlayers":
        record.update({"minimum": 1, "maximum": 128})
    if setting.native_key in HIGH_RISK_KEYS:
        record["risk"] = "confirmation"
    return record


def render_inventory(records: list[dict[str, Any]]) -> str:
    lines = ["["]
    for index, record in enumerate(records):
        suffix = "," if index + 1 < len(records) else ""
        lines.append(
            "  "
            + json.dumps(record, ensure_ascii=False, separators=(",", ":"), sort_keys=True)
            + suffix
        )
    lines.append("]")
    return "\n".join(lines) + "\n"


def write_inventory(source: Path) -> None:
    source_digest = hashlib.sha256(source.read_bytes()).hexdigest().upper()
    if source_digest != EXPECTED_SERVER_SETTINGS_FILE_SHA256:
        raise ValueError(
            f"ServerSettings.ini SHA256 is {source_digest}, "
            f"expected {EXPECTED_SERVER_SETTINGS_FILE_SHA256}"
        )
    parsed = parse_generated_ini(source)
    records = [inventory_record(setting) for setting in parsed]
    records.extend(inventory_record(NativeSetting(*setting)) for setting in OFFICIAL_AUGUST_ADDITIONS)
    failures = validate_inventory(records)
    if failures:
        raise ValueError("; ".join(failures))
    INVENTORY_ROOT.mkdir(parents=True, exist_ok=True)
    for section in SECTION_COUNTS:
        section_records = [record for record in records if record["section"] == section]
        (INVENTORY_ROOT / f"{section.lower()}.json").write_text(
            render_inventory(section_records), encoding="utf-8", newline="\n"
        )


def write_json_defaults(source_root: Path) -> None:
    destination_root = MODULE_ROOT / "native-defaults"
    destination_root.mkdir(parents=True, exist_ok=True)
    for name in ("EconomyOverride.json", "RaidTimes.json", "Notifications.json"):
        source = source_root / name
        source_digest = hashlib.sha256(source.read_bytes()).hexdigest().upper()
        if source_digest != EXPECTED_JSON_FILE_SHA256[name]:
            raise ValueError(
                f"{name} SHA256 is {source_digest}, expected {EXPECTED_JSON_FILE_SHA256[name]}"
            )
        document = json.loads(source.read_text(encoding="utf-8-sig"))
        (destination_root / name).write_text(
            json.dumps(document, ensure_ascii=False, indent=2, sort_keys=False) + "\n",
            encoding="utf-8",
            newline="\n",
        )


def write_ledger() -> None:
    from audit_scum_ledger import write_scum_ledger

    write_scum_ledger(MODULE_ROOT, read_inventory(), SECTION_SCHEMA_KEYS)


def read_inventory() -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for section in SECTION_COUNTS:
        path = INVENTORY_ROOT / f"{section.lower()}.json"
        records.extend(json.loads(path.read_text(encoding="utf-8")))
    return records


def validate_inventory(records: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    native_keys = [str(record.get("nativeKey", "")) for record in records]
    schema_keys = [str(record.get("key", "")) for record in records]
    if len(records) != 437:
        failures.append(f"inventory has {len(records)} records, expected 437")
    if len(set(native_keys)) != len(native_keys):
        failures.append("inventory repeats native keys")
    if len(set(schema_keys)) != len(schema_keys):
        failures.append("inventory repeats schema keys")
    digest = hashlib.sha256("\n".join(sorted(native_keys)).encode()).hexdigest()
    if digest != EXPECTED_SHA256:
        failures.append(f"inventory key digest is {digest}, expected {EXPECTED_SHA256}")
    for section, count in SECTION_COUNTS.items():
        actual = sum(record.get("section") == section for record in records)
        if actual != count:
            failures.append(f"{section} has {actual} records, expected {count}")
    generated = [
        record.get("nativeKey")
        for record in records
        if record.get("presentation") == "generated"
    ]
    if generated != ["scum.ServerSettingsVersion"]:
        failures.append(f"generated inventory is {generated!r}")
    if sum(record.get("presentation") == "specialized" for record in records) != 436:
        failures.append("inventory must classify exactly 436 settings as specialized")
    if {
        record.get("nativeKey")
        for record in records
        if record.get("risk") == "confirmation"
    } != HIGH_RISK_KEYS:
        failures.append("high-risk inventory does not match the four confirmed booleans")
    return failures


def read_schema() -> dict[str, Any]:
    return json.loads((MODULE_ROOT / "schema.json").read_text(encoding="utf-8-sig"))


def write_schema_inventory() -> None:
    schema = read_schema()
    properties = schema["properties"]
    for section, field_key in SECTION_SCHEMA_KEYS.items():
        nested: dict[str, Any] = {}
        for record in read_inventory():
            if record["section"] != section:
                continue
            prop: dict[str, Any] = {
                "type": record["type"],
                "default": record["default"],
                "title": record["title"],
                "description": (
                    f"Official August 2026 SCUM parameter {record['nativeKey']}."
                    if record["nativeKey"] in {entry[1] for entry in OFFICIAL_AUGUST_ADDITIONS}
                    else f"Exact server-generated SCUM v7 parameter {record['nativeKey']}."
                ),
                "x-lsgm-native-key": record["nativeKey"],
            }
            if record["type"] == "string":
                prop["pattern"] = r"^[^\r\n]*$"
            if "minimum" in record:
                prop["minimum"] = record["minimum"]
            if "maximum" in record:
                prop["maximum"] = record["maximum"]
            if record["presentation"] == "generated":
                prop["readOnly"] = True
            if record.get("risk"):
                prop["x-lsgm-risk"] = record["risk"]
            nested[record["key"]] = prop
        properties[field_key]["properties"] = nested
    (MODULE_ROOT / "schema.json").write_text(
        json.dumps(schema, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def verify_schema(records: list[dict[str, Any]]) -> list[str]:
    failures: list[str] = []
    properties = read_schema().get("properties", {})
    expected = set(SECTION_SCHEMA_KEYS.values()) | {
        "economy_override",
        "raid_times",
        "notifications",
        "admin_steam_ids",
        "extra_launch_args",
    }
    if set(properties) != expected:
        failures.append(
            "schema property set differs "
            f"(missing={sorted(expected - set(properties))}, extra={sorted(set(properties) - expected)})"
        )
    for section, key in SECTION_SCHEMA_KEYS.items():
        prop = properties.get(key, {})
        if prop.get("type") != "object" or prop.get("x-lsgm-section") != section.lower():
            failures.append(f"{key} must be a structured {section} Configuration field")
            continue
        nested = prop.get("properties", {})
        expected = {record["key"]: record for record in records if record["section"] == section}
        if set(nested) != set(expected):
            failures.append(f"{key} nested schema inventory differs from exact {section} keys")
            continue
        for nested_key, record in expected.items():
            nested_prop = nested[nested_key]
            if (
                nested_prop.get("type") != record["type"]
                or nested_prop.get("default") != record["default"]
                or nested_prop.get("x-lsgm-native-key") != record["nativeKey"]
            ):
                failures.append(f"{key}.{nested_key} differs from its exact inventory contract")
                break
    return failures


def verify_ledger(records: list[dict[str, Any]]) -> list[str]:
    ledger = tomllib.loads(
        (MODULE_ROOT / "config-sources.toml").read_text(encoding="utf-8-sig")
    )
    failures: list[str] = []
    if ledger.get("status") != "download_verified_requires_elevation":
        failures.append("SCUM provenance status must preserve requires-elevation evidence")
    items = ledger.get("items", [])
    server_items = {
        item.get("key"): item
        for item in items
        if item.get("source") in {"generated_current_server_settings", OFFICIAL_AUGUST_SOURCE}
    }
    expected_keys = {record["nativeKey"] for record in records}
    if set(server_items) != expected_keys:
        failures.append(
            "ledger generated ServerSettings inventory differs "
            f"(missing={len(expected_keys - set(server_items))}, extra={len(set(server_items) - expected_keys)})"
        )
    for record in records:
        item = server_items.get(record["nativeKey"], {})
        if item.get("schema_key") != SECTION_SCHEMA_KEYS[record["section"]]:
            failures.append(f"ledger binding differs for {record['nativeKey']}")
            break
    return failures


def verify_writers() -> list[str]:
    failures: list[str] = []
    template = (MODULE_ROOT / "templates" / "ServerSettings.ini.hbs").read_text(
        encoding="utf-8-sig"
    )
    if template.strip() != "{{scum.server_settings_ini}}":
        failures.append("ServerSettings.ini template must use the typed SCUM renderer")
    materializer_path = (
        ROOT
        / "crates"
        / "app-storage"
        / "src"
        / "templates_materialize"
        / "scum.rs"
    )
    if not materializer_path.is_file():
        failures.append("SCUM managed materializer is missing")
    else:
        materializer = materializer_path.read_text(encoding="utf-8-sig")
        for marker in (
            "merge_rendered_config_files",
            "ManagedConfigFile::Ini",
            "ManagedConfigFile::JsonObject",
            "ManagedConfigFile::Text",
        ):
            if marker not in materializer:
                failures.append(f"SCUM materializer is missing {marker}")
    return failures


def verify_repository() -> list[str]:
    try:
        records = read_inventory()
    except (OSError, json.JSONDecodeError) as error:
        return [f"cannot read SCUM inventory: {error}"]
    return [
        *validate_inventory(records),
        *verify_schema(records),
        *verify_ledger(records),
        *verify_writers(),
    ]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Audit SCUM native server settings coverage.")
    parser.add_argument("--source", type=Path)
    parser.add_argument("--write-inventory", action="store_true")
    parser.add_argument("--json-source-root", type=Path)
    parser.add_argument("--write-json-defaults", action="store_true")
    parser.add_argument("--write-ledger", action="store_true")
    parser.add_argument("--write-schema", action="store_true")
    parser.add_argument("--verify-repository", action="store_true")
    args = parser.parse_args(argv)
    if args.write_inventory:
        if args.source is None:
            parser.error("--write-inventory requires --source")
        write_inventory(args.source)
    if args.write_json_defaults:
        if args.json_source_root is None:
            parser.error("--write-json-defaults requires --json-source-root")
        write_json_defaults(args.json_source_root)
    if args.write_ledger:
        write_ledger()
    if args.write_schema:
        write_schema_inventory()
    failures = verify_repository() if args.verify_repository else []
    if failures:
        print("SCUM settings audit failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    if args.verify_repository:
        print("SCUM settings repository contract verified: 436 specialized + 1 generated")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
