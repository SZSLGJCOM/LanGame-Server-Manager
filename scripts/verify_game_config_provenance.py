from __future__ import annotations

import argparse
import csv
import io
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
DOCS_DIR = ROOT / "docs"
REPORT_FILENAMES = ("game-config-source-ledger.csv", "game-config-source-ledger.md")

VALID_STATUSES = {
    "exhaustive_verified",
    "best_effort_verified",
    "blocked_upstream",
    "verified_with_direct_connection",
    "download_verified_requires_elevation",
}
VALID_SOURCE_KINDS = {
    "config_file",
    "install_probe",
    "launch_arg",
    "runtime_probe",
    "runtime_console",
    "runtime_rcon",
    "runtime_api",
    "generated_roster",
    "workshop",
    "materializer",
    "upstream_blocker",
    "documentation",
    "release",
}
VALID_SURFACES = {
    "config_file",
    "launch_arg",
    "runtime_console",
    "runtime_rcon",
    "runtime_api",
    "generated_roster",
    "generated_modpack",
    "materializer",
    "upstream_blocker",
}

REMOTE_URI = re.compile(r"(?i)\b(?!file:)[a-z][a-z0-9+.-]*://[^\s\"'<>]+")
UNREAL_VIRTUAL_PATH = re.compile(r"/(?:Game|Script)(?:/[A-Za-z0-9_.-]+)+")
WINDOWS_ABSOLUTE_PATH = re.compile(r"(?<![A-Za-z0-9_])[A-Za-z]:[\\/]")
UNC_ABSOLUTE_PATH = re.compile(
    r"(?:^|[\s\"'=(])(?:\\\\|//)[^\\/\s\"'<>]+[\\/]"
)
POSIX_ABSOLUTE_PATH = re.compile(
    r"(?:^|[\s\"'=(])/(?![/\s])[^\s\"'<>]+"
)
HOME_RELATIVE_PATH = re.compile(r"(?:^|[\s\"'=(])~[\\/]")


def contains_local_absolute_path(value: str) -> bool:
    if re.search(r"(?i)\bfile:(?://)?", value):
        return True
    without_virtual_paths = UNREAL_VIRTUAL_PATH.sub("", value)
    without_remote_uris = REMOTE_URI.sub("", without_virtual_paths)
    return any(
        pattern.search(without_remote_uris)
        for pattern in (
            WINDOWS_ABSOLUTE_PATH,
            UNC_ABSOLUTE_PATH,
            POSIX_ABSOLUTE_PATH,
            HOME_RELATIVE_PATH,
        )
    )


@dataclass
class ModuleProvenanceSummary:
    module_id: str
    status: str
    schema_fields: int
    ledger_items: int
    exclusions: int
    sources: int


def read_modules(modules_dir: Path) -> list[Path]:
    return sorted(
        path
        for path in modules_dir.iterdir()
        if path.is_dir() and (path / "schema.json").exists() and (path / "module.toml").exists()
    )


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def is_table_list(value: Any) -> bool:
    return isinstance(value, list) and all(isinstance(item, dict) for item in value)


def string_value(table: dict[str, Any], key: str) -> str:
    value = table.get(key)
    return value.strip() if isinstance(value, str) else ""


