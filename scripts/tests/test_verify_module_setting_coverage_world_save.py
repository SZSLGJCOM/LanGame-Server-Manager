from __future__ import annotations

import copy
from pathlib import Path
import re
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from scripts import verify_module_setting_coverage as verifier
from scripts import verify_module_setting_coverage_world_save as world_verifier


class WorldSaveCoverageTests(unittest.TestCase):
    @staticmethod
    def validate(module_id: str, module: dict) -> list[str]:
        return verifier.validate_runtime_player_actions(
            module_id, module, verifier.read_rendered_surfaces(module_id))

    @staticmethod
    def world_actions(module: dict) -> list[dict]:
        return [action for action in module.get("runtime", {}).get("player_actions", [])
                if action.get("id") == "save_world"]

    def module(self, module_id: str) -> dict:
        module = verifier.read_module_toml(module_id)
        module["runtime"]["player_actions"] = self.world_actions(module)[:1]
        return module

    def test_current_world_action_manifests_pass_runtime_validation(self) -> None:
        for module_id in sorted(verifier.TARGET_MODULE_IDS):
            module = verifier.read_module_toml(module_id)
            if not self.world_actions(module):
                continue
            with self.subTest(module_id=module_id):
                self.assertEqual(self.validate(module_id, module), [])

    def test_world_actions_do_not_claim_player_management_or_roster_transport(self) -> None:
        for module_id in ("abioticfactor", "corekeeper", "scum", "sonsoftheforest", "valheim", "vrising"):
            module = verifier.read_module_toml(module_id)
            with self.subTest(module_id=module_id):
                self.assertFalse(verifier.module_has_runtime_player_actions(module))
                self.assertEqual(verifier.runtime_player_action_transports(self.module(module_id)), set())
                self.assertEqual(verifier.validate_player_management_contract(
                    module_id, module, verifier.read_schema_property_defs(module_id)), [])

    def test_native_save_rejects_changed_authority_or_extra_target_metadata(self) -> None:
        changes = (
            ("id", "show_players"), ("id", "execute_anything"),
            ("transport", "unknown_save"), ("transport", "returntomoria_save"),
            ("command_template", "save extra"), ("command_template", "save\nstop"),
            ("command_template", "save {{target}}"), ("destructive", True),
            ("target_required", True), ("target_required", False),
            ("target_encoding", "raw"), ("target_label", "Player ID"),
            ("role_values", ["admin"]), ("process_key", "main"),
            ("port_name", "rcon"), ("password_setting_key", "password"),
            ("enabled_setting_key", "enabled"), ("kind", "broadcast"),
        )
        for key, value in changes:
            module = self.module("corekeeper")
            module["runtime"]["player_actions"][0][key] = value
            with self.subTest(key=key, value=value):
                self.assertTrue(self.validate("corekeeper", module))
        module = self.module("corekeeper")
        module["id"] = "other"
        self.assertTrue(self.validate("corekeeper", module))
        self.assertTrue(self.validate("valheim", self.module("corekeeper")))

    def test_invalid_transport_types_and_unknown_targetless_actions_stay_rejected(self) -> None:
        for value in ("future_unverified_save", ["corekeeper_save"], {"transport": "corekeeper_save"}, 1):
            module = self.module("corekeeper")
            module["runtime"]["player_actions"][0]["transport"] = value
            with self.subTest(transport=value):
                self.assertTrue(self.validate("corekeeper", module))
        module = self.module("corekeeper")
        module["runtime"]["player_actions"][0].update(
            id="arbitrary_mutation", transport="stdin", command_template="anything", process_key="main")
        self.assertTrue(self.validate("corekeeper", module))

    def test_world_scope_does_not_hide_real_or_fabricated_player_actions(self) -> None:
        module = self.module("corekeeper")
        module["runtime"]["player_actions"].append({
            "id": "list_players", "label": "Players", "transport": "stdin",
            "command_template": "list", "process_key": "main",
        })
        self.assertTrue(verifier.module_has_runtime_player_actions(module))
        self.assertEqual(verifier.runtime_player_action_transports(module), {"stdin"})
        self.assertTrue(any("must not hide" in failure for failure in
                            verifier.validate_player_management_contract(
                                "corekeeper", module, verifier.read_schema_property_defs("corekeeper"))))
        module = self.module("corekeeper")
        module["runtime"]["player_actions"][0]["kind"] = "broadcast"
        self.assertTrue(self.validate("corekeeper", module))

    def test_standard_save_contracts_keep_transport_command_and_process_authority(self) -> None:
        for module_id, key, value in (
            ("minecraft", "command_template", "stop"),
            ("rust", "transport", "source_rcon"),
            ("dontstarve", "process_key", "caves"),
            ("unturned", "command_template", "shutdown"),
            ("palworld", "kind", "world_save_preparation"),
        ):
            module = self.module(module_id)
            module["runtime"]["player_actions"][0][key] = value
            with self.subTest(module_id=module_id, key=key):
                self.assertTrue(self.validate(module_id, module))

    def test_astroneer_native_save_requires_its_console_contract(self) -> None:
        for mutation in ("udp", "missing_port", "wrong_password", "missing_schema", "unmaterialized"):
            module = self.module("astroneer")
            properties = copy.deepcopy(verifier.read_schema_property_defs("astroneer"))
            if mutation == "udp":
                next(port for port in module["default_ports"] if port["name"] == "console")["protocol"] = "udp"
            elif mutation == "missing_port":
                module["default_ports"] = []
            elif mutation == "wrong_password":
                module["runtime"]["player_actions"][0]["password_setting_key"] = "other"
            elif mutation == "missing_schema":
                del properties["console_password"]
            with self.subTest(mutation=mutation), verifier_property_patch(properties):
                if mutation == "unmaterialized":
                    from unittest.mock import patch
                    with patch.object(verifier, "referenced_ports", return_value=set()):
                        failures = self.validate("astroneer", module)
                else:
                    failures = self.validate("astroneer", module)
                self.assertTrue(failures)

    def test_native_contracts_stay_equal_to_the_actual_dispatcher(self) -> None:
        source = (verifier.DESKTOP_TAURI_COMMANDS_RS.parent / "commands_world_save.rs").read_text(encoding="utf-8-sig")
        transports = dict(re.findall(r'"([a-z_]+)" => Some\(Self::(\w+)\)', source))
        native = re.findall(r'Self::(\w+) => \("([a-z]+)", "([^"]+)"\)', source)
        self.assertEqual(
            world_verifier.NATIVE_WORLD_SAVE_CONTRACTS,
            {module: (next(transport for transport, variant in transports.items() if variant == enum), command)
             for enum, module, command in native})


def verifier_property_patch(properties: dict):
    from unittest.mock import patch
    return patch.object(verifier, "read_schema_property_defs", return_value=properties)


if __name__ == "__main__":
    unittest.main()
