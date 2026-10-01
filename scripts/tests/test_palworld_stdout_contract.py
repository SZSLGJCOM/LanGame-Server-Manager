from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


MODULE = Path(__file__).resolve().parents[2] / "modules" / "palworld"
ENTRY = "Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"


class PalworldStdoutContractTests(unittest.TestCase):
    def test_managed_launch_enables_the_installed_engine_stdout_device(self) -> None:
        process = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))["process"]
        # Build 25247047 creates the stdout device only with -stdout. Its parsed
        # FullStdOutLogOutput flag admits all already-enabled log categories.
        self.assertEqual(process["executable"], ENTRY)
        self.assertEqual(process["args_template"].count("-stdout"), 1)
        self.assertEqual(process["args_template"].count("-FullStdOutLogOutput"), 1)
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")
        self.assertIn("-logformat={{settings.log_format}}", process["args_template"])

    def test_every_config_fixture_preserves_the_stdout_device_and_verbosity(self) -> None:
        fixtures = list((MODULE / "config-fixtures").glob("*.json"))
        self.assertTrue(fixtures)
        for path in fixtures:
            with self.subTest(fixture=path.name):
                launch = json.loads(path.read_text(encoding="utf-8-sig"))["expected"]["launch"]
                self.assertEqual(launch["executable_suffix"], ENTRY)
                self.assertEqual(launch["arguments"].count("-stdout"), 1)
                self.assertEqual(launch["arguments"].count("-FullStdOutLogOutput"), 1)
                self.assertTrue(any(value.startswith("-logformat=") for value in launch["arguments"]))


if __name__ == "__main__":
    unittest.main()
