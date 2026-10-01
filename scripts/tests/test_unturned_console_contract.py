from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class UnturnedConsoleContractTests(unittest.TestCase):
    def test_unturned_commands_use_the_owned_console_input_buffer(self) -> None:
        module = tomllib.loads((ROOT / "modules/unturned/module.toml").read_text(encoding="utf-8"))
        process = module["process"]
        evidence = json.loads((ROOT / "modules/unturned/runtime-fixtures/2026-10-01-windows-console-input.json")
                              .read_text(encoding="utf-8"))
        # The official Windows handler gates ReadLine on Console.KeyAvailable.
        # Pipe writes alone do not supply those console keyboard events.
        self.assertEqual(process["host_surface"], "managed_pseudo_console")
        self.assertEqual(process["host_surface"], evidence["candidate_host_surface"])
        self.assertEqual(evidence["input_calls_in_order"], ["get_KeyAvailable", "ReadLine", "Enqueue"])
        self.assertEqual(process["window_policy"], "background")
        self.assertEqual(process["executable"], "Unturned.exe")
        self.assertEqual(process["working_directory_template"], "{{paths.install_root}}")
        commands = module["runtime"]["shutdown"]["commands"]
        self.assertEqual([(item["transport"], item["command"]) for item in commands],
                         [("stdin", "Save"), ("stdin", "Shutdown")])
        self.assertTrue(all(action["transport"] == "stdin"
                            for action in module["runtime"]["player_actions"]))
        arguments = process["args_template"]
        self.assertNotIn("-NoDefaultConsole", arguments)
        self.assertNotIn("-LegacyConsole", arguments)


if __name__ == "__main__":
    unittest.main()
