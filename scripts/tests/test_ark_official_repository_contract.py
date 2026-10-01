from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
from ark_official_repository_contract import verify_repository
from ark_official_apply_support import PROJECT_FIELD_DESCRIPTIONS, property_for
import apply_ark_official_crosswalk as applicator
from apply_ark_official_crosswalk import rust_data_file
from ark_asa_additional_settings import ASA_ADDITIONAL_ARRAY, ASA_ADDITIONAL_SOURCE


REPORT = json.loads(
    (ROOT / "docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json").read_text(
        encoding="utf-8"
    )
)


class ArkOfficialRepositoryContractTests(unittest.TestCase):
    def test_generated_rust_inventory_omits_empty_tables(self) -> None:
        source = rust_data_file(
            {
                "ARK_ASA_EMPTY": [],
                "ARK_ASA_PRESENT": ['    ("Native", "setting", false, false, false),'],
            },
            "ArkIniSetting",
        )

        self.assertNotIn("ARK_ASA_EMPTY", source)
        self.assertIn("ARK_ASA_PRESENT", source)

    def test_inferred_numeric_launch_defaults_keep_their_native_type(self) -> None:
        item = {
            "raw_name": "-MaxNumOfSaveBackups=<integer>",
            "native_name": "-MaxNumOfSaveBackups",
            "native_surface": "launch_arg",
            "type": "",
            "default": "20",
            "evidence": "Native integer option.",
            "section": "Command line options",
        }

        prop = property_for(item, 1)

        self.assertEqual(prop["type"], "integer")
        self.assertEqual(prop["default"], 20)

    def test_live_repository_proves_every_native_mapping(self) -> None:
        self.assertEqual(verify_repository(REPORT, ROOT), [])

    def test_native_refresh_preserves_reviewed_semantic_section(self) -> None:
        item = {
            "raw_name": "CraftXPMultiplier",
            "native_name": "CraftXPMultiplier",
            "native_surface": "Game.ini:[/script/shootergame.shootergamemode]",
            "type": "float",
            "default": "1.0",
            "evidence": "Scales the amount of XP earned for crafting.",
            "section": "Game.ini",
        }
        original = property_for(item, 120)
        refreshed = property_for(item, 120, section="rates")
        self.assertEqual(original["x-lsgm-section"], "advanced")
        self.assertEqual(refreshed, {**original, "x-lsgm-section": "rates"})

    def test_native_refresh_does_not_import_source_prose_or_discard_project_help(self) -> None:
        item = {
            "raw_name": "CraftXPMultiplier", "native_name": "CraftXPMultiplier",
            "native_surface": "Game.ini:[/script/shootergame.shootergamemode]",
            "type": "float", "default": "1.0", "section": "Game.ini",
            "evidence": "Synthetic source prose with separate redistribution terms.",
        }
        original = property_for(item, 120)
        self.assertNotIn("description", original)
        help_text = "Choose the crafting reward rate for this world."
        refreshed = property_for(item, 120, description=help_text)
        self.assertEqual(refreshed, {**original, "description": help_text})

    def test_native_refresh_requires_reviewed_help_and_uses_its_canonical_source(self) -> None:
        item = {
            "raw_name": "-crossplay", "native_name": "-crossplay",
            "native_surface": "launch_arg", "type": "boolean", "default": "false",
            "section": "Command line options", "schema_key": "crossplay",
            "evidence": "Synthetic upstream explanation that must never become help.",
        }
        module_id = "arksurvivalevolved"
        refreshed = property_for(item, 1, module_id=module_id, description="Stale help")
        self.assertEqual(refreshed["description"], PROJECT_FIELD_DESCRIPTIONS[module_id]["crossplay"]["en"])
        item["schema_key"] = "unreviewed_field"
        with self.assertRaisesRegex(ValueError, "Missing project-authored help"):
            property_for(item, 1, module_id=module_id)

    def test_description_refresh_changes_only_reviewed_help_and_is_idempotent(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crosswalk = root / "crosswalk.json"
            crosswalk.write_text(json.dumps(REPORT), encoding="utf-8")
            original_schemas = {}
            for module_id, descriptions in PROJECT_FIELD_DESCRIPTIONS.items():
                schema = json.loads((ROOT / "modules" / module_id / "schema.json").read_text(encoding="utf-8"))
                for key, translations in descriptions.items():
                    self.assertEqual(set(translations), {"en", "zh-CN"})
                    self.assertTrue(all(text.strip() for text in translations.values()))
                    schema["properties"][key]["description"] = "Stale project help"
                original_schemas[module_id] = schema
                schema_path = root / "modules" / module_id / "schema.json"
                schema_path.parent.mkdir(parents=True)
                schema_path.write_text(json.dumps(schema), encoding="utf-8")
            with patch.object(applicator, "ROOT", root), patch.object(applicator, "CROSSWALK", crosswalk), \
                    patch.object(sys, "argv", ["apply_ark_official_crosswalk.py", "--descriptions-only"]):
                self.assertEqual(applicator.main(), 0)
                first = {path: path.read_bytes() for path in root.rglob("*.json")}
                self.assertEqual(applicator.main(), 0)
                self.assertEqual(first, {path: path.read_bytes() for path in root.rglob("*.json")})
            self.assertEqual(json.loads(crosswalk.read_text(encoding="utf-8")), REPORT)
            for module_id, before in original_schemas.items():
                after = json.loads((root / "modules" / module_id / "schema.json").read_text(encoding="utf-8"))
                for key, translations in PROJECT_FIELD_DESCRIPTIONS[module_id].items():
                    self.assertEqual(after["properties"][key]["description"], translations["en"])
                    before["properties"][key].pop("description")
                    after["properties"][key].pop("description")
                self.assertEqual(before, after, "Descriptions-only generation must preserve every native contract")

    def test_deleting_a_ledger_mapping_fails(self) -> None:
        def delete_mapping(edition: str, contract: dict[str, object]) -> None:
            if edition != "ase":
                return
            ledger = contract["ledger"]
            items = ledger["items"]
            items[:] = [
                row
                for row in items
                if not (
                    row.get("source") == "launch_args"
                    and row.get("key") == "-ActiveEvent"
                )
            ]

        failures = verify_repository(REPORT, ROOT, delete_mapping)
        self.assertTrue(any("-ActiveEvent" in failure for failure in failures))

    def test_supplement_cannot_rely_on_the_historical_unknown_source(self) -> None:
        def remove_evidence(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                sources = contract["ledger"]["sources"]
                sources[:] = [row for row in sources if row.get("id") != ASA_ADDITIONAL_SOURCE]

        failures = verify_repository(REPORT, ROOT, remove_evidence)
        self.assertTrue(any("independently documented evidence" in failure for failure in failures))

    def test_supplement_requires_an_independent_native_output_fixture(self) -> None:
        def remove_fixture(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                for fixture in contract["fixtures"]:
                    if ASA_ADDITIONAL_SOURCE in fixture.get("evidence", {}).get("source_ids", []):
                        fixture["expected"]["files"] = []

        failures = verify_repository(REPORT, ROOT, remove_fixture)
        self.assertTrue(any("independent fixture omits native Game.ini output" in failure for failure in failures))

    def test_supplement_cannot_write_the_same_key_to_two_ini_sections(self) -> None:
        def duplicate_writer(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                arrays = contract["ini_arrays"]
                arrays["ARK_ASA_GUS_SERVER_SETTINGS"].append(arrays[ASA_ADDITIONAL_ARRAY][0])

        failures = verify_repository(REPORT, ROOT, duplicate_writer)
        self.assertTrue(any("duplicate or misplaced writer" in failure for failure in failures))

    def test_supplement_does_not_allow_arbitrary_unverified_fields(self) -> None:
        def add_unknown(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                contract["schema"]["properties"]["unverified_setting"] = {
                    "type": "number", "x-lsgm-source": ASA_ADDITIONAL_SOURCE,
                }

        failures = verify_repository(REPORT, ROOT, add_unknown)
        self.assertTrue(any("supplemental schema inventory differs" in failure for failure in failures))

    def test_supplement_rejects_a_matching_schema_and_ledger_typo(self) -> None:
        def change_source_key(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                key = "supply_crate_loot_quality_multiplier"
                prop = contract["schema"]["properties"][key]
                prop["x-lsgm-source-key"] += "_typo"
                for row in contract["ledger"]["items"]:
                    if row.get("schema_key") == key:
                        row["key"] = prop["x-lsgm-source-key"]

        failures = verify_repository(REPORT, ROOT, change_source_key)
        self.assertTrue(any("schema source, surface or repeated-line editor is incorrect" in failure for failure in failures))

    def test_duplicating_a_writer_row_fails(self) -> None:
        def duplicate_writer(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                rows = contract["launch_arrays"]["ARK_ASA_ADDITIONAL_LAUNCH_SETTINGS"]
                rows.append(rows[0])

        failures = verify_repository(REPORT, ROOT, duplicate_writer)
        self.assertTrue(any("exact writer mismatch" in failure for failure in failures))

    def test_moving_an_ini_row_to_the_wrong_surface_fails(self) -> None:
        def move_surface(edition: str, contract: dict[str, object]) -> None:
            if edition == "ase":
                arrays = contract["ini_arrays"]
                row = arrays["ARK_ASE_GUS_SERVER_SETTINGS"].pop(0)
                arrays["ARK_ASE_GAME_INI"].append(row)

        failures = verify_repository(REPORT, ROOT, move_surface)
        self.assertGreaterEqual(
            sum("exact writer mismatch" in failure for failure in failures),
            2,
        )

    def test_removing_instance_name_default_source_fails(self) -> None:
        def remove_default_source(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                contract["schema"]["properties"]["server_name"].pop(
                    "x-lsgm-default-source"
                )

        failures = verify_repository(REPORT, ROOT, remove_default_source)

        self.assertTrue(
            any(
                "asa:server_name" in failure and "instance_name" in failure
                for failure in failures
            )
        )

    def test_removing_a_server_url_string_pattern_fails(self) -> None:
        def remove_pattern(edition: str, contract: dict[str, object]) -> None:
            key = "server_name" if edition == "asa" else "map_name"
            contract["schema"]["properties"][key].pop("pattern", None)

        failures = verify_repository(REPORT, ROOT, remove_pattern)

        self.assertTrue(
            any(
                "asa:server_name" in failure and "unsafe server URL pattern" in failure
                for failure in failures
            )
        )
        self.assertTrue(
            any(
                "ase:map_name" in failure and "unsafe server URL pattern" in failure
                for failure in failures
            )
        )

    def test_removing_an_ini_password_line_boundary_fails(self) -> None:
        def remove_pattern(edition: str, contract: dict[str, object]) -> None:
            if edition == "asa":
                for key in ("server_password", "admin_password"):
                    contract["schema"]["properties"][key].pop("pattern", None)

        failures = verify_repository(REPORT, ROOT, remove_pattern)

        for key in ("server_password", "admin_password"):
            self.assertTrue(
                any(f"asa:{key}: unsafe INI password pattern" in failure for failure in failures)
            )


if __name__ == "__main__":
    unittest.main()
