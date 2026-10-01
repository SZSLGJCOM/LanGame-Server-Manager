import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.project_license_policy import verify_project_license
from scripts.tests.project_license_fixtures import PROJECT_LICENSE_TEXT, write_project_license_fixture


class ProjectLicensePolicyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        write_project_license_fixture(self.root)

    def findings(self):
        return {(item.rule, item.path) for item in verify_project_license(self.root)}

    def test_accepts_consistent_project_license_with_third_party_mit(self):
        self.assertEqual(verify_project_license(self.root), [])

    def test_rejects_legacy_cargo_license_and_wrong_license_file(self):
        for declaration in ('license = "MIT"', 'license-file = "NOTICE"'):
            with self.subTest(declaration=declaration):
                manifest = self.root / "Cargo.toml"
                manifest.write_text(
                    '[workspace]\nmembers = ["crates/fixture"]\n'
                    f'[workspace.package]\n{declaration}\n', encoding="utf-8",
                )
                self.assertIn(("cargo-license", "Cargo.toml"), self.findings())

    def test_rejects_mixed_cargo_license_declarations(self):
        manifest = self.root / "Cargo.toml"
        with manifest.open("a", encoding="utf-8") as stream:
            stream.write('license = "MIT"\n')
        self.assertIn(("cargo-license", "Cargo.toml"), self.findings())

    def test_checks_each_workspace_member_and_rejects_overrides(self):
        for declaration in (
            'license.workspace = true', 'license-file = "LICENSE"',
            'license-file.workspace = false', 'license-file.workspace = 1',
            'license-file.workspace = true\nlicense = "MIT"',
        ):
            with self.subTest(declaration=declaration):
                path = "crates/fixture/Cargo.toml"
                (self.root / path).write_text(f'[package]\n{declaration}\n', encoding="utf-8")
                self.assertEqual(self.findings(), {("cargo-member-license", path)})

    def test_rejects_missing_workspace_member(self):
        path = "crates/fixture/Cargo.toml"
        (self.root / path).unlink()
        self.assertEqual(self.findings(), {("cargo-member-license", path)})

    def test_rejects_external_workspace_member(self):
        manifest = self.root / "Cargo.toml"
        manifest.write_text(
            '[workspace]\nmembers = ["../private"]\n'
            '[workspace.package]\nlicense-file = "LICENSE"\n', encoding="utf-8",
        )
        self.assertEqual(self.findings(), {("cargo-member-license", "Cargo.toml")})

    def test_rejects_each_npm_root_license_drift(self):
        for name in ("package.json", "package-lock.json"):
            with self.subTest(name=name):
                write_project_license_fixture(self.root)
                path = "apps/desktop/" + name
                document = json.loads((self.root / path).read_text(encoding="utf-8"))
                package = document if name == "package.json" else document["packages"][""]
                package["license"] = "MIT"
                (self.root / path).write_text(json.dumps(document), encoding="utf-8")
                self.assertEqual(self.findings(), {("npm-license", path)})

    def test_requires_private_npm_package(self):
        path = "apps/desktop/package.json"
        document = json.loads((self.root / path).read_text(encoding="utf-8"))
        document["private"] = False
        (self.root / path).write_text(json.dumps(document), encoding="utf-8")
        self.assertEqual(self.findings(), {("npm-license", path)})

    def test_rejects_missing_or_changed_npm_license_copy(self):
        path = "apps/desktop/LICENSE"
        (self.root / path).unlink()
        self.assertEqual(self.findings(), {("npm-license-copy", path)})
        (self.root / path).write_bytes(PROJECT_LICENSE_TEXT.replace("\n", "\r\n").encode("utf-8"))
        self.assertEqual(self.findings(), {("npm-license-copy", path)})

    def test_rejects_missing_license_section_and_wrong_title(self):
        for removed in ("4. No independent modified releases", "LanGame Source-Available License 1.0"):
            with self.subTest(removed=removed):
                payload = PROJECT_LICENSE_TEXT.replace(removed, "").encode("utf-8")
                (self.root / "LICENSE").write_bytes(payload)
                (self.root / "apps/desktop/LICENSE").write_bytes(payload)
                self.assertEqual(self.findings(), {("root-license", "LICENSE")})

    def test_invalid_documents_report_findings_without_crashing(self):
        for path, payload, expected in (
            ("Cargo.toml", b"[", "cargo-license"),
            ("Cargo.toml", b"workspace = []", "cargo-license"),
            ("apps/desktop/package.json", b"[]", "npm-license"),
            ("apps/desktop/package.json", b"\xff", "npm-license"),
            ("apps/desktop/package-lock.json", b'{"packages":[]}', "npm-license"),
        ):
            with self.subTest(path=path, payload=payload):
                write_project_license_fixture(self.root)
                (self.root / path).write_bytes(payload)
                self.assertIn((expected, path), self.findings())

    def test_nonregular_license_file_is_not_accepted(self):
        (self.root / "LICENSE").unlink()
        (self.root / "LICENSE").mkdir()
        findings = verify_project_license(self.root)
        self.assertTrue(any(
            item.path == "LICENSE" and item.message == "non-regular-source-file"
            for item in findings
        ))

    def test_license_symlink_guard_is_preserved(self):
        with patch.object(Path, "is_symlink", return_value=True):
            findings = verify_project_license(self.root)
        self.assertTrue(any(
            item.path == "LICENSE" and item.message == "non-regular-source-file"
            for item in findings
        ))

    def test_unreadable_license_is_not_accepted(self):
        read_bytes = Path.read_bytes
        def read_or_deny(path):
            if path == self.root / "LICENSE":
                raise PermissionError("private filesystem detail")
            return read_bytes(path)
        with patch.object(Path, "read_bytes", read_or_deny):
            findings = verify_project_license(self.root)
        self.assertTrue(any(
            item.path == "LICENSE" and item.message == "unreadable-source-file"
            for item in findings
        ))
        self.assertFalse(any("private filesystem detail" in item.message for item in findings))


if __name__ == "__main__":
    unittest.main()
