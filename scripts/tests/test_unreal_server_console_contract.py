from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class UnrealServerConsoleContractTests(unittest.TestCase):
    def test_documented_log_console_is_requested_on_the_private_desktop(self) -> None:
        # Soulmask StartServer.bat, Windrose StartServerForeground.bat, and the
        # Jagex and Nightingale dedicated-server guides explicitly use -log.
        # SCUM needs Epic's documented -log after its real GUI entry had no console.
        # GUI subsystem alone does not establish runtime console ownership
        # or graceful-stop success.
        for module_id in ("soulmask", "windrose", "runescapedragonwilds", "nightingale", "scum"):
            with self.subTest(module=module_id):
                root = ROOT / "modules" / module_id
                process = tomllib.loads((root / "module.toml").read_text(encoding="utf-8"))["process"]
                self.assertEqual(process["args_template"].count("-log"), 1)
                self.assertEqual(process["window_policy"], "background")
                self.assertEqual(process["host_surface"], "managed_terminal")
                for path in (root / "config-fixtures").glob("*.json"):
                    fixture = json.loads(path.read_text(encoding="utf-8-sig"))
                    launch = fixture.get("expected", {}).get("launch")
                    if launch is not None:
                        self.assertIn("-log", launch["arguments"], path.name)

    def test_modules_without_native_gui_commands_do_not_request_new_console(self) -> None:
        for module_id in ("soulmask", "runescapedragonwilds", "nightingale", "scum"):
            with self.subTest(module=module_id):
                root = ROOT / "modules" / module_id
                process = tomllib.loads((root / "module.toml").read_text(encoding="utf-8"))["process"]
                self.assertNotIn("-newconsole", [arg.lower() for arg in process["args_template"]])

    def test_windrose_uses_native_gui_quit_with_managed_standard_output(self) -> None:
        root = ROOT / "modules" / "windrose"
        manifest = tomllib.loads((root / "module.toml").read_text(encoding="utf-8"))
        arguments = [arg.lower() for arg in manifest["process"]["args_template"]]
        # The verified native quit channel requires FConsoleWindow; it keeps
        # shutdown and backup work on the game thread (native_console_quit_20261001).
        for argument in ("-newconsole", "-stdout", "-fullstdoutlogoutput"):
            self.assertEqual(arguments.count(argument), 1)
        commands = manifest["runtime"]["shutdown"]["commands"]
        self.assertEqual(
            [(command["transport"], command["command"]) for command in commands],
            [("unreal_console", "quit")],
        )
        self.assertNotIn("fallback_transport", commands[0])
        for path in (root / "config-fixtures").glob("*.json"):
            fixture = json.loads(path.read_text(encoding="utf-8-sig"))
            launch = fixture.get("expected", {}).get("launch")
            if launch is not None:
                fixture_arguments = [arg.lower() for arg in launch["arguments"]]
                for argument in ("-newconsole", "-stdout", "-fullstdoutlogoutput"):
                    self.assertEqual(fixture_arguments.count(argument), 1, path.name)


if __name__ == "__main__":
    unittest.main()
