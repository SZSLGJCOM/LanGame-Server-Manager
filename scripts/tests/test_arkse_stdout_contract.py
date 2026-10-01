from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


MODULE = Path(__file__).resolve().parents[2] / "modules" / "arksurvivalevolved"


class ArkseStdoutContractTests(unittest.TestCase):
    def test_managed_launch_includes_the_installed_engine_log_verbosity_switch(self) -> None:
        process = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))["process"]
        arguments = process["args_template"]
        # Build 21241282's stdout device drops Log (5) without this parsed flag.
        # Its serializer accepts <= 4 by default and <= 5 when the flag is set.
        self.assertEqual(arguments.count("-stdout"), 1)
        self.assertEqual(arguments.count("-AllowStdOutLogVerbosity"), 1)
        self.assertNotIn("-FullStdOutLogOutput", arguments)
        self.assertIn("-abslog={{arkse.native_log_path}}", arguments)
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")

    def test_every_configuration_fixture_preserves_the_managed_log_arguments(self) -> None:
        fixtures = list((MODULE / "config-fixtures").glob("*.json"))
        self.assertTrue(fixtures)
        for path in fixtures:
            with self.subTest(fixture=path.name):
                fixture = json.loads(path.read_text(encoding="utf-8-sig"))
                arguments = fixture["expected"]["launch"]["arguments"]
                self.assertEqual(arguments.count("-stdout"), 1)
                self.assertEqual(arguments.count("-AllowStdOutLogVerbosity"), 1)
                self.assertTrue(any(value.startswith("-abslog=") for value in arguments))

    def test_native_game_log_launch_requests_the_installed_flush_switch(self) -> None:
        process = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))["process"]
        # Run 101's native document remained empty until shutdown flushed it.
        # The installed file writer parses this flag and flushes after each line.
        self.assertEqual(process["args_template"].count("-FORCELOGFLUSH"), 1)
        for path in (MODULE / "config-fixtures").glob("*.json"):
            with self.subTest(fixture=path.name):
                launch = json.loads(path.read_text(encoding="utf-8-sig"))["expected"]["launch"]
                self.assertEqual(launch["arguments"].count("-FORCELOGFLUSH"), 1)


if __name__ == "__main__":
    unittest.main()
