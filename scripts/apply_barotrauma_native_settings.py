from __future__ import annotations

import json
import re
import sys
import tomllib
from collections.abc import Iterable
from pathlib import Path
from typing import Any
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.barotrauma_setting_metadata import BOUNDS, ENUMS, FIELDS

MODULE_ROOT = ROOT / "modules" / "barotrauma"
REFERENCE_PATH = MODULE_ROOT / "reference-configs" / "serversettings-v1.13.4.0.xml"
SCHEMA_PATH = MODULE_ROOT / "schema.json"
LEDGER_PATH = MODULE_ROOT / "config-sources.toml"
TEMPLATE_PATH = MODULE_ROOT / "templates" / "serversettings.xml.hbs"

EXCLUDED_ROOT_ATTRIBUTES = {"port", "queryport"}
EXISTING_KEY_OVERRIDES = {
    "ServerName": "server_name",
    "ServerMessageText": "server_message",
    "password": "server_password",
    "MaxPlayers": "max_players",
    "IsPublic": "public_server",
    "VoiceChatEnabled": "voice_chat_enabled",
    "AllowFileTransfers": "allow_file_transfers",
    "TickRate": "tick_rate",
    "RandomizeSeed": "randomize_seed",
    "KarmaEnabled": "karma_enabled",
    "UseRespawnShuttle": "use_respawn_shuttle",
    "RespawnInterval": "respawn_interval",
    "AutoRestart": "auto_restart",
    "AutoRestartInterval": "auto_restart_interval",
    "StartWhenClientsReady": "start_when_clients_ready",
    "AllowVoteKick": "allow_vote_kick",
    "AllowEndVoting": "allow_end_voting",
    "BanAfterWrongPassword": "ban_after_wrong_password",
    "MaxPasswordRetriesBeforeBan": "max_password_retries_before_ban",
    "AutoBanTime": "auto_ban_time",
}


def snake_case(native_key: str) -> str:
    value = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", native_key)
    value = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", value).lower()
    return (
        value.replace("pv_p", "pvp")
        .replace("do_s", "dos")
        .replace("np_cs", "npcs")
        .replace("enableupnp", "enable_upnp")
    )


def setting_key(native_key: str, *, campaign: bool) -> str:
    base = EXISTING_KEY_OVERRIDES.get(native_key, snake_case(native_key))
    return f"campaign_{base}" if campaign else base


def exact_inventory() -> tuple[list[tuple[str, str]], list[tuple[str, str]]]:
    root = ElementTree.parse(REFERENCE_PATH).getroot()
    campaign = root.find("campaignsettings")
    if root.tag != "serversettings" or campaign is None:
        raise ValueError("pinned Barotrauma XML does not contain the expected hierarchy")
    return list(root.attrib.items()), list(campaign.attrib.items())


def native_property(
    native_key: str,
    raw_default: str,
    *,
    campaign: bool,
    order: int,
) -> dict[str, Any]:
    key = setting_key(native_key, campaign=campaign)
    metadata = FIELDS[key]
    if metadata.value_type == "boolean":
        if raw_default.lower() not in {"true", "false"}:
            raise ValueError(f"invalid boolean default for {native_key}")
        default: Any = raw_default.lower() == "true"
    elif metadata.value_type == "integer":
        default = int(raw_default)
    elif metadata.value_type == "number":
        default = float(raw_default)
    else:
        default = raw_default
    prefix = "campaignsettings" if campaign else "serversettings"
    return {
        "default": default,
        "x-lsgm-order": order,
        "x-lsgm-source": "barotrauma_campaign_settings_xml" if campaign else "server_settings",
        "x-lsgm-source-key": f"{prefix}.{native_key}",
        "x-lsgm-source-surface": "config_file",
    }


def apply_metadata(key: str, property_value: dict[str, Any]) -> None:
    metadata = FIELDS[key]
    property_value.update({
        "type": metadata.value_type,
        "title": metadata.en_title,
        "description": metadata.en_description,
        "x-lsgm-section": metadata.section,
    })
    if metadata.value_type == "string" and property_value.get("format") != "textarea":
        property_value["pattern"] = r"^[^\r\n]*$"
    elif metadata.value_type != "string":
        property_value.pop("pattern", None)
    for bound, value in zip(("minimum", "maximum"), BOUNDS.get(key, (None, None))):
        if value is None:
            property_value.pop(bound, None)
        else:
            property_value[bound] = value
    if key in ENUMS:
        property_value["enum"] = [option[0] for option in ENUMS[key]]
    else:
        property_value.pop("enum", None)


