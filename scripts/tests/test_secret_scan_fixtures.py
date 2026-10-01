"""Synthetic credentials must never exempt a whole test file from scanning."""

import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from scripts.verify_no_tracked_secrets import scan_paths, scan_payload


class SecretScanFixtureTests(unittest.TestCase):
    def test_synthetic_assignment_is_limited_to_test_source(self):
        payload = ('let text = "server_pass' + 'word=synthetic-fixture\\n";').encode()
        for name in ("config_documents_tests.rs", "tests.rs", "settings.test.cjs", "test_settings.py"):
            with self.subTest(path=name):
                self.assertEqual(scan_payload(Path(name), payload), [])
        for name in ("settings.rs", "contests.rs", "tests/settings.json", "test_settings.py.txt", "settings.py"):
            with self.subTest(path=name):
                self.assertEqual(
                    [finding.rule for finding in scan_payload(Path(name), payload)],
                    ["assigned-secret"],
                )

    def test_only_exact_url_fixture_pair_is_accepted_in_test_source(self):
        for password in ("password", "pass", "fixture"):
            with self.subTest(password=password):
                self._assert_url_fixture_scope(password)

    def _assert_url_fixture_scope(self, password):
        placeholder = '"https://user:' + password + '@example.com/file"'
        for name in ("tests.rs", "policy_tests.rs", "policy.test.ts", "test_policy.py"):
            with self.subTest(path=name):
                self.assertEqual(scan_payload(Path(name), placeholder.encode()), [])
                for candidate in (
                    placeholder.replace("user:", "operator:"),
                    placeholder.replace(password + "@", password + "-active@"),
                    placeholder + ' "https://user:' + 'AnotherSecret123@example.com/file"',
                ):
                    self.assertEqual(
                        [finding.rule for finding in scan_payload(Path(name), candidate.encode())],
                        ["url-credential"],
                    )
        for name in ("policy.rs", "contests.rs", "settings.json", "tests/settings.txt", "test_policy.py.txt", "policy.py"):
            with self.subTest(path=name):
                self.assertEqual(
                    [finding.rule for finding in scan_payload(Path(name), placeholder.encode())],
                    ["url-credential"],
                )

    def test_short_url_credentials_are_not_ignored(self):
        for scheme in ("https", "postgresql", "mysql", "redis"):
            for password in ("x", "1234", "short12"):
                payload = (scheme + "://operator:" + password + "@example.test/service").encode()
                with self.subTest(scheme=scheme, password=password):
                    self.assertEqual(
                        [item.rule for item in scan_payload(Path("settings.env"), payload)],
                        ["url-credential"],
                    )

    def test_synthetic_fixture_does_not_hide_another_credential_on_the_line(self):
        payload = ('"https://user:' + 'password@example.com/file"; "api_'
                   + 'key=DifferentSecret123456"').encode()
        self.assertEqual(
            [finding.rule for finding in scan_payload(Path("tests.rs"), payload)],
            ["assigned-secret"],
        )

    def test_password_only_database_urls_are_scanned(self):
        for scheme in ("redis", "rediss", "postgresql"):
            payload = (scheme + "://:" + "1234@example.test/database").encode()
            with self.subTest(scheme=scheme):
                self.assertEqual(
                    [item.rule for item in scan_payload(Path("settings.env"), payload)],
                    ["url-credential"],
                )

    def test_native_fixture_digest_requires_its_exact_path_key_and_value(self):
        digest = hashlib.sha256(b"fixture-server-password").hexdigest().upper()
        concrete_value = "AnotherConcreteValue123"
        paths = (
            Path("modules/rimworld/config-fixtures/2026-09-28-rimworld_together_latest_release_api.json"),
            Path("docs/game-config-acceptance/rimworld.md"),
        )
        for path in paths:
            with self.subTest(path=path):
                payload = json.dumps({"Password": digest}).encode()
                self.assertEqual(scan_payload(path, payload), [])
                for rejected_path, rejected_payload in (
                    (Path("private") / path, payload),
                    (path.with_name("other.json"), payload),
                    (path, json.dumps({"Password": "A" * 64}).encode()),
                    (path, json.dumps({"Password": digest, "api_key": concrete_value}).encode()),
                    (path, json.dumps({"api_key": digest}).encode()),
                    (path, json.dumps({"ServerPassword": digest}).encode()),
                    (path, json.dumps({"password": digest}).encode()),
                ):
                    self.assertEqual(
                        [finding.rule for finding in scan_payload(rejected_path, rejected_payload)],
                        ["assigned-secret"],
                    )

    def test_known_hash_example_is_limited_to_its_rust_fixture(self):
        digest = hashlib.sha256(b"abc").hexdigest().upper()
        payload = json.dumps({"Password": digest}).encode()
        self.assertEqual(
            scan_payload(Path("crates/app-storage/src/templates_rimworld_tests.rs"), payload),
            [],
        )
        for path in (
            "crates/app-storage/src/other_tests.rs",
            "crates/app-storage/src/templates_rimworld.rs",
            "docs/game-config-acceptance/rimworld.md",
        ):
            with self.subTest(path=path):
                self.assertEqual(
                    [finding.rule for finding in scan_payload(Path(path), payload)],
                    ["assigned-secret"],
                )

    def test_snapshot_scans_resolve_fixture_scope_against_the_supplied_root(self):
        digest = hashlib.sha256(b"fixture-server-password").hexdigest().upper()
        relative = Path("docs/game-config-acceptance/rimworld.md")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / relative
            path.parent.mkdir(parents=True)
            path.write_text(json.dumps({"Password": digest}), encoding="utf-8")
            self.assertEqual(scan_paths([path], repository_root=root), [])
            findings = scan_paths([path], repository_root=root.parent)
            self.assertEqual([finding.rule for finding in findings], ["assigned-secret"])
            self.assertEqual(findings[0].path, path)

    def test_python_fixture_does_not_hide_adjacent_concrete_assignments(self):
        concrete_value = "AnotherConcreteValue123"
        payload = json.dumps({
            "password": "synthetic-fixture",
            "api_key": concrete_value,
        }).encode()
        self.assertEqual(
            [finding.rule for finding in scan_payload(Path("test_settings.py"), payload)],
            ["assigned-secret"],
        )


if __name__ == "__main__":
    unittest.main()
