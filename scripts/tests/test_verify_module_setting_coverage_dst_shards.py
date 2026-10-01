from __future__ import annotations

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts import verify_module_setting_coverage as verifier
from verify_module_setting_coverage_dst import DST_NATIVE_SHARDS, dst_aggregate_token_settings


class DstShardCoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.source = verifier.read_app_storage_templates_rs()
        cls.fields = verifier.read_schema_properties("dontstarve")

    def test_actual_mod_routes_consume_all_three_fields_on_each_shard(self) -> None:
        for shard in DST_NATIVE_SHARDS:
            with self.subTest(shard=shard):
                self.assertEqual(
                    dst_aggregate_token_settings(f"{shard}_modoverrides", self.source, self.fields),
                    {f"{shard}_{suffix}" for suffix in (
                        "modoverrides_lua", "enabled_workshop_mod_ids", "mod_configuration_options",
                    )},
                )

    def test_extra_world_routes_consume_their_own_raw_configuration(self) -> None:
        for shard in ("islands", "volcano"):
            self.assertEqual(
                dst_aggregate_token_settings(f"{shard}_worldgenoverride", self.source, self.fields),
                {f"{shard}_worldgenoverride_lua"},
            )

    def test_effective_layout_and_shard_id_fields_are_read_from_actual_arms(self) -> None:
        self.assertEqual(
            dst_aggregate_token_settings("shard_enabled", self.source, self.fields),
            {"shard_layout", "enable_caves"},
        )
        self.assertEqual(
            dst_aggregate_token_settings("caves_shard_id", self.source, self.fields),
            {"shard_layout"},
        )

    def test_wrong_shard_or_commented_route_cannot_claim_coverage(self) -> None:
        marker = '"islands_modoverrides" => Some(render_dst_modoverrides(settings, "islands")),'
        self.assertIn(marker, self.source)
        for replacement in (
            marker.replace('settings, "islands"', 'settings, "caves"'),
            f"/* {marker} */",
        ):
            changed = self.source.replace(marker, replacement, 1)
            self.assertIsNone(dst_aggregate_token_settings("islands_modoverrides", changed, self.fields))

    def test_dynamic_assignment_without_settings_consumption_is_not_coverage(self) -> None:
        for consumer, variable, suffix in (
            ("lookup_setting_text", "raw_key", "modoverrides_lua"),
            ("parse_workshop_id_list", "enabled_key", "enabled_workshop_mod_ids"),
            ("render_dst_mod_configuration_block", "config_key", "mod_configuration_options"),
        ):
            marker = f"{consumer}(settings, &{variable}"
            self.assertIn(marker, self.source)
            changed = self.source.replace(marker, f"removed_consumer(settings, &{variable}", 1)
            for shard in DST_NATIVE_SHARDS:
                coverage = dst_aggregate_token_settings(f"{shard}_modoverrides", changed, self.fields)
                self.assertIsNotNone(coverage)
                self.assertNotIn(f"{shard}_{suffix}", coverage)

    def test_comment_with_the_dynamic_assignment_cannot_restore_coverage(self) -> None:
        marker = 'let enabled_key = format!("{shard}_enabled_workshop_mod_ids");'
        self.assertIn(marker, self.source)
        changed = self.source.replace(marker, f'/* {marker} */ let enabled_key = "removed";', 1)
        coverage = dst_aggregate_token_settings("volcano_modoverrides", changed, self.fields)
        self.assertNotIn("volcano_enabled_workshop_mod_ids", coverage)

    def test_world_preset_key_without_its_bound_consumer_is_not_coverage(self) -> None:
        marker = "lookup_setting_text(settings, settings_preset_key)"
        self.assertIn(marker, self.source)
        changed = self.source.replace(marker, f"/* {marker} */ removed_consumer(settings, settings_preset_key)", 1)
        for shard in ("master", "caves"):
            coverage = dst_aggregate_token_settings(f"{shard}_worldgenoverride", changed, self.fields)
            self.assertNotIn(f"{shard}_settings_preset", coverage)

    def test_all_dst_templates_resolve_with_complete_schema_coverage(self) -> None:
        referenced, unknown = verifier.referenced_settings(
            "dontstarve", verifier.read_rendered_surfaces("dontstarve"), self.fields,
        )
        self.assertEqual(unknown, set())
        self.assertEqual(self.fields - referenced, set())


if __name__ == "__main__":
    unittest.main()
