from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def schema_properties(module_id: str) -> dict[str, object]:
    path = ROOT / "modules" / module_id / "schema.json"
    return json.loads(path.read_text(encoding="utf-8-sig"))["properties"]


def ledger(module_id: str) -> dict[str, object]:
    path = ROOT / "modules" / module_id / "config-sources.toml"
    return tomllib.loads(path.read_text(encoding="utf-8-sig"))


def item_map(module_id: str) -> dict[tuple[str, str], str]:
    return {
        (item["source"], item["key"]): item["schema_key"]
        for item in ledger(module_id)["items"]
    }


class NativeInventoryContractTests(unittest.TestCase):
    def test_exact_build_gaps_are_native_schema_or_specialized_fields(self) -> None:
        corekeeper = schema_properties("corekeeper")
        self.assertIn("hashed_world_seed", corekeeper)

        project_zomboid = schema_properties("projectzomboid")
        self.assertIn("allow_coop", project_zomboid)

        vrising = schema_properties("vrising")
        typed_vrising = {
            "vampire_max_health_modifier",
            "vampire_physical_power_modifier",
            "vampire_spell_power_modifier",
            "vampire_resource_power_modifier",
            "vampire_siege_power_modifier",
            "vampire_damage_received_modifier",
            "vampire_revive_cancel_delay",
            "global_unit_max_health_modifier",
            "global_unit_power_modifier",
            "global_unit_level_increase",
            "vblood_unit_max_health_modifier",
            "vblood_unit_power_modifier",
            "vblood_unit_level_increase",
            "global_equipment_max_health_modifier",
            "global_equipment_resource_yield_modifier",
            "global_equipment_physical_power_modifier",
            "global_equipment_spell_power_modifier",
            "global_equipment_siege_power_modifier",
            "global_equipment_movement_speed_modifier",
            *{
                f"castle_heart_level_{level}_{limit}"
                for level in range(1, 6)
                for limit in ("floor_limit", "servant_limit", "height_limit")
            },
        }
        self.assertEqual(34, len(typed_vrising))
        self.assertTrue(typed_vrising.issubset(vrising))

        vrising_items = item_map("vrising")
        for native_key in (
            "VBloodUnitSettings",
            "UnlockedAchievements",
            "UnlockedResearchs",
        ):
            self.assertEqual(
                "server_game_settings_json",
                vrising_items[("server_game_settings_json", native_key)],
            )

    def test_exact_fixture_names_are_present(self) -> None:
        expected = {
            "corekeeper": "2026-07-13-steamcmd_anonymous_probe.json",
            "enshrouded": "2026-07-13-steamcmd_anonymous_probe_2278520.json",
            "minecraft": "2026-07-13-mojang_manifest_release_26_2.json",
            "necesse": "2026-09-28-native_server_cfg_build_24926481.json",
            "projectzomboid": "2026-09-28-native_server_options_build_24909836.json",
            "sonsoftheforest": "2026-07-13-steamcmd_anonymous_app_2465200.json",
            "vrising": "2026-07-13-steamcmd_anonymous_app_1829350.json",
        }
        for module_id, file_name in expected.items():
            with self.subTest(module_id=module_id):
                self.assertTrue(
                    (ROOT / "modules" / module_id / "config-fixtures" / file_name).is_file()
                )


if __name__ == "__main__":
    unittest.main()
