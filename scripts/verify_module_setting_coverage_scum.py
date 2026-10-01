from __future__ import annotations

import json
from pathlib import Path
from typing import Any


SCUM_NATIVE_SECTIONS = ("general", "world", "features", "respawn", "vehicles", "damage")
SCUM_STRUCTURED_FIELDS = {
    "server_general": "general",
    "server_world": "world",
    "server_features": "features",
    "server_respawn": "respawn",
    "server_vehicles": "vehicles",
    "server_damage": "damage",
    "economy_override": "economy",
    "raid_times": "raid",
    "notifications": "notifications",
}
SCUM_TEMPLATE_CONTRACT = {
    "ServerSettings.ini.hbs": "{{scum.server_settings_ini}}",
    "EconomyOverride.json.hbs": "{{scum.economy_override_json}}",
    "RaidTimes.json.hbs": "{{scum.raid_times_json}}",
    "Notifications.json.hbs": "{{scum.notifications_json}}",
}


def scum_native_section_ids_from_source(source: str) -> set[str]:
    markers = (
        "...NATIVE_SECTIONS.map",
        'id: section.toLowerCase()',
        'parentId: "server-settings"',
    )
    return set(SCUM_NATIVE_SECTIONS) if all(marker in source for marker in markers) else set()


def _read_templates(template_root: Path) -> dict[str, str]:
    return {
        name: (template_root / name).read_text(encoding="utf-8-sig")
        for name in SCUM_TEMPLATE_CONTRACT
    }


def validate_scum_native_settings_contract(
    root: Path,
    *,
    module_source: str | None = None,
    schema: dict[str, Any] | None = None,
    renderer_source: str | None = None,
    template_sources: dict[str, str] | None = None,
) -> list[str]:
    module_root = root / "modules" / "scum"
    if module_source is None:
        module_source = (root / "apps" / "desktop" / "src" / "views" / "settings" /
                         "modules" / "scum.ts").read_text(encoding="utf-8-sig")
    if schema is None:
        schema = json.loads((module_root / "schema.json").read_text(encoding="utf-8-sig"))
    if renderer_source is None:
        renderer_source = (root / "crates" / "app-storage" / "src" /
                           "templates_render_scum.rs").read_text(encoding="utf-8-sig")
    if template_sources is None:
        template_sources = _read_templates(module_root / "templates")

    failures: list[str] = []
    if scum_native_section_ids_from_source(module_source) != set(SCUM_NATIVE_SECTIONS):
        failures.append(
            "scum: Configuration section map must expose all six native ServerSettings sections"
        )

    properties = schema.get("properties", {}) if isinstance(schema, dict) else {}
    for field, section in SCUM_STRUCTURED_FIELDS.items():
        prop = properties.get(field, {}) if isinstance(properties, dict) else {}
        if prop.get("type") not in ({"array"} if field in {"raid_times", "notifications"} else {"object"}):
            failures.append(f"scum: structured schema field {field!r} has the wrong type")
        if prop.get("x-lsgm-section") != section:
            failures.append(
                f"scum: structured schema field {field!r} must remain in section {section!r}"
            )
        if f'"{field}"' not in renderer_source:
            failures.append(
                f"scum: structured schema field {field!r} is not consumed by the typed renderer"
            )

    for name, token in SCUM_TEMPLATE_CONTRACT.items():
        if template_sources.get(name, "").strip() != token:
            failures.append(f"scum: {name} must render through {token}")
    return failures
