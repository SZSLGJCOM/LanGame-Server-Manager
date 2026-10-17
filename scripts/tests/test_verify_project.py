import argparse
import contextlib
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.verify_project import Runner, main, rust_test_matrix, validate_frontend_output
from scripts.verify_scope import (DESKTOP, ROOT, QUICK_BROWSER_FILES, browser_repository_files,
                                  frontend_selection, frontend_tests, full_scopes, needs_full, parse_browser_files)


class VerificationSelectionTests(unittest.TestCase):
    def test_daily_browser_selection_contains_critical_flows_and_related_features(self):
        selected = set(frontend_selection(["apps/desktop/src/views/system/SystemCoreDial.tsx"]))
        self.assertTrue(set(QUICK_BROWSER_FILES) <= selected)
        self.assertIn("apps/desktop/tests/system-resources-browser.test.cjs", selected)
        self.assertIn("apps/desktop/tests/instance-retirement-browser.test.cjs", selected)
        self.assertIn("apps/desktop/tests/app-update-browser.test.cjs", selected)
        self.assertNotIn("apps/desktop/tests/asa-mod-controls-browser.test.cjs", selected)
        self.assertEqual(full_scopes(["apps/desktop/src/views/system/SystemCoreDial.tsx"]), (False, False, False))

    def test_browser_selection_preserves_cross_feature_configuration_consumers(self):
        selected = frontend_selection(["apps/desktop/src/views/settings/modules/dontstarve-shards.ts"])
        # Guided schemas also feed player access, mod editors and field-help navigation.
        self.assertEqual(selected, browser_repository_files())

    def test_workbenches_reused_by_archives_keep_read_only_acceptance(self):
        for component in ("ModWorkbench", "PlayerCenterWorkbench", "RuntimeSurfaceWorkbench"):
            with self.subTest(component=component):
                selected = frontend_selection([f"apps/desktop/src/views/servers/{component}.tsx"])
                self.assertIn("apps/desktop/tests/server-archives-browser.test.cjs", selected)
        selected = frontend_selection(["apps/desktop/src/views/servers/RuntimeSurfaceWorkbench.tsx"])
        for name in ("ark-cluster-maps", "dst-four-shards", "instance-retirement", "desktop-layout"):
            self.assertIn(f"apps/desktop/tests/{name}-browser.test.cjs", selected)
        for component in ("ModWorkbench", "RuntimeSurfaceWorkbench"):
            selected = frontend_selection([f"apps/desktop/src/views/servers/{component}.tsx"])
            self.assertIn("apps/desktop/tests/steam-workshop-account-browser.test.cjs", selected)
        for component in ("BackupTable", "ProgramUpdatePolicyEditor"):
            selected = frontend_selection([f"apps/desktop/src/views/servers/{component}.tsx"])
            self.assertIn("apps/desktop/tests/steam-server-install-account-browser.test.cjs", selected)

    def test_library_changes_keep_full_coverage_for_shared_installation_consumers(self):
        for path in ("apps/desktop/src/views/library/LibraryServerActions.tsx", "apps/desktop/src/views/LibraryView.tsx"):
            with self.subTest(path=path):
                self.assertEqual(frontend_selection([path]), browser_repository_files())

    def test_unknown_or_shared_frontend_changes_fall_back_to_every_browser(self):
        complete = [f"apps/desktop/{Path(path).as_posix()}" for path in frontend_tests(True)]
        for path in ("apps/desktop/src/views/new-feature/Page.tsx", "apps/desktop/src/App.tsx",
                     "apps/desktop/src/api-transport.ts", "apps/desktop/src/app.css",
                     "apps/desktop/index.html", "apps/desktop/public/new-font.woff2",
                     "apps/desktop/new-bundler.config.ts",
                     "apps/desktop/src/components/InlineConfirmAction.tsx",
                     "apps/desktop/tests/helpers/runtime-browser-process.cjs"):
            with self.subTest(path=path):
                self.assertEqual(frontend_selection([path]), complete)

    def test_browser_test_and_private_fixture_select_the_same_feature(self):
        expected = frontend_selection(["apps/desktop/tests/app-update-browser.test.cjs"])
        for suffix in ("html", "tsx"):
            self.assertEqual(frontend_selection([f"apps/desktop/tests/helpers/app-update-browser.{suffix}"]), expected)
        self.assertTrue(set(QUICK_BROWSER_FILES) <= set(expected))
        self.assertIn("apps/desktop/tests/program-update-policy-browser.test.cjs", expected)
        self.assertNotIn("apps/desktop/tests/asa-mod-controls-browser.test.cjs", expected)

    def test_non_browser_changes_do_not_create_a_browser_run(self):
        self.assertEqual(frontend_selection(["README.md", "apps/desktop/tests/runtime-browser-process.test.cjs"]), [])

    def test_risky_boundaries_run_full_before_release(self):
        for path in ("crates/app-storage/src/backups.rs", "crates/app-runtime/src/lib.rs",
                     "crates/app-steamcmd/src/lib.rs", "apps/desktop/src-tauri/src/commands.rs",
                     "modules/palworld/schema.json", "migrations/0001.sql", "Cargo.lock",
                     "apps/desktop/package-lock.json", "scripts/verify_project.py",
                     "apps/desktop/tests/helpers/runtime-browser-process.cjs"):
            with self.subTest(path=path):
                self.assertTrue(needs_full([path]))
        self.assertFalse(needs_full(["README.md", "docs/development.md"]))

    def test_frontend_changes_do_not_force_rust_or_native_regression(self):
        for path in ("apps/desktop/src/styles/shell.css", "apps/desktop/src/desktop-exit-lifecycle.ts",
                     "apps/desktop/src/runtime-log-stream.ts", "apps/desktop/src/storage-management-requests.ts",
                     "apps/desktop/src/hooks/useInstanceArchives.ts", "apps/desktop/tsconfig.json"):
            with self.subTest(path=path):
                self.assertEqual(full_scopes([path]), (True, False, False))
        for path in ("apps/desktop/src/views/servers/BackupTable.tsx",
                     "apps/desktop/tests/runtime-reliability-browser.test.cjs",
                     "apps/desktop/tests/knowledge-settings.test.cjs"):
            with self.subTest(path=path):
                self.assertEqual(full_scopes([path]), (False, False, False))
                self.assertTrue(frontend_selection([path]))

    def test_unit_and_scanner_changes_do_not_select_unrelated_slow_suites(self):
        for path in ("apps/desktop/tests/runtime-browser-process.test.cjs",
                     "apps/desktop/tests/unsupported-games-removal.test.cjs",
                     "scripts/tests/test_verify_game_knowledge.py", ".github/ISSUE_TEMPLATE/bug_report.yml"):
            with self.subTest(path=path):
                self.assertEqual(full_scopes([path]), (False, False, False))
        for path in ("apps/desktop/tests/helpers/runtime-browser-process.cjs",
                     "apps/desktop/tests/fixtures/dontstarve-world-lua.json"):
            with self.subTest(path=path):
                self.assertEqual(full_scopes([path]), (True, False, False))

    def test_rust_internal_changes_only_select_the_relevant_boundary(self):
        for crate in ("app-knowledge", "app-modules"):
            with self.subTest(crate=crate):
                self.assertEqual(full_scopes([f"crates/{crate}/src/lib.rs"]), (False, True, False))
        for crate in ("app-network", "app-steamcmd", "app-new-component"):
            with self.subTest(crate=crate):
                self.assertEqual(full_scopes([f"crates/{crate}/src/lib.rs"]), (False, True, False))
        for crate in ("app-storage", "app-runtime", "app-platform-win"):
            with self.subTest(crate=crate):
                self.assertEqual(full_scopes([f"crates/{crate}/src/lib.rs"]), (False, True, True))
        for path in ("scripts/verify_project.py", "Cargo.lock", "apps/desktop/package-lock.json",
                     ".github/actions/setup-frontend/action.yml", "crates/app-core/src/lib.rs",
                     "apps/desktop/src-tauri/src/commands.rs", "crates/app-knowledge/Cargo.toml",
                     "crates/app-modules/Cargo.toml", "crates/app-new-component/Cargo.toml",
                     "crates/app-knowledge/build.rs", "crates/app-knowledge/runtime_build.rs"):
            with self.subTest(path=path):
                self.assertEqual(full_scopes([path]), (True, True, True))
        self.assertEqual(full_scopes(["apps/desktop/src/api-transport.ts"]), (True, True, False))
        self.assertEqual(full_scopes(["modules/palworld/schema.json"]), (True, True, False))
        self.assertEqual(full_scopes(["apps/desktop/tests/helpers/desktop-reliability.tsx"]), (True, False, True))
        self.assertEqual(full_scopes(["scripts/tests/test_desktop_installer_guard.ps1"]), (False, False, True))

    def test_browser_partition_is_complete_disjoint_and_preserves_runner_unit_tests(self):
        quick, browser = set(frontend_tests(False)), set(frontend_tests(True))
        actual = {str(path.relative_to(DESKTOP)) for path in (DESKTOP / "tests").glob("*.test.cjs")}
        self.assertEqual(quick | browser, actual)
        self.assertFalse(quick & browser)
        self.assertIn(str(Path("tests/desktop-main-pages-browser.test.cjs")), browser)
        self.assertIn(str(Path("tests/knowledge-settings.test.cjs")), browser)
        self.assertIn(str(Path("tests/runtime-browser-navigation.test.cjs")), browser)
        self.assertIn(str(Path("tests/runtime-browser-process.test.cjs")), quick)
        self.assertIn(str(Path("tests/browser-fixture-preparation.test.cjs")), quick)
        self.assertIn(str(Path("tests/lan-runtime-token.test.cjs")), quick)
        shards = [set(frontend_tests(True)[index::2]) for index in range(2)]
        self.assertEqual(shards[0] | shards[1], browser)
        self.assertFalse(shards[0] & shards[1])

    def test_missing_or_unresolvable_base_selects_full(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            output = Path(directory) / "output"
            self.assertEqual(main(["--component", "scope", "--github-output", str(output)]), 0)
            self.assertIn("full=true\n", output.read_text())
            with patch("scripts.verify_project.subprocess.run", return_value=subprocess.CompletedProcess([], 128, "", "unknown")):
                self.assertEqual(main(["--component", "scope", "--base", "unknown", "--github-output", str(output)]), 0)
            self.assertEqual(output.read_text().splitlines().count("full=true"), 2)

    def test_manual_quick_is_explicit_without_a_diff_base(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            output = Path(directory) / "output"
            self.assertEqual(main(["--component", "scope", "--event", "workflow_dispatch", "--tier", "quick",
                                   "--github-output", str(output)]), 0)
            self.assertEqual(output.read_text().splitlines()[:7],
                             ["full=false", "frontend_full=false", "rust_full=false", "native_full=false",
                              "frontend_changed=false", "frontend_files=[]", "rust_packages=[]"])
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(values["policy_full"], "false")
            self.assertEqual(json.loads(values["rust_matrix"]), rust_test_matrix([]))

    def test_rust_matrix_preserves_baseline_and_isolates_all_selected_packages(self):
        quick = rust_test_matrix([])["include"]
        self.assertEqual([row["group"] for row in quick], ["core", "storage"])
        self.assertEqual(sum(row["cache_writer"] for row in quick), 0)
        full = rust_test_matrix(["app-core", "app-storage", "app-runtime", "langame-desktop"])["include"]
        self.assertEqual([row["group"] for row in full[:3]], ["core", "storage", "services"])
        self.assertEqual([row["shard"] for row in full[3:]], [1, 2, 3, 4])
        self.assertTrue(all(row["count"] == 4 for row in full[3:]))
        self.assertEqual([row["group"] for row in full if row["cache_writer"]], ["desktop"])

    def test_full_behavior_scope_does_not_force_publication_history(self):
        changed = "apps/desktop/src-tauri/src/commands.rs"
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch("scripts.verify_project.changed_paths", return_value=[changed]):
            output = Path(directory) / "scope"
            self.assertEqual(main(["--component", "scope", "--base", "base", "--github-output", str(output)]), 0)
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(values["rust_full"], "true")
            self.assertEqual(values["native_full"], "true")
            self.assertEqual(values["policy_full"], "false")

    def test_rust_group_forwards_selection_and_shard_to_shared_runner(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(Runner, "python") as python:
            self.assertEqual(main(["--component", "rust-tests", "--rust-group", "desktop",
                                   "--rust-packages-json", '["langame-desktop"]',
                                   "--shard-index", "2", "--shard-count", "4", "--output", directory]), 0)
            arguments = python.call_args.args
            self.assertEqual(arguments[arguments.index("--group") + 1], "desktop")
            self.assertEqual(arguments[arguments.index("--shard-index") + 1], "2")
            self.assertEqual(arguments[arguments.index("--shard-count") + 1], "4")
            self.assertEqual(json.loads(arguments[arguments.index("--rust-packages-json") + 1]), ["langame-desktop"])

    def test_explicit_full_and_schedule_select_every_slow_layer(self):
        for arguments in (["--tier", "full"], ["--event", "schedule"]):
            with self.subTest(arguments=arguments), tempfile.TemporaryDirectory() as directory, \
                    contextlib.redirect_stdout(io.StringIO()):
                output = Path(directory) / "output"
                self.assertEqual(main(["--component", "scope", "--base", "unused",
                                       "--github-output", str(output), *arguments]), 0)
                lines = output.read_text().splitlines()
                self.assertEqual(lines[:5], ["full=true", "frontend_full=true", "rust_full=true",
                                            "native_full=true", "frontend_changed=true"])
                self.assertEqual(json.loads(lines[5].removeprefix("frontend_files=")), browser_repository_files())

    def test_changed_paths_keep_unicode_and_embedded_newlines(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            output = Path(directory) / "output"
            changed = "apps/desktop/src/测试\nview.tsx\0README.md\0"
            with patch("scripts.verify_project.subprocess.run", return_value=subprocess.CompletedProcess([], 0, changed, "")) as run:
                self.assertEqual(main(["--component", "scope", "--base", "base", "--github-output", str(output)]), 0)
            self.assertIn("-z", run.call_args.args[0])
            lines = output.read_text().splitlines()
            self.assertEqual(lines[:5], ["full=true", "frontend_full=true", "rust_full=false",
                                        "native_full=false", "frontend_changed=true"])
            self.assertEqual(json.loads(lines[5].removeprefix("frontend_files=")), browser_repository_files())

    def test_scope_emits_a_data_only_selection_for_local_ui_changes(self):
        changed = "apps/desktop/src/views/system/SystemCoreDial.tsx"
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch("scripts.verify_project.subprocess.run", return_value=subprocess.CompletedProcess([], 0, changed + "\0", "")):
            output = Path(directory) / "scope"
            self.assertEqual(main(["--component", "scope", "--base", "base", "--github-output", str(output)]), 0)
            values = dict(line.split("=", 1) for line in output.read_text().splitlines())
            self.assertEqual(values["frontend_full"], "false")
            self.assertEqual(values["frontend_changed"], "true")
            self.assertEqual(json.loads(values["frontend_files"]), frontend_selection([changed]))

    def test_browser_json_rejects_invalid_empty_duplicate_and_non_suite_paths(self):
        valid = QUICK_BROWSER_FILES[0]
        for value in ("not json", "null", "{}", "[]", '[1]', json.dumps([valid, valid]),
                      json.dumps(["apps/desktop/tests/missing-browser.test.cjs"]),
                      json.dumps(["apps/desktop/tests/runtime-browser-process.test.cjs"]),
                      json.dumps(["../tests/instance-create-browser.test.cjs"]),
                      json.dumps(["C:/external.test.cjs"]), json.dumps(["$(Write-Output injected)"])):
            with self.subTest(value=value), self.assertRaises(ValueError):
                parse_browser_files(value, full=False)
        with self.assertRaisesRegex(ValueError, "complete browser suite"):
            parse_browser_files(json.dumps([valid]), full=True)
        self.assertEqual(parse_browser_files(json.dumps(list(reversed(browser_repository_files()))), full=True),
                         browser_repository_files())

    def test_selected_browser_shards_execute_each_selected_file_exactly_once(self):
        selected = frontend_selection(["apps/desktop/src/views/system/SystemCoreDial.tsx"])
        seen = []
        for index in range(1, 4):
            with patch.object(Runner, "node") as node, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(["--component", "frontend-browser", "--browser-files-json", json.dumps(selected),
                                       "--shard-index", str(index), "--shard-count", "3"]), 0)
                seen.extend(f"apps/desktop/{Path(path).as_posix()}" for call in node.call_args_list for path in call.args[2:])
        self.assertEqual(sorted(seen), selected)
        self.assertEqual(len(seen), len(set(seen)))
        with patch.object(Runner, "node") as node, contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(main(["--component", "frontend-browser", "--browser-files-json", json.dumps([selected[0]]),
                                   "--shard-index", "2", "--shard-count", "2"]), 0)
            node.assert_not_called()

    def test_local_browser_base_uses_the_scope_selection_and_safe_fallback(self):
        for changed, code, expected in (("apps/desktop/src/views/system/SystemCoreDial.tsx", 0,
                                        frontend_selection(["apps/desktop/src/views/system/SystemCoreDial.tsx"])),
                                       ("README.md", 0, []), ("", 128, browser_repository_files())):
            with self.subTest(changed=changed, code=code), patch.object(Runner, "node") as node, \
                    patch("scripts.verify_project.subprocess.run", return_value=subprocess.CompletedProcess([], code, changed + "\0", "")), \
                    contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(["--component", "frontend-browser", "--base", "base"]), 0)
                actual = [f"apps/desktop/{Path(path).as_posix()}" for call in node.call_args_list for path in call.args[2:]]
                self.assertEqual(actual, expected)

    def test_full_browser_and_legacy_entry_always_run_the_complete_suite(self):
        for arguments in ([], ["--tier", "full", "--base", "base"]):
            with self.subTest(arguments=arguments), patch.object(Runner, "node") as node, \
                    patch("scripts.verify_project.subprocess.run") as diff, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(main(["--component", "frontend-browser", *arguments]), 0)
                actual = [f"apps/desktop/{Path(path).as_posix()}" for call in node.call_args_list for path in call.args[2:]]
                self.assertEqual(actual, browser_repository_files())
                diff.assert_not_called()
        for arguments in (["--base", "base"], ["--tier", "full"]):
            with self.subTest(arguments=arguments), self.assertRaises(SystemExit), \
                    contextlib.redirect_stderr(io.StringIO()):
                main(["--component", "frontend-browser", "--browser-files-json", json.dumps([QUICK_BROWSER_FILES[0]]), *arguments])

    def test_local_frontend_entry_preserves_publication_checks_without_python_suite(self):
        for tier in ("quick", "full"):
            with self.subTest(tier=tier), patch.object(Runner, "python") as python, \
                    patch.object(Runner, "frontend_build"), patch.object(Runner, "frontend_suite"):
                self.assertEqual(main(["--component", "frontend", "--tier", tier]), 0)
                expected = [("scripts/verify_no_tracked_secrets.py",), ("scripts/verify_open_source_boundary.py",)]
                if tier == "full":
                    expected.append(("scripts/verify_open_source_history.py",))
                self.assertEqual([call.args for call in python.call_args_list], expected)

    def test_ci_frontend_components_do_not_repeat_the_policy_job(self):
        for component in ("frontend-build", "frontend-tests", "frontend-browser"):
            with self.subTest(component=component), patch.object(Runner, "publication") as publication, \
                    patch.object(Runner, "frontend_build"), patch.object(Runner, "frontend_suite"):
                self.assertEqual(main(["--component", component, "--tier", "full"]), 0)
                publication.assert_not_called()

    def test_all_checks_reuse_publication_rules_once(self):
        with patch.object(Runner, "publication") as publication, patch.object(Runner, "python"), \
                patch.object(Runner, "frontend_build"), patch.object(Runner, "frontend_suite") as suite, \
                patch.object(Runner, "rust"):
            self.assertEqual(main(["--tier", "quick"]), 0)
            publication.assert_called_once_with()
            suite.assert_called_once_with(False)

    def test_failed_check_stops_the_runner(self):
        runner = Runner(argparse.Namespace(plan=False, frontend_dist=None))
        with patch("scripts.verify_project.subprocess.run", return_value=subprocess.CompletedProcess([], 7)), \
                contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                runner.run(["test-command"])
        self.assertEqual(raised.exception.returncode, 7)

    def test_managed_cargo_arguments_are_data_and_use_snapshots(self):
        runner = Runner(argparse.Namespace(plan=False, frontend_dist=None, cargo_wrapper=Path("wrapper path.ps1")))
        with patch.object(runner, "run") as run:
            runner.cargo("clippy", "--locked", "--", "-D", "warnings")
        self.assertIn("-SourceSnapshot", run.call_args.args[0][-1])
        self.assertNotIn("wrapper path.ps1", run.call_args.args[0][-1])
        self.assertEqual(runner.environment["LGSM_VERIFY_CARGO_COMMAND"], "clippy")

    def test_managed_formatting_uses_the_live_worktree_without_a_snapshot(self):
        runner = Runner(argparse.Namespace(plan=False, frontend_dist=None, cargo_wrapper=Path("wrapper.ps1")))
        with patch.object(runner, "run") as run:
            runner.cargo("fmt", "--all", "--", "--check")
        self.assertNotIn("-SourceSnapshot", run.call_args.args[0][-1])
        self.assertEqual(runner.environment["LGSM_VERIFY_CARGO_COMMAND"], "fmt")

    def test_frontend_output_never_empties_existing_data_or_repository_ancestors(self):
        with tempfile.TemporaryDirectory() as directory:
            retained = Path(directory) / "saved-data.txt"
            retained.write_text("retained", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "new directory"):
                validate_frontend_output(Path(directory))
            self.assertEqual(retained.read_text(), "retained")
            new = Path(directory) / "new-build"
            self.assertEqual(validate_frontend_output(new), new.resolve())
            self.assertFalse(new.exists())
        for path in (ROOT, ROOT / "dist", ROOT.parent, Path(ROOT.anchor)):
            with self.subTest(path=path), self.assertRaises(ValueError):
                validate_frontend_output(path)

    def test_portable_license_verification_fetches_locked_sources_before_offline_read(self):
        runner = Runner(argparse.Namespace(frontend_dist=None, cargo_wrapper=None, tier="quick"))
        calls = []
        with patch.object(runner, "cargo", side_effect=lambda *args: calls.append(("cargo", *args))), \
                patch.object(runner, "python", side_effect=lambda *args: calls.append(("python", *args))):
            runner.rust()
        fetch = calls.index(("cargo", "fetch", "--locked"))
        license_check = calls.index(("python", "scripts/generate_rust_third_party_licenses.py", "--check", "--offline"))
        self.assertLess(fetch, license_check)

    def test_managed_license_verification_does_not_request_an_unsupported_fetch(self):
        runner = Runner(argparse.Namespace(frontend_dist=None, cargo_wrapper=Path("wrapper.ps1"), tier="quick"))
        with patch.object(runner, "cargo") as cargo, patch.object(runner, "python") as python:
            runner.rust()
        self.assertFalse(any(call.args[0] == "fetch" for call in cargo.call_args_list))
        python.assert_called_once_with("scripts/generate_rust_third_party_licenses.py", "--check")

    def test_managed_native_check_needs_a_receipted_executable(self):
        runner = Runner(argparse.Namespace(frontend_dist=None, native_executable=None, cargo_wrapper=Path("wrapper.ps1")))
        with self.assertRaisesRegex(ValueError, "build receipt"):
            runner.native()


if __name__ == "__main__":
    unittest.main()
