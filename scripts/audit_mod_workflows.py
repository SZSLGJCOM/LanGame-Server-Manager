from __future__ import annotations

import argparse
import csv
import json
import os
import re
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
DESKTOP_SRC = ROOT / "apps" / "desktop" / "src"
MOD_WORKBENCH_PLANS = DESKTOP_SRC / "views" / "servers" / "mod-workbench-plans.ts"
MOD_WORKBENCH_CAPABILITY = DESKTOP_SRC / "views" / "servers" / "mod-workbench-capability.ts"
STORAGE_SRC = ROOT / "crates" / "app-storage" / "src"
APP_RUNTIME_SRC = ROOT / "crates" / "app-runtime" / "src"
REPORT_DIR = Path(
    os.environ.get("LANGAME_REPORT_DIR", ROOT / ".runtime-data" / "reports")
) / "mod-workflows"
CSV_PATH = REPORT_DIR / "game-mod-workflow-audit.csv"
MD_PATH = REPORT_DIR / "game-mod-workflow-audit.md"

NOT_MODELLED_NOTES = {
    "runescapedragonwilds": "The catalog notes Shockbyte-hosted Mod management, but LanGame has no self-hosted mod installation contract. The Mod workspace is hidden.",
}


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig")


def module_ids() -> list[str]:
    return sorted(
        path.name
        for path in MODULES_DIR.iterdir()
        if path.is_dir()
        and (path / "module.toml").exists()
    )


def mod_workbench_catalog() -> dict[str, dict[str, str]]:
    source = read_text(MOD_WORKBENCH_CAPABILITY)
    match = re.search(
        r"MOD_WORKFLOW_CATALOG\s*:[^{]+\{([\s\S]*?)\n\};",
        source,
    )
    if not match:
        raise ValueError("Cannot read the Mod workflow catalog")
    return {
        entry.group(1): dict(re.findall(
            r'(provider|supportStatus|installScope|steamDownloadMode):\s*"([^"\n]*)"', entry.group(2)
        ))
        for entry in re.finditer(r"^\s+([a-z0-9]+):\s*\{([\s\S]*?)^\s+\},?", match.group(1), re.MULTILINE)
    }


def mod_workbench_guardrail_ids() -> set[str]:
    # The catalog's supportStatus: "not_modelled" is not an installation capability.
    return {module_id for module_id, entry in mod_workbench_catalog().items()
            if entry.get("supportStatus") == "not_modelled" or entry.get("installScope") == "client_only"}


def mod_workbench_apply_ids() -> set[str]:
    source = read_text(MOD_WORKBENCH_PLANS)
    match = re.search(r"export function buildModSettingsApplyPlan\([\s\S]*?\n\}", source)
    if not match:
        raise ValueError("Cannot read the Mod settings apply plan")
    return set(re.findall(r'case\s+"([a-z0-9]+)"\s*:', match.group(0)))


def schema_mod_fields(schema: dict) -> list[str]:
    properties = schema.get("properties")
    if not isinstance(properties, dict):
        return []
    fields: list[str] = []
    for key, value in properties.items():
        if not isinstance(value, dict):
            continue
        source_key = str(value.get("x-lsgm-source-key") or "").lower()
        source = str(value.get("x-lsgm-source") or "").lower()
        normalized = key.lower()
        if (
            value.get("x-lsgm-section") == "mods"
            or "workshop" in normalized
            or "workshop" in source
            or "workshop" in source_key
            or re.search(r"(^|[_-])mods?([_-]|$)", normalized)
            or normalized == "allow_client_mod"
        ):
            fields.append(key)
    return sorted(fields)


def storage_surface_text() -> str:
    paths = [
        *sorted(STORAGE_SRC.rglob("*.rs")),
        *sorted(APP_RUNTIME_SRC.rglob("*.rs")),
    ]
    return "\n".join(read_text(path) for path in paths)


def template_surface_text(module_id: str) -> str:
    templates_dir = MODULES_DIR / module_id / "templates"
    if not templates_dir.exists():
        return ""
    return "\n".join(read_text(path) for path in sorted(templates_dir.glob("**/*.hbs")))


def materialization_evidence(module_id: str, fields: list[str], storage_text: str) -> str:
    evidence: list[str] = []
    template_text = template_surface_text(module_id)
    for field in fields:
        if field in template_text:
            evidence.append(f"template:{field}")
        if field in storage_text:
            evidence.append(f"rust:{field}")
    return "; ".join(sorted(set(evidence)))


def read_module(module_id: str) -> tuple[dict, dict]:
    module_root = MODULES_DIR / module_id
    module_toml = tomllib.loads(read_text(module_root / "module.toml"))
    schema = json.loads(read_text(module_root / "schema.json"))
    return module_toml, schema


