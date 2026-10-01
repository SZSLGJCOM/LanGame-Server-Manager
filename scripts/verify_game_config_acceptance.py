from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from datetime import date
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))
from game_config_acceptance_fixture import normalize_relative_path, validate_initial_files, validate_lifecycle

ROOT = Path(__file__).resolve().parents[1]
CATEGORIES = ("editable", "specialized", "derived", "generated", "excluded")
CANONICAL_MODULE_IDS = (
    "abioticfactor", "arksurvivalascended", "arksurvivalevolved", "astroneer",
    "barotrauma", "conanexiles", "corekeeper", "dontstarve", "enshrouded",
    "humanitz", "minecraft", "necesse", "nightingale", "palworld",
    "projectzomboid", "returntomoria", "rimworld", "romestead",
    "runescapedragonwilds", "rust", "satisfactory", "scum", "sevendaystodie",
    "sonsoftheforest", "soulmask", "squad", "terraria", "theforest", "unturned",
    "valheim", "vrising", "windrose",
)
VALID_EVIDENCE_STATUSES = {
    "exhaustive_verified",
    "best_effort_verified",
    "blocked_upstream",
    "verified_with_direct_connection",
    "download_verified_requires_elevation",
}
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))
def object_value(value: Any) -> dict[str, Any]:
    return value if isinstance(value, dict) else {}
def list_value(value: Any) -> list[Any]:
    return value if isinstance(value, list) else []
def discover_module_ids(modules_dir: Path) -> set[str]:
    if not modules_dir.is_dir():
        return set()
    return {
        path.name
        for path in modules_dir.iterdir()
        if path.is_dir()
        and (path / "module.toml").is_file()
        and (path / "schema.json").is_file()
    }
def discover_fixture_module_ids(modules_dir: Path) -> list[str]:
    return sorted(
        module_id
        for module_id in discover_module_ids(modules_dir)
        if any((modules_dir / module_id / "config-fixtures").glob("*.json"))
    )
def read_module_contract(module_root: Path) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    manifest = tomllib.loads((module_root / "module.toml").read_text(encoding="utf-8-sig"))
    schema = read_json(module_root / "schema.json")
    ledger = tomllib.loads((module_root / "config-sources.toml").read_text(encoding="utf-8-sig"))
    if not isinstance(schema, dict):
        raise ValueError("schema.json must contain an object")
    return manifest, schema, ledger
def validate_evidence(
    prefix: str,
    fixture: dict[str, Any],
    ledger: dict[str, Any],
    failures: list[str],
) -> list[str]:
    evidence = object_value(fixture.get("evidence"))
    if not isinstance(fixture.get("evidence"), dict):
        failures.append(f"{prefix}: evidence must be an object")
    status = evidence.get("status")
    if status not in VALID_EVIDENCE_STATUSES:
        failures.append(f"{prefix}: evidence.status is invalid")
    if status != ledger.get("status"):
        failures.append(f"{prefix}: evidence.status must match config-sources.toml status")
    verified_at = evidence.get("verified_at")
    try:
        valid_date = (
            isinstance(verified_at, str)
            and DATE_RE.fullmatch(verified_at) is not None
            and date.fromisoformat(verified_at).isoformat() == verified_at
        )
    except ValueError:
        valid_date = False
    if not valid_date:
        failures.append(f"{prefix}: evidence.verified_at must be a real YYYY-MM-DD date")
    if not isinstance(evidence.get("build"), str) or not evidence["build"].strip():
        failures.append(f"{prefix}: evidence.build must be a non-empty string")
    source_ids = evidence.get("source_ids")
    if (
        not isinstance(source_ids, list)
        or not source_ids
        or any(not isinstance(item, str) or not item for item in source_ids)
    ):
        failures.append(f"{prefix}: evidence.source_ids must be a non-empty string list")
        source_ids = []
    if len(source_ids) != len(set(source_ids)):
        failures.append(f"{prefix}: evidence.source_ids must not contain duplicates")
    known_sources = {
        source.get("id")
        for source in list_value(ledger.get("sources"))
        if isinstance(source, dict) and isinstance(source.get("id"), str)
    }
    for source_id in source_ids:
        if source_id not in known_sources:
            failures.append(f"{prefix}: unknown evidence source_id {source_id!r}")
    return source_ids
