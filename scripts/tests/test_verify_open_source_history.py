from __future__ import annotations

import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.open_source_product_policy import scan_product_payload
from scripts.verify_open_source_history import (
    HistoricalFileVersion,
    _scan_file_version,
    run_scan,
    scan_reachable_history,
)


class OpenSourceHistoryScannerTests(unittest.TestCase):
    def _initialize_repository(self, repository: Path) -> None:
        self._git(repository, "init", "--quiet")
        self._git(repository, "config", "user.name", "History Scanner Test")
        self._git(repository, "config", "user.email", "history-scanner@invalid")

    def _git(self, repository: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", *arguments],
            cwd=repository,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def _commit_all(self, repository: Path, message: str) -> str:
        self._git(repository, "add", "--all")
        self._git(repository, "commit", "--quiet", "--message", message)
        return self._git(repository, "rev-parse", "HEAD").stdout.strip()

    def test_clean_reachable_history_passes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "src" / "lib.rs"
            source.parent.mkdir()
            source.write_text("pub fn ready() -> bool { true }\n", encoding="utf-8")
            self._commit_all(repository, "clean source")

            output = io.StringIO()
            result = scan_reachable_history(repository)
            exit_code = run_scan(repository, output)

            self.assertEqual(result.commit_count, 1)
            self.assertEqual(result.findings, ())
            self.assertEqual(exit_code, 0)
            self.assertIn("open-source history scan passed", output.getvalue())

    def test_detached_head_is_scanned_without_a_branch_reference(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "config.txt"
            source.write_text("safe source\n", encoding="utf-8")
            self._commit_all(repository, "clean source")
            self._git(repository, "checkout", "--detach", "--quiet")
            source.write_text("sec" + "ret=DetachedSecretValue123\n", encoding="utf-8")
            secret_commit = self._commit_all(repository, "detached credential")

            result = scan_reachable_history(repository)

            self.assertEqual(result.commit_count, 2)
            self.assertIn(
                (secret_commit, "history-secret:assigned-secret"),
                {(finding.commit, finding.rule) for finding in result.findings},
            )

    def test_commit_messages_are_scanned_without_disclosing_values(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            (repository / "source.txt").write_text("safe source\n", encoding="utf-8")
            credential_value = "CommitMessageCredential123"
            commit = self._commit_all(repository, "configure\n\napi_" + "key=" + credential_value)
            output = io.StringIO()

            exit_code = run_scan(repository, output)
            result = scan_reachable_history(repository)

            self.assertEqual(exit_code, 1)
            self.assertEqual(
                [(item.commit, item.path.as_posix(), item.line, item.rule) for item in result.findings],
                [(commit, "git-metadata/commit-message.txt", 3, "history-secret:assigned-secret")],
            )
            self.assertNotIn(credential_value, output.getvalue())

    def test_nested_annotated_tag_messages_are_scanned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            (repository / "source.txt").write_text("safe source\n", encoding="utf-8")
            self._commit_all(repository, "safe source")
            self._git(repository, "tag", "-a", "inner", "-m", "api_" + "key=TagMessageCredential123")
            inner = self._git(repository, "rev-parse", "inner").stdout.strip()
            machine_path = "D:" + "\\LanGame\\projects\\private-repository"
            self._git(repository, "tag", "-a", "outer", "inner", "-m", "built at " + machine_path)
            outer = self._git(repository, "rev-parse", "outer").stdout.strip()
            self._git(repository, "tag", "-d", "inner")
            self._git(repository, "tag", "lightweight")

            result = scan_reachable_history(repository)

            self.assertEqual(
                {(item.commit, item.path.as_posix(), item.rule) for item in result.findings},
                {
                    (inner, "git-metadata/tag-message.txt", "history-secret:assigned-secret"),
                    (outer, "git-metadata/tag-message.txt", "developer-machine-path"),
                },
            )

    def test_replacement_blobs_cannot_hide_committed_credentials(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "config.txt"
            source.write_text("sec" + "ret=OriginalSecretValue123\n", encoding="utf-8")
            self._commit_all(repository, "credential")
            original_blob = self._git(repository, "rev-parse", "HEAD:config.txt").stdout.strip()
            source.write_text("safe source\n", encoding="utf-8")
            replacement_blob = self._git(repository, "hash-object", "-w", "config.txt").stdout.strip()
            self._git(repository, "replace", original_blob, replacement_blob)

            result = scan_reachable_history(repository)

            self.assertIn(
                "history-secret:assigned-secret",
                {finding.rule for finding in result.findings},
            )

    def test_shallow_history_cannot_be_reported_as_complete(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            repository = root / "source"
            repository.mkdir()
            self._initialize_repository(repository)
            source = repository / "config.txt"
            source.write_text("sec" + "ret=EarlierSecretValue123\n", encoding="utf-8")
            self._commit_all(repository, "credential")
            source.write_text("safe source\n", encoding="utf-8")
            self._commit_all(repository, "clean source")
            clone = root / "shallow"
            self._git(root, "clone", "--quiet", "--depth=1", repository.as_uri(), str(clone))

            with self.assertRaisesRegex(RuntimeError, "shallow"):
                scan_reachable_history(clone)

    def test_replacement_commits_cannot_hide_original_trees(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "config.txt"
            source.write_text("sec" + "ret=OriginalSecretValue123\n", encoding="utf-8")
            original_commit = self._commit_all(repository, "credential")
            source.write_text("safe source\n", encoding="utf-8")
            self._git(repository, "add", "config.txt")
            safe_tree = self._git(repository, "write-tree").stdout.strip()
            replacement = self._git(repository, "commit-tree", safe_tree, "-m", "safe tree").stdout.strip()
            self._git(repository, "replace", original_commit, replacement)

            result = scan_reachable_history(repository)

            self.assertIn(
                (original_commit, "history-secret:assigned-secret"),
                {(finding.commit, finding.rule) for finding in result.findings},
            )

    def test_history_has_no_identity_based_product_policy_exceptions(self) -> None:
        payload = b"// historical fixture\n" * 57 + b"// cloud_account_id\n"
        for commit in ("0" * 40, "1" * 40):
            for object_id in ("2" * 40, "3" * 40):
                path = Path("crates/app-storage/src/history_tests.rs")
                version = HistoricalFileVersion(commit, path, object_id)
                with self.subTest(commit=commit, object_id=object_id):
                    findings = _scan_file_version(version, payload, frozenset())
                    self.assertEqual([(finding.line, finding.rule) for finding in findings], [(58, "prohibited-product-marker")])
                    worktree_findings = scan_product_payload(path, payload)
                    self.assertEqual([(finding.line, finding.rule) for finding in worktree_findings], [(58, "prohibited-product-marker")])

    def test_deleted_prohibited_product_file_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            prohibited = repository / "services" / "auth-server" / "main.go"
            prohibited.parent.mkdir(parents=True)
            prohibited.write_text("package main\n", encoding="utf-8")
            sensitive_commit = self._commit_all(repository, "add private product")
            prohibited.unlink()
            self._commit_all(repository, "remove private product")

            result = scan_reachable_history(repository)

            self.assertEqual(result.commit_count, 2)
            self.assertIn(
                (
                    sensitive_commit,
                    "services/auth-server/main.go",
                    "prohibited-product-path",
                ),
                {
                    (finding.commit, finding.path.as_posix(), finding.rule)
                    for finding in result.findings
                },
            )

    def test_deleted_private_path_and_developer_path_still_fail(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            report = repository / "artifacts" / "audit" / "report.txt"
            report.parent.mkdir(parents=True)
            machine_path = "D:" + "\\LanGame\\projects\\private-repository"
            report.write_text(f"workspace={machine_path}\n", encoding="utf-8")
            self._commit_all(repository, "add local audit")
            report.unlink()
            self._commit_all(repository, "remove local audit")

            result = scan_reachable_history(repository)
            report_rules = {
                finding.rule
                for finding in result.findings
                if finding.path.as_posix() == "artifacts/audit/report.txt"
            }

            self.assertEqual(
                report_rules,
                {"historical-private-path", "developer-machine-path"},
            )

    def test_deleted_media_file_still_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            image = (
                repository
                / "apps"
                / "desktop"
                / "public"
                / "game-assets"
                / "private-capture.png"
            )
            image.parent.mkdir(parents=True)
            image.write_bytes(b"\x89PNG\r\n\x1a\nprivate-capture")
            sensitive_commit = self._commit_all(repository, "add private capture")
            image.unlink()
            self._commit_all(repository, "remove private capture")

            result = scan_reachable_history(repository)

            self.assertIn(
                (
                    sensitive_commit,
                    "apps/desktop/public/game-assets/private-capture.png",
                    "historical-media",
                ),
                {
                    (finding.commit, finding.path.as_posix(), finding.rule)
                    for finding in result.findings
                },
            )

    def test_deleted_local_metadata_backups_and_unapproved_assets_still_fail(self) -> None:
        expected = {
            "nested/.codex/config.toml": "historical-private-path",
            "nested/.agents/skills/local/SKILL.md": "historical-private-path",
            "nested/.CLAUDE/settings.json": "historical-private-path",
            "nested/CLAUDE.md": "historical-private-path",
            "nested/AGENTS.override.md": "historical-private-path",
            "nested/AGENTS.md": "historical-private-path",
            "src/settings.rs.orig": "historical-private-path",
            "src/settings.rs.rej": "historical-private-path",
            "src/settings.rs.swo": "historical-private-path",
            "docs/screenshots/capture.PNG": "historical-media",
            "tools/server.zip": "historical-media",
        }
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            for name in expected:
                path = repository / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("local fixture\n", encoding="utf-8")
            sensitive_commit = self._commit_all(repository, "add local fixtures")
            for name in expected:
                (repository / name).unlink()
            self._commit_all(repository, "remove local fixtures")
            result = scan_reachable_history(repository)

        self.assertEqual(
            {(item.commit, item.path.as_posix(), item.rule) for item in result.findings},
            {(sensitive_commit, name, rule) for name, rule in expected.items()},
        )

    def test_cli_redacts_secret_content_and_returns_nonzero(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            secret_value = "history-" + "secret-value-123456"
            credential = repository / "config.txt"
            credential.write_text(
                "client_" + "secret=" + secret_value + "\n",
                encoding="utf-8",
            )
            self._commit_all(repository, "add credential")
            credential.unlink()
            self._commit_all(repository, "remove credential")

            scanner = (
                Path(__file__).resolve().parents[1]
                / "verify_open_source_history.py"
            )
            completed = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    str(scanner),
                    "--repository",
                    str(repository),
                ],
                check=False,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )

            self.assertEqual(completed.returncode, 1)
            self.assertIn("history-secret:assigned-secret", completed.stdout)
            self.assertNotIn(secret_value, completed.stdout)
            self.assertNotIn(secret_value, completed.stderr)


if __name__ == "__main__":
    unittest.main()
