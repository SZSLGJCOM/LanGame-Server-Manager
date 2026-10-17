import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts import verify_scope as scope
from scripts.verify_project import Runner, main
from scripts.verify_runtime_reliability import QUICK_STORAGE_FILTERS, stages


class PublicationScopeTests(unittest.TestCase):
    def test_publication_implementations_and_shared_rule_helpers_recheck_history(self):
        for path in (
            "scripts/verify_no_tracked_secrets.py", "scripts/verify_open_source_boundary.py",
            "scripts/verify_open_source_history.py", "scripts/export_public_snapshot.py",
            "scripts/publication_source_io.py", "scripts/source_literal_scan.py",
            "scripts/open_source_product_policy.py", "scripts/project_license_policy.py",
            "scripts/third_party_asset_policy.py",
        ):
            with self.subTest(path=path):
                self.assertTrue(scope.publication_full([path]))

    def test_root_and_nested_git_rules_recheck_publication_even_when_deleted(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(scope, "ROOT", Path(directory)):
            for path in (".gitignore", ".gitattributes", "apps/desktop/.gitignore",
                         "modules/example/.gitattributes"):
                with self.subTest(path=path):
                    self.assertFalse((scope.ROOT / path).exists())
                    self.assertTrue(scope.publication_full([path]))
            self.assertTrue(scope.publication_full(["scripts/verify_no_tracked_secrets.py"]))

    def test_other_complete_layers_and_scanned_content_do_not_recheck_history(self):
        paths = ["apps/desktop/src-tauri/src/commands.rs", "crates/app-storage/src/lib.rs",
                 "apps/desktop/src/App.tsx", "Cargo.lock", "apps/desktop/package-lock.json",
                 "scripts/verify_project.py", "scripts/verify_scope.py",
                 "docs/development.md", "LICENSE", "modules/palworld/schema.json"]
        self.assertTrue(scope.needs_full(paths))
        self.assertFalse(scope.publication_full(paths))
        self.assertFalse(scope.publication_full([]))

    def test_snapshot_entrypoints_keep_the_excluded_lifecycle_reference_check(self):
        for path in (".github/workflows/ci.yml", ".github/workflows/sync-gitcode-release.yml",
                     "CONTRIBUTING.md", "Cargo.toml", "README.md", "README.en.md",
                     "apps/desktop/package.json", "apps/desktop/src-tauri/Cargo.toml"):
            with self.subTest(path=path):
                self.assertTrue(scope.publication_full([path]))

    def test_script_tests_and_similarly_named_files_stay_in_daily_checks(self):
        paths = ["scripts/tests/test_verify_scope.py", "scripts/tests/test_verify_no_tracked_secrets.py",
                 "scripts/tests/test_export_public_snapshot.py", "scripts/generate_rust_third_party_licenses.py",
                 "docs/.gitignore.example", "scripts/tests/fixtures/.gitattributes.txt",
                 "docs/verify_open_source_history.py"]
        self.assertFalse(scope.publication_full(paths))
        self.assertTrue(scope.publication_full([*paths, "scripts/source_literal_scan.py"]))


class RustScopeTests(unittest.TestCase):
    def test_desktop_changes_run_its_complete_tests_without_unrelated_dependency_suites(self):
        self.assertEqual(scope.rust_selection(["apps/desktop/src-tauri/src/commands.rs"]), ["langame-desktop"])
        selected = stages(full_workspace=True, include_web=False, packages=["langame-desktop"])
        core, storage, desktop = selected
        self.assertIn("app-core", core.command)
        self.assertTrue(set(QUICK_STORAGE_FILTERS) <= set(storage.command))
        self.assertNotIn("--workspace", desktop.command)
        self.assertEqual(desktop.command[2], "langame-desktop")
        self.assertIn("--test-threads=1", desktop.command)
        self.assertTrue(any("runtime_reconciliation" in name for name in desktop.required))
        self.assertFalse(any("owned_process_tree" in name for name in desktop.required))
        self.assertFalse(desktop.metrics_required)

    def test_storage_production_changes_keep_its_real_consumers_and_process_evidence(self):
        packages = scope.rust_selection(["crates/app-storage/src/lib.rs"])
        self.assertEqual(packages, ["app-platform-win", "app-runtime", "app-steamcmd", "app-storage", "langame-desktop"])
        core, storage, workspace = stages(full_workspace=True, include_web=False, packages=packages)
        self.assertIn("--test-threads=2", storage.command)
        self.assertNotIn("--lib", storage.command)
        self.assertNotIn("backups", storage.command)
        self.assertTrue(workspace.metrics_required)
        self.assertTrue(any("owned_process_tree" in name for name in workspace.required))
        self.assertTrue(any("runtime_reconciliation" in name for name in workspace.required))
        self.assertNotIn("app-storage", workspace.command)
        self.assertNotIn("app-core", workspace.command)

    def test_source_declared_test_modules_do_not_select_production_browser_or_native_checks(self):
        paths = ["apps/desktop/src-tauri/src/commands_tests.rs",
                 "apps/desktop/src-tauri/src/commands_managed_save_tests.rs",
                 "apps/desktop/src-tauri/src/commands_assistant_ops/task_file_probe_tests.rs"]
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            desktop = root / "apps/desktop"
            (desktop / "src-tauri").mkdir(parents=True)
            (root / "Cargo.toml").write_text('[workspace]\nmembers=["apps/desktop/src-tauri"]\n', encoding="utf-8")
            (desktop / "src-tauri/Cargo.toml").write_text('[package]\nname="langame-desktop"\n', encoding="utf-8")
            (desktop / "tests").mkdir()
            (desktop / "tests/instance-create-browser.test.cjs").write_text("", encoding="utf-8")
            for path in paths:
                source = root / path
                source.parent.mkdir(parents=True, exist_ok=True)
                source.write_text("#![cfg(test)]\n", encoding="utf-8")
            production = "apps/desktop/src-tauri/src/commands.rs"
            (root / production).write_text("pub fn command() {}\n", encoding="utf-8")
            with patch.object(scope, "ROOT", root), patch.object(scope, "DESKTOP", desktop):
                for path in paths:
                    with self.subTest(path=path):
                        self.assertTrue(scope.rust_test_only(path))
                        self.assertEqual(scope.full_scopes([path]), (False, True, False))
                        self.assertEqual(scope.rust_selection([path]), ["langame-desktop"])
                # Mixed changes retain the production source's stronger scope.
                self.assertEqual(scope.full_scopes([*paths, production]), (True, True, True))

    def test_test_filename_alone_does_not_reduce_coverage_and_deleted_sources_are_conservative(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(scope, "ROOT", Path(temporary)):
            source = Path(temporary) / "production_tests.rs"
            source.write_text('const TEXT: &str = "#![cfg(test)]";', encoding="utf-8")
            self.assertFalse(scope.rust_test_only(source.name))
            source.unlink()
            self.assertFalse(scope.rust_test_only(source.name))

    def test_dependencies_are_read_from_manifests_including_target_and_workspace_dependencies(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(scope, "ROOT", Path(temporary)):
            root = Path(temporary)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers=["crates/*"]\n[workspace.dependencies]\nshared={path="crates/base"}\n',
                encoding="utf-8")
            manifests = {
                "base": '[package]\nname="base"\n',
                "middle": '[package]\nname="middle"\n[dependencies]\nshared={workspace=true}\n',
                "top": '[package]\nname="top"\n[target.\'cfg(windows)\'.dev-dependencies]\nrenamed={package="middle",path="../middle"}\n',
                "unrelated": '[package]\nname="unrelated"\n',
            }
            for name, manifest in manifests.items():
                directory = root / "crates" / name
                directory.mkdir(parents=True)
                (directory / "Cargo.toml").write_text(manifest, encoding="utf-8")
            self.assertEqual(scope.rust_selection(["crates/base/src/lib.rs"]), ["base", "middle", "top"])
            test_source = root / "crates/base/tests.rs"
            test_source.write_text("#![cfg(test)]\n", encoding="utf-8")
            self.assertEqual(scope.rust_selection(["crates/base/tests.rs"]), ["base"])

    def test_shared_inputs_and_unknown_ownership_select_all_packages(self):
        for path in ("Cargo.lock", "scripts/verify_scope.py", "crates/new-crate/src/lib.rs",
                     "modules/palworld/schema.json", "migrations/0001.sql", "apps/desktop/src/api.ts"):
            with self.subTest(path=path):
                self.assertEqual(scope.rust_selection([path]), sorted(scope.rust_workspace()))

    def test_quick_package_production_changes_still_verify_their_consumers(self):
        for package in ("app-knowledge", "app-modules"):
            with self.subTest(package=package):
                packages = scope.rust_selection([f"crates/{package}/src/lib.rs"])
                self.assertIn("langame-desktop", packages)
                self.assertEqual(scope.full_scopes([f"crates/{package}/src/lib.rs"]), (False, True, False))

    def test_package_selection_rejects_duplicate_unknown_and_non_string_arguments(self):
        for value in ('["--workspace"]', '["langame-desktop", "langame-desktop"]', '[3]', '{}', 'null', 'invalid'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                scope.parse_rust_packages(value)
        self.assertEqual(scope.parse_rust_packages('[]'), [])

    def test_full_tier_cannot_be_reduced_by_explicit_package_selection(self):
        with self.assertRaises(SystemExit), contextlib.redirect_stderr(io.StringIO()):
            main(["--component", "rust-tests", "--tier", "full", "--rust-packages-json", '["langame-desktop"]'])

    def test_schedule_and_manual_full_select_every_package(self):
        for arguments in (["--event", "schedule"], ["--event", "workflow_dispatch", "--tier", "full"]):
            with self.subTest(arguments=arguments), tempfile.TemporaryDirectory() as directory, \
                    contextlib.redirect_stdout(io.StringIO()):
                output = Path(directory) / "scope"
                self.assertEqual(main(["--component", "scope", "--github-output", str(output), *arguments]), 0)
                values = dict(line.split("=", 1) for line in output.read_text().splitlines())
                self.assertEqual(json.loads(values["rust_packages"]), sorted(scope.rust_workspace()))

    def test_local_rust_base_uses_the_same_selection_as_ci(self):
        import subprocess
        changed = "apps/desktop/src-tauri/src/commands_tests.rs"
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch("scripts.verify_scope.subprocess.run", return_value=subprocess.CompletedProcess([], 0, changed + "\0", "")), \
                patch.object(Runner, "python") as python:
            self.assertEqual(main(["--component", "rust-tests", "--base", "base", "--output", directory]), 0)
            arguments = python.call_args.args
            index = arguments.index("--rust-packages-json")
            self.assertEqual(json.loads(arguments[index + 1]), scope.rust_selection([changed]))

    def test_unresolvable_local_rust_base_retains_every_package(self):
        import subprocess
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch("scripts.verify_scope.subprocess.run", return_value=subprocess.CompletedProcess([], 128, "", "unknown")), \
                patch.object(Runner, "python") as python:
            self.assertEqual(main(["--component", "rust-tests", "--base", "unknown", "--output", directory]), 0)
            arguments = python.call_args.args
            self.assertEqual(json.loads(arguments[arguments.index("--rust-packages-json") + 1]), sorted(scope.rust_workspace()))


if __name__ == "__main__":
    unittest.main()
