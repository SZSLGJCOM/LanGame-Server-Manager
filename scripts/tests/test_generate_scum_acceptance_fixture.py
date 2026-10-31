from __future__ import annotations

import importlib.util
import json
import re
import tempfile
import unittest
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "generate_scum_acceptance_fixture.py"
SPEC = importlib.util.spec_from_file_location("generate_scum_acceptance_fixture", SCRIPT)
assert SPEC and SPEC.loader
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)


class ScumHistoricalAcceptanceTests(unittest.TestCase):
    def test_later_provenance_review_preserves_historical_evidence_and_native_expectations(self) -> None:
        historical = json.loads(generator.FIXTURE_PATH.read_text(encoding="utf-8"))
        ledger = (generator.MODULE_ROOT / "config-sources.toml").read_text(encoding="utf-8-sig")
        ledger = re.sub(r'^status = .*$', 'status = "best_effort_verified"', ledger, count=1, flags=re.M)
        ledger = re.sub(r'^last_verified = .*$', 'last_verified = "2027-01-01"', ledger, count=1, flags=re.M)
        ledger += '\n[[sources]]\nid = "later_runtime_observation"\nkind = "runtime_probe"\n'
        with tempfile.TemporaryDirectory() as temporary:
            module = Path(temporary)
            (module / "config-sources.toml").write_text(ledger, encoding="utf-8")
            with mock.patch.object(generator, "MODULE_ROOT", module):
                generated = generator.build_fixture()
        self.assertEqual(historical["evidence"], generated["evidence"])
        for section in ("initial", "settings", "expected"):
            with self.subTest(section=section):
                self.assertEqual(historical[section], generated[section])


if __name__ == "__main__":
    unittest.main()
