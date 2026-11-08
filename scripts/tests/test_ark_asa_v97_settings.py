from __future__ import annotations

import json
from pathlib import Path
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
import apply_ark_official_crosswalk as applicator
from verify_module_setting_coverage_ark import ark_aggregate_token_settings


class ArkAsaV97SettingsTests(unittest.TestCase):
    def test_refresh_preserves_v97_keys_and_their_separate_optional_writer(self) -> None:
        schema = json.loads(
            (ROOT / "modules/arksurvivalascended/schema.json").read_text(encoding="utf-8")
        )
        fields = {
            key: prop
            for key, prop in schema["properties"].items()
            if prop.get("x-lsgm-source") == "asa_server_patch_notes_v97"
        }
        self.assertEqual(len(fields), 6)
        tables = applicator.rust_extra_ini_tables("asa", schema)
        generated = applicator.rust_data_file(tables, "ArkIniSetting")
        for key, prop in fields.items():
            with self.subTest(key=key):
                self.assertIn(key, applicator.EDITION_SCHEMA_EXTRAS["asa"])
                self.assertNotIn(key, applicator.EDITION_SCHEMA_EXTRAS["ase"])
                self.assertNotIn("default", prop)
                native = prop["x-lsgm-source-key"].rsplit(".", 1)[1]
                self.assertIn(
                    f'    ("{native}", "{key}", false, false, false),',
                    tables["ARK_ASA_OPTIONAL_GAME_INI"],
                )
        self.assertNotIn("ARK_ASA_OPTIONAL_GAME_INI", applicator.rust_extra_ini_tables("ase", schema))

        # Coverage must inspect the generated optional array as well as older tables.
        generated += "\n".join(
            f"pub(crate) const {name}: &[ArkIniSetting] = &[\n];"
            for name in ("ARK_ASA_GAME_INI", "ARK_ASA_ADVANCED_GAME_INI")
        )
        self.assertEqual(
            ark_aggregate_token_settings("arksa", "additional_game_ini", generated, ""),
            set(fields),
        )


if __name__ == "__main__":
    unittest.main()
