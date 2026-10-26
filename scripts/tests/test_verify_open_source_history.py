from __future__ import annotations

import io
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts.open_source_product_policy import scan_product_payload
from scripts.verify_open_source_history import (
    HistoricalFileVersion,
    _scan_file_version,
    run_scan,
    scan_reachable_history,
)


class OpenSourceHistoryScannerTests(unittest.TestCase):
    def test_historical_documentation_example_exception_is_bound_to_exact_bytes_and_path(self) -> None:
        path = Path("docs/example.md")
        payload = b"Example: D:" + b"\\LanGameTemp\\example\n"
        blob = lambda content: hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()
        with patch("scripts.verify_open_source_history.HISTORICAL_DOCUMENTATION_PATH_EXAMPLES", {(path.as_posix(), blob(payload)): {1}}):
            self.assertEqual(_scan_file_version(HistoricalFileVersion("fixture", path, blob(payload)), payload, frozenset()), ())
            for changed_path, changed_payload in ((Path("docs/other.md"), payload), (path, payload + b"changed")):
                findings = _scan_file_version(HistoricalFileVersion("fixture", changed_path, blob(changed_payload)), changed_payload, frozenset())
                self.assertIn("developer-machine-path", {item.rule for item in findings})

    def test_later_notice_cannot_hide_an_earlier_reference_map_without_attribution(self) -> None:
        root = Path(__file__).resolve().parents[2]
        image = Path("assets/ark-map-artwork/2026-10-25/The_Center_Topographic_Map.jpg")
        notice = Path("assets/ark-map-artwork/2026-10-25/NOTICE.md")
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            (repository / image).parent.mkdir(parents=True)
            (repository / image).write_bytes((root / image).read_bytes())
            missing_notice_commit = self._commit_all(repository, "reference map without notice")
            (repository / notice).write_bytes((root / notice).read_bytes())
            self._commit_all(repository, "restore rights notice")
            findings = scan_reachable_history(repository).findings
            self.assertEqual(
                [(item.commit, item.path.as_posix(), item.rule) for item in findings],
                [(missing_notice_commit, notice.as_posix(), "historical-reference-media-notice")],
            )

    def test_reference_media_registration_does_not_allow_replaced_or_unlisted_images(self) -> None:
        path = Path("assets/ark-map-artwork/2026-10-25/The_Center_Topographic_Map.jpg")
        payload = b"reviewed-reference-media\0"
        version = HistoricalFileVersion("fixture", path, "fixture")
        with patch("scripts.verify_open_source_history.REFERENCE_MEDIA_SHA256", {path.as_posix(): hashlib.sha256(payload).hexdigest()}):
            self.assertEqual(_scan_file_version(version, payload, frozenset()), ())
            replaced = _scan_file_version(version, payload + b"changed", frozenset())
            self.assertIn("historical-reference-media-integrity", {item.rule for item in replaced})
        unlisted = HistoricalFileVersion("fixture", Path("assets/ark-map-artwork/2026-10-25/Unlisted_Map.jpg"), "fixture")
        self.assertIn("historical-media", {item.rule for item in _scan_file_version(unlisted, payload, frozenset())})

    def test_reviewed_assignment_exception_cannot_hide_changed_or_relocated_content(self) -> None:
        path = Path("tests/console_tests.rs")
        payload = b'let ' + b'password' + b' = "HistoricalSyntheticPassword";\n'

        def blob_id(content: bytes) -> str:
            return hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()

        object_id = blob_id(payload)
        with patch("scripts.verify_open_source_history.SYNTHETIC_FIXTURE_ASSIGNMENTS", {(path.as_posix(), object_id): {1}}):
            self.assertEqual(_scan_file_version(HistoricalFileVersion("fixture", path, object_id), payload, frozenset()), ())
            for candidate_path, candidate_payload in (
                (path, payload + b'let ' + b'password' + b' = "UnreviewedCredentialValue";\n'),
                (Path("src/console.rs"), payload),
            ):
                findings = _scan_file_version(HistoricalFileVersion("fixture", candidate_path, blob_id(candidate_payload)), candidate_payload, frozenset())
                self.assertTrue(any(item.rule == "history-secret:assigned-secret" for item in findings))

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

    def test_removed_synthetic_proxy_rejection_fixture_remains_safe_in_history(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "tests" / "test_proxy_rejection.py"
            source.parent.mkdir()
            value = "http" + "://user:" + "sec" + "ret@127.0.0.1:80"
            source.write_text('rejected_proxy = "' + value + '"\n', encoding="utf-8")
            self._commit_all(repository, "synthetic proxy rejection fixture")
            source.unlink()
            self._commit_all(repository, "remove fixture")

            result = scan_reachable_history(repository)

            self.assertEqual(result.commit_count, 2)
            self.assertEqual(result.findings, ())

    def test_proxy_fixture_rule_cannot_hide_deleted_real_or_production_url_credentials(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            self._initialize_repository(repository)
            source = repository / "tests" / "test_proxy_rejection.py"
            production = repository / "proxy.ini"
            source.parent.mkdir()
            source.write_text('rejected_proxy = "http' + '://user:SyntheticCredentialValue123@127.0.0.1:80"\n',
                              encoding="utf-8")
            production.write_text('proxy=http' + '://user:' + 'sec' + 'ret@127.0.0.1:80\n', encoding="utf-8")
            original_commit = self._commit_all(repository, "URL credentials")
            source.unlink()
            production.unlink()
            self._commit_all(repository, "remove credentials")

            result = scan_reachable_history(repository)

            self.assertEqual(
                {(finding.commit, finding.path.as_posix(), finding.rule) for finding in result.findings},
                {(original_commit, "tests/test_proxy_rejection.py", "history-secret:url-credential"),
                 (original_commit, "proxy.ini", "history-secret:url-credential")},
            )

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