def validate_classifications(
    prefix: str,
    fixture: dict[str, Any],
    ledger: dict[str, Any],
    source_ids: list[str],
    failures: list[str],
) -> None:
    coverage = fixture.get("coverage")
    classifications = fixture.get("classifications")
    if not isinstance(coverage, dict):
        failures.append(f"{prefix}: coverage must be an object")
        coverage = {}
    if not isinstance(classifications, dict):
        failures.append(f"{prefix}: classifications must be an object")
        classifications = {}

    known_keys: dict[str, str] = {}
    for table_name in ("items", "exclusions"):
        for item in list_value(ledger.get(table_name)):
            if (
                isinstance(item, dict)
                and item.get("source") in source_ids
                and isinstance(item.get("key"), str)
            ):
                qualified = f"{item['source']}.{item['key']}"
                known_keys[qualified] = table_name

    classified: dict[str, str] = {}
    for category in CATEGORIES:
        count = coverage.get(category)
        keys = classifications.get(category)
        if not isinstance(count, int) or isinstance(count, bool) or count < 0:
            failures.append(f"{prefix}: coverage.{category} must be a non-negative integer")
        if not isinstance(keys, list) or any(not isinstance(key, str) or not key for key in keys):
            failures.append(f"{prefix}: classifications.{category} must be a string list")
            keys = []
        if isinstance(count, int) and not isinstance(count, bool) and count != len(keys):
            failures.append(
                f"{prefix}: coverage.{category} is {count} but {len(keys)} keys are classified"
            )
        for key in keys:
            if key in classified:
                failures.append(f"{prefix}: {key!r} is classified more than once")
            else:
                classified[key] = category
            if key not in known_keys:
                failures.append(f"{prefix}: unknown classified key {key!r}")
            elif known_keys[key] == "exclusions" and category != "excluded":
                failures.append(f"{prefix}: ledger exclusion {key!r} must be classified as excluded")
    for key in sorted(set(known_keys) - set(classified)):
        failures.append(f"{prefix}: {key} is not classified")
def validate_expected_outputs(
    prefix: str,
    fixture: dict[str, Any],
    failures: list[str],
) -> None:
    expected = fixture.get("expected")
    if not isinstance(expected, dict):
        failures.append(f"{prefix}: expected must be an object")
        expected = {}
    files = expected.get("files")
    if not isinstance(files, list):
        failures.append(f"{prefix}: expected.files must be a list")
        files = []
    normalized_paths: set[str] = set()
    has_native_assertion = False
    valid_roots = {"config", "install", "saves", "instance"}
    for index, output in enumerate(files):
        item_prefix = f"{prefix}: expected.files[{index}]"
        if not isinstance(output, dict):
            failures.append(f"{item_prefix} must be an object")
            continue
        output_root = output.get("root")
        if output_root not in valid_roots:
            failures.append(
                f"{item_prefix}.root must be one of {', '.join(sorted(valid_roots))}"
            )
        normalized = normalize_relative_path(output.get("path"))
        if normalized is None:
            failures.append(f"{item_prefix} has unsafe relative path")
        else:
            output["path"] = normalized
            rooted_path = f"{output_root}:{normalized}"
            if rooted_path in normalized_paths:
                failures.append(f"{item_prefix} duplicates output path {rooted_path!r}")
            normalized_paths.add(rooted_path)
        if not isinstance(output.get("format"), str) or not output["format"].strip():
            failures.append(f"{item_prefix}.format is required")
        output_format = str(output.get("format", "")).strip().lower()
        keys, entries, fragments = (
            output.get("keys"), output.get("entries"), output.get("fragments")
        )
        for name, value, expected_type in (
            ("keys", keys, dict), ("entries", entries, list), ("fragments", fragments, list)
        ):
            if name in output and not isinstance(value, expected_type):
                failures.append(f"{item_prefix}.{name} has the wrong type")
        declarations = [isinstance(keys, dict), isinstance(entries, list), isinstance(fragments, list)]
        if sum(declarations) != 1:
            failures.append(
                f"{item_prefix} must declare exactly one of keys, entries, or fragments"
            )
            continue
        assertion = keys if declarations[0] else entries if declarations[1] else fragments
        if not assertion:
            failures.append(f"{item_prefix} assertion must not be empty")
            continue
        if declarations[1] and any(
            not isinstance(entry, dict)
            or not isinstance(entry.get("key"), str)
            or not entry["key"].strip()
            or "value" not in entry
            for entry in entries
        ):
            failures.append(f"{item_prefix}.entries must contain non-empty key/value objects")
            continue
        if declarations[0] and output_format not in {"json", "properties", "ini"}:
            failures.append(f"{item_prefix}.keys cannot verify format {output_format!r}")
            continue
        if declarations[1] and output_format not in {"properties", "ini"}:
            failures.append(f"{item_prefix}.entries cannot verify format {output_format!r}")
            continue
        if declarations[2] and any(
            not isinstance(fragment, str) or not fragment.strip() for fragment in fragments
        ):
            failures.append(f"{item_prefix}.fragments must be a non-empty string list")
            continue
        has_native_assertion = True

    launch = expected.get("launch")
    if not isinstance(launch, dict):
        failures.append(f"{prefix}: expected.launch must be an object")
        return
    suffix = launch.get("executable_suffix")
    if "executable_suffix" not in launch or (suffix is not None and not isinstance(suffix, str)):
        failures.append(f"{prefix}: expected.launch.executable_suffix must be a string or null")
    elif suffix is not None:
        normalized = normalize_relative_path(suffix)
        if normalized is None:
            failures.append(f"{prefix}: expected.launch.executable_suffix has unsafe relative path")
        else:
            launch["executable_suffix"] = normalized
            has_native_assertion = True
    arguments = launch.get("arguments")
    if not isinstance(arguments, list) or any(not isinstance(arg, str) for arg in arguments):
        failures.append(f"{prefix}: expected.launch.arguments must be a string list")
    elif arguments:
        has_native_assertion = True
    if not has_native_assertion:
        failures.append(f"{prefix}: fixture must assert at least one native output")