def manifest_workflow_summary(module_toml: dict) -> tuple[str, str, str, str, str]:
    workshop = module_toml.get("workshop")
    mods = module_toml.get("mods") if isinstance(module_toml.get("mods"), dict) else {}
    source = mods.get("source") if isinstance(mods.get("source"), dict) else None
    staging = mods.get("manual_staging") if isinstance(mods.get("manual_staging"), dict) else None
    enablement = mods.get("enablement") if isinstance(mods.get("enablement"), dict) else None

    workshop_text = ""
    if isinstance(workshop, dict):
        provider = str(workshop.get("provider") or "")
        app_id = workshop.get("consumer_app_id")
        workshop_text = f"{provider}:{app_id}" if app_id else provider

    source_text = ""
    if source:
        source_text = f"{source.get('provider', '')}:{source.get('label', '')}".strip(":")

    staging_text = ""
    if staging:
        staging_text = str(staging.get("target_template") or "")

    enablement_key = ""
    if enablement:
        enablement_key = str(enablement.get("setting_key") or "")

    return workshop_text, source_text, staging_text, enablement_key, str(module_toml.get("name") or "")


def classify(
    workshop: str,
    source: str,
    staging: str,
    enablement: str,
    fields: list[str],
    evidence: str,
    frontend_tab: bool,
    frontend_apply: bool,
    frontend_guardrail: bool,
) -> tuple[str, str]:
    has_manifest = bool(workshop or source or staging or enablement)
    has_mod_fields = bool(fields)

    if frontend_guardrail:
        return "not_modelled", "This workspace only explains client dependencies; it cannot install server Mods."

    if not frontend_tab and not has_manifest:
        return "not_modelled", "No LanGame Mod/Workshop install workflow or workspace is declared."

    if frontend_tab and enablement and has_mod_fields and evidence and frontend_apply:
        return "supported", "Declared source, UI entry, enablement field, settings write path, and materialization evidence are present."

    if frontend_tab and workshop and has_mod_fields and frontend_apply:
        return "supported", "Steam Workshop UI and schema-backed settings write path are present."

    if frontend_tab and frontend_apply and workshop and source and staging and not enablement:
        return "supported", "Steam Workshop items can be downloaded and installed into the game's declared mod loading directory."

    if frontend_tab and source and staging and not enablement:
        return "manual_only", "Local file import and instance inventory are available; there is no generic enable/disable or removal action. Required server loaders are managed separately."

    if has_manifest or has_mod_fields or frontend_tab:
        return "partial", "Some Mod surface exists, but at least one of enablement, materialization evidence, or frontend apply support is missing."

    return "not_modelled", "No LanGame Mod or Workshop workflow is declared for this module."


def audit_rows() -> list[dict[str, str]]:
    catalog = mod_workbench_catalog()
    guardrail_ids = mod_workbench_guardrail_ids()
    apply_ids = mod_workbench_apply_ids()
    storage_text = storage_surface_text()
    rows: list[dict[str, str]] = []

    for module_id in module_ids():
        module_toml, schema = read_module(module_id)
        workshop, source, staging, enablement, name = manifest_workflow_summary(module_toml)
        schema_properties = schema.get("properties") if isinstance(schema.get("properties"), dict) else {}
        fields = schema_mod_fields(schema)
        if enablement in schema_properties and enablement not in fields:
            fields.append(enablement)
            fields.sort()
        has_manifest = bool(workshop or source or staging or enablement)
        catalog_entry = catalog.get(module_id, {})
        # Match moduleHasModWorkbench: a not_modelled catalog entry alone does not expose a tab.
        frontend_tab = has_manifest or (module_id in catalog and module_id not in guardrail_ids)
        frontend_guardrail = frontend_tab and (
            catalog_entry.get("installScope") == "client_only"
            or (module_id in guardrail_ids and not has_manifest)
        )
        evidence = materialization_evidence(module_id, fields, storage_text)
        if frontend_guardrail:
            evidence = "; ".join(filter(bool, [evidence, "UI: unsupported guardrail"]))
        frontend_apply = not frontend_guardrail and (
            module_id in apply_ids
            or (bool(source and staging) and not workshop)
            or bool(source and enablement)
            or bool(workshop and source and staging and not enablement)
        )
        status, note = classify(
            workshop,
            source,
            staging,
            enablement,
            fields,
            evidence,
            frontend_tab,
            frontend_apply,
            frontend_guardrail,
        )
        if status == "not_modelled":
            note = NOT_MODELLED_NOTES.get(module_id, note)
        workflow_kind = "not_modelled" if status == "not_modelled" else "steam_workshop" if workshop else "community_packages"
        package_installation = "none"
        if frontend_apply:
            package_installation = "steamcmd" if workshop else "local_files"
            if not workshop and enablement:
                package_installation = "reference_ids_and_local_files"
            elif not workshop and source.startswith("thunderstore:"):
                package_installation = "verified_thunderstore_and_local_files"
        if module_id == "minecraft":
            note += " Automatic Modrinth installation is blocked until instance loader/version compatibility is modeled."
        elif source.startswith("thunderstore:") and not workshop:
            note += " Thunderstore package links additionally require verified runtime and dependencies."
        rows.append(
            {
                "module_id": module_id,
                "name": name,
                "status": status,
                "workflow_kind": workflow_kind,
                "frontend_tab": str(frontend_tab),
                "frontend_apply": str(frontend_apply),
                "frontend_guardrail": str(frontend_guardrail),
                "package_installation": package_installation,
                "collections": str(workflow_kind == "steam_workshop" and frontend_apply),
                "workshop": workshop,
                "source": source,
                "manual_staging": staging,
                "enablement": enablement,
                "schema_mod_fields": ";".join(fields),
                "materialization_evidence": evidence,
                "note": note,
            }
        )

    return rows


