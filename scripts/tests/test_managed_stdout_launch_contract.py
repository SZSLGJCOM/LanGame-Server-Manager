from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SATISFACTORY_ENTRY = "Engine/Binaries/Win64/FactoryServer-Win64-Shipping-Cmd.exe"


class ManagedStdoutLaunchContractTests(unittest.TestCase):
    def test_rust_routes_unity_log_to_the_owned_standard_output(self) -> None:
        module = ROOT / "modules" / "rust"
        process = tomllib.loads((module / "module.toml").read_text(encoding="utf-8"))["process"]
        arguments = process["args_template"]
        self.assertEqual(arguments[arguments.index("-logfile") + 1], "-")
        self.assertEqual(process["host_surface"], "managed_terminal")
        for path in (module / "config-fixtures").glob("*.json"):
            launch = json.loads(path.read_text(encoding="utf-8-sig")).get("expected", {}).get("launch")
            if launch is not None:
                args = launch["arguments"]
                self.assertEqual(args[args.index("-logfile") + 1], "-", path.name)

    def test_satisfactory_preserves_bootstrap_project_and_direct_console_entry(self) -> None:
        module = ROOT / "modules" / "satisfactory"
        process = tomllib.loads((module / "module.toml").read_text(encoding="utf-8"))["process"]
        # Build 24656085 bootstrap resources 201/202 select this executable and
        # project. Direct invocation preserves the explicit inherited handles.
        self.assertEqual(process["executable"], SATISFACTORY_ENTRY)
        self.assertEqual(process["args_template"][:4], ["FactoryGame", "-unattended", "-stdout", "-FullStdOutLogOutput"])
        self.assertEqual(process["working_directory_template"], "{{paths.install_root}}")
        self.assertEqual(process["window_policy"], "background")
        self.assertEqual(process["environment_template"]["USERPROFILE"], "{{paths.data_dir}}/profile")
        for path in (module / "config-fixtures").glob("*.json"):
            launch = json.loads(path.read_text(encoding="utf-8-sig")).get("expected", {}).get("launch")
            if launch is not None:
                self.assertEqual(launch["executable_suffix"], SATISFACTORY_ENTRY, path.name)
                self.assertEqual(launch["arguments"][0], "FactoryGame", path.name)


if __name__ == "__main__":
    unittest.main()