def validate_fixture(
    fixture_path: Path,
    module_id: str,
    schema: dict[str, Any],
    ledger: dict[str, Any],
) -> tuple[list[str], dict[str, Any] | None]:
    prefix = fixture_path.as_posix()
    try:
        fixture = read_json(fixture_path)
    except (OSError, json.JSONDecodeError) as error:
        return [f"{prefix}: invalid JSON: {error}"], None
    if not isinstance(fixture, dict):
        return [f"{prefix}: fixture must be an object"], None

    failures: list[str] = []
    fixture_version = fixture.get("fixture_version")
    if (
        not isinstance(fixture_version, int)
        or isinstance(fixture_version, bool)
        or fixture_version != 1
    ):
        failures.append(f"{prefix}: fixture_version must be 1")
    if fixture.get("module_id") != module_id:
        failures.append(f"{prefix}: fixture module_id must equal {module_id!r}")
    source_ids = validate_evidence(prefix, fixture, ledger, failures)
    evidence = object_value(fixture.get("evidence"))
    verified_at = evidence.get("verified_at")
    fixture_stem = fixture_path.stem
    if not (
        isinstance(verified_at, str)
        and any(fixture_stem == f"{verified_at}-{source_id}" for source_id in source_ids)
    ):
        failures.append(
            f"{prefix}: fixture filename must be <verified-date>-<source-id>.json"
        )

    settings = fixture.get("settings")
    if not isinstance(settings, dict):
        failures.append(f"{prefix}: settings must be an object")
        settings = {}
    properties = object_value(schema.get("properties"))
    for key in settings:
        if key not in properties:
            failures.append(f"{prefix}: unknown setting key {key!r}")

    validate_initial_files(prefix, fixture, failures)
    validate_lifecycle(prefix, fixture, failures)
    validate_classifications(prefix, fixture, ledger, source_ids, failures)
    validate_expected_outputs(prefix, fixture, failures)
    return failures, fixture


def validate_module_fixture_inventory(
    module_id: str,
    fixtures: list[tuple[Path, dict[str, Any]]],
    ledger: dict[str, Any],
) -> list[str]:
    ledger_keys = {
        f"{item['source']}.{item['key']}"
        for table_name in ("items", "exclusions")
        for item in list_value(ledger.get(table_name))
        if isinstance(item, dict)
        and isinstance(item.get("source"), str)
        and isinstance(item.get("key"), str)
    }
    covered_keys = {
        key
        for _, fixture in fixtures
        for keys in object_value(fixture.get("classifications")).values()
        if isinstance(keys, list)
        for key in keys
        if isinstance(key, str)
    }
    return [
        f"{module_id}: {key} is not covered by any fixture"
        for key in sorted(ledger_keys - covered_keys)
    ]


def render_record(module_id: str, fixtures: list[tuple[Path, dict[str, Any]]]) -> bytes:
    lines = [
        "<!-- Generated by scripts/verify_game_config_acceptance.py. Regenerate with --write; do not edit directly. -->",
        "",
        f"# Game Configuration Acceptance: {module_id}",
        "",
        f"- Module: `{module_id}`",
        "- Fixture version: `1`",
        "",
    ]
    for path, fixture in sorted(fixtures, key=lambda item: item[0].name):
        evidence = fixture["evidence"]
        lines.extend([
            f"## `{path.name}`", "",
            f"- Evidence: `{evidence['status']}` at `{evidence['verified_at']}` (build `{evidence['build']}`)",
            f"- Sources: {', '.join(f'`{source}`' for source in evidence['source_ids'])}",
            "- Coverage: " + ", ".join(
                f"{category} `{fixture['coverage'][category]}`" for category in CATEGORIES
            ),
        ])
        if "initial" in fixture:
            lines.extend(["", "### Initial native files", "", "```json",
                json.dumps(fixture["initial"], ensure_ascii=False, indent=2, sort_keys=True), "```"])
        if "lifecycle" in fixture:
            lines.extend(["", "### Lifecycle", "", "```json",
                json.dumps(fixture["lifecycle"], ensure_ascii=False, indent=2, sort_keys=True), "```"])
        lines.extend(["", "### Settings", "", "```json",
            json.dumps(fixture["settings"], ensure_ascii=False, indent=2, sort_keys=True),
            "```", "", "### Expected outputs", "", "```json",
            json.dumps(fixture["expected"], ensure_ascii=False, indent=2, sort_keys=True),
            "```", "",
        ])
    return "\n".join(lines).encode("utf-8")


