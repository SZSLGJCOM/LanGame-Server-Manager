from __future__ import annotations

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts import apply_barotrauma_native_settings as generator
from scripts.barotrauma_setting_metadata import FIELDS


class BarotraumaSettingMetadataTests(unittest.TestCase):
    def test_all_native_settings_have_reviewed_metadata(self):
        root, campaign = generator.exact_inventory()
        native_keys = {
            generator.setting_key(key, campaign=is_campaign)
            for is_campaign, attributes in ((False, root), (True, campaign))
            for key, _ in attributes
            if is_campaign or key not in generator.EXCLUDED_ROOT_ATTRIBUTES
        }
        self.assertEqual(native_keys, set(FIELDS) - {
            "admin_entries", "mod_workshop_ids", "extra_launch_args"
        })
        self.assertEqual(len(FIELDS), 125)

    def test_integer_looking_float_defaults_keep_native_types(self):
        for native_key, campaign in (
            ("RespawnInterval", False), ("TraitorProbability", False),
            ("CrewVitalityMultiplier", True), ("FuelMultiplier", True),
        ):
            with self.subTest(native_key=native_key):
                value = generator.native_property(native_key, "1", campaign=campaign, order=1)
                generator.apply_metadata(generator.setting_key(native_key, campaign=campaign), value)
                self.assertEqual(value["type"], "number")
                self.assertIsInstance(value["default"], float)

    def test_unknown_native_setting_requires_a_semantic_review(self):
        with self.assertRaises(KeyError):
            generator.native_property("FutureSetting", "1", campaign=False, order=1)

    def test_ui_slider_ranges_do_not_restrict_native_configuration(self):
        for key in (
            "campaign_crew_vitality_multiplier", "campaign_oxygen_multiplier",
            "new_campaign_default_salary", "disembark_point_allowance",
            "min_respawn_ratio", "skill_loss_percentage_on_death",
        ):
            with self.subTest(key=key):
                value = {"minimum": 0, "maximum": 1}
                generator.apply_metadata(key, value)
                self.assertNotIn("minimum", value)
                self.assertNotIn("maximum", value)
        value = {}
        generator.apply_metadata("respawn_interval", value)
        self.assertEqual(value["minimum"], 1)
        self.assertNotIn("maximum", value)

    def test_content_identifiers_and_lists_remain_extensible(self):
        for key in (
            "game_mode_identifier", "mission_types", "allowed_random_mission_types",
            "language", "biome", "selected_submarine", "selected_coalition_perks",
            "campaign_start_item_set", "karma_preset",
        ):
            with self.subTest(key=key):
                value = {"enum": ["old_value"]}
                generator.apply_metadata(key, value)
                self.assertEqual(value["type"], "string")
                self.assertNotIn("enum", value)

    def test_regeneration_preserves_curated_sections_and_existing_defaults(self):
        original = generator.SCHEMA_PATH.read_text(encoding="utf-8")
        expected_defaults = {
            key: value.get("default") for key, value in json.loads(original)["properties"].items()
        }
        root, campaign = generator.exact_inventory()
        with tempfile.TemporaryDirectory() as directory:
            schema_path = Path(directory) / "schema.json"
            schema_path.write_text(original, encoding="utf-8")
            with patch.object(generator, "SCHEMA_PATH", schema_path):
                generator.update_schema(root, campaign)
                first = schema_path.read_bytes()
                generator.update_schema(root, campaign)
                self.assertEqual(schema_path.read_bytes(), first)
            properties = json.loads(first)["properties"]
        self.assertEqual({key: value.get("default") for key, value in properties.items()}, expected_defaults)
        for key, section in {
            "tick_rate": "runtime", "lines_per_log_file": "runtime",
            "save_server_logs": "runtime", "max_transport_time": "round",
            "allow_remote_campaign_interactions": "campaign", "allow_spectating": "gameplay",
            "event_removal_time": "network", "auto_restart": "round",
        }.items():
            self.assertEqual(properties[key]["x-lsgm-section"], section, key)

    def test_field_catalog_regeneration_keeps_public_export_names_and_group_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            catalogs = repository / "apps/desktop/src/i18n/games"
            catalogs.mkdir(parents=True)
            for locale, symbol in (("en", "EN_US"), ("zh-cn", "ZH_CN")):
                (catalogs / f"barotrauma.{locale}.ts").write_text(
                    f"export const {symbol}_BAROTRAUMA_MESSAGES: MessageCatalog = "
                    '{"barotrauma.settings.groups.logs.title": "reviewed-group"};\n',
                    encoding="utf-8",
                )
            with patch.object(generator, "ROOT", repository):
                generator.update_field_catalogs()
                before = {path.name: path.read_bytes() for path in catalogs.iterdir()}
                generator.update_field_catalogs()
                self.assertEqual({path.name: path.read_bytes() for path in catalogs.iterdir()}, before)
            for locale, symbol in (("en", "EN_US"), ("zh-cn", "ZH_CN")):
                output = (catalogs / f"barotrauma.{locale}.ts").read_text(encoding="utf-8")
                self.assertIn(f"export const {symbol}_BAROTRAUMA_MESSAGES", output)
                self.assertIn('"barotrauma.settings.groups.logs.title": "reviewed-group"', output)
                self.assertIn('"settings.schema.barotrauma.campaign_oxygen_multiplier.title"', output)


if __name__ == "__main__":
    unittest.main()
