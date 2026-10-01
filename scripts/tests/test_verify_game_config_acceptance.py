from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "verify_game_config_acceptance.py"
SPEC = importlib.util.spec_from_file_location("verify_game_config_acceptance", SCRIPT)
assert SPEC and SPEC.loader
acceptance = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(acceptance)


class AcceptanceRepository:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.modules = root / "modules"
        self.docs = root / "docs" / "game-config-acceptance"

    def add_module(self, module_id: str = "alpha") -> Path:
        module = self.modules / module_id
        module.mkdir(parents=True)
        (module / "module.toml").write_text(f'id = "{module_id}"\n', encoding="utf-8")
        (module / "schema.json").write_text(
            json.dumps({"properties": {"server_name": {"type": "string"}}}),
            encoding="utf-8",
        )
        (module / "config-sources.toml").write_text(
            """status = "best_effort_verified"
last_verified = "2026-08-11"
notes = "test evidence"

[[sources]]
id = "official"
kind = "documentation"
path = "https://example.invalid/config"
authority = "test"
description = "controlled fixture source"

[[items]]
source = "official"
key = "ServerName"
schema_key = "server_name"
surface = "config_file"
""",
            encoding="utf-8",
        )
        return module

    def fixture(self, module_id: str = "alpha", **changes: object) -> Path:
        payload: dict[str, object] = {
            "fixture_version": 1,
            "module_id": module_id,
            "evidence": {
                "status": "best_effort_verified",
                "verified_at": "2026-08-11",
                "source_ids": ["official"],
                "build": "controlled-build",
            },
            "coverage": {
                "editable": 1,
                "specialized": 0,
                "derived": 0,
                "generated": 0,
                "excluded": 0,
            },
            "classifications": {
                "editable": ["official.ServerName"],
                "specialized": [],
                "derived": [],
                "generated": [],
                "excluded": [],
            },
            "settings": {"server_name": "LanGame 配置服"},
            "expected": {
                "files": [
                    {
                        "root": "config",
                        "path": "server.properties",
                        "format": "properties",
                        "keys": {"ServerName": "LanGame 配置服"},
                    }
                ],
                "launch": {"executable_suffix": None, "arguments": []},
            },
        }
        payload.update(changes)
        path = self.modules / module_id / "config-fixtures" / "2026-08-11-official.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(payload, ensure_ascii=False), encoding="utf-8")
        return path


class VerifyGameConfigAcceptanceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = AcceptanceRepository(Path(self.temp.name))
        self.repo.add_module()

    def verify(self, *, mode: str = "check") -> list[str]:
        return acceptance.verify_repository(self.repo.root, ["alpha"], False, mode)

    def test_rejects_malformed_fixture_version(self) -> None:
        for version in (2, True, "1"):
            with self.subTest(version=version):
                self.repo.fixture(fixture_version=version)
                self.assert_contains(self.verify(), "fixture_version must be 1")

    def test_rejects_unknown_selected_fixture_module_and_source(self) -> None:
        self.assert_contains(
            acceptance.verify_repository(self.repo.root, ["missing"], False, "check"),
            "unknown selected module 'missing'",
        )
        fixture_path = self.repo.fixture()
        payload = json.loads(fixture_path.read_text(encoding="utf-8"))
        payload["module_id"] = "ghost"
        fixture_path.write_text(json.dumps(payload), encoding="utf-8")
        self.assert_contains(self.verify(), "fixture module_id must equal 'alpha'")
        self.repo.fixture(evidence={
            "status": "best_effort_verified",
            "verified_at": "2026-08-11",
            "source_ids": ["unknown"],
            "build": "x",
        })
        self.assert_contains(self.verify(), "unknown evidence source_id 'unknown'")

    def test_rejects_manifest_id_that_does_not_match_module_directory(self) -> None:
        self.repo.fixture()
        (self.repo.modules / "alpha" / "module.toml").write_text('id = "other"\n', encoding="utf-8")
        self.assert_contains(self.verify(), "module.toml id must equal 'alpha'")

    def test_rejects_duplicate_classification_and_count_mismatch(self) -> None:
        self.repo.fixture(classifications={
            "editable": ["official.ServerName"],
            "specialized": ["official.ServerName"],
            "derived": [],
            "generated": [],
            "excluded": [],
        })
        self.assert_contains(self.verify(), "classified more than once")
        self.repo.fixture(coverage={
            "editable": 2,
            "specialized": 0,
            "derived": 0,
            "generated": 0,
            "excluded": 0,
        })
        self.assert_contains(self.verify(), "coverage.editable is 2 but 1 keys are classified")

    def test_rejects_unknown_classified_key_and_unclassified_ledger_key(self) -> None:
        self.repo.fixture(classifications={
            "editable": ["official.Unknown"],
            "specialized": [],
            "derived": [],
            "generated": [],
            "excluded": [],
        })
        failures = self.verify()
        self.assert_contains(failures, "unknown classified key 'official.Unknown'")
        self.assert_contains(failures, "official.ServerName is not classified")

    def test_rejects_missing_and_ambiguous_expected_outputs(self) -> None:
        self.repo.fixture(expected={"files": []})
        self.assert_contains(self.verify(), "expected.launch must be an object")
        self.repo.fixture(expected={"launch": {"executable_suffix": None, "arguments": []}})
        self.assert_contains(self.verify(), "expected.files must be a list")
        self.repo.fixture(expected={
            "files": [{
                "root": "config",
                "path": "server.properties",
                "format": "properties",
                "keys": {},
                "fragments": [],
            }],
            "launch": {"executable_suffix": None, "arguments": []},
        })
        self.assert_contains(self.verify(), "must declare exactly one of keys, entries, or fragments")

    def test_requires_explicit_launch_suffix_like_the_native_runner(self) -> None:
        self.repo.fixture(expected={
            "files": [{
                "root": "config", "path": "server.json", "format": "json", "keys": {"name": "kept"},
            }],
            "launch": {"arguments": []},
        })
        self.assert_contains(self.verify(), "expected.launch.executable_suffix must be a string or null")

    def test_rejects_fixture_without_a_concrete_native_output_assertion(self) -> None:
        self.repo.fixture(expected={
            "files": [],
            "launch": {"executable_suffix": None, "arguments": []},
        })
        self.assert_contains(self.verify(), "must assert at least one native output")
        self.repo.fixture(expected={
            "files": [{
                "root": "config",
                "path": "server.properties",
                "format": "properties",
                "keys": {},
            }],
            "launch": {"executable_suffix": None, "arguments": []},
        })
        self.assert_contains(self.verify(), "assertion must not be empty")

    def test_accepts_ordered_repeated_native_entries(self) -> None:
        self.repo.fixture(expected={
            "files": [{
                "root": "config",
                "path": "Game.ini",
                "format": "ini",
                "entries": [
                    {"key": "Server.Mod", "value": "first"},
                    {"key": "Server.Mod", "value": "second"},
                ],
            }],
            "launch": {"executable_suffix": None, "arguments": []},
        })
        self.assertEqual(self.verify(mode="write"), [])

    def test_rejects_invalid_output_root_and_empty_ordered_entries(self) -> None:
        self.repo.fixture(expected={
            "files": [{
                "root": "somewhere",
                "path": "Game.ini",
                "format": "ini",
                "entries": [],
            }],
            "launch": {"executable_suffix": None, "arguments": []},
        })
        failures = self.verify()
        self.assert_contains(failures, ".root must be one of")
        self.assert_contains(failures, "assertion must not be empty")

    def test_rejects_empty_text_fragments(self) -> None:
        for fragment in ("", "   "):
            with self.subTest(fragment=fragment):
                self.repo.fixture(expected={
                    "files": [{
                        "root": "config",
                        "path": "motd.txt",
                        "format": "text",
                        "fragments": [fragment],
                    }],
                    "launch": {"executable_suffix": None, "arguments": []},
                })
                self.assert_contains(self.verify(), ".fragments must be a non-empty string list")

    def test_module_fixtures_must_cover_every_ledger_native_key(self) -> None:
        ledger = self.repo.modules / "alpha" / "config-sources.toml"
        ledger.write_text(
            ledger.read_text(encoding="utf-8")
            + """

[[sources]]
id = "second"
kind = "config_file"
path = "second.ini"
authority = "test"
description = "second native source"

[[items]]
source = "second"
key = "Difficulty"
schema_key = "server_name"
surface = "config_file"
""",
            encoding="utf-8",
        )
        self.repo.fixture()
        self.assert_contains(self.verify(), "second.Difficulty is not covered by any fixture")

    def test_rejects_unsafe_fixture_paths(self) -> None:
        unsafe = [
            "../secret",
            "nested/../secret",
            "/absolute/file",
            "C:/drive/file",
            "C:\\drive\\file",
            "nested\\escape.ini",
            "server.properties:secret",
            "bad<name.ini",
            "CON",
            "nested/aux.txt",
            "CONIN$",
            "CONOUT$",
            "./nested/server.ini",
            "nested/./server.ini",
            "nested//server.ini",
            "trailing./file.ini",
            "trailing /file.ini",
        ]
        for value in unsafe:
            with self.subTest(value=value):
                self.repo.fixture(expected={
                    "files": [{"root": "config", "path": value, "format": "text", "fragments": []}],
                    "launch": {"executable_suffix": None, "arguments": []},
                })
                self.assert_contains(self.verify(), "unsafe relative path")

    def test_rejects_invalid_initial_native_files(self) -> None:
        cases = [
            ({"files": []}, "initial.files must be a non-empty list"),
            ({"files": [{"root": "other", "path": "native.json", "content": {}}]},
             ".root must be one of"),
            ({"files": [{"root": "install", "path": "../escape.json", "content": {}}]},
             "unsafe relative path"),
            ({"files": [
                {"root": "install", "path": "native.json", "content": {}},
                {"root": "install", "path": "native.json", "content": {}},
            ]}, "duplicates initial path"),
            ({"files": [
                {"root": "config", "path": "native.json", "content": {}},
                {"root": "instance", "path": "config/native.json", "content": {}},
            ]}, "duplicates initial path"),
            ({"files": [{"root": "install", "path": "native.json", "content": 42}]},
             ".content must be text, an object, or an array"),
        ]
        for initial, message in cases:
            with self.subTest(message=message):
                self.repo.fixture(initial=initial)
                self.assert_contains(self.verify(), message)

    def test_initial_native_files_are_recorded_and_checked_for_drift(self) -> None:
        fixture = self.repo.fixture(
            initial={"files": [{
                "root": "install",
                "path": "native.json",
                "content": {"UnknownNative": "preserve"},
            }]},
            lifecycle={"save_stage": "stage only", "pre_start": "apply while stopped"},
        )
        self.assertEqual(self.verify(mode="write"), [])
        record = self.repo.docs / "alpha.md"
        self.assertIn("### Initial native files", record.read_text(encoding="utf-8"))
        self.assertIn("### Lifecycle", record.read_text(encoding="utf-8"))
        payload = json.loads(fixture.read_text(encoding="utf-8"))
        payload["initial"]["files"][0]["content"]["UnknownNative"] = "changed"
        fixture.write_text(json.dumps(payload), encoding="utf-8")
        self.assert_contains(self.verify(), "stale generated record")

    def test_rejects_incomplete_lifecycle_contract(self) -> None:
        self.repo.fixture(lifecycle={"save_stage": "stage only"})
        self.assert_contains(self.verify(), "exactly save_stage and pre_start")

    def test_rejects_invalid_dates_and_unversioned_fixture_names(self) -> None:
        fixture_path = self.repo.fixture(evidence={
            "status": "best_effort_verified",
            "verified_at": "2026-99-99",
            "source_ids": ["official"],
            "build": "controlled-build",
        })
        self.assert_contains(self.verify(), "evidence.verified_at must be a real YYYY-MM-DD date")

        self.repo.fixture()
        fixture_path.rename(fixture_path.with_name("not-versioned.json"))
        self.assert_contains(self.verify(), "fixture filename must be <verified-date>-<source-id>.json")

    def test_check_reports_stale_record_without_mutating_it(self) -> None:
        self.repo.fixture()
        self.repo.docs.mkdir(parents=True)
        record = self.repo.docs / "alpha.md"
        record.write_bytes(b"stale\r\n")
        before = record.read_bytes()
        self.assert_contains(self.verify(), "stale generated record")
        self.assertEqual(record.read_bytes(), before)

    def test_check_reports_missing_record_without_creating_directories(self) -> None:
        self.repo.fixture()
        self.assert_contains(self.verify(), "missing generated record")
        self.assertFalse(self.repo.docs.exists())

    def test_write_is_utf8_lf_and_byte_deterministic(self) -> None:
        self.repo.fixture()
        self.assertEqual(self.verify(mode="write"), [])
        record = self.repo.docs / "alpha.md"
        first = record.read_bytes()
        self.assertIn("LanGame 配置服".encode(), first)
        self.assertNotIn(b"\r\n", first)
        self.assertEqual(self.verify(mode="write"), [])
        self.assertEqual(record.read_bytes(), first)
        self.assertEqual(self.verify(mode="check"), [])

    def test_write_does_not_partially_update_records_when_validation_fails(self) -> None:
        self.repo.fixture()
        self.repo.add_module("broken")
        self.repo.fixture("broken", fixture_version=99)
        self.repo.docs.mkdir(parents=True)
        record = self.repo.docs / "alpha.md"
        record.write_bytes(b"preserve-me\n")
        failures = acceptance.verify_repository(self.repo.root, ["alpha", "broken"], False, "write")
        self.assert_contains(failures, "fixture_version must be 1")
        self.assertEqual(record.read_bytes(), b"preserve-me\n")

    def test_empty_selection_is_clean_and_does_not_require_records(self) -> None:
        self.assertEqual(acceptance.verify_repository(self.repo.root, [], False, "check"), [])
        self.assertFalse(self.repo.docs.exists())

    def test_cli_treats_omitted_empty_modules_value_as_empty_selection(self) -> None:
        with mock.patch.object(acceptance, "verify_repository", return_value=[]) as verify:
            self.assertEqual(acceptance.main(["--modules", "--check"]), 0)
        verify.assert_called_once_with(acceptance.ROOT, [], False, "check")

    def test_cli_preserves_targeted_modules(self) -> None:
        with mock.patch.object(acceptance, "verify_repository", return_value=[]) as verify:
            self.assertEqual(acceptance.main(["--modules", "minecraft,rust", "--check"]), 0)
        verify.assert_called_once_with(acceptance.ROOT, ["minecraft", "rust"], False, "check")

    def test_cli_without_modules_selects_every_existing_fixture_cohort(self) -> None:
        with (
            mock.patch.object(
                acceptance,
                "discover_fixture_module_ids",
                return_value=["minecraft", "rust"],
            ),
            mock.patch.object(acceptance, "verify_repository", return_value=[]) as verify,
        ):
            self.assertEqual(acceptance.main(["--check"]), 0)
        verify.assert_called_once_with(acceptance.ROOT, ["minecraft", "rust"], False, "check")

    def test_targeted_selection_rejects_duplicates(self) -> None:
        failures = acceptance.verify_repository(
            self.repo.root,
            ["alpha", "alpha"],
            False,
            "check",
        )
        self.assert_contains(failures, "duplicate selected module 'alpha'")

    def test_require_all_demands_the_canonical_module_set(self) -> None:
        failures = acceptance.verify_repository(self.repo.root, [], True, "check")
        self.assert_contains(failures, "--require-all requires exactly the canonical 32 modules")

    def test_require_all_rejects_duplicate_module_selection(self) -> None:
        selected = [*acceptance.CANONICAL_MODULE_IDS, acceptance.CANONICAL_MODULE_IDS[0]]
        with mock.patch.object(
            acceptance, "discover_module_ids", return_value=set(acceptance.CANONICAL_MODULE_IDS)
        ):
            failures = acceptance.verify_repository(self.repo.root, selected, True, "check")
        self.assertEqual(failures, ["--require-all requires exactly the canonical 32 modules"])

    def test_require_all_rejects_explicit_empty_module_selection(self) -> None:
        with mock.patch.object(
            acceptance, "discover_module_ids", return_value=set(acceptance.CANONICAL_MODULE_IDS)
        ):
            failures = acceptance.verify_repository(self.repo.root, [], True, "check")
        self.assertEqual(failures, ["--require-all requires exactly the canonical 32 modules"])

    def test_require_all_rejects_orphan_generated_records(self) -> None:
        self.repo.fixture()
        self.assertEqual(self.verify(mode="write"), [])
        (self.repo.docs / "ghost.md").write_text("orphan\n", encoding="utf-8")
        with mock.patch.object(acceptance, "CANONICAL_MODULE_IDS", ("alpha",)):
            failures = acceptance.verify_repository(self.repo.root, ["alpha"], True, "check")
        self.assert_contains(failures, "generated record IDs must exactly match the canonical modules")

    def assert_contains(self, failures: list[str], text: str) -> None:
        self.assertTrue(any(text in failure for failure in failures), failures)


if __name__ == "__main__":
    unittest.main()