def verify_repository(root: Path, selected: list[str], require_all: bool, mode: str) -> list[str]:
    if mode not in {"check", "write"}:
        raise ValueError(f"unsupported mode {mode!r}")
    modules_dir = root / "modules"
    docs_dir = root / "docs" / "game-config-acceptance"
    available = discover_module_ids(modules_dir)
    canonical = set(CANONICAL_MODULE_IDS)
    if require_all:
        if (
            available != canonical
            or len(selected) != len(CANONICAL_MODULE_IDS)
            or set(selected) != canonical
        ):
            return ["--require-all requires exactly the canonical 32 modules"]

    seen: set[str] = set()
    duplicate_failures: list[str] = []
    for module_id in selected:
        if module_id in seen:
            duplicate_failures.append(f"duplicate selected module {module_id!r}")
        seen.add(module_id)
    if duplicate_failures:
        return duplicate_failures

    failures = [f"unknown selected module {module_id!r}" for module_id in selected if module_id not in available]
    if failures:
        return failures

    rendered: dict[str, bytes] = {}
    for module_id in sorted(set(selected)):
        module_root = modules_dir / module_id
        try:
            manifest, schema, ledger = read_module_contract(module_root)
        except (OSError, ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
            failures.append(f"{module_id}: cannot read module contract: {error}")
            continue
        if manifest.get("id") != module_id:
            failures.append(f"{module_id}: module.toml id must equal {module_id!r}")
            continue
        fixture_paths = sorted((module_root / "config-fixtures").glob("*.json"))
        if not fixture_paths:
            failures.append(f"{module_id}: no config acceptance fixtures")
            continue
        fixtures: list[tuple[Path, dict[str, Any]]] = []
        module_failed = False
        for fixture_path in fixture_paths:
            fixture_failures, fixture = validate_fixture(fixture_path, module_id, schema, ledger)
            failures.extend(fixture_failures)
            module_failed = module_failed or bool(fixture_failures)
            if fixture is not None:
                fixtures.append((fixture_path, fixture))
        inventory_failures = validate_module_fixture_inventory(module_id, fixtures, ledger)
        failures.extend(inventory_failures)
        module_failed = module_failed or bool(inventory_failures)
        if not module_failed:
            rendered[module_id] = render_record(module_id, fixtures)

    if failures:
        return failures
    if require_all:
        record_ids = (
            {path.stem for path in docs_dir.glob("*.md") if path.is_file()}
            if docs_dir.is_dir()
            else set()
        )
        unexpected_records = record_ids - canonical
        if unexpected_records or (mode == "check" and record_ids != canonical):
            return [
                "generated record IDs must exactly match the canonical modules "
                f"(missing: {', '.join(sorted(canonical - record_ids)) or 'none'}; "
                f"unexpected: {', '.join(sorted(unexpected_records)) or 'none'})"
            ]
    if mode == "write":
        if rendered:
            docs_dir.mkdir(parents=True, exist_ok=True)
        for module_id, content in rendered.items():
            (docs_dir / f"{module_id}.md").write_bytes(content)
        return []

    for module_id, content in rendered.items():
        record_path = docs_dir / f"{module_id}.md"
        if not record_path.is_file():
            failures.append(f"{module_id}: missing generated record")
        elif record_path.read_bytes() != content:
            failures.append(f"{module_id}: stale generated record")
    return failures


def parse_modules(value: str | None) -> list[str] | None:
    if value is None:
        return None
    return [item.strip() for item in value.split(",") if item.strip()]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Verify versioned game configuration acceptance fixtures.")
    parser.add_argument(
        "--modules",
        nargs="?",
        const="",
        help="comma-separated module IDs; an empty value selects none",
    )
    parser.add_argument("--require-all", action="store_true", help="require all canonical modules")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", action="store_true", help="compare records without writing")
    mode.add_argument("--write", action="store_true", help="write deterministic records")
    args = parser.parse_args(argv)
    selected = parse_modules(args.modules)
    if selected is None:
        selected = (
            list(CANONICAL_MODULE_IDS)
            if args.require_all
            else discover_fixture_module_ids(ROOT / "modules")
        )
    failures = verify_repository(
        ROOT,
        selected,
        args.require_all,
        "write" if args.write else "check",
    )
    if failures:
        print("game config acceptance verification failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print(f"game config acceptance verified for {len(set(selected))} selected modules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