def validate_ledger(
    module_id: str,
    ledger: dict[str, Any],
    properties: dict[str, Any],
) -> tuple[list[str], ModuleProvenanceSummary]:
    failures: list[str] = []
    status = string_value(ledger, "status")
    last_verified = string_value(ledger, "last_verified")
    notes = string_value(ledger, "notes")

    if status not in VALID_STATUSES:
        failures.append(f"{module_id}: status must be one of {sorted(VALID_STATUSES)}, got {status!r}")
    if not last_verified:
        failures.append(f"{module_id}: last_verified is required")
    if not notes:
        failures.append(f"{module_id}: notes is required")
    elif contains_local_absolute_path(notes):
        failures.append(
            f"{module_id}: notes must use a symbolic evidence path instead of a local machine path"
        )

    raw_sources = ledger.get("sources", [])
    raw_items = ledger.get("items", [])
    raw_exclusions = ledger.get("exclusions", [])
    sources = raw_sources if is_table_list(raw_sources) else []
    items = raw_items if is_table_list(raw_items) else []
    exclusions = raw_exclusions if is_table_list(raw_exclusions) else []

    if not is_table_list(raw_sources) or not sources:
        failures.append(f"{module_id}: at least one [[sources]] table is required")
    if not is_table_list(raw_items):
        failures.append(f"{module_id}: [[items]] must be a list of tables")
    if not is_table_list(raw_exclusions):
        failures.append(f"{module_id}: [[exclusions]] must be a list of tables")

    source_ids: set[str] = set()
    for index, source in enumerate(sources, start=1):
        source_id = string_value(source, "id")
        kind = string_value(source, "kind")
        path = string_value(source, "path")
        url = string_value(source, "url")
        authority = string_value(source, "authority")
        description = string_value(source, "description")

        if not source_id:
            failures.append(f"{module_id}: sources[{index}] missing id")
        elif source_id in source_ids:
            failures.append(f"{module_id}: duplicate source id {source_id!r}")
        else:
            source_ids.add(source_id)
        if kind not in VALID_SOURCE_KINDS:
            failures.append(f"{module_id}: sources[{index}] has invalid kind {kind!r}")
        if not path:
            failures.append(f"{module_id}: sources[{index}] missing path")
        elif contains_local_absolute_path(path):
            failures.append(
                f"{module_id}: sources[{index}] path must be portable and must not expose a local machine path"
            )
        if url and contains_local_absolute_path(url):
            failures.append(
                f"{module_id}: sources[{index}] url must be portable and must not expose a local machine path"
            )
        if not authority:
            failures.append(f"{module_id}: sources[{index}] missing authority")
        elif contains_local_absolute_path(authority):
            failures.append(
                f"{module_id}: sources[{index}] authority must be portable and must not expose a local machine path"
            )
        if not description:
            failures.append(f"{module_id}: sources[{index}] missing description")
        elif contains_local_absolute_path(description):
            failures.append(
                f"{module_id}: sources[{index}] description must use a symbolic evidence path"
            )

    property_keys = set(properties)
    item_schema_keys: set[str] = set()
    item_native_pairs: set[tuple[str, str]] = set()
    item_schema_bindings: set[tuple[str, str, str, str]] = set()

    for index, item in enumerate(items, start=1):
        source_id = string_value(item, "source")
        native_key = string_value(item, "key")
        schema_key = string_value(item, "schema_key")
        surface = string_value(item, "surface")

        if source_id not in source_ids:
            failures.append(f"{module_id}: items[{index}] references unknown source {source_id!r}")
        if not native_key:
            failures.append(f"{module_id}: items[{index}] missing key")
        if schema_key not in property_keys:
            failures.append(f"{module_id}: items[{index}] schema_key {schema_key!r} is not in schema.properties")
        else:
            item_schema_keys.add(schema_key)
        if surface not in VALID_SURFACES:
            failures.append(f"{module_id}: items[{index}] has invalid surface {surface!r}")

        pair = (source_id, native_key)
        if source_id and native_key and pair in item_native_pairs:
            failures.append(f"{module_id}: duplicate item source/key pair {source_id}.{native_key}")
        item_native_pairs.add(pair)
        if source_id and native_key and schema_key and surface:
            item_schema_bindings.add((schema_key, source_id, native_key, surface))

    exclusion_pairs: set[tuple[str, str]] = set()
    for index, exclusion in enumerate(exclusions, start=1):
        source_id = string_value(exclusion, "source")
        native_key = string_value(exclusion, "key")
        reason = string_value(exclusion, "reason")
        if source_id not in source_ids:
            failures.append(f"{module_id}: exclusions[{index}] references unknown source {source_id!r}")
        if not native_key:
            failures.append(f"{module_id}: exclusions[{index}] missing key")
        if not reason:
            failures.append(f"{module_id}: exclusions[{index}] missing reason")
        elif contains_local_absolute_path(reason):
            failures.append(
                f"{module_id}: exclusions[{index}] reason must use a symbolic evidence path"
            )
        if source_id and native_key:
            exclusion_pairs.add((source_id, native_key))

    overlap = sorted(item_native_pairs & exclusion_pairs)
    for source_id, native_key in overlap:
        failures.append(f"{module_id}: {source_id}.{native_key} is both an item and an exclusion")

    for key, prop in properties.items():
        if not isinstance(prop, dict):
            failures.append(f"{module_id}.{key}: schema property must be an object")
            continue

        source_id = string_value(prop, "x-lsgm-source")
        source_key = string_value(prop, "x-lsgm-source-key")
        source_surface = string_value(prop, "x-lsgm-source-surface")

        if not source_id:
            failures.append(f"{module_id}.{key}: missing x-lsgm-source")
        elif source_id not in source_ids:
            failures.append(f"{module_id}.{key}: unknown x-lsgm-source {source_id!r}")
        if not source_key:
            failures.append(f"{module_id}.{key}: missing x-lsgm-source-key")
        if source_surface not in VALID_SURFACES:
            failures.append(f"{module_id}.{key}: invalid or missing x-lsgm-source-surface {source_surface!r}")
        if key not in item_schema_keys:
            failures.append(f"{module_id}.{key}: missing config-sources.toml item")
        elif source_id and source_key and source_surface:
            binding = (key, source_id, source_key, source_surface)
            if binding not in item_schema_bindings:
                failures.append(
                    f"{module_id}.{key}: schema source metadata does not match a config-sources item "
                    f"({source_id}.{source_key}/{source_surface})"
                )

    if status == "exhaustive_verified" and not exclusions:
        # Exhaustive modules may have no exclusions, but they still need a deliberate marker.
        # This avoids accidentally claiming exhaustiveness without recording reviewed native keys.
        if string_value(ledger, "notes").lower().find("no exclusions") < 0:
            failures.append(f"{module_id}: exhaustive_verified without exclusions must state 'no exclusions' in notes")

    return failures, ModuleProvenanceSummary(
        module_id=module_id,
        status=status or "missing",
        schema_fields=len(property_keys),
        ledger_items=len(items),
        exclusions=len(exclusions),
        sources=len(sources),
    )


