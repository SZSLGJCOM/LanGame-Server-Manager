import json
import io
import os
import subprocess
from contextlib import redirect_stderr
from pathlib import Path, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

from scripts.verify_runtime_reliability import (
    ROOT, Stage, browser_evidence_errors, command_for, inspect_result, main, resource_evidence_errors, stages, validate_output,
    run_stage, tauri_frontend_config, validate_frontend_dist,
)


class RuntimeReliabilityRunnerTests(unittest.TestCase):
    def setUp(self):
        self.stage = Stage("fixture", (), ROOT, ("reliability_fixture",))
        self.success = (
            "test fixture::reliability_fixture ... ok\n"
            'RELIABILITY_METRICS {"samples": [12, 12]}\n'
            "test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n"
        )

    def test_records_actual_completed_test_and_resource_evidence(self):
        result = inspect_result(self.stage, self.success, 0)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["passed"], 1)
        self.assertEqual(result["skipped"], 1)
        self.assertEqual(result["metrics"], [{"samples": [12, 12]}])

    def test_successful_body_does_not_hide_a_failed_wrapper(self):
        self.assertEqual(inspect_result(self.stage, self.success, 1)["status"], "failed")

    def test_zero_tests_and_missing_required_group_fail_closed(self):
        empty = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 50 filtered out; finished in 0.00s\n"
        self.assertEqual(inspect_result(self.stage, empty, 0)["status"], "failed")
        renamed = self.success.replace("fixture::reliability_fixture", "unrelated::test")
        self.assertEqual(inspect_result(self.stage, renamed, 0)["missing_test_groups"], ["reliability_fixture"])

    def test_any_failed_binary_prevents_aggregate_success(self):
        failed = "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        self.assertEqual(inspect_result(self.stage, self.success + failed, 0)["status"], "failed")

    def test_unicode_failure_preserves_result_and_utf8_log_on_a_gbk_console(self):
        failure = (
            "test fixture::reliability_fixture ... FAILED\n"
            "diagnostic: 游戏 🎮\n"
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        )

        def failed_command(command, **kwargs):
            kwargs["stdout"].write(failure)
            return subprocess.CompletedProcess(command, 101)

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            console_bytes = io.BytesIO()
            with io.TextIOWrapper(console_bytes, encoding="gbk", errors="strict") as console, \
                 patch("scripts.verify_runtime_reliability.sys.stdout", console), \
                 patch("scripts.verify_runtime_reliability.subprocess.run", side_effect=failed_command):
                result = run_stage(self.stage, output, 8)
                console.flush()
                self.assertIn(b"\\U0001f3ae", console_bytes.getvalue())
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["exit_code"], 101)
            self.assertEqual(result["failed"], 1)
            self.assertEqual((output / "fixture.log").read_text(encoding="utf-8"), failure)

    def test_node_requires_completed_nonempty_results_and_rejects_cancellation(self):
        stage = Stage("web", (), ROOT, (), "node")
        self.assertEqual(inspect_result(stage, "TAP version 13\n", 0)["status"], "failed")
        summary = "# pass 5\n# fail 0\n# cancelled 0\n# skipped 0\n"
        self.assertEqual(inspect_result(stage, summary, 0)["status"], "passed")
        self.assertEqual(inspect_result(stage, summary.replace("cancelled 0", "cancelled 1"), 0)["status"], "failed")

    def test_existing_evidence_and_repository_paths_are_rejected(self):
        with self.assertRaises(ValueError):
            validate_output(ROOT / "runtime-report")
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaises(ValueError):
                validate_output(Path(temporary))
            self.assertEqual(validate_output(Path(temporary) / "new"), (Path(temporary) / "new").resolve())

    def test_resource_probe_requires_complete_bounded_measurements_for_requested_cycles(self):
        sample = {"handles": 12, "threads": 3, "private_bytes": 8192, "owned_output_sinks_remaining": 0}
        report = {"schema_version": 2, "measured_cycles": 4, "warmup_cycles": 8, "launches_per_cycle": 4,
                  "warmup": [sample] * 8, "measured": [sample] * 4, "limits": sample,
                  "reader_thread_exits_verified": 48}
        self.assertEqual(resource_evidence_errors([report], 4), [])
        self.assertTrue(resource_evidence_errors([], 4))
        self.assertTrue(resource_evidence_errors([report], 24))
        self.assertTrue(resource_evidence_errors([{**report, "measured": [sample] * 3}], 4))
        growing = {**report, "measured": [sample] * 3 + [{**sample, "handles": 13}]}
        self.assertTrue(resource_evidence_errors([growing], 4))

    def test_missing_resource_metrics_and_malformed_json_cannot_pass(self):
        stage = Stage("resources", (), ROOT, ("reliability_fixture",), metrics_required=True)
        self.assertEqual(inspect_result(stage, self.success, 0)["status"], "failed")
        self.assertEqual(inspect_result(self.stage, self.success + "RELIABILITY_METRICS invalid\n", 0)["status"], "failed")

    def test_node_skipped_or_removed_required_behavior_is_not_success(self):
        stage = Stage("web", (), ROOT, ("required lifecycle",), "node")
        summary = "# pass 5\n# fail 0\n# cancelled 0\n# skipped 0\n"
        self.assertEqual(inspect_result(stage, summary, 0)["status"], "failed")
        self.assertEqual(inspect_result(stage, "ok 1 - required lifecycle # SKIP\n" + summary, 0)["status"], "failed")
        self.assertEqual(inspect_result(stage, "ok 1 - required lifecycle\n" + summary, 0)["status"], "passed")

    def test_browser_report_requires_real_lifecycle_and_cleanup_evidence(self):
        report = {"status": "passed", "cycles": 12, "renders": 36, "strict_mode_replays": 12,
                  "listeners_created": 48, "listeners_released": 48, "listeners_remaining": 0,
                  "dom_nodes_after_unmount": 0, "max_tail_lines": 400, "browser_errors": [],
                  "browser_exited": True, "scratch_removed": True,
                  "browser_processes_observed": 4, "browser_processes_remaining": 0}
        self.assertEqual(browser_evidence_errors([report]), [])
        for change in ({"listeners_remaining": 1}, {"browser_errors": ["render failed"]},
                       {"scratch_removed": False}, {"strict_mode_replays": 0}, {"max_tail_lines": 401}):
            self.assertTrue(browser_evidence_errors([{**report, **change}]))
        self.assertTrue(browser_evidence_errors([]))

    def test_managed_arguments_are_data_instead_of_powershell_interpolation(self):
        stage = stages()[0]
        wrapper = Path("wrapper directory") / "invoke-cargo.ps1"
        with patch("scripts.verify_runtime_reliability.shutil.which", return_value="pwsh"):
            command, environment = command_for(stage, wrapper, None)
        self.assertNotIn(str(wrapper), command[-1])
        self.assertEqual(environment["LGSM_RELIABILITY_CARGO_WRAPPER"], str(wrapper))
        self.assertEqual(json.loads(environment["LGSM_RELIABILITY_CARGO_ARGUMENTS"]), list(stage.command[1:]))
        self.assertIn("-SourceSnapshot", command[-1])

    def test_ci_preserves_full_workspace_tests_and_every_required_group(self):
        scoped = stages()
        full = stages(full_workspace=True)
        self.assertEqual(len(full), 2)
        self.assertIn("--workspace", full[0].command)
        self.assertNotIn("--ignored", full[0].command)
        self.assertEqual(set(full[0].required), {selector for stage in scoped[:3] for selector in stage.required})
        self.assertEqual(full[1], scoped[-1])

    def test_frontend_distribution_is_external_absolute_and_has_an_entrypoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            distribution = Path(temporary).resolve()
            with self.assertRaises(ValueError):
                validate_frontend_dist(distribution)
            (distribution / "index.html").write_text("<html></html>", encoding="utf-8")
            self.assertEqual(validate_frontend_dist(distribution), distribution)
            for invalid in (ROOT, Path("relative-dist"), distribution / "index.html"):
                with self.subTest(path=invalid), self.assertRaises(ValueError):
                    validate_frontend_dist(invalid)
            with self.assertRaises(OSError):
                validate_frontend_dist(distribution / "missing")

    def test_frontend_override_merges_only_rust_child_configuration(self):
        existing = {"build": {"frontendDist": "../previous", "devUrl": "http://127.0.0.1:1420"},
                    "app": {"security": {"csp": "default-src 'self'"}}}
        encoded = json.dumps(existing)
        with tempfile.TemporaryDirectory() as temporary:
            distribution = Path(temporary).resolve()
            (distribution / "index.html").write_text("<html></html>", encoding="utf-8")
            with patch.dict(os.environ, {"TAURI_CONFIG": encoded}):
                command, environment = command_for(stages()[0], None, None, distribution)
                merged = json.loads(environment["TAURI_CONFIG"])
                _, node_environment = command_for(stages()[-1], None, None, distribution)
                self.assertEqual(os.environ["TAURI_CONFIG"], encoded)
            self.assertEqual(command[0], "cargo")
            self.assertEqual(merged["app"], existing["app"])
            self.assertEqual(merged["build"]["devUrl"], existing["build"]["devUrl"])
            expected_path = "\\\\?\\" + str(distribution) if os.name == "nt" else str(distribution)
            self.assertEqual(merged["build"]["frontendDist"], expected_path)
            self.assertEqual(node_environment["TAURI_CONFIG"], encoded)

    def test_windows_frontend_paths_select_directory_instead_of_url(self):
        paths = (
            (PureWindowsPath("C:/frontend/dist"), "\\\\?\\C:\\frontend\\dist"),
            (PureWindowsPath("//server/share/dist"), "\\\\?\\UNC\\server\\share\\dist"),
            (PureWindowsPath("//?/C:/frontend/dist"), "\\\\?\\C:\\frontend\\dist"),
        )
        for path, expected in paths:
            with self.subTest(path=path), patch("scripts.verify_runtime_reliability.os.name", "nt"):
                merged = json.loads(tauri_frontend_config(None, path))
            self.assertEqual(merged, {"build": {"frontendDist": expected}})

    def test_frontend_override_rejects_invalid_existing_configuration(self):
        for existing in ("not-json", "[]", "null", "42", '{"build":null}', '{"build":[]}',
                         '{"app":{"value":NaN}}'):
            with self.subTest(existing=existing), self.assertRaises(ValueError):
                tauri_frontend_config(existing, ROOT)

    def test_report_records_frontend_input_without_serializing_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            distribution = Path(temporary).resolve() / "frontend"
            distribution.mkdir()
            (distribution / "index.html").write_text("<html></html>", encoding="utf-8")
            output = Path(temporary) / "report"
            with patch("scripts.verify_runtime_reliability.windows_runtime_supported", return_value=True), \
                 patch("scripts.verify_runtime_reliability.run_stage", return_value={"status": "failed"}) as run, \
                 patch.dict(os.environ, {"TAURI_CONFIG": '{"app":{"security":{"csp":"default-src none"}}}'}):
                self.assertEqual(main(["--output", str(output), "--frontend-dist", str(distribution)]), 1)
            self.assertEqual(run.call_args.args[-1], distribution)
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(report["frontend_dist"], str(distribution))
            self.assertNotIn("TAURI_CONFIG", report)

    def test_frontend_distribution_cannot_contain_growing_test_evidence(self):
        with tempfile.TemporaryDirectory() as temporary:
            distribution = Path(temporary).resolve()
            (distribution / "index.html").write_text("<html></html>", encoding="utf-8")
            output = distribution / "report"
            with patch("scripts.verify_runtime_reliability.windows_runtime_supported", return_value=True), \
                 patch("scripts.verify_runtime_reliability.run_stage") as run, \
                 patch.dict(os.environ, {"TAURI_CONFIG": "{}"}), \
                 redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                main(["--output", str(output), "--frontend-dist", str(distribution)])
            self.assertEqual(raised.exception.code, 2)
            run.assert_not_called()
            self.assertFalse(output.exists())

    def test_managed_node_profile_matches_the_stage_browser_requirement(self):
        wrapper = Path("wrapper directory") / "run-guarded-node-command.cjs"
        browser_stage = stages()[-1]
        contract_stage = Stage("contracts", ("--test", "tests/fixture.test.cjs"), ROOT, (), "node")
        for stage, profile in ((browser_stage, "e2e"), (contract_stage, "contracts")):
            with self.subTest(stage=stage.name):
                command, _ = command_for(stage, None, wrapper)
                self.assertEqual(command, ["node", str(wrapper), "--profile", profile, "--", *stage.command])
                portable_command, _ = command_for(stage, None, None)
                self.assertEqual(portable_command, ["node", *stage.command])

    def test_failed_stage_stops_following_stages_and_leaves_failed_report(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report"
            with patch("scripts.verify_runtime_reliability.windows_runtime_supported", return_value=True), \
                 patch("scripts.verify_runtime_reliability.run_stage", return_value={"status": "failed"}) as run:
                self.assertEqual(main(["--output", str(output)]), 1)
            run.assert_called_once()
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertIn("finished_at", report)


if __name__ == "__main__":
    unittest.main()
