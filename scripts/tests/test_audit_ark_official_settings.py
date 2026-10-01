from __future__ import annotations

import json
import subprocess
import sys
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY_ROOT / "scripts" / "audit_ark_official_settings.py"
sys.path.insert(0, str(SCRIPT.parent))
from audit_ark_official_settings import build_report, parse_records
from ark_official_apply_support import property_for, repeated, scalar_default


class ArkOfficialSettingsAuditCliTests(unittest.TestCase):
    def test_public_report_normalizes_type_facts_and_preserves_both_edition_defaults(self) -> None:
        source = """== Configuration Files ==
=== GameUserSettings.ini ===
==== [ServerSettings] ====
{{Server config variable
| name = BadWordListURL
| inASE = Yes
| inASA = Yes
| type = string with a URL
| default = {{ItemLink|ARK: Survival Evolved|&nbsp;}}: "https://example.test/ase.txt"<br/>{{ItemLink|ARK: Survival Ascended|&nbsp;}}: "https://example.test/asa.txt"
}}
"""
        original = parse_records(source)[0]
        report = build_report([original])
        for edition in report["editions"].values():
            item = edition["items"][0]
            self.assertEqual(item["type"], "URL")
            self.assertEqual(
                item["default"],
                'ASE="https://example.test/ase.txt"; ASA="https://example.test/asa.txt"',
            )
            native = {**original, "native_surface": item["native_surface"]}
            self.assertEqual(property_for(native, 1), property_for(item, 1))
            self.assertEqual(repeated(original), repeated(item))
            self.assertEqual(scalar_default(original), scalar_default(item))
        self.assertNotIn("{{ItemLink", json.dumps(report))

    def test_public_report_rejects_unreviewed_type_or_default_markup(self) -> None:
        base = parse_records("""== Command line options ==
{{Server config variable
| name = -insecure
| inASE = Yes
| inASA = No
| type = boolean
| default = false
}}
""")[0]
        for field, value in (("type", "An unreviewed explanation"), ("default", "{{Unknown|true}}")):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "Unreviewed public"):
                build_report([{**base, field: value}])

    def test_public_report_retains_native_contract_without_source_prose(self) -> None:
        source = """== Command line options ==
{{Server config variable
| name = -insecure
| inASE = Yes
| inASA = No
| default = false
| type = boolean
| info = Synthetic upstream explanation that must not be redistributed.
}}
"""
        records = parse_records(source)
        self.assertEqual(records[0]["native_name"], "-insecure")
        self.assertEqual(records[0]["default"], "false")
        self.assertNotIn("evidence", records[0])
        records[0]["evidence"] = "Synthetic upstream explanation from an older parser."
        report = build_report(records)
        self.assertEqual(
            report["editions"]["ase"]["items"][0]["classification"],
            "excluded_unsafe_internal",
        )
        self.assertNotIn("Synthetic upstream explanation", json.dumps(report))

    def test_checked_in_crosswalk_contains_only_technical_records_and_project_classification(self) -> None:
        report_path = (
            REPOSITORY_ROOT
            / "docs/game-config-acceptance/ark-official-crosswalk-2026-07-13.json"
        )
        report = json.loads(report_path.read_text(encoding="utf-8"))
        allowed = {
            "ordinal", "line", "raw_name", "native_name", "canonical_name", "section",
            "inASE", "inASA", "status", "type", "default", "version", "classification",
            "reason", "native_surface", "schema_key",
        }
        for edition in report["editions"].values():
            for item in edition["items"] + edition["edition_no_unknown_items"]:
                self.assertEqual(set(item) - allowed, set(), item["native_name"])
                self.assertTrue(item["reason"].strip())
                self.assertNotIn("<code>", item["type"], item["native_name"])
                self.assertNotIn("{{", item["default"], item["native_name"])
                if item["canonical_name"] == "activemods":
                    self.assertEqual(item["type"], "list<ModID>; delimiter=','; spaces=false; lines=1")
                if item["canonical_name"] == "valgueromemorialentries":
                    self.assertEqual(item["type"], "list<player_name>; delimiter=';'; spaces=false; lines=1")

    def test_requires_the_externally_supplied_pinned_source(self) -> None:
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT)],
            cwd=REPOSITORY_ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn("the following arguments are required: source", result.stderr)

    def test_rejects_a_missing_source_before_reading_it(self) -> None:
        missing = REPOSITORY_ROOT / "missing-ark-source.wikitext"
        result = subprocess.run(
            [sys.executable, "-B", str(SCRIPT), str(missing)],
            cwd=REPOSITORY_ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )

        self.assertEqual(result.returncode, 2)
        self.assertIn(f"source file does not exist: {missing}", result.stderr)


if __name__ == "__main__":
    unittest.main()
