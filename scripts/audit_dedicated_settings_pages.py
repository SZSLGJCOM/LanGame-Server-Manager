from __future__ import annotations

import argparse
import json
import subprocess
import tomllib
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
SETTINGS_DIR = ROOT / "apps" / "desktop" / "src" / "views" / "settings"
CONFIGURATION_WORKSPACE_TSX = SETTINGS_DIR / "ConfigurationWorkspace.tsx"
SETTINGS_REGISTRY_READER = ROOT / "scripts" / "read_settings_module_ids.cjs"

MODULE_SETTINGS_FILE_NAMES = {
    "arksurvivalascended": "ark-asa",
    "arksurvivalevolved": "ark-ase",
}


@dataclass(frozen=True)
class ModuleSettingsPageStatus:
    module_id: str
    name: str
    schema_fields: int
    template_count: int
    route_kind: str
    definition_file: str
    player_management: str


def read_text(path: Path) -> str:
    return path.read_text(encoding="utf-8-sig") if path.exists() else ""


def module_ids() -> list[str]:
    return sorted(
        path.name
        for path in MODULES_DIR.iterdir()
        if path.is_dir() and (path / "module.toml").exists() and (path / "schema.json").exists()
    )


def read_module_toml(module_id: str) -> dict:
    return tomllib.loads(read_text(MODULES_DIR / module_id / "module.toml"))


def read_schema_field_count(module_id: str) -> int:
    data = json.loads((MODULES_DIR / module_id / "schema.json").read_text(encoding="utf-8-sig"))
    properties = data.get("properties", {})
    return len(properties) if isinstance(properties, dict) else 0


def read_template_count(module_id: str) -> int:
    template_root = MODULES_DIR / module_id / "templates"
    if not template_root.exists():
        return 0
    return len(list(template_root.rglob("*.hbs")))


def definition_file_name(module_id: str) -> str:
    return MODULE_SETTINGS_FILE_NAMES.get(module_id, module_id)


def definition_file_status(module_id: str) -> str:
    file_name = definition_file_name(module_id)
    path = SETTINGS_DIR / "modules" / f"{file_name}.ts"
    return path.relative_to(ROOT).as_posix() if path.exists() else ""


def registered_module_ids() -> set[str]:
    try:
        result = subprocess.run(
            ["node", str(SETTINGS_REGISTRY_READER)],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
        value = json.loads(result.stdout)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
        raise RuntimeError(f"unable to evaluate the Configuration registry: {error}") from error
    if not isinstance(value, list) or any(not isinstance(module_id, str) for module_id in value):
        raise RuntimeError("Configuration registry reader returned an invalid module ID list")
    return set(value)


def legacy_settings_shells() -> list[Path]:
    legacy = [path for path in SETTINGS_DIR.glob("*SettingsModal.tsx")]
    router = SETTINGS_DIR / "SettingsModalRouter.tsx"
    if router.exists():
        legacy.append(router)
    return sorted(set(legacy))


def player_management_status(module_toml: dict) -> str:
    section = module_toml.get("player_management", {})
    if not isinstance(section, dict):
        return "missing"
    return str(section.get("status") or "missing")


def collect_statuses(registered_ids: set[str] | None = None) -> list[ModuleSettingsPageStatus]:
    statuses: list[ModuleSettingsPageStatus] = []
    registered_ids = registered_ids if registered_ids is not None else registered_module_ids()
    workspace_exists = CONFIGURATION_WORKSPACE_TSX.exists()

    for module_id in module_ids():
        module_toml = read_module_toml(module_id)
        statuses.append(
            ModuleSettingsPageStatus(
                module_id=module_id,
                name=str(module_toml.get("name") or module_id),
                schema_fields=read_schema_field_count(module_id),
                template_count=read_template_count(module_id),
                route_kind=(
                    "configuration_workspace"
                    if module_id in registered_ids and workspace_exists
                    else "missing"
                ),
                definition_file=definition_file_status(module_id),
                player_management=player_management_status(module_toml),
            )
        )

    return statuses


def print_markdown(statuses: list[ModuleSettingsPageStatus], unexpected_registrations: set[str]) -> None:
    missing_workspace = [status for status in statuses if status.route_kind != "configuration_workspace"]
    missing_definition = [status for status in statuses if not status.definition_file]
    legacy_shells = legacy_settings_shells()

    print("# Dedicated Configuration Workspace Audit")
    print()
    print(f"- modules: {len(statuses)}")
    print(f"- unified Configuration registrations: {len(statuses) - len(missing_workspace)}")
    print(f"- missing Configuration registrations: {len(missing_workspace)}")
    print(f"- unexpected Configuration registrations: {len(unexpected_registrations)}")
    print(f"- missing per-game definition files: {len(missing_definition)}")
    print(f"- legacy settings shells: {len(legacy_shells)}")
    print()
    print("| module | route | definition | fields | templates | player management |")
    print("| --- | --- | --- | ---: | ---: | --- |")

    for status in statuses:
        definition = f"`{status.definition_file}`" if status.definition_file else "missing"
        print(
            f"| `{status.module_id}` | {status.route_kind} | {definition} | "
            f"{status.schema_fields} | {status.template_count} | {status.player_management} |"
        )

    if unexpected_registrations:
        print()
        print("Unexpected Configuration registrations:")
        for module_id in sorted(unexpected_registrations):
            print(f"- `{module_id}`")


def main() -> int:
    parser = argparse.ArgumentParser(description="Audit unified per-game Configuration workspace coverage.")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail if a module lacks Configuration registration/definition or a legacy settings shell remains.",
    )
    args = parser.parse_args()

    bundled_ids = set(module_ids())
    registered_ids = registered_module_ids()
    unexpected_registrations = registered_ids - bundled_ids
    statuses = collect_statuses(registered_ids)
    print_markdown(statuses, unexpected_registrations)

    if args.strict:
        failures = [
            status.module_id
            for status in statuses
            if status.route_kind != "configuration_workspace" or not status.definition_file
        ]
        failures.extend(sorted(unexpected_registrations))
        failures.extend(path.name for path in legacy_settings_shells())
        if failures:
            print()
            print("strict dedicated settings page audit failed:")
            for module_id in failures:
                print(f"- {module_id}")
            return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
