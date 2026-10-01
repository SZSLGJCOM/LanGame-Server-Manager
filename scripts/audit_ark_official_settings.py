#!/usr/bin/env python3
"""Build an exact, edition-aware crosswalk from ARK's official configuration table.

The input is the raw wikitext returned by
https://ark.wiki.gg/wiki/Server_configuration?action=raw. Network access is
deliberately kept outside this script. The caller must provide a local copy;
the script verifies its pinned digest before using it.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from ark_official_repository_contract import (
    MODULES,
    ledger_identity,
    ledger_row_matches,
    verify_repository,
)


SOURCE_URL = "https://ark.wiki.gg/wiki/Server_configuration"
SOURCE_RAW_URL = f"{SOURCE_URL}?action=raw"
VERIFIED_ON = "2026-08-12"
PINNED_SOURCE_SHA256 = "72b00288f130913ad6dfac1b680c62c159e76e86c70b3f90a2369260d63d5f63"
# Public output carries the native contract and provenance, not the source's
# explanatory prose. Keep this allowlist explicit when the parser gains fields.
PUBLIC_RECORD_FIELDS = (
    "ordinal", "line", "raw_name", "native_name", "canonical_name", "section",
    "inASE", "inASA", "status", "type", "default", "version",
)
PUBLIC_TYPE_NAMES = {
    "", '"<string>"', "(...)", "IP_ADDRESS", "ModID", "URL", "boolean",
    "float", "integer", "multiplier", "seconds", "string", "value",
}
# Match the exact reviewed source fields without embedding their explanatory
# sentences here. A changed source spelling requires review, not silent omission.
PUBLIC_LIST_TYPE_FACTS = {
    "0a64225592fcd6c607f85b84eb50f030d8e53f090cac52d527653933386794c9":
        "list<ModID>; delimiter=','; spaces=false; lines=1",
    "f69ccdb1e74d646d07aec3eeb6c98bac35b6dc7470e38d7b6cf244eb81e7699f":
        "list<player_name>; delimiter=';'; spaces=false; lines=1",
}


@dataclass(frozen=True)
class EditionPolicy:
    code: str
    field: str
    baseline: str


POLICIES = (
    EditionPolicy("ase", "inASE", "literal Yes rows whose status is not deprecated"),
    EditionPolicy("asa", "inASA", "unique literal Yes names, including rows marked deprecated"),
)


CLIENT_OR_SINGLE_PLAYER = {
    "ase": {
        "allowansel",
        "d3d10 dx10 sm4",
        "d3d11 dx11 sm5",
        "lowmemory",
        "nomansky",
        "nomemorybias",
        "norhithread",
        "nosteamclient",
        "preventhibernation",
        "listenservertetherdistancemultiplier",
    },
    "asa": {
        "d3d11 dx11 sm5",
        "forceignoresingleplayerspawnrangecheck",
    },
}


UNSAFE_OR_INTERNAL = {
    "ase": {
        "insecure",
        "noantispeedhack",
        "noundermeshchecking",
        "noundermeshkilling",
        "forcedisablemeshchecking",
        "disabledupelogdeletes",
        "enableofficialonlyversioningcode",
        "forcedupelog",
        "ignoredupeditems",
        "nitradotest2",
    },
    "asa": {
        "disabledupelogdeletes",
        "forcedupelog",
        "ignoredupeditems",
        "ip",
        "nitradoqueryport",
        "serverip",
    },
}


ONE_SHOT_MAINTENANCE = {
    "ase": {"converttostore", "parseservertojson", "reloadedforbackup"},
    "asa": {"converttostore"},
}


MANAGER_DERIVED = {
    "ase": {
        "altsavedirectoryname",
        "multihome",
        "port",
        "queryport",
        "rconport",
    },
    "asa": {
        "altsavedirectoryname",
        "multihome",
        "port",
        "queryport",
        "rconport",
        "winlivemaxplayers",
    },
}


GENERATED_FIXED = {"ase": set(), "asa": set()}


SPECIALIZED_OWNER = {
    "ase": {"activemods"},
    "asa": {"mods", "activemods"},
}


PREFERRED_SURFACE_ALIASES = {
    "ase": {
        "allowflyerspeedleveling",
        "gamemodids",
        "mapplayerlocation",
        "pvedisallowtribewar",
        "pveallowtribewar",
    },
    "asa": set(),
}


def field(body: str, key: str) -> str:
    match = re.search(
        rf"^\|[ \t]*{re.escape(key)}[ \t]*=[ \t]*(.*)$", body, re.MULTILINE
    )
    return match.group(1).strip() if match else ""


def plain_text(value: str) -> str:
    value = re.sub(r"<[^>]+>", "", value)
    value = re.sub(r"\{\{[^{}]*\}\}", "", value)
    value = re.sub(r"\[\[[^]|]+\|([^]]+)\]\]", r"\1", value)
    value = re.sub(r"\[\[([^]]+)\]\]", r"\1", value)
    return re.sub(r"\s+", " ", value).strip()


def public_record(record: dict[str, object]) -> dict[str, object]:
    item = {key: record[key] for key in PUBLIC_RECORD_FIELDS}
    source_type = str(item["type"])
    type_name = {
        "string with a URL": "URL",
        "mod ID for currently active mod map": "ModID",
    }.get(source_type, source_type)
    type_name = PUBLIC_LIST_TYPE_FACTS.get(
        hashlib.sha256(source_type.encode("utf-8")).hexdigest(), type_name
    )
    if type_name not in PUBLIC_TYPE_NAMES and type_name not in PUBLIC_LIST_TYPE_FACTS.values():
        raise ValueError(f"Unreviewed public type for {item['native_name']}")
    item["type"] = type_name

    default = str(item["default"])
    if "{{ItemLink" in default:
        parts = re.split(r"<br\s*/?>", default)
        parsed = [re.fullmatch(
            r'\{\{ItemLink\|ARK: Survival (Evolved|Ascended)\|&nbsp;\}\}: "([^"\r\n]+)"',
            part.strip(),
        ) for part in parts]
        if len(parsed) != 2 or any(match is None for match in parsed):
            raise ValueError(f"Unreviewed public default for {item['native_name']}")
        values = {match[1]: match[2] for match in parsed if match is not None}
        if set(values) != {"Evolved", "Ascended"}:
            raise ValueError(f"Unreviewed public default for {item['native_name']}")
        default = f'ASE="{values["Evolved"]}"; ASA="{values["Ascended"]}"'
    if re.search(r"\{\{|\}\}|</?[A-Za-z]|\[\[|&(?:[A-Za-z]+|#\d+);", default):
        raise ValueError(f"Unreviewed public default for {item['native_name']}")
    item["default"] = default
    return item


def native_name(raw_name: str) -> str:
    value = raw_name.replace("'''", "").strip()
    value = re.split(r"=", value, maxsplit=1)[0]
    return value.strip()


def schema_key_for(record: dict[str, object]) -> str:
    value = str(record["native_name"]).lstrip("-?")
    value = re.sub(r"\[<([^>]+)>\]", r"_\1", value)
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", value)
    return re.sub(r"[^A-Za-z0-9]+", "_", value).strip("_").lower()


def canonical_name(raw_name: str) -> str:
    value = native_name(raw_name).lstrip("-?")
    return re.sub(r"[^a-z0-9]+", " ", value.lower()).strip()


def parse_records(source: str) -> list[dict[str, object]]:
    headings: list[tuple[int, int, str]] = []
    for match in re.finditer(r"^(={2,6})\s*(.*?)\s*\1\s*$", source, re.MULTILINE):
        headings.append((match.start(), len(match.group(1)), match.group(2).strip()))

    records: list[dict[str, object]] = []
    pattern = re.compile(r"\{\{Server config variable[ \t]*(.*?)\n\}\}", re.DOTALL)
    for ordinal, match in enumerate(pattern.finditer(source), start=1):
        chain: list[tuple[int, str]] = []
        for position, level, heading in headings:
            if position >= match.start():
                break
            while chain and chain[-1][0] >= level:
                chain.pop()
            chain.append((level, heading))

        body = match.group(1)
        raw_name = field(body, "name")
        records.append(
            {
                "ordinal": ordinal,
                "line": source.count("\n", 0, match.start()) + 1,
                "raw_name": raw_name,
                "native_name": native_name(raw_name),
                "canonical_name": canonical_name(raw_name),
                "section": " > ".join(name for _, name in chain),
                "inASE": field(body, "inASE"),
                "inASA": field(body, "inASA"),
                "status": field(body, "status") or "current",
                "type": field(body, "type"),
                "default": field(body, "default"),
                "version": plain_text(field(body, "version")),
            }
        )
    return records


def is_dynamic(record: dict[str, object]) -> bool:
    return str(record["section"]).endswith("DynamicConfig")


def edition_baseline(records: list[dict[str, object]], policy: EditionPolicy) -> list[dict[str, object]]:
    selected = [record for record in records if record[policy.field] == "Yes"]
    if policy.code == "ase":
        return [record for record in selected if record["status"] != "deprecated"]

    unique: list[dict[str, object]] = []
    seen: set[str] = set()
    for record in selected:
        # The official 270-item ASA set collapses exact repeated names in the
        # DynamicConfig table, but keeps different native spellings/surfaces
        # such as command-line ``-port`` and SessionSettings ``Port``.
        name = str(record["native_name"]).lower()
        if name in seen:
            continue
        seen.add(name)
        unique.append(record)
    return unique


def classify(
    record: dict[str, object],
    edition: str,
    local_names: set[str],
) -> tuple[str, str]:
    name = str(record["canonical_name"])
    status = str(record["status"])

    if status == "deprecated":
        if name in GENERATED_FIXED[edition]:
            return "generated", "Mandatory server URL marker generated by LanGame; the source row is deprecated as a user option."
        return "excluded_deprecated", "The official table marks this option deprecated."
    if name in CLIENT_OR_SINGLE_PLAYER[edition]:
        return "excluded_client", "LanGame excludes client, single-player, and non-dedicated options from persistent dedicated-server configuration."
    if name in UNSAFE_OR_INTERNAL[edition]:
        return "excluded_unsafe_internal", "LanGame excludes vendor-internal and security-disabling switches from automatically persisted configuration."
    if name in ONE_SHOT_MAINTENANCE[edition]:
        return "excluded_one_shot", "LanGame excludes one-time maintenance operations because persistent launch arguments would repeat them on every start."
    if is_dynamic(record):
        if name in local_names:
            return "derived", "Optional HTTP DynamicConfig mirror of a setting already written to the native local configuration surface."
        return "excluded_external_dynamic", "DynamicConfig is fetched from an externally hosted HTTP document; LanGame has no stable local native file path to write."
    if name in MANAGER_DERIVED[edition]:
        return "manager_derived", "Derived from the instance save path, bind address, allocated ports, or player-cap lifecycle owned by LanGame."
    if name == "clusterdiroverride":
        return "specialized", "Owned by the cluster directory setting, with an instance-local fallback that preserves existing uploads."
    if name in GENERATED_FIXED[edition]:
        return "generated", "Fixed mandatory dedicated-server marker generated by LanGame."
    if name in SPECIALIZED_OWNER[edition]:
        return "specialized", "Owned by the dedicated Mods workflow and serialized by the edition-specific mod renderer."
    if name in PREFERRED_SURFACE_ALIASES[edition]:
        return "derived", "Official alias of a structured setting written through the preferred native surface."
    return "schema_native", "Editable structured setting with an edition-specific native writer."


def build_report(records: list[dict[str, object]]) -> dict[str, object]:
    editions: dict[str, object] = {}
    for policy in POLICIES:
        baseline = edition_baseline(records, policy)
        local_names = {
            str(record["canonical_name"])
            for record in baseline
            if not is_dynamic(record)
        }
        items: list[dict[str, object]] = []
        for record in baseline:
            classification, reason = classify(record, policy.code, local_names)
            item = public_record(record)
            item["classification"] = classification
            item["reason"] = reason
            item["native_surface"] = native_surface(record)
            if classification == "schema_native":
                item["schema_key"] = schema_key_for(record)
            items.append(item)

        classifications = Counter(str(item["classification"]) for item in items)
        rejected_items: list[dict[str, object]] = []
        for record in records:
            availability = str(record[policy.field]).lower()
            if availability not in {"no", "unknown"}:
                continue
            item = public_record(record)
            item["classification"] = f"excluded_edition_{availability}"
            item["reason"] = (
                f"The pinned official table marks this setting {availability.title()} "
                f"for {policy.code.upper()}; it is not exposed by this edition model."
            )
            item["native_surface"] = native_surface(record)
            rejected_items.append(item)
        rejected = Counter(
            str(item["classification"]).removeprefix("excluded_edition_")
            for item in rejected_items
        )
        editions[policy.code] = {
            "baseline_rule": policy.baseline,
            "official_total": len(items),
            "classification_counts": dict(sorted(classifications.items())),
            "unclassified": len(items) - sum(classifications.values()),
            "edition_no_unknown_counts": dict(sorted(rejected.items())),
            "edition_no_unknown_total": sum(rejected.values()),
            "edition_no_unknown_items": rejected_items,
            "items": items,
        }

    return {
        "source": {
            "url": SOURCE_URL,
            "raw_url": SOURCE_RAW_URL,
            "verified_on": VERIFIED_ON,
        },
        "records_in_source": len(records),
        "editions": editions,
    }


def bind_repository_schema_keys(report: dict[str, object], root: Path) -> None:
    """Bind editable and derived rows to the exact live ledger schema keys."""
    for edition, module_id in MODULES.items():
        ledger_path = root / "modules" / module_id / "config-sources.toml"
        ledger = tomllib.loads(ledger_path.read_text(encoding="utf-8-sig"))
        ledger_items = ledger.get("items", [])
        edition_report = report["editions"][edition]
        for item in edition_report["items"]:
            if item["classification"] not in {"schema_native", "specialized", "derived"}:
                continue
            item.pop("schema_key", None)
            source, native_key = ledger_identity(item)
            matches = [
                row
                for row in ledger_items
                if ledger_row_matches(row, item, source, native_key)
            ]
            if len(matches) == 1 and matches[0].get("schema_key"):
                item["schema_key"] = str(matches[0]["schema_key"])


def native_surface(record: dict[str, object]) -> str:
    section = str(record["section"])
    if section.endswith("Command line options"):
        return "server_url" if str(record["raw_name"]).startswith("?") else "launch_arg"
    if section.endswith("[ServerSettings]"):
        return "GameUserSettings.ini:[ServerSettings]"
    if section.endswith("[SessionSettings]"):
        return "GameUserSettings.ini:[SessionSettings]"
    if section.endswith("[/Script/Engine.GameSession]"):
        return "GameUserSettings.ini:[/Script/Engine.GameSession]"
    if section.endswith("[Ragnarok]"):
        return "GameUserSettings.ini:[Ragnarok]"
    if section.endswith("[MessageOfTheDay]"):
        return "GameUserSettings.ini:[MessageOfTheDay]"
    if section.endswith("[ModInstaller]"):
        return "Game.ini:[ModInstaller]"
    if section.endswith("Game.ini"):
        return "Game.ini:[/script/shootergame.shootergamemode]"
    return "external_dynamic_config"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "source",
        type=Path,
        help=(
            "local copy of the pinned ARK Wiki raw wikitext; download it from "
            f"{SOURCE_RAW_URL} and retain SHA256 {PINNED_SOURCE_SHA256}"
        ),
    )
    parser.add_argument("--write", type=Path)
    parser.add_argument(
        "--verify-repository",
        action="store_true",
        help="verify every target mapping against the live schema, ledger, template and writer",
    )
    args = parser.parse_args()
    if not args.source.is_file():
        parser.error(f"source file does not exist: {args.source}")
    try:
        source_bytes = args.source.read_bytes()
    except OSError as error:
        parser.error(f"cannot read source file {args.source}: {error}")
    source_hash = hashlib.sha256(source_bytes).hexdigest()
    if source_hash != PINNED_SOURCE_SHA256:
        print(
            f"pinned source SHA256 mismatch: expected {PINNED_SOURCE_SHA256}, got {source_hash}",
            file=sys.stderr,
        )
        return 1
    report = build_report(parse_records(source_bytes.decode("utf-8")))
    bind_repository_schema_keys(report, Path(__file__).resolve().parents[1])
    report["source"]["sha256"] = source_hash
    report["source"]["license"] = "CC BY-NC-SA 4.0"
    report["source"]["license_url"] = "https://ark.wiki.gg/wiki/ARK_Wiki:Copyrights"
    rendered = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    if args.write:
        args.write.parent.mkdir(parents=True, exist_ok=True)
        args.write.write_text(rendered, encoding="utf-8")
    elif not args.verify_repository:
        print(rendered, end="")

    for edition, result in report["editions"].items():
        assert result["unclassified"] == 0, edition
    assert report["editions"]["ase"]["official_total"] == 432
    assert report["editions"]["asa"]["official_total"] == 270
    if args.verify_repository:
        failures = verify_repository(report, Path(__file__).resolve().parents[1])
        if failures:
            print("ARK official crosswalk verification failed:", file=sys.stderr)
            for failure in failures:
                print(f"  - {failure}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
