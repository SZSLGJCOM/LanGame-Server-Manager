from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / "modules" / "conanexiles"


class ConanShutdownContractTests(unittest.TestCase):
    def test_shutdown_uses_command_reported_by_installed_enhanced_server(self) -> None:
        manifest = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))
        evidence = json.loads(
            (MODULE / "runtime-fixtures" / "2026-10-01-rcon-shutdown.json").read_text(encoding="utf-8")
        )
        commands = manifest["runtime"]["shutdown"]["commands"]
        self.assertEqual(len(commands), 1)
        command = commands[0]
        native_commands = {
            line.strip().removeprefix("Usage: ").split()[0].lower()
            for line in evidence["help_response"].splitlines()
            if line.strip().startswith("Usage: ")
        }
        self.assertIn(command["command"].lower(), native_commands)
        self.assertNotIn("saveworld", native_commands)
        self.assertNotIn("doexit", native_commands)
        self.assertEqual(command["transport"], "source_rcon")
        self.assertEqual(command["fallback_transport"], "console_ctrl_c")
        self.assertEqual(command["port_name"], "rcon")
        self.assertEqual(command["password_setting_key"], "rcon_password")
        self.assertEqual(command["enabled_setting_key"], "rcon_enabled")
        self.assertGreater(manifest["runtime"]["shutdown"]["grace_period_ms"], evidence["observed_native_shutdown_ms"])
        self.assertGreater(command["wait_after_ms"], 0)
        self.assertNotIn("wait_after_ms", manifest["player_management"])


if __name__ == "__main__":
    unittest.main()
