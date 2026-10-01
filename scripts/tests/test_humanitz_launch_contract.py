from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


MODULE = Path(__file__).resolve().parents[2] / "modules" / "humanitz"


class HumanitzLaunchContractTests(unittest.TestCase):
    def test_launch_preserves_first_party_log_console_on_private_desktop(self) -> None:
        manifest = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))
        process = manifest["process"]
        # The shipped build 23914958 ReadMe includes -log in both Windows
        # examples. This contract tests the launch, not runtime signal handling.
        self.assertEqual(process["args_template"].count("-log"), 1)
        self.assertNotIn("-newconsole", [arg.lower() for arg in process["args_template"]])
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")
        fixture = json.loads((MODULE / "config-fixtures" / "2026-07-13-steamcmd_anonymous_app_2728330.json").read_text(encoding="utf-8"))
        self.assertIn("-log", fixture["expected"]["launch"]["arguments"])


if __name__ == "__main__":
    unittest.main()