def update_schema(
    root_attributes: list[tuple[str, str]],
    campaign_attributes: list[tuple[str, str]],
) -> None:
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8-sig"))
    properties: dict[str, dict[str, Any]] = schema["properties"]
    order = 1000
    for campaign, attributes in ((False, root_attributes), (True, campaign_attributes)):
        for native_key, default in attributes:
            if not campaign and native_key in EXCLUDED_ROOT_ATTRIBUTES:
                continue
            key = setting_key(native_key, campaign=campaign)
            if key in properties:
                properties[key]["x-lsgm-source"] = (
                    "barotrauma_campaign_settings_xml" if campaign else "server_settings"
                )
                properties[key]["x-lsgm-source-key"] = (
                    f"{'campaignsettings' if campaign else 'serversettings'}.{native_key}"
                )
                continue
            properties[key] = native_property(
                native_key,
                default,
                campaign=campaign,
                order=order,
            )
            order += 1
    if set(properties) != set(FIELDS):
        raise ValueError(f"Barotrauma metadata coverage mismatch: {set(properties) ^ set(FIELDS)}")
    for key, property_value in properties.items():
        apply_metadata(key, property_value)
    SCHEMA_PATH.write_text(
        json.dumps(schema, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
        newline="\n",
    )


def toml_item(source: str, native_key: str, schema_key: str, surface: str) -> str:
    return (
        "[[items]]\n"
        f'source = {json.dumps(source)}\n'
        f'key = {json.dumps(native_key)}\n'
        f'schema_key = {json.dumps(schema_key)}\n'
        f'surface = {json.dumps(surface)}\n'
    )


def update_ledger(
    root_attributes: list[tuple[str, str]],
    campaign_attributes: list[tuple[str, str]],
) -> None:
    text = LEDGER_PATH.read_text(encoding="utf-8-sig")
    ledger = tomllib.loads(text)
    prefix = text.split("[[items]]", 1)[0].rstrip()
    exclusions = text.split("[[exclusions]]", 1)[1]
    blocks: list[str] = []
    for native_key, _ in root_attributes:
        if native_key in EXCLUDED_ROOT_ATTRIBUTES:
            continue
        blocks.append(toml_item(
            "server_settings",
            f"serversettings.{native_key}",
            setting_key(native_key, campaign=False),
            "config_file",
        ))
    for native_key, _ in campaign_attributes:
        blocks.append(toml_item(
            "barotrauma_campaign_settings_xml",
            f"campaignsettings.{native_key}",
            setting_key(native_key, campaign=True),
            "config_file",
        ))
    for item in ledger.get("items", []):
        if item["source"] in {"server_settings", "barotrauma_campaign_settings_xml"}:
            continue
        blocks.append(toml_item(
            item["source"], item["key"], item["schema_key"], item["surface"]
        ))
    rendered = (
        f"{prefix}\n\n"
        + "\n".join(block.rstrip() for block in blocks)
        + "\n\n[[exclusions]]"
        + exclusions
    )
    LEDGER_PATH.write_text(rendered, encoding="utf-8", newline="\n")


def template_attribute(native_key: str, *, campaign: bool) -> str:
    if not campaign and native_key == "port":
        value = "{{ports.game.port}}"
    elif not campaign and native_key == "queryport":
        value = "{{ports.query.port}}"
    else:
        value = f"{{{{xml.settings.{setting_key(native_key, campaign=campaign)}}}}}"
    return f'  {native_key}="{value}"'


def update_template(
    root_attributes: Iterable[tuple[str, str]],
    campaign_attributes: Iterable[tuple[str, str]],
) -> None:
    lines = ["<?xml version=\"1.0\" encoding=\"utf-8\"?>", "<serversettings"]
    lines.extend(template_attribute(key, campaign=False) for key, _ in root_attributes)
    lines.append(">")
    lines.append("  <campaignsettings")
    lines.extend(template_attribute(key, campaign=True) for key, _ in campaign_attributes)
    lines.append("  />")
    lines.append("</serversettings>")
    TEMPLATE_PATH.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def update_field_catalogs() -> None:
    catalog_root = ROOT / "apps" / "desktop" / "src" / "i18n" / "games"
    for locale, symbol, title_attr, description_attr, option_index in (
        ("en", "EN_US", "en_title", "en_description", 1),
        ("zh-cn", "ZH_CN", "zh_title", "zh_description", 2),
    ):
        path = catalog_root / f"barotrauma.{locale}.ts"
        text = path.read_text(encoding="utf-8-sig")
        start = text.index("{", text.index(" = "))
        catalog = json.loads(text[start:text.rfind("}") + 1])
        catalog = {key: value for key, value in catalog.items() if not key.startswith("settings.schema.barotrauma.")}
        for key, metadata in FIELDS.items():
            prefix = f"settings.schema.barotrauma.{key}"
            catalog[f"{prefix}.title"] = getattr(metadata, title_attr)
            catalog[f"{prefix}.description"] = getattr(metadata, description_attr)
            for option in ENUMS.get(key, ()):
                catalog[f"{prefix}.option.{option[0].lower()}"] = option[option_index]
        path.write_text(
            'import type { MessageCatalog } from "../../i18n-config";\n\n'
            + f"export const {symbol}_BAROTRAUMA_MESSAGES: MessageCatalog = "
            + json.dumps(catalog, ensure_ascii=False, indent=2) + ";\n",
            encoding="utf-8", newline="\n",
        )


def main() -> int:
    root_attributes, campaign_attributes = exact_inventory()
    update_schema(root_attributes, campaign_attributes)
    update_ledger(root_attributes, campaign_attributes)
    update_template(root_attributes, campaign_attributes)
    update_field_catalogs()
    print(
        f"Barotrauma native inventory applied: {len(root_attributes)} root attributes, "
        f"{len(campaign_attributes)} campaign attributes"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
