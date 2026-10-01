from pathlib import Path
import subprocess
import tempfile
import unittest

from scripts.export_public_snapshot import (
    FIRST_PARTY_ASSET_ALLOWLIST,
    PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS,
    PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES,
    PUBLIC_SNAPSHOT_REQUIRED_PATHS,
    PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS,
    THIRD_PARTY_ASSET_LICENSES,
)
from scripts.open_source_product_policy import scan_product_payload
from scripts.tests.project_license_fixtures import write_project_license_fixture
from scripts.verify_open_source_boundary import (
    verify_boundary,
    verify_public_snapshot_contract,
)


MIT_TEXT = """MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
THE SOFTWARE IS PROVIDED "AS IS"
"""
REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


class OpenSourceBoundaryTests(unittest.TestCase):
    def _write_required_files(
        self, repository: Path, *, cargo_license: str | None = None
    ) -> tuple[Path, ...]:
        write_project_license_fixture(repository)
        if cargo_license is not None:
            manifest = repository / "Cargo.toml"
            manifest.write_text(
                manifest.read_text(encoding="utf-8").replace(
                    'license-file = "LICENSE"', f'license = "{cargo_license}"'
                ),
                encoding="utf-8",
            )
        for notice in ("NOTICE", "assets/NOTICE"):
            notice_path = repository / notice
            notice_path.parent.mkdir(parents=True, exist_ok=True)
            notice_path.write_text(
                "Project-owned assets:\n"
                + "\n".join(
                    path for path in sorted(FIRST_PARTY_ASSET_ALLOWLIST)
                    if path.startswith("assets/") == (notice == "assets/NOTICE")
                )
                + "\n",
                encoding="utf-8",
            )
        asset_notice = repository / "THIRD_PARTY_ASSETS" / "NOTICE"
        asset_notice.parent.mkdir()
        third_party_paths = set(THIRD_PARTY_ASSET_LICENSES) | set(THIRD_PARTY_ASSET_LICENSES.values())
        asset_notice.write_text(
            "Approved third-party assets and licenses:\n"
            + "\n".join(sorted(third_party_paths)) + "\n",
            encoding="utf-8",
        )
        first_party_assets = tuple(
            Path(path) for path in sorted(FIRST_PARTY_ASSET_ALLOWLIST)
        )
        for asset_path in first_party_assets:
            (repository / asset_path).parent.mkdir(parents=True, exist_ok=True)
            (repository / asset_path).write_bytes(b"project-owned-build-asset")
        for asset_path in third_party_paths:
            (repository / asset_path).parent.mkdir(parents=True, exist_ok=True)
            (repository / asset_path).write_bytes((REPOSITORY_ROOT / asset_path).read_bytes())
        return (*first_party_assets, *(Path(path) for path in sorted(third_party_paths)))

    def test_third_party_fonts_are_separate_from_first_party_assets(self):
        self.assertTrue(FIRST_PARTY_ASSET_ALLOWLIST.isdisjoint(THIRD_PARTY_ASSET_LICENSES))
        self.assertEqual(
            set(THIRD_PARTY_ASSET_LICENSES),
            {
                "apps/desktop/src/assets/fonts/InterVariable.woff2",
                "apps/desktop/src/assets/fonts/InterVariable-Italic.woff2",
            },
        )

    def test_rejects_missing_first_party_notices(self):
        for notice, other in (("NOTICE", "assets/NOTICE"), ("assets/NOTICE", "NOTICE")):
            with self.subTest(notice=notice), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                tracked_paths = self._write_required_files(repository)
                contents = (repository / notice).read_text(encoding="utf-8")
                with (repository / other).open("a", encoding="utf-8") as destination:
                    destination.write(contents)
                (repository / notice).unlink()
                findings = verify_boundary(repository, tracked_paths, frozenset())
                self.assertEqual(
                    [(item.rule, item.path, item.detail) for item in findings],
                    [("asset-notice", notice, "file is missing")],
                )

    def test_rejects_first_party_declarations_in_other_notice(self):
        for asset in sorted(FIRST_PARTY_ASSET_ALLOWLIST):
            with self.subTest(asset=asset), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                tracked_paths = self._write_required_files(repository)
                notice, other = ("assets/NOTICE", "NOTICE") if asset.startswith("assets/") else ("NOTICE", "assets/NOTICE")
                notice_path = repository / notice
                notice_path.write_text(
                    notice_path.read_text(encoding="utf-8").replace(asset + "\n", ""),
                    encoding="utf-8",
                )
                with (repository / other).open("a", encoding="utf-8") as destination:
                    destination.write(asset + "\n")
                findings = verify_boundary(repository, tracked_paths, frozenset())
                self.assertEqual(
                    [(item.rule, item.path, item.detail) for item in findings],
                    [("asset-notice", notice, f"first-party asset is not declared: {asset}")],
                )

    def test_rejects_missing_or_untracked_font_license(self):
        license_path = Path("apps/desktop/public/fonts/inter/OFL.txt")
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            tracked_paths = self._write_required_files(repository)
            untracked_findings = verify_boundary(
                repository,
                tuple(path for path in tracked_paths if path != license_path),
                frozenset(),
            )
            self.assertEqual(
                [(item.rule, item.path) for item in untracked_findings],
                [("third-party-asset-license", license_path.as_posix())],
            )
            (repository / license_path).unlink()
            missing_findings = verify_boundary(repository, tracked_paths, frozenset())
            self.assertIn(
                ("third-party-asset-license", license_path.as_posix()),
                {(item.rule, item.path) for item in missing_findings},
            )

    def test_rejects_undeclared_third_party_font_attribution(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            tracked_paths = self._write_required_files(repository)
            (repository / "THIRD_PARTY_ASSETS/NOTICE").write_text(
                "Approved entries: none.\n", encoding="utf-8"
            )
            findings = verify_boundary(repository, tracked_paths, frozenset())
        self.assertEqual(len(findings), 3)
        self.assertEqual({item.rule for item in findings}, {"asset-notice"})

    def test_rejects_changed_bytes_under_approved_font_and_license_paths(self):
        paths = set(THIRD_PARTY_ASSET_LICENSES) | set(THIRD_PARTY_ASSET_LICENSES.values())
        for path in sorted(paths):
            with self.subTest(path=path), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                tracked_paths = self._write_required_files(repository)
                original = (repository / path).read_bytes()
                (repository / path).write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
                findings = verify_boundary(repository, tracked_paths, frozenset())
                self.assertEqual(
                    [(item.rule, item.path) for item in findings],
                    [("third-party-asset-integrity", path)],
                )

    def test_rejects_previous_mit_grant_under_source_available_policy(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            tracked_paths = self._write_required_files(repository)
            (repository / "LICENSE").write_bytes(MIT_TEXT.encode("utf-8"))
            (repository / "apps/desktop/LICENSE").write_bytes(MIT_TEXT.encode("utf-8"))
            findings = verify_boundary(repository, tracked_paths, frozenset())
        self.assertIn("root-license", {finding.rule for finding in findings})

    def test_accepts_source_available_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            (repository / "src").mkdir()
            (repository / "src/lib.rs").write_text("pub fn ready() {}\n", encoding="utf-8")

            findings = verify_boundary(
                repository,
                (
                    Path("Cargo.toml"),
                    Path("LICENSE"),
                    Path("src/lib.rs"),
                    *first_party_assets,
                ),
                frozenset(),
            )

            self.assertEqual(findings, [])

    def test_public_snapshot_contract_keeps_required_files_and_excludes_private_paths(
        self,
    ):
        tracked_paths = tuple(
            Path(path)
            for path in (
                PUBLIC_SNAPSHOT_REQUIRED_PATHS
                | PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS
                | PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS
            )
        )
        tracked_paths += tuple(
            Path(f"{prefix}fixture.rs")
            for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
        )

        self.assertEqual(verify_public_snapshot_contract(tracked_paths), [])

        missing_path = next(iter(PUBLIC_SNAPSHOT_REQUIRED_PATHS))
        findings = verify_public_snapshot_contract(
            tuple(path for path in tracked_paths if path.as_posix() != missing_path)
        )
        self.assertEqual(
            [(finding.rule, finding.path) for finding in findings],
            [("public-snapshot-required-path", missing_path)],
        )

    def test_rejects_retired_product_markers_in_every_migration(self):
        payload = b"ALTER TABLE instances ADD COLUMN cloud_" + b"account_id TEXT;"
        for name in ("0001_initial.sql", "0007_instance_cloud_account.sql", "0008_instance_runtime_isolation.sql", "copied/history.sql"):
            with self.subTest(name=name):
                findings = scan_product_payload(Path("migrations") / name, payload)
                self.assertEqual([finding.rule for finding in findings], ["prohibited-product-marker"])
        baseline = Path("migrations/0001_initial.sql")
        self.assertEqual(scan_product_payload(baseline, (REPOSITORY_ROOT / baseline).read_bytes()), [])

    def test_reports_license_media_and_private_path_violations(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(
                repository, cargo_license="Proprietary"
            )
            tracked_paths = (
                Path("apps/desktop/public/game-assets/map.svg"),
                Path("apps/desktop/src/assets/maps/world.png"),
                Path("apps/desktop/src/assets/unapproved.svg"),
                Path("prompt.txt"),
                Path("artifacts/audit/report.md"),
                Path("docs/product-focus-roadmap.md"),
                Path("scripts/local_probe.py"),
                *first_party_assets,
            )
            for relative_path in tracked_paths:
                source_path = repository / relative_path
                source_path.parent.mkdir(parents=True, exist_ok=True)
                if not source_path.exists():
                    source_path.write_text("fixture\n", encoding="utf-8")
            (repository / "docs").mkdir(exist_ok=True)
            (repository / "docs" / "product-focus-roadmap.md").write_text(
                "Internal roadmap\n", encoding="utf-8"
            )
            (repository / "scripts").mkdir(exist_ok=True)
            (repository / "scripts" / "local_probe.py").write_text(
                'ROOT = r"' + "\\".join(("D:", "LanGame", "projects", "LanGame Server Manager")) + '"\n',
                encoding="utf-8",
            )

            findings = verify_boundary(
                repository,
                tracked_paths,
                frozenset({"artifacts/audit/report.md"}),
            )

            rules = {finding.rule for finding in findings}
            self.assertEqual(
                rules,
                {
                    "cargo-license",
                    "developer-machine-path",
                    "tracked-ignored-path",
                    "tracked-media",
                    "tracked-sensitive-path",
                },
            )

    def test_rejects_workstation_paths_in_public_documentation(self):
        roots = (
            ("D:", "LanGame", "scripts", "invoke-codex-cargo.ps1"),
            ("D:", "LanGame", "projects", "Example"),
            ("D:", "LanGameTemp", "cargo-target", "codex"),
            ("C:", "Users", "developer", "source"),
        )
        for parts in roots:
            for separator in ("/", "\\", "\\\\"):
                with self.subTest(parts=parts, separator=separator), tempfile.TemporaryDirectory() as directory:
                    repository = Path(directory)
                    assets = self._write_required_files(repository)
                    path = Path("docs/development.md")
                    (repository / path).parent.mkdir()
                    (repository / path).write_text(
                        "# Development\n\nRun `" + separator.join(parts) + "`.\n",
                        encoding="utf-8",
                    )
                    findings = verify_boundary(repository, (*assets, path), frozenset())
                    self.assertEqual(
                        [(item.rule, item.path) for item in findings],
                        [("developer-machine-path", path.as_posix())],
                    )

    def test_accepts_documented_product_data_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            assets = self._write_required_files(repository)
            path = Path("docs/server-configuration.md")
            (repository / path).parent.mkdir()
            (repository / path).write_text(
                "Default data root: D:/LanGame; instances: D:/LanGame/instances.\n"
                "Custom log example: " + "\\".join(("D:", "ValheimLogs", "server.log")) + "\n",
                encoding="utf-8",
            )
            self.assertEqual(verify_boundary(repository, (*assets, path), frozenset()), [])

    def test_reports_the_legacy_conflicting_license(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            (repository / "LICENSE").unlink()
            conflicting_license = repository / "license" / "license.txt"
            conflicting_license.parent.mkdir()
            conflicting_license.write_text("All rights reserved\n", encoding="utf-8")

            findings = verify_boundary(repository, first_party_assets, frozenset())

            self.assertIn("conflicting-license", {finding.rule for finding in findings})

    def test_reports_tracked_sqlite_sidecar_files(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            sidecar = Path("runtime/app.db-wal")

            findings = verify_boundary(
                repository,
                (sidecar, *first_party_assets),
                frozenset(),
            )

            self.assertIn(
                ("tracked-sensitive-path", sidecar.as_posix()),
                {(finding.rule, finding.path) for finding in findings},
            )

    def test_reports_credential_paths_and_sensitive_runtime_artifacts(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            sensitive_paths = (
                Path("keys/operator.ppk"),
                Path("shell/.envrc"),
                Path("gcloud/application_default_credentials.json"),
                Path("crash/app.wer"),
                Path("archive/state.backup"),
                Path("crash/app.mdmp"),
                Path("crash/app.hdmp"),
                Path("home/.aws/credentials"),
                Path("home/.cargo/credentials"),
                Path("home/.cargo/credentials.toml"),
                Path("home/.kube/config"),
                Path("home/.docker/config.json"),
                Path("oauth/client_secret_local.json"),
                Path("gcp/service-account-production.json"),
                Path("runtime/credentials.json"),
            )

            findings = verify_boundary(
                repository,
                (*sensitive_paths, *first_party_assets),
                frozenset(),
            )

            reported_paths = {
                finding.path
                for finding in findings
                if finding.rule == "tracked-sensitive-path"
            }
            self.assertEqual(
                reported_paths,
                {path.as_posix() for path in sensitive_paths},
            )

    def test_reports_application_account_and_vip_surfaces(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            account_migration = Path("migrations/0007_remote_owner.sql")
            (repository / account_migration).parent.mkdir()
            (repository / account_migration).write_text(
                "ALTER TABLE instances ADD COLUMN cloud_account_id TEXT;\n",
                encoding="utf-8",
            )
            login_screen = Path("apps/desktop/src/auth/AuthLoginScreen.tsx")
            (repository / login_screen).parent.mkdir(parents=True)
            (repository / login_screen).write_text(
                "export const screen = null;\n", encoding="utf-8"
            )
            historical_paths = (
                Path("apps/desktop/src/auth-core.ts"),
                Path("apps/desktop/src-tauri/src/auth_service.rs"),
                Path("apps/desktop/src/assets/langame-login-background.png"),
                Path("apps/desktop/src/assets/privacy.md"),
                Path("apps/desktop/tests/vip-access-control.test.cjs"),
                Path("artifacts/audit/01-wechat-login.png"),
                Path("services/payment-service/main.go"),
            )
            for historical_path in historical_paths:
                (repository / historical_path).parent.mkdir(
                    parents=True, exist_ok=True
                )
                (repository / historical_path).write_bytes(b"\x00fixture")
            marker_paths = (
                Path("apps/desktop/src/product-vip.ts"),
                Path("apps/desktop/src/product-membership.ts"),
                Path("apps/desktop/src/product-payment.ts"),
            )
            marker_payloads = (
                "const tier = 'vip_tier';\n",
                "const active = 'has_membership';\n",
                "const flow = 'recharge_order payment_order wechat_login';\n",
            )
            for marker_path, payload in zip(marker_paths, marker_payloads):
                (repository / marker_path).parent.mkdir(parents=True, exist_ok=True)
                (repository / marker_path).write_text(payload, encoding="utf-8")

            findings = verify_boundary(
                repository,
                (
                    account_migration,
                    login_screen,
                    *historical_paths,
                    *marker_paths,
                    *first_party_assets,
                ),
                frozenset(),
            )

            self.assertEqual(
                {
                    finding.path
                    for finding in findings
                    if finding.rule == "prohibited-product-surface"
                },
                {
                    account_migration.as_posix(),
                    login_screen.as_posix(),
                    *(path.as_posix() for path in historical_paths),
                    *(path.as_posix() for path in marker_paths),
                },
            )

    def test_rejects_local_metadata_and_editor_recovery_files_without_ignore_rules(self):
        private_paths = tuple(Path(name) for name in (
            ".codex/config.toml", "nested/.CODEX/config.toml",
            ".agents/skills/local/SKILL.md", "nested/.AGENTS/state.json",
            ".claude/settings.local.json", "nested/.CLAUDE/settings.json",
            "AGENTS.md", "nested/agents.MD", "CLAUDE.md", "nested/claude.MD",
            "AGENTS.override.md", "nested/agents.OVERRIDE.MD",
            "src/settings.rs.orig", "src/settings.rs.REJ", "src/settings.rs.swo",
        ))
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            for path in private_paths:
                (repository / path).parent.mkdir(parents=True, exist_ok=True)
                (repository / path).write_text("local configuration\n", encoding="utf-8")
            findings = verify_boundary(repository, (*private_paths, *first_party_assets), frozenset())

        self.assertEqual(
            {item.path for item in findings if item.rule == "tracked-sensitive-path"},
            {path.as_posix() for path in private_paths},
        )

    def test_rejects_unapproved_assets_and_binaries_in_any_directory(self):
        asset_paths = tuple(Path(name) for name in (
            "cover.png", "docs/screenshots/capture.PNG", "modules/game/map.svg",
            "downloads/server.zip", "tools/helper.exe", "docs/manual.pdf", "fonts/font.woff2",
            "apps/desktop/src/assets/fonts/Other.woff2",
            "apps/desktop/src/assets/fonts/InterVariable.ttf",
        ))
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            for path in asset_paths:
                (repository / path).parent.mkdir(parents=True, exist_ok=True)
                (repository / path).write_bytes(b"\0asset fixture")
            findings = verify_boundary(repository, (*asset_paths, *first_party_assets), frozenset())

        self.assertEqual(
            {item.path for item in findings if item.rule == "tracked-media"},
            {path.as_posix() for path in asset_paths},
        )

    def test_repository_ignore_rules_cover_local_metadata_and_recovery_files(self):
        paths = (
            ".codex/config.toml", "nested/.codex/config.toml",
            ".agents/skills/local/SKILL.md", "nested/.agents/state.json",
            ".claude/settings.local.json", "nested/.claude/settings.json",
            "AGENTS.md", "nested/AGENTS.md", "AGENTS.override.md",
            "nested/AGENTS.override.md", "CLAUDE.md", "nested/CLAUDE.md",
            "src/settings.rs.orig", "src/settings.rs.rej", "src/settings.rs.swo",
            "nested/.CODEX/config.toml", "nested/.AGENTS/state.json",
            "nested/.CLAUDE/settings.json", "nested/agents.MD",
            "nested/agents.OVERRIDE.MD", "nested/claude.MD",
            "src/settings.rs.ORIG", "src/settings.rs.REJ", "src/settings.rs.SWO",
            "src/settings.rs.SWP", "src/settings.rs.TMP",
        )
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            (repository / ".gitignore").write_bytes((REPOSITORY_ROOT / ".gitignore").read_bytes())
            result = subprocess.run(
                ["git", "-c", "core.ignorecase=false", "check-ignore", "--no-index", "-z", "--stdin"],
                cwd=repository, input="\0".join(paths) + "\0", text=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True,
            )
        self.assertEqual(set(result.stdout.rstrip("\0").split("\0")), set(paths))

    def test_reports_known_private_runtime_artifact_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            first_party_assets = self._write_required_files(repository)
            private_paths = (
                Path("$log"),
                Path("open-langame-lan-client.bat"),
                Path("docs/real-instance-startup-smoke-123.json"),
            )

            findings = verify_boundary(
                repository,
                (*private_paths, *first_party_assets),
                frozenset(),
            )

            self.assertEqual(
                {
                    finding.path
                    for finding in findings
                    if finding.rule == "tracked-sensitive-path"
                },
                {path.as_posix() for path in private_paths},
            )


if __name__ == "__main__":
    unittest.main()