def collect_summaries(modules_dir: Path) -> tuple[list[str], list[ModuleProvenanceSummary]]:
    failures: list[str] = []
    summaries: list[ModuleProvenanceSummary] = []
    for module_root in read_modules(modules_dir):
        module_id = module_root.name
        source_path = module_root / "config-sources.toml"
        schema = read_json(module_root / "schema.json")
        properties = schema.get("properties", {})
        if not isinstance(properties, dict):
            failures.append(f"{module_id}: schema.properties must be an object")
            properties = {}
        if not source_path.exists():
            failures.append(f"{module_id}: missing config-sources.toml")
            summaries.append(ModuleProvenanceSummary(module_id, "missing", len(properties), 0, 0, 0))
            continue
        try:
            ledger = tomllib.loads(source_path.read_text(encoding="utf-8-sig"))
        except tomllib.TOMLDecodeError as error:
            failures.append(f"{module_id}: config-sources.toml parse failed: {error}")
            summaries.append(ModuleProvenanceSummary(module_id, "invalid", len(properties), 0, 0, 0))
            continue
        module_failures, summary = validate_ledger(module_id, ledger, properties)
        failures.extend(module_failures)
        summaries.append(summary)
    return failures, summaries


def render_csv_report(summaries: list[ModuleProvenanceSummary]) -> str:
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerow(["module", "status", "schema_fields", "ledger_items", "exclusions", "sources"])
    for summary in summaries:
        writer.writerow([
            summary.module_id,
            summary.status,
            summary.schema_fields,
            summary.ledger_items,
            summary.exclusions,
            summary.sources,
        ])
    return output.getvalue()


def render_markdown_report(summaries: list[ModuleProvenanceSummary]) -> str:
    lines = [
        "<!-- Generated by scripts/verify_game_config_provenance.py. Regenerate with --write; do not edit directly. -->",
        "",
        "# Game Config Source Ledger",
        "",
        "| module | status | schema fields | ledger items | exclusions | sources |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for summary in summaries:
        lines.append(
            f"| `{summary.module_id}` | {summary.status} | {summary.schema_fields} | "
            f"{summary.ledger_items} | {summary.exclusions} | {summary.sources} |"
        )
    return "\n".join(lines) + "\n"


def check_reports(docs_dir: Path, expected: dict[str, str]) -> list[str]:
    failures: list[str] = []
    for filename in REPORT_FILENAMES:
        path = docs_dir / filename
        if not path.is_file():
            failures.append(f"missing report {filename}; run verify_game_config_provenance.py --write")
            continue
        if path.read_bytes() != expected[filename].encode("utf-8"):
            failures.append(f"stale report {filename}; run verify_game_config_provenance.py --write")
    return failures


def write_reports(docs_dir: Path, expected: dict[str, str]) -> None:
    docs_dir.mkdir(parents=True, exist_ok=True)
    for filename in REPORT_FILENAMES:
        with (docs_dir / filename).open("w", encoding="utf-8", newline="\n") as handle:
            handle.write(expected[filename])


def print_failures(failures: list[str]) -> None:
    print("game config provenance verification failed:", file=sys.stderr)
    for failure in failures:
        print(f"  - {failure}", file=sys.stderr)


def run_verification(modules_dir: Path, docs_dir: Path, mode: str) -> int:
    if mode not in {"check", "write"}:
        raise ValueError(f"unsupported provenance verification mode {mode!r}")
    failures, summaries = collect_summaries(modules_dir)
    if failures:
        print_failures(failures)
        return 1
    expected = {
        "game-config-source-ledger.csv": render_csv_report(summaries),
        "game-config-source-ledger.md": render_markdown_report(summaries),
    }
    if mode == "write":
        write_reports(docs_dir, expected)
    else:
        report_failures = check_reports(docs_dir, expected)
        if report_failures:
            print_failures(report_failures)
            return 1
    print(
        f"game config provenance verified for {len(summaries)} modules; "
        f"{sum(summary.schema_fields for summary in summaries)} schema fields"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify game configuration source provenance.")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="compare committed reports without writing them")
    mode.add_argument("--write", action="store_true", help="regenerate reports after ledger validation")
    arguments = parser.parse_args(argv)
    return run_verification(MODULES_DIR, DOCS_DIR, "write" if arguments.write else "check")


if __name__ == "__main__":
    raise SystemExit(main())
