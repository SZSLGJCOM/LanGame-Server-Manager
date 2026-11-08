from __future__ import annotations

import json
import tomllib
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SATISFACTORY_ENTRY = "Engine/Binaries/Win64/FactoryServer-Win64-Shipping-Cmd.exe"
PALWORLD_ENTRY = "Pal/Binaries/Win64/PalServer-Win64-Shipping-Cmd.exe"


def module_manifest(module_id: str) -> dict:
    return tomllib.loads((ROOT / "modules" / module_id / "module.toml").read_text(encoding="utf-8"))


def config_fixtures(module_id: str) -> list[tuple[str, dict]]:
    return [(path.name, json.loads(path.read_text(encoding="utf-8-sig")))
            for path in (ROOT / "modules" / module_id / "config-fixtures").glob("*.json")]


class ManagedStdoutLaunchContractTests(unittest.TestCase):
    def test_rust_routes_unity_log_to_the_owned_standard_output(self) -> None:
        process = module_manifest("rust")["process"]
        arguments = process["args_template"]
        self.assertEqual(arguments[arguments.index("-logfile") + 1], "-")
        self.assertEqual(process["host_surface"], "managed_terminal")
        for name, fixture in config_fixtures("rust"):
            launch = fixture.get("expected", {}).get("launch")
            if launch is not None:
                args = launch["arguments"]
                self.assertEqual(args[args.index("-logfile") + 1], "-", name)

    def test_satisfactory_preserves_bootstrap_project_and_direct_console_entry(self) -> None:
        process = module_manifest("satisfactory")["process"]
        # Build 24656085 bootstrap resources 201/202 select this executable and
        # project. Direct invocation preserves the explicit inherited handles.
        self.assertEqual(process["executable"], SATISFACTORY_ENTRY)
        self.assertEqual(process["args_template"][:4], ["FactoryGame", "-unattended", "-stdout", "-FullStdOutLogOutput"])
        self.assertEqual(process["working_directory_template"], "{{paths.install_root}}")
        self.assertEqual(process["window_policy"], "background")
        self.assertEqual(process["environment_template"]["USERPROFILE"], "{{paths.data_dir}}/profile")
        for name, fixture in config_fixtures("satisfactory"):
            launch = fixture.get("expected", {}).get("launch")
            if launch is not None:
                self.assertEqual(launch["executable_suffix"], SATISFACTORY_ENTRY, name)
                self.assertEqual(launch["arguments"][0], "FactoryGame", name)

    def test_arkse_managed_launch_includes_the_installed_engine_log_verbosity_switch(self) -> None:
        process = module_manifest("arksurvivalevolved")["process"]
        arguments = process["args_template"]
        # Build 21241282 drops Log (5) without this parsed flag; the default limit is 4.
        self.assertEqual(arguments.count("-stdout"), 1)
        self.assertEqual(arguments.count("-AllowStdOutLogVerbosity"), 1)
        self.assertNotIn("-FullStdOutLogOutput", arguments)
        self.assertIn("-abslog={{arkse.native_log_path}}", arguments)
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")

    def test_arkse_configuration_fixtures_preserve_the_managed_log_arguments(self) -> None:
        fixtures = config_fixtures("arksurvivalevolved")
        self.assertTrue(fixtures)
        for name, fixture in fixtures:
            with self.subTest(fixture=name):
                arguments = fixture["expected"]["launch"]["arguments"]
                self.assertEqual(arguments.count("-stdout"), 1)
                self.assertEqual(arguments.count("-AllowStdOutLogVerbosity"), 1)
                self.assertTrue(any(value.startswith("-abslog=") for value in arguments))

    def test_arkse_native_game_log_launch_requests_the_installed_flush_switch(self) -> None:
        process = module_manifest("arksurvivalevolved")["process"]
        # The installed writer flushes each line with this flag, making live logs readable.
        self.assertEqual(process["args_template"].count("-FORCELOGFLUSH"), 1)
        for name, fixture in config_fixtures("arksurvivalevolved"):
            with self.subTest(fixture=name):
                self.assertEqual(fixture["expected"]["launch"]["arguments"].count("-FORCELOGFLUSH"), 1)

    def test_palworld_managed_launch_enables_the_installed_engine_stdout_device(self) -> None:
        process = module_manifest("palworld")["process"]
        # Build 25247047 creates stdout only with -stdout; the verbosity flag admits enabled categories.
        self.assertEqual(process["executable"], PALWORLD_ENTRY)
        self.assertEqual(process["args_template"].count("-stdout"), 1)
        self.assertEqual(process["args_template"].count("-FullStdOutLogOutput"), 1)
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")
        self.assertIn("-logformat={{settings.log_format}}", process["args_template"])

    def test_palworld_config_fixtures_preserve_the_stdout_device_and_verbosity(self) -> None:
        fixtures = config_fixtures("palworld")
        self.assertTrue(fixtures)
        for name, fixture in fixtures:
            with self.subTest(fixture=name):
                launch = fixture["expected"]["launch"]
                self.assertEqual(launch["executable_suffix"], PALWORLD_ENTRY)
                self.assertEqual(launch["arguments"].count("-stdout"), 1)
                self.assertEqual(launch["arguments"].count("-FullStdOutLogOutput"), 1)
                self.assertTrue(any(value.startswith("-logformat=") for value in launch["arguments"]))

    def test_humanitz_launch_preserves_first_party_log_console_on_private_desktop(self) -> None:
        process = module_manifest("humanitz")["process"]
        # Build 23914958's ReadMe uses -log in both Windows examples.
        self.assertEqual(process["args_template"].count("-log"), 1)
        self.assertNotIn("-newconsole", [arg.lower() for arg in process["args_template"]])
        self.assertEqual(process["host_surface"], "managed_terminal")
        self.assertEqual(process["window_policy"], "background")
        fixture = dict(config_fixtures("humanitz"))["2026-07-13-steamcmd_anonymous_app_2728330.json"]
        self.assertIn("-log", fixture["expected"]["launch"]["arguments"])

    def test_documented_unreal_log_console_is_requested_on_the_private_desktop(self) -> None:
        # Shipped launchers and dedicated-server guides use -log. SCUM also needs it
        # after its GUI entry had no console; the PE subsystem alone is insufficient.
        for module_id in ("soulmask", "windrose", "runescapedragonwilds", "nightingale", "scum"):
            with self.subTest(module=module_id):
                process = module_manifest(module_id)["process"]
                self.assertEqual(process["args_template"].count("-log"), 1)
                self.assertEqual(process["window_policy"], "background")
                self.assertEqual(process["host_surface"], "managed_terminal")
                for name, fixture in config_fixtures(module_id):
                    launch = fixture.get("expected", {}).get("launch")
                    if launch is not None:
                        self.assertIn("-log", launch["arguments"], name)

    def test_modules_without_native_gui_commands_do_not_request_new_console(self) -> None:
        for module_id in ("soulmask", "nightingale", "scum"):
            with self.subTest(module=module_id):
                process = module_manifest(module_id)["process"]
                self.assertNotIn("-newconsole", [arg.lower() for arg in process["args_template"]])

    def test_verified_unreal_quit_profiles_keep_console_and_managed_standard_output(self) -> None:
        for module_id in ("windrose", "runescapedragonwilds"):
            with self.subTest(module=module_id):
                manifest = module_manifest(module_id)
                arguments = [arg.lower() for arg in manifest["process"]["args_template"]]
                # Both game-thread quit channels require FConsoleWindow; Dragonwilds quit does not save.
                for argument in ("-newconsole", "-stdout", "-fullstdoutlogoutput"):
                    self.assertEqual(arguments.count(argument), 1)
                commands = manifest["runtime"]["shutdown"]["commands"]
                self.assertEqual([(command["transport"], command["command"]) for command in commands],
                                 [("unreal_console", "quit")])
                self.assertNotIn("fallback_transport", commands[0])
                for name, fixture in config_fixtures(module_id):
                    launch = fixture.get("expected", {}).get("launch")
                    if launch is not None:
                        fixture_arguments = [arg.lower() for arg in launch["arguments"]]
                        extra_arguments = fixture["settings"].get("extra_launch_args", "").lower().split()
                        for argument in ("-newconsole", "-stdout", "-fullstdoutlogoutput"):
                            self.assertEqual(fixture_arguments.count(argument),
                                             1 + extra_arguments.count(argument), name)


if __name__ == "__main__":
    unittest.main()