def write_csv(rows: list[dict[str, str]]) -> None:
    CSV_PATH.parent.mkdir(parents=True, exist_ok=True)
    fieldnames = [
        "module_id",
        "name",
        "status",
        "workflow_kind",
        "frontend_tab",
        "frontend_apply",
        "frontend_guardrail",
        "package_installation",
        "collections",
        "workshop",
        "source",
        "manual_staging",
        "enablement",
        "schema_mod_fields",
        "materialization_evidence",
        "note",
    ]
    with CSV_PATH.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def write_markdown(rows: list[dict[str, str]]) -> None:
    counts: dict[str, int] = {}
    for row in rows:
        counts[row["status"]] = counts.get(row["status"], 0) + 1

    lines = [
        "# Game Mod Workflow Audit",
        "",
        "Scope: static coverage of every shipped module's current LanGame declarations and code routes. This is not a game-launch or loader compatibility test, and does not claim a game has no external mod ecosystem. Materialization references are navigation evidence, not proof of correct execution.",
        "",
        "Status meanings:",
        "- supported: UI entry, settings write path, and game-specific enablement/materialization evidence are present.",
        "- manual_only: local file import and inventory, without a generic enable/disable or removal action; some providers also support verified package links.",
        "- partial: some Mod surface exists, but a required link is missing.",
        "- not_modelled: no LanGame Mod/Workshop install or enablement workflow is declared; modules may still expose a guardrail panel that explains why LanGame will not write mod state.",
        "",
        "Summary:",
        f"- modules: {len(rows)}",
        *[f"- {status}: {counts[status]}" for status in sorted(counts)],
        *[f"- {kind}: {sum(row['workflow_kind'] == kind for row in rows)}"
          for kind in ("steam_workshop", "community_packages", "not_modelled")],
        "",
        "| # | Module | Status | UI | Package installation | Source / target | Enablement | Collections | Boundary |",
        "|---:|---|---|---|---|---|---|---|---|",
    ]

    for index, row in enumerate(rows, 1):
        source = row["workshop"] or row["source"] or "-"
        enablement = row["enablement"] or "-"
        target = row["manual_staging"] or "-"
        ui = "explanation only" if row["frontend_guardrail"] == "True" else "workspace" if row["frontend_tab"] == "True" else "hidden"
        lines.append(
            f"| {index} | `{row['module_id']}` | {row['status']} | {ui} | {row['package_installation']} | {source}<br>{target} | {enablement} | {row['collections']} | {row['note']} |"
        )

    MD_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    parser = argparse.ArgumentParser(description="Audit current Mod workflow declarations for every module.")
    parser.add_argument("--check", action="store_true", help="Print coverage without writing report files; fail on partial workflows.")
    args = parser.parse_args()
    rows = audit_rows()
    if args.check:
        for kind in ("steam_workshop", "community_packages", "not_modelled"):
            print(f"{kind}: {sum(row['workflow_kind'] == kind for row in rows)}")
        partial = [row["module_id"] for row in rows if row["status"] == "partial"]
        print(f"checked {len(rows)} modules; partial: {', '.join(partial) or 'none'}")
        raise SystemExit(bool(partial))
    write_csv(rows)
    write_markdown(rows)
    print(f"wrote {len(rows)} module audit rows")
    print(CSV_PATH)
    print(MD_PATH)


if __name__ == "__main__":
    main()
