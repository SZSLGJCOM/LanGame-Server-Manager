from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


MODULE = Path(__file__).resolve().parents[2] / "modules" / "squad"


class SquadRuntimeContractTests(unittest.TestCase):
    def test_declared_shutdown_uses_native_nonforced_rcon_command(self) -> None:
        module = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))
        evidence = json.loads((MODULE / "runtime-fixtures" / "2026-10-01-rcon-shutdown.json").read_text(encoding="utf-8"))
        commands = module["runtime"]["shutdown"]["commands"]
        self.assertEqual(len(commands), 1)
        command = commands[0]
        self.assertTrue(evidence["help_response"].startswith("AdminKillServer <Force [0|1]>"))
        self.assertFalse(evidence["force"])
        self.assertEqual(command["command"], evidence["shutdown_command"])
        self.assertEqual(command["command"].split(), ["AdminKillServer", "0"])
        self.assertEqual(command["transport"], "source_rcon")
        self.assertEqual(command["port_name"], "rcon")
        self.assertEqual(command["password_setting_key"], "rcon_password")
        self.assertNotIn("fallback_transport", command)
        self.assertGreater(command["wait_after_ms"], evidence["observed_native_shutdown_ms"])
        self.assertGreater(module["runtime"]["shutdown"]["grace_period_ms"], 0)

    def test_launch_requests_full_native_log_without_visible_window(self) -> None:
        process = tomllib.loads((MODULE / "module.toml").read_text(encoding="utf-8"))["process"]
        self.assertEqual(process["args_template"].count("-log"), 1)
        self.assertEqual(process["args_template"].count("-stdout"), 1)
        self.assertEqual(process["args_template"].count("-FullStdOutLogOutput"), 1)
        self.assertEqual(process["window_policy"], "background")
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertNotIn("-newconsole", process["args_template"])
        for path in (MODULE / "config-fixtures").glob("*.json"):
            launch = json.loads(path.read_text(encoding="utf-8-sig")).get("expected", {}).get("launch")
            if launch is not None:
                self.assertEqual(launch["arguments"].count("-log"), 1, path.name)
                self.assertEqual(launch["arguments"].count("-stdout"), 1, path.name)
                self.assertEqual(launch["arguments"].count("-FullStdOutLogOutput"), 1, path.name)


if __name__ == "__main__":
    unittest.main()
