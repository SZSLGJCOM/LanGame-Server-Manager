import json
import os
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
    ExcludedPath,
    SnapshotError,
    SnapshotPlan,
    _decode_git_paths,
    _validated_archive_member_path,
    _validated_export_paths,
    build_head_snapshot_plan,
    build_snapshot_plan,
    capture_head_commit,
    exclusion_reason,
    ensure_clean_worktree,
    scan_snapshot_files,
    scan_snapshot_entrypoint_references,
    validate_output_path,
    validate_public_snapshot_plan,
    write_snapshot_from_head,
)


class PublicSnapshotTests(unittest.TestCase):
    def test_snapshot_requires_the_locked_react_three_fiber_license(self):
        repository = Path(__file__).resolve().parents[2]
        lock = json.loads(
            (repository / "apps/desktop/package-lock.json").read_text(encoding="utf-8")
        )
        version = lock["packages"]["node_modules/@react-three/fiber"]["version"]
        license_path = Path(
            "apps/desktop/third-party-license-sources/npm"
        ) / f"react-three-fiber-{version}-LICENSE"
        self.assertIn(license_path.as_posix(), PUBLIC_SNAPSHOT_REQUIRED_PATHS)
        self.assertTrue((repository / license_path).is_file())
        self.assertIsNone(exclusion_reason(license_path, frozenset()))

    @staticmethod
    def _git_without_replace_environment() -> dict[str, str]:
        environment = os.environ.copy()
        environment["GIT_NO_REPLACE_OBJECTS"] = "1"
        return environment

    @staticmethod
    def _commit_index(repository: Path, message: str = "fixture") -> None:
        subprocess.run(
            [
                "git",
                "-c",
                "user.name=Snapshot Test",
                "-c",
                "user.email=snapshot-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                message,
            ],
            cwd=repository,
            check=True,
        )

    @staticmethod
    def _commit(repository: Path, *paths: str) -> None:
        subprocess.run(["git", "add", *paths], cwd=repository, check=True)
        PublicSnapshotTests._commit_index(repository)

    def test_excludes_ignored_sensitive_and_unlicensed_asset_paths(self):
        ignored_paths = frozenset({"docs/private-audit.md"})

        self.assertEqual(
            exclusion_reason(Path("docs/private-audit.md"), ignored_paths),
            "git-ignored",
        )
        self.assertEqual(
            exclusion_reason(Path("apps/desktop/public/logo.png"), frozenset()),
            "unlicensed-asset-or-binary",
        )
        self.assertIsNone(
            exclusion_reason(
                Path("apps/desktop/src-tauri/icons/icon.png"), frozenset()
            )
        )
        self.assertEqual(
            exclusion_reason(Path("deployment/.env.production"), frozenset()),
            "environment-file",
        )
        self.assertEqual(
            exclusion_reason(Path("keys/server.pem"), frozenset()),
            "sensitive-file-type",
        )
        self.assertEqual(
            exclusion_reason(Path("keys/operator.ppk"), frozenset()),
            "sensitive-file-type",
        )
        for sidecar in (
            "runtime/app.db-wal",
            "runtime/app.db-shm",
            "runtime/app.db-journal",
            "runtime/app.sqlite-wal",
            "runtime/app.sqlite-shm",
            "runtime/app.sqlite-journal",
            "runtime/app.sqlite3-wal",
            "runtime/app.sqlite3-shm",
            "runtime/app.sqlite3-journal",
        ):
            with self.subTest(sidecar=sidecar):
                self.assertEqual(
                    exclusion_reason(Path(sidecar), frozenset()),
                    "sensitive-file-type",
                )
        self.assertEqual(
            exclusion_reason(Path("home/.aws/credentials"), frozenset()),
            "credential-file",
        )
        self.assertEqual(
            exclusion_reason(Path("oauth/client_secret_local.json"), frozenset()),
            "credential-file",
        )
        self.assertIsNone(
            exclusion_reason(Path("crates/app-core/src/lib.rs"), frozenset())
        )

    def test_excludes_local_assistant_metadata_at_every_depth(self):
        for name in (
            ".codex/config.toml",
            "apps/desktop/.CODEX/config.toml",
            ".agents/skills/local/SKILL.md",
            "scripts/.AGENTS/state.json",
            ".claude/settings.local.json",
            "apps/desktop/.CLAUDE/settings.json",
            "CLAUDE.md",
            "docs/claude.MD",
            "AGENTS.override.md",
            "apps/desktop/agents.OVERRIDE.MD",
            "scripts/AGENTS.md",
        ):
            with self.subTest(path=name):
                self.assertEqual(
                    exclusion_reason(Path(name), frozenset()),
                    "local-development-metadata",
                )
        self.assertEqual(exclusion_reason(Path("AGENTS.md"), frozenset()), "internal-lifecycle")
        self.assertIsNone(exclusion_reason(Path("docs/agent-development.md"), frozenset()))

    def test_snapshot_preserves_licensed_fonts_and_license_bytes(self):
        font_paths = tuple(Path(path) for path in THIRD_PARTY_ASSET_LICENSES)
        license_path = Path("apps/desktop/public/fonts/inter/OFL.txt")
        notice_path = Path("THIRD_PARTY_ASSETS/NOTICE")
        files = {
            **{path: (Path(__file__).resolve().parents[2] / path).read_bytes()
               for path in (*font_paths, license_path)},
            notice_path: b"Inter provenance fixture\n",
        }
        for path in files:
            self.assertIn(path.as_posix(), PUBLIC_SNAPSHOT_REQUIRED_PATHS)
            self.assertIsNone(exclusion_reason(path, frozenset()))
        unapproved = Path("apps/desktop/src/assets/fonts/Other.woff2")
        self.assertEqual(
            exclusion_reason(unapproved, frozenset()), "unlicensed-asset-or-binary"
        )
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory) / "repository"
            repository.mkdir()
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            (repository / ".gitattributes").write_bytes(
                (Path(__file__).resolve().parents[2] / ".gitattributes").read_bytes()
            )
            for path, payload in files.items():
                (repository / path).parent.mkdir(parents=True, exist_ok=True)
                (repository / path).write_bytes(payload)
            (repository / unapproved).write_bytes(b"unlicensed-font-fixture")
            self._commit(repository, ".gitattributes", *[path.as_posix() for path in files], unapproved.as_posix())
            source_commit = capture_head_commit(repository)
            plan = build_head_snapshot_plan(repository, source_commit)
            self.assertTrue(set(files) <= set(plan.included))
            self.assertNotIn(unapproved, plan.included)
            output = Path(directory) / "snapshot"
            write_snapshot_from_head(repository, output, source_commit, plan.included)
            for path, payload in files.items():
                self.assertEqual((output / path).read_bytes(), payload)

    def test_excludes_editor_recovery_files_without_git_ignore_rules(self):
        for suffix in ("~", ".orig", ".rej", ".swp", ".swo", ".tmp"):
            with self.subTest(suffix=suffix):
                self.assertEqual(
                    exclusion_reason(Path(f"src/settings.rs{suffix.upper()}"), frozenset()),
                    "editor-or-temporary-file",
                )

    def test_excludes_managed_workstation_entry_points_and_their_tests(self):
        for path in (
            "start.bat",
            "scripts/start_langame_desktop_local.ps1",
            "apps/desktop/tests/managed-desktop-launcher.test.cjs",
        ):
            with self.subTest(path=path):
                self.assertEqual(exclusion_reason(Path(path), frozenset()), "internal-lifecycle")

    def test_excludes_internal_lifecycle_and_third_party_source_material(self):
        for path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS:
            with self.subTest(path=path):
                self.assertEqual(
                    exclusion_reason(Path(path), frozenset()),
                    "internal-lifecycle",
                )
        for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES:
            with self.subTest(prefix=prefix):
                self.assertEqual(
                    exclusion_reason(Path(f"{prefix}fixture.rs"), frozenset()),
                    "internal-lifecycle",
                )
        for path in PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS:
            with self.subTest(path=path):
                self.assertEqual(
                    exclusion_reason(Path(path), frozenset()),
                    "third-party-source-material",
                )

        self.assertEqual(
            exclusion_reason(
                Path("apps/desktop/src-tauri/tests/real_valheim_smoke.rs"),
                frozenset(),
            ),
            "internal-lifecycle",
        )
        self.assertEqual(
            exclusion_reason(
                Path("apps/desktop/src-tauri/tests/support/real_smoke.rs"),
                frozenset(),
            ),
            "internal-lifecycle",
        )
    def test_public_snapshot_plan_requires_public_files_and_private_exclusions(self):
        included = tuple(Path(path) for path in PUBLIC_SNAPSHOT_REQUIRED_PATHS)
        excluded = tuple(
            ExcludedPath(Path(path), "internal-lifecycle")
            for path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS
        ) + tuple(
            ExcludedPath(Path(path), "third-party-source-material")
            for path in PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS
        )

        validate_public_snapshot_plan(SnapshotPlan(included, excluded))

        with self.assertRaisesRegex(SnapshotError, "missing required path"):
            validate_public_snapshot_plan(SnapshotPlan(included[1:], excluded))
        leaked_path = Path(next(iter(PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS)))
        with self.assertRaisesRegex(SnapshotError, "includes private publication path"):
            validate_public_snapshot_plan(
                SnapshotPlan(
                    included + (leaked_path,),
                    excluded,
                )
            )
        leaked_prefix_path = Path(
            f"{next(iter(PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES))}fixture.rs"
        )
        with self.assertRaisesRegex(SnapshotError, "includes private publication path"):
            validate_public_snapshot_plan(
                SnapshotPlan(
                    included + (leaked_prefix_path,),
                    excluded,
                )
            )

    def test_public_snapshot_plan_rejects_missing_legal_documents(self):
        legal_documents = {"CONTRIBUTOR_AGREEMENT.md", "PRIVACY.md"}
        included = tuple(
            Path(path) for path in PUBLIC_SNAPSHOT_REQUIRED_PATHS | legal_documents
        )
        excluded = tuple(
            ExcludedPath(Path(path), "internal-lifecycle")
            for path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS
        ) + tuple(
            ExcludedPath(Path(path), "third-party-source-material")
            for path in PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS
        )
        validate_public_snapshot_plan(SnapshotPlan(included, excluded))
        for document in sorted(legal_documents):
            with self.subTest(document=document):
                without_document = tuple(path for path in included if path != Path(document))
                with self.assertRaisesRegex(SnapshotError, "missing required path.*" + document.replace(".", r"\.")):
                    validate_public_snapshot_plan(SnapshotPlan(without_document, excluded))

    def test_rejects_public_entrypoint_reference_to_excluded_lifecycle_content(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            readme = repository / "README.md"
            readme.write_text(
                "Run powershell -File .\\scripts\\smoke.ps1 status.\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(
                SnapshotError,
                "public entrypoint references excluded internal lifecycle content",
            ):
                scan_snapshot_entrypoint_references(repository, (Path("README.md"),))

            readme.write_text("Run the standard verification commands.\n", encoding="utf-8")
            scan_snapshot_entrypoint_references(repository, (Path("README.md"),))

    def test_head_plan_is_complete_without_internal_or_raw_source_files(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            fixture_paths = (
                PUBLIC_SNAPSHOT_REQUIRED_PATHS
                | PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS
                | PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS
                | {
                    f"{prefix}fixture.rs"
                    for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
                }
            )
            for path_name in fixture_paths:
                path = repository / path_name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"fixture for {path_name}\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=repository, check=True)
            self._commit_index(repository)

            plan = build_head_snapshot_plan(repository, capture_head_commit(repository))
            validate_public_snapshot_plan(plan)

            included = {path.as_posix() for path in plan.included}
            excluded = {item.path.as_posix(): item.reason for item in plan.excluded}
            self.assertTrue(PUBLIC_SNAPSHOT_REQUIRED_PATHS <= included)
            self.assertTrue(
                PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS <= excluded.keys()
            )
            self.assertTrue(
                PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS <= excluded.keys()
            )
            for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES:
                self.assertEqual(excluded[f"{prefix}fixture.rs"], "internal-lifecycle")

    def test_head_plan_applies_gitignore_to_a_forced_tracked_file(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            (repository / ".gitignore").write_text("private.txt\n", encoding="utf-8")
            (repository / "private.txt").write_text("internal\n", encoding="utf-8")
            (repository / "safe.rs").write_text("fn main() {}\n", encoding="utf-8")
            first_party_asset = Path("apps/desktop/src-tauri/icons/icon.png")
            (repository / first_party_asset).parent.mkdir(parents=True)
            (repository / first_party_asset).write_bytes(b"project-owned-icon")
            unlicensed_asset = Path("apps/desktop/public/cover.png")
            (repository / unlicensed_asset).parent.mkdir(parents=True)
            (repository / unlicensed_asset).write_bytes(b"unknown-cover")
            subprocess.run(
                [
                    "git",
                    "add",
                    ".gitignore",
                    "safe.rs",
                    first_party_asset.as_posix(),
                    unlicensed_asset.as_posix(),
                ],
                cwd=repository,
                check=True,
            )
            subprocess.run(
                ["git", "add", "--force", "private.txt"], cwd=repository, check=True
            )
            self._commit_index(repository)
            source_commit = capture_head_commit(repository)
            subprocess.run(
                ["git", "update-index", "--assume-unchanged", ".gitignore"],
                cwd=repository,
                check=True,
            )
            (repository / ".gitignore").write_text("", encoding="utf-8")
            ensure_clean_worktree(repository)

            plan = build_head_snapshot_plan(repository, source_commit)

            self.assertIn(Path("safe.rs"), plan.included)
            self.assertIn(first_party_asset, plan.included)
            self.assertIn(first_party_asset.as_posix(), FIRST_PARTY_ASSET_ALLOWLIST)
            self.assertIn(
                (Path("private.txt"), "git-ignored"),
                {(item.path, item.reason) for item in plan.excluded},
            )
            self.assertIn(
                (unlicensed_asset, "unlicensed-asset-or-binary"),
                {(item.path, item.reason) for item in plan.excluded},
            )

    def test_build_plan_never_exports_untracked_files(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            (repository / "tracked.rs").write_text(
                "pub fn ready() -> bool { true }\n", encoding="utf-8"
            )
            (repository / "private-notes.md").write_text(
                "private release notes\n", encoding="utf-8"
            )
            subprocess.run(
                ["git", "add", "tracked.rs"], cwd=repository, check=True
            )

            plan = build_snapshot_plan(repository)

            self.assertIn(Path("tracked.rs"), plan.included)
            self.assertNotIn(Path("private-notes.md"), plan.included)

    def test_execute_preflight_rejects_every_dirty_worktree_state(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            tracked = repository / "tracked.rs"
            tracked.write_text("pub fn ready() {}\n", encoding="utf-8")
            self._commit(repository, "tracked.rs")
            ensure_clean_worktree(repository)

            tracked.write_text("pub fn changed() {}\n", encoding="utf-8")
            with self.assertRaisesRegex(SnapshotError, "clean worktree"):
                ensure_clean_worktree(repository)
            subprocess.run(
                ["git", "restore", "tracked.rs"], cwd=repository, check=True
            )

            (repository / "staged.rs").write_text("pub fn staged() {}\n", encoding="utf-8")
            subprocess.run(
                ["git", "add", "staged.rs"], cwd=repository, check=True
            )
            with self.assertRaisesRegex(SnapshotError, "clean worktree"):
                ensure_clean_worktree(repository)
            subprocess.run(
                ["git", "reset", "--quiet", "HEAD", "staged.rs"],
                cwd=repository,
                check=True,
            )
            (repository / "staged.rs").unlink()

            (repository / "private-notes.md").write_text(
                "untracked\n", encoding="utf-8"
            )
            with self.assertRaisesRegex(SnapshotError, "clean worktree"):
                ensure_clean_worktree(repository)

    def test_secret_scan_blocks_concrete_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            secret_path = repository / "config.txt"
            secret_path.write_text(
                'client_' + 'se' + 'cret="live-' + 'secret-value-123456"\n',
                encoding="utf-8",
            )

            with self.assertRaisesRegex(SnapshotError, "secret scan failed"):
                scan_snapshot_files(repository, (Path("config.txt"),))

    def test_product_policy_blocks_application_account_surfaces(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            migration = repository / "migrations" / "0007_remote_owner.sql"
            migration.parent.mkdir()
            migration.write_text(
                "ALTER TABLE instances ADD COLUMN cloud_account_id TEXT;\n",
                encoding="utf-8",
            )

            with self.assertRaisesRegex(SnapshotError, "product policy failed"):
                scan_snapshot_files(
                    repository,
                    (Path("migrations/0007_remote_owner.sql"),),
                )

    def test_product_policy_blocks_historical_login_asset_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            login_asset = (
                repository
                / "apps"
                / "desktop"
                / "src"
                / "assets"
                / "langame-login-background.png"
            )
            login_asset.parent.mkdir(parents=True)
            login_asset.write_bytes(b"\x00fixture")

            with self.assertRaisesRegex(SnapshotError, "product policy failed"):
                scan_snapshot_files(
                    repository,
                    (Path("apps/desktop/src/assets/langame-login-background.png"),),
                )

    def test_output_must_be_new_and_outside_the_workspace(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()

            accepted = validate_output_path(repository, base / "public-snapshot")
            self.assertEqual(accepted, (base / "public-snapshot").resolve())

            with self.assertRaisesRegex(SnapshotError, "outside"):
                validate_output_path(repository, repository / "public-snapshot")

            existing = base / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(SnapshotError, "must not already exist"):
                validate_output_path(repository, existing)

    def test_export_writes_head_bytes_and_a_source_commit_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            (repository / "src").mkdir()
            (repository / "src" / "lib.rs").write_text(
                "pub fn ready() -> bool { true }\n", encoding="utf-8"
            )
            (repository / "private.txt").write_text("not selected\n", encoding="utf-8")
            self._commit(repository, "src/lib.rs", "private.txt")
            source_commit = capture_head_commit(repository)
            committed_bytes = subprocess.run(
                ["git", "show", f"{source_commit}:src/lib.rs"],
                cwd=repository,
                check=True,
                stdout=subprocess.PIPE,
            ).stdout
            output = base / "public-snapshot"

            write_snapshot_from_head(
                repository,
                output,
                source_commit,
                (Path("src/lib.rs"),),
            )

            self.assertTrue((output / "src" / "lib.rs").is_file())
            self.assertEqual((output / "src" / "lib.rs").read_bytes(), committed_bytes)
            self.assertFalse((output / "private.txt").exists())
            self.assertFalse((output / ".git").exists())
            manifest = json.loads(
                (output / "PUBLIC_SNAPSHOT_MANIFEST.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["schema_version"], 1)
            self.assertEqual(manifest["source_commit"], source_commit)
            self.assertEqual(
                [item["path"] for item in manifest["files"]], ["src/lib.rs"]
            )

    def test_assume_unchanged_worktree_bytes_cannot_change_head_export(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            tracked = repository / "tracked.rs"
            tracked.write_text("pub fn committed() {}\n", encoding="utf-8")
            self._commit(repository, "tracked.rs")
            source_commit = capture_head_commit(repository)
            committed_bytes = subprocess.run(
                ["git", "show", f"{source_commit}:tracked.rs"],
                cwd=repository,
                check=True,
                stdout=subprocess.PIPE,
            ).stdout
            subprocess.run(
                ["git", "update-index", "--assume-unchanged", "tracked.rs"],
                cwd=repository,
                check=True,
            )
            tracked.write_text("pub fn hidden_worktree_change() {}\n", encoding="utf-8")
            ensure_clean_worktree(repository)
            output = base / "public-snapshot"

            write_snapshot_from_head(
                repository,
                output,
                source_commit,
                (Path("tracked.rs"),),
            )

            self.assertEqual((output / "tracked.rs").read_bytes(), committed_bytes)
            self.assertNotEqual((output / "tracked.rs").read_bytes(), tracked.read_bytes())

    def test_git_replace_refs_cannot_change_exported_commit_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            subprocess.run(
                ["git", "config", "core.autocrlf", "false"],
                cwd=repository,
                check=True,
            )
            tracked = repository / "tracked.txt"
            tracked.write_bytes(b"ORIGINAL\n")
            self._commit(repository, "tracked.txt")
            original_commit = capture_head_commit(repository)
            tracked.write_bytes(b"REPLACEMENT\n")
            self._commit(repository, "tracked.txt")
            replacement_commit = capture_head_commit(repository)
            subprocess.run(
                ["git", "replace", original_commit, replacement_commit],
                cwd=repository,
                check=True,
            )
            ensure_clean_worktree(repository)
            replaced_bytes = subprocess.run(
                ["git", "show", f"{original_commit}:tracked.txt"],
                cwd=repository,
                check=True,
                stdout=subprocess.PIPE,
            ).stdout
            original_bytes = subprocess.run(
                ["git", "show", f"{original_commit}:tracked.txt"],
                cwd=repository,
                env=self._git_without_replace_environment(),
                check=True,
                stdout=subprocess.PIPE,
            ).stdout
            self.assertEqual(replaced_bytes, b"REPLACEMENT\n")
            self.assertEqual(original_bytes, b"ORIGINAL\n")
            output = base / "public-snapshot"

            write_snapshot_from_head(
                repository,
                output,
                original_commit,
                (Path("tracked.txt"),),
            )

            self.assertEqual((output / "tracked.txt").read_bytes(), original_bytes)
            manifest = json.loads(
                (output / "PUBLIC_SNAPSHOT_MANIFEST.json").read_text(encoding="utf-8")
            )
            self.assertEqual(manifest["source_commit"], original_commit)

    def test_source_manifest_name_is_reserved_and_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            source_manifest = repository / "PUBLIC_SNAPSHOT_MANIFEST.json"
            source_manifest.write_text('{"source":"repository"}\n', encoding="utf-8")
            self._commit(repository, source_manifest.name)
            source_commit = capture_head_commit(repository)

            with self.assertRaisesRegex(SnapshotError, "reserved snapshot path"):
                build_head_snapshot_plan(repository, source_commit)
            with self.assertRaisesRegex(SnapshotError, "reserved snapshot path"):
                write_snapshot_from_head(
                    repository,
                    base / "public-snapshot",
                    source_commit,
                    tuple(),
                )
            self.assertFalse((base / "public-snapshot").exists())

    def test_secret_scan_failure_removes_the_temporary_staging_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            secret_path = repository / "config.txt"
            secret_path.write_text(
                "sec" + "ret=CleanupSecretValue123\n",
                encoding="utf-8",
            )
            self._commit(repository, secret_path.name)
            source_commit = capture_head_commit(repository)

            with self.assertRaisesRegex(SnapshotError, "secret scan failed"):
                write_snapshot_from_head(
                    repository,
                    base / "public-snapshot",
                    source_commit,
                    (Path("config.txt"),),
                )

            self.assertEqual(list(base.iterdir()), [repository])

    def test_head_plan_excludes_symlinks_and_gitlinks_and_writer_rejects_them(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(
                ["git", "init", "--quiet"], cwd=repository, check=True
            )
            (repository / "target.txt").write_text("target\n", encoding="utf-8")
            self._commit(repository, "target.txt")
            initial_commit = capture_head_commit(repository)
            link_blob = subprocess.run(
                ["git", "hash-object", "-w", "--stdin"],
                cwd=repository,
                input=b"target.txt",
                check=True,
                stdout=subprocess.PIPE,
            ).stdout.decode("ascii").strip()
            subprocess.run(
                [
                    "git",
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    f"120000,{link_blob},link.txt",
                ],
                cwd=repository,
                check=True,
            )
            subprocess.run(
                [
                    "git",
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    f"160000,{initial_commit},nested-repository",
                ],
                cwd=repository,
                check=True,
            )
            self._commit_index(repository, "tree entry fixtures")
            source_commit = capture_head_commit(repository)

            plan = build_head_snapshot_plan(repository, source_commit)

            excluded = {(item.path, item.reason) for item in plan.excluded}
            self.assertIn((Path("link.txt"), "symbolic-link"), excluded)
            self.assertIn((Path("nested-repository"), "non-regular-file"), excluded)
            with self.assertRaisesRegex(SnapshotError, "not a regular file"):
                write_snapshot_from_head(
                    repository,
                    base / "symlink-output",
                    source_commit,
                    (Path("link.txt"),),
                )
            with self.assertRaisesRegex(
                SnapshotError, "not a regular file|missing selected path"
            ):
                write_snapshot_from_head(
                    repository,
                    base / "gitlink-output",
                    source_commit,
                    (Path("nested-repository"),),
                )
            self.assertFalse((base / "symlink-output").exists())
            self.assertFalse((base / "gitlink-output").exists())

    def test_export_rejects_a_path_that_escapes_the_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            (base / "private.txt").write_text("private\n", encoding="utf-8")

            with self.assertRaisesRegex(SnapshotError, "unsafe export path"):
                write_snapshot_from_head(
                    repository,
                    base / "public-snapshot",
                    "0" * 40,
                    (Path("../private.txt"),),
                )

    def test_git_and_archive_path_decoders_reject_unsafe_paths(self):
        for payload in (
            b"../private.txt\0",
            b"C:private.txt\0",
            b"/private.txt\0",
            b"trailing-dot.\0",
            b"CON.txt\0",
            b"CONIN$.txt\0",
            "COM¹.log\0".encode("utf-8"),
            "LPT²\0".encode("utf-8"),
            b"NUL .json\0",
            b"directory/stream:name\0",
        ):
            with self.subTest(payload=payload):
                with self.assertRaisesRegex(SnapshotError, "unsafe repository path"):
                    _decode_git_paths(payload)
        with self.assertRaisesRegex(SnapshotError, "collide on Windows"):
            _decode_git_paths(b"case.txt\0CASE.txt\0")
        with self.assertRaisesRegex(SnapshotError, "collide on Windows"):
            _validated_export_paths((Path("case.txt"), Path("CASE.txt")))
        for member_name in (
            "../private.txt",
            "C:private.txt",
            "/private.txt",
            "a\\b",
            "trailing-space ",
            "AUX.json",
            "CONOUT$.txt",
            "COM³.config",
            "LPT¹",
            "PRN .txt",
            "directory/stream:name",
        ):
            with self.subTest(member_name=member_name):
                with self.assertRaisesRegex(SnapshotError, "unsafe git archive path"):
                    _validated_archive_member_path(member_name)


if __name__ == "__main__":
    unittest.main()
