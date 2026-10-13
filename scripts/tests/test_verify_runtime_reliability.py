import json
import argparse
import base64
import io
import os
import subprocess
import sys
import threading
from contextlib import redirect_stderr
from pathlib import Path, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

from scripts import verify_runtime_reliability as reliability

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
                self.assertEqual(console_bytes.getvalue().count(b"diagnostic:"), 1)
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["exit_code"], 101)
            self.assertEqual(result["failed"], 1)
            self.assertEqual((output / "fixture.log").read_text(encoding="utf-8"), failure)

    def test_child_output_is_visible_before_the_child_exits(self):
        marker = "streaming-progress-handshake"
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary)
            acknowledgement = output / "observed"

            class HandshakeConsole(io.StringIO):
                def write(self, text):
                    written = super().write(text)
                    if marker in text:
                        acknowledgement.touch()
                    return written

            child = (
                "import os, pathlib, sys, time; "
                "ack = pathlib.Path(sys.argv[1]); "
                f"os.write(1, {marker.encode()!r}); "
                "deadline = time.monotonic() + 5\n"
                "while not ack.exists() and time.monotonic() < deadline: time.sleep(0.01)\n"
                "if not ack.exists(): sys.exit(3)\n"
                f"print({(chr(10) + self.success)!r}, flush=True)\n"
            )
            command = [sys.executable, "-u", "-c", child, str(acknowledgement)]
            with patch.object(reliability, "command_for", return_value=(command, os.environ.copy())), \
                 patch.object(reliability, "LOG_PROGRESS_INTERVAL_SECONDS", 0.01), \
                 patch.object(reliability.sys, "stdout", HandshakeConsole()) as console:
                result = run_stage(self.stage, output, 8)
            self.assertEqual(result["exit_code"], 0, "The child timed out waiting for live output acknowledgement")
            self.assertEqual(result["status"], "passed", "The child must receive live output acknowledgement")
            self.assertIn(marker, console.getvalue())
            self.assertFalse(any(thread.name == "reliability-log-fixture" for thread in threading.enumerate()))

    def test_progress_decodes_split_utf8_and_preserves_a_legacy_console(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "fixture.log"
            text = "游戏 🎮"
            encoded = text.encode("utf-8")
            log.write_bytes(b"prefix " + encoded[:1])
            progress = reliability._StageLogProgress("fixture", log, 0)
            console_bytes = io.BytesIO()
            with io.TextIOWrapper(console_bytes, encoding="gbk", errors="strict") as console, \
                 patch.object(reliability.sys, "stdout", console):
                progress.emit()
                with log.open("ab") as stream:
                    stream.write(encoded[1:])
                progress.emit(final=True)
                console.flush()
                rendered = console_bytes.getvalue().decode("gbk")
            self.assertEqual(rendered.count("prefix "), 1)
            self.assertIn("游戏 \\U0001f3ae", rendered)
            self.assertNotIn("\\ufffd", rendered)
            self.assertEqual(log.read_bytes(), b"prefix " + encoded)

    def test_progress_is_bounded_without_repeating_or_truncating_the_file(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "fixture.log"
            payload = b"discarded-" + b"TAIL1234"
            log.write_bytes(payload)
            progress = reliability._StageLogProgress("fixture", log, 0)
            with patch.object(reliability, "LOG_PROGRESS_MAX_BYTES", 8), \
                 patch.object(reliability.sys, "stdout", io.StringIO()) as console:
                progress.emit()
                progress.emit()
                with log.open("ab") as stream:
                    stream.write(b"next")
                progress.emit(final=True)
            rendered = console.getvalue()
            self.assertIn("10 bytes omitted from console", rendered)
            self.assertNotIn("discarded-", rendered)
            self.assertEqual(rendered.count("TAIL1234"), 1)
            self.assertEqual(rendered.count("next"), 1)
            self.assertEqual(progress.offset, len(payload) + 4)
            self.assertEqual(log.read_bytes(), payload + b"next")

    def test_stage_exceptions_stop_the_observer_and_preserve_process_options(self):
        for failure in (KeyboardInterrupt(), OSError("child launch failed")):
            with self.subTest(failure=type(failure).__name__), tempfile.TemporaryDirectory() as temporary:
                def interrupted_command(command, **kwargs):
                    self.assertNotEqual(kwargs["stdout"], subprocess.PIPE)
                    self.assertEqual(kwargs["stderr"], subprocess.STDOUT)
                    self.assertNotIn("timeout", kwargs)
                    if os.name == "nt":
                        self.assertEqual(kwargs["creationflags"], subprocess.CREATE_NO_WINDOW)
                    kwargs["stdout"].write("last child output")
                    raise failure

                with patch.object(reliability.subprocess, "run", side_effect=interrupted_command), \
                     patch.object(reliability.sys, "stdout", io.StringIO()) as console, \
                     self.assertRaises(type(failure)) as raised:
                    run_stage(self.stage, Path(temporary), 8)
                self.assertIs(raised.exception, failure)
                self.assertIn("last child output", console.getvalue())
                self.assertFalse(any(thread.name == "reliability-log-fixture" for thread in threading.enumerate()))

    def test_observer_failure_is_reported_after_the_child_and_thread_finish(self):
        with tempfile.TemporaryDirectory() as temporary, \
             patch.object(reliability.subprocess, "run", return_value=subprocess.CompletedProcess([], 0)) as run, \
             patch.object(reliability._StageLogProgress, "emit", side_effect=OSError("log read failed")), \
             patch.object(reliability.sys, "stdout", io.StringIO()), \
             self.assertRaisesRegex(OSError, "Stage log observation failed: log read failed"):
            run_stage(self.stage, Path(temporary), 8)
        run.assert_called_once()
        self.assertFalse(any(thread.name == "reliability-log-fixture" for thread in threading.enumerate()))

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
        self.assertEqual(len(full), 4)
        core, storage, stateful, web = full
        self.assertIn("--workspace", stateful.command)
        parallel_packages = [core.command[index + 1] for index, arg in enumerate(core.command) if arg == "-p"]
        storage_packages = [storage.command[index + 1] for index, arg in enumerate(storage.command) if arg == "-p"]
        excluded_packages = [stateful.command[index + 1] for index, arg in enumerate(stateful.command) if arg == "--exclude"]
        self.assertEqual(parallel_packages + storage_packages, excluded_packages,
                         "Every workspace package must run exactly once")
        self.assertEqual(set(parallel_packages), {"app-core", "app-modules", "app-knowledge"})
        self.assertEqual(storage_packages, ["app-storage"])
        self.assertEqual(len(excluded_packages), len(set(excluded_packages)))
        self.assertIn("--test-threads=4", core.command)
        self.assertEqual(storage.command, ("test", "-p", "app-storage", "--all-features", "--locked", "--",
                                          "--test-threads=2", "--show-output"),
                         "Storage runs the complete package, including doctests, without filters")
        self.assertIn("--test-threads=1", stateful.command)
        self.assertNotIn("--ignored", stateful.command)
        self.assertEqual(storage.required, scoped[1].required)
        self.assertEqual(set(stateful.required), {selector for stage in (scoped[0], scoped[2]) for selector in stage.required})
        self.assertEqual(set(storage.required) | set(stateful.required),
                         {selector for stage in scoped[:3] for selector in stage.required})
        self.assertFalse(storage.metrics_required)
        self.assertTrue(stateful.metrics_required)
        self.assertEqual(web, scoped[-1])
        self.assertEqual(stages(full_workspace=True, include_web=False), full[:-1],
                         "Shared full verification gives frontend checks a single owner")

    def test_failed_full_storage_stage_prevents_workspace_execution(self):
        completed = []

        def complete(stage, *_args):
            completed.append(stage.name)
            return {"name": stage.name, "status": "failed" if stage.name == "storage" else "passed"}

        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report"
            with patch.object(reliability, "windows_runtime_supported", return_value=True), \
                 patch.object(reliability, "run_stage", side_effect=complete):
                self.assertEqual(main(["--full-workspace", "--rust-only", "--output", str(output)]), 1)
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
        self.assertEqual(completed, ["core", "storage"])
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["stages"][-1], {"name": "storage", "status": "failed"})

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


class RuntimeReliabilityShardingTests(unittest.TestCase):
    def artifact(self, name="langame-desktop", kind="bin", *, test=True, doctest=False):
        return json.dumps({"reason": "compiler-artifact", "package_id": "path+file:///snapshot#langame-desktop@0.1.0",
                           "profile": {"test": test}, "executable": f"C:/target/{name}.exe" if test else None,
                           "target": {"name": name, "kind": [kind], "doctest": doctest}})

    def build_output(self, *artifacts):
        return "\n".join(["[OPC] Source snapshot captured: fixture input abc123", *artifacts,
                          '{"reason":"build-finished","success":true}', "[OPC] Completed"])

    def metadata(self):
        package = {"name": "langame-desktop", "id": "path+file:///desktop#langame-desktop@0.1.0",
                   "features": {"default": [], "desktop-reliability": []}, "targets": [
                       {"name": "langame-desktop", "kind": ["bin"], "test": True, "doctest": False},
                       {"name": "install_catalog", "kind": ["bin"], "test": True, "doctest": False},
                       {"name": "desktop_lib", "kind": ["lib"], "test": False, "doctest": True}]}
        return {"version": 1, "packages": [package], "workspace_members": [package["id"]]}

    def test_metadata_selects_enabled_workspace_targets_and_retains_library_doctests(self):
        metadata = self.metadata()
        package = metadata["packages"][0]
        package["targets"].extend([
            {"name": "integration", "kind": ["test"], "test": True, "doctest": False,
             "required-features": ["desktop-reliability"]},
            {"name": "tested_example", "kind": ["example"], "test": True, "doctest": False},
            {"name": "disabled_example", "kind": ["example"], "test": False, "doctest": False},
            {"name": "unavailable", "kind": ["bin"], "test": True, "doctest": False,
             "required-features": ["not-defined"]},
        ])
        targets, doctests = reliability.desktop_metadata_targets("warning: fixture\n" + json.dumps(metadata))
        self.assertEqual(targets, [("--bin", "install_catalog"), ("--bin", "langame-desktop"),
                                   ("--example", "tested_example"), ("--test", "integration")])
        self.assertTrue(doctests)

    def test_metadata_rejects_missing_or_nonworkspace_desktop_and_unsupported_test_targets(self):
        missing = self.metadata()
        missing["workspace_members"] = []
        unsupported = self.metadata()
        unsupported["packages"][0]["targets"][0]["kind"] = ["bench"]
        for output in ("", json.dumps(missing), json.dumps(unsupported),
                       json.dumps(self.metadata()) + "\n" + json.dumps(self.metadata())):
            with self.subTest(output=output), self.assertRaises(ValueError):
                reliability.desktop_metadata_targets(output)

    def test_groups_partition_full_packages_and_retain_quick_baseline(self):
        selected = ["app-runtime", "langame-desktop"]
        core = reliability.grouped_stages("core", selected)[0]
        storage = reliability.grouped_stages("storage", selected)[0]
        services = reliability.grouped_stages("services", selected)[0]
        self.assertEqual(core, stages(True, False, selected)[0])
        self.assertEqual(storage, stages(True, False, selected)[1])
        self.assertIn("backups", storage.command)
        self.assertEqual(services.command[:3], ("test", "-p", "app-runtime"))
        self.assertNotIn("langame-desktop", services.command)
        self.assertTrue(services.metrics_required)
        self.assertEqual(services.required, stages()[0].required)
        self.assertIn("--test-threads=1", services.command)
        self.assertEqual(reliability.grouped_stages("services", []), ())
        self.assertFalse(reliability.grouped_stages("services", ["app-network"])[0].metrics_required)
        full = reliability.grouped_stages("services", None)[0]
        actual = {full.command[i + 1] for i, arg in enumerate(full.command) if arg == "-p"}
        expected = set(reliability.rust_workspace()) - {*reliability.PARALLEL_PACKAGES, "app-storage", "langame-desktop"}
        self.assertEqual(actual, expected)

    def test_cargo_json_discovers_every_target_amid_source_snapshot_logs(self):
        output = self.build_output(self.artifact(), self.artifact("install_catalog"),
                                   self.artifact("integration", "test"), self.artifact("desktop_lib", "lib", doctest=True))
        targets, doctests = reliability.desktop_targets(output)
        self.assertEqual(targets, [("--bin", "install_catalog"), ("--bin", "langame-desktop"),
                                   ("--lib",), ("--test", "integration")])
        self.assertTrue(doctests)
        stage = Stage("compile", ("test", "-p", "langame-desktop", "--no-run", "--message-format=json"), ROOT, ())
        with patch.object(reliability.shutil, "which", return_value="pwsh"):
            _, env = command_for(stage, Path("fixture-wrapper.ps1"), None)
        self.assertEqual(json.loads(env["LGSM_RELIABILITY_CARGO_ARGUMENTS"]), list(stage.command[1:]))
        self.assertIn("--message-format=json", env["LGSM_RELIABILITY_CARGO_ARGUMENTS"])

    def test_failed_or_incomplete_cargo_inventory_cannot_pass(self):
        success = self.build_output(self.artifact())
        for output in ("[OPC] build done", self.build_output(), success.replace('"success":true', '"success":false'),
                       success.replace('"bin"', '"bench"'), success + "\n{broken-json}"):
            with self.subTest(output=output), self.assertRaises(ValueError):
                reliability.desktop_targets(output)

    def test_library_doctests_detected_even_when_library_unit_tests_disabled(self):
        output = self.build_output(self.artifact(), self.artifact("desktop_lib", "lib", test=False, doctest=True))
        self.assertTrue(reliability.desktop_targets(output)[1])

    def test_listing_requires_complete_summary_including_empty_targets(self):
        self.assertEqual(reliability.listed_tests("0 tests, 0 benchmarks\n"), [])
        self.assertEqual(reliability.listed_tests("fixture::one: test\n1 test, 0 benchmarks\n"), ["fixture::one"])
        for output in ("", "fixture::one: test\n", "fixture::one: test\n2 tests, 0 benchmarks\n",
                       "fixture::one: test\nfixture::one: test\n2 tests, 0 benchmarks\n",
                       "fixture::one: benchmark\n0 tests, 1 benchmark\n"):
            with self.subTest(output=output), self.assertRaises(ValueError):
                reliability.listed_tests(output)

    def test_partition_is_repeatable_balanced_and_has_no_missing_or_duplicate_tests(self):
        inventory = {("--bin", "langame-desktop"): [f"group::test_{i:04}" for i in range(2085)],
                     ("--bin", "install_catalog"): ["test_one", "test_two"], ("--test", "empty"): []}
        partitions = [reliability.desktop_partition(inventory, i, 4) for i in range(1, 5)]
        flattened = [[(target, name) for target, names in shard.items() for name in names] for shard in partitions]
        all_tests = [item for shard in flattened for item in shard]
        self.assertEqual(set(all_tests), {(target, name) for target, names in inventory.items() for name in names})
        self.assertEqual(len(all_tests), len(set(all_tests)))
        self.assertLessEqual(max(map(len, flattened)) - min(map(len, flattened)), 1)
        reversed_inventory = {target: list(reversed(names)) for target, names in reversed(list(inventory.items()))}
        self.assertEqual(partitions[2], reliability.desktop_partition(reversed_inventory, 3, 4))
        for index, count in ((0, 4), (5, 4), (1, 5), (2, 1)):
            with self.assertRaises(ValueError):
                reliability.desktop_partition(inventory, index, count)

    def test_exact_batches_respect_windows_utf16_budget_without_name_loss(self):
        names = [f"fixture::游戏_{i:03}" + "x" * 150 for i in range(311)]
        batches = reliability.desktop_batches(("--bin", "langame-desktop"), names, limit=2000)
        actual = []
        self.assertGreater(len(batches), 1)
        for command in batches:
            self.assertLessEqual(len(subprocess.list2cmdline(["cargo", *command]).encode("utf-16-le")) // 2, 2000)
            self.assertIn("--test-threads=1", command)
            self.assertIn("--exact", command)
            actual.extend(command[command.index("--format=pretty") + 1:])
        self.assertEqual(actual, names)
        with self.assertRaises(ValueError):
            reliability.desktop_batches(("--lib",), ["x" * 24000])

    def test_exact_batch_requires_selected_names_and_exact_outer_summary_count(self):
        stage = Stage("batch", (), ROOT, (), exact_tests=("fixture::one", "fixture::panic"))
        success = ("test fixture::one ... ok\ntest fixture::panic - should panic ... ok\n"
                   "\nsuccesses:\n    fixture::one\n    fixture::panic\n\n"
                   "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")
        self.assertEqual(inspect_result(stage, success, 0)["status"], "passed")
        for output in (success.replace("fixture::panic", "fixture::different"),
                       success.replace("2 passed", "1 passed"), success.replace("2 passed", "3 passed"),
                       success.replace("0 ignored", "1 ignored"),
                       success.replace("    fixture::one\n", ""),
                       success.replace("    fixture::one\n", "    fixture::one\n    fixture::one\n")):
            with self.subTest(output=output):
                self.assertEqual(inspect_result(stage, output, 0)["status"], "failed")
        self.assertEqual(inspect_result(stage, success, 101)["status"], "failed")

    def test_child_process_names_and_summaries_do_not_inflate_exact_outer_batch(self):
        stage = Stage("batch", (), ROOT, (), exact_tests=("fixture::one", "fixture::two"))
        success = ("test fixture::one ... ok\ntest fixture::two ... ok\n"
                   "---- fixture::one stdout ----\n"
                   "test fixture::one ... ok\n"
                   "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                   "test fixture::nested_child ... ok\n"
                   "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
                   "\nsuccesses:\n    fixture::one\n    fixture::two\n\n"
                   "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")
        result = inspect_result(stage, success, 0)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["passed"], 2)
        self.assertEqual(inspect_result(stage, success, 101)["status"], "failed")

    def test_interrupted_parent_progress_uses_final_outer_success_list(self):
        parent = "runtime_service::client::ctrl_c_tests::detached_service_first_managed_game_receives_ctrl_c"
        child = "runtime_service::client::ctrl_c_tests::console_inheritance_process_fixture"
        stage = Stage("batch", (), ROOT, (), exact_tests=(parent, "fixture::other"))
        output = (f"test {parent} ... warning: `--logfile` is deprecated\nrunning 1 test\n"
                  f"test {child} ... ok\nsuccesses:\n    {child}\n\n"
                  "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 2168 filtered out\n"
                  "\nok\ntest fixture::other ... ok\n\nsuccesses:\n"
                  "---- fixture::other stdout ----\nchild diagnostics\n"
                  f"\nsuccesses:\n    fixture::other\n    {parent}\n\n"
                  "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n")
        result = inspect_result(stage, output, 0)
        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["passed"], 2)
        self.assertEqual(inspect_result(stage, output.replace(f"    {parent}\n", f"    {child}\n"), 0)["status"], "failed")

    def test_managed_batches_fit_json_base64_guard_payload_for_ascii_and_unicode(self):
        for suffix in ("long_test_name_" * 15, "游戏𓃠" * 80):
            names = [f"fixture::test_{index:04}_{suffix}" for index in range(311)]
            batches = reliability.desktop_batches(("--bin", "langame-desktop"), names, managed=True)
            actual = []
            for command in batches:
                # Match Start-CodexCargoDirectChild's payload fields, including
                # wrapper-added Cargo paths and resource metadata.
                path = "D:/managed/" + "directory/" * 60
                payload = {"executable": path + "cargo.exe",
                           "arguments": [*command, "--manifest-path", path + "Cargo.toml", "--target-dir", path + "target"],
                           "workingDirectory": path, "memoryLimitBytes": 17179869184,
                           "hostReserveBytes": 4294967296, "maximumActiveProcesses": 128, "timeoutSeconds": 0,
                           "ownerProcessId": 12345, "ownerStartTimeUtcTicks": 639011234567890000,
                           "jobName": "Local\\CodexCargoFixture"}
                for escaped in (False, True):
                    encoded = base64.b64encode(json.dumps(payload, ensure_ascii=escaped).encode("utf-8")).decode("ascii")
                    guard = ["C:/Windows/System32/WindowsPowerShell/v1.0/powershell.exe", "-NoLogo", "-NoProfile",
                             "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File",
                             path + "invoke-cargo-resource-guard.ps1", "-CommandPayload", encoded]
                    self.assertLess(len(subprocess.list2cmdline(guard).encode("utf-16-le")) // 2, 32767)
                actual.extend(command[command.index("--format=pretty") + 1:])
            self.assertEqual(actual, names)
            self.assertGreater(len(batches), 1)

    def test_only_doctest_stage_accepts_a_real_empty_summary(self):
        empty = "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n"
        stage = Stage("docs", (), ROOT, (), allow_empty=True)
        self.assertEqual(inspect_result(stage, empty, 0)["status"], "passed")
        self.assertEqual(inspect_result(stage, "", 0)["status"], "failed")
        self.assertEqual(inspect_result(stage, empty, 1)["status"], "failed")
        self.assertEqual(inspect_result(Stage("batch", (), ROOT, ()), empty, 0)["status"], "failed")

    def fake_capture(self, stage, *_args):
        if stage.name == "desktop-metadata":
            output = json.dumps(self.metadata())
        elif stage.name == "desktop-build":
            output = self.build_output(self.artifact(), self.artifact("install_catalog"),
                                       self.artifact("desktop_lib", "lib", test=False, doctest=True))
        elif stage.name.endswith("-1"):
            output = "0 tests, 0 benchmarks\n"
        else:
            output = ("fixture::ignored: test\n1 test, 0 benchmarks\n" if "ignored" in stage.name else
                      "fixture::first: test\nfixture::second: test\nfixture::ignored: test\n3 tests, 0 benchmarks\n")
        return output, {"name": stage.name, "status": "passed", "exit_code": 0, "log": f"{stage.name}.log"}

    def test_portable_discovery_uses_metadata_and_managed_discovery_keeps_supported_test_command(self):
        baseline = list(stages())
        baseline[2] = Stage("desktop", (), ROOT, ("fixture::first", "fixture::second"))
        for wrapper in (None, Path("managed.ps1")):
            args = argparse.Namespace(cycles=4, cargo_wrapper=wrapper, node_wrapper=None, frontend_dist=None,
                                      shard_index=1, shard_count=4)
            with self.subTest(wrapper=wrapper), tempfile.TemporaryDirectory() as temporary, \
                 patch.object(reliability, "capture_stage", side_effect=self.fake_capture) as capture, \
                 patch.object(reliability, "stages", return_value=tuple(baseline)):
                reliability.prepare_desktop(args, None, Path(temporary), {"stages": []})
            commands = [call.args[0].command for call in capture.call_args_list]
            if wrapper is None:
                self.assertEqual(commands[0], ("metadata", "--no-deps", "--all-features", "--locked", "--format-version", "1"))
                self.assertEqual(commands[1], ("test", "-p", "langame-desktop", "--all-features", "--locked",
                                               "--bin", "install_catalog", "--bin", "langame-desktop", "--no-run"))
                command, _ = command_for(capture.call_args_list[0].args[0], None, None)
                self.assertEqual(command, ["cargo", *commands[0]])
            else:
                self.assertTrue(all(command[0] == "test" for command in commands))
                self.assertIn("--no-run", commands[0])
            self.assertEqual(sum("--no-run" in command for command in commands), 1)

    def test_prepare_shards_omit_ignored_tests_and_own_doctests_once(self):
        args = argparse.Namespace(cycles=4, cargo_wrapper=None, node_wrapper=None, frontend_dist=None,
                                  shard_index=1, shard_count=4)
        baseline = list(stages())
        baseline[2] = Stage("desktop", (), ROOT, ("fixture::first", "fixture::second"))
        executed = []
        with tempfile.TemporaryDirectory() as temporary, \
             patch.object(reliability, "capture_stage", side_effect=self.fake_capture), \
             patch.object(reliability, "stages", return_value=tuple(baseline)):
            for index in range(1, 5):
                args.shard_index = index
                report = {"stages": []}
                checks = reliability.prepare_desktop(args, None, Path(temporary), report)
                executed.extend(checks)
                self.assertEqual(report["desktop_partition"]["doctests"], index == 1)
                self.assertEqual(report["desktop_partition"]["targets"][1]["ignored"], ["fixture::ignored"])
                self.assertFalse(any(stage.required for stage in checks), "Whole-suite requirements do not apply per shard")
        self.assertEqual(sorted(name for stage in executed for name in stage.exact_tests),
                         ["fixture::first", "fixture::second"])
        self.assertEqual(sum(stage.name == "desktop-doctests" for stage in executed), 1)

    def test_unselected_groups_are_explicit_success_without_cargo(self):
        for group in ("desktop", "services"):
            with self.subTest(group=group), tempfile.TemporaryDirectory() as temporary, \
                 patch.object(reliability, "windows_runtime_supported", return_value=True), \
                 patch.object(reliability, "capture_stage") as capture, \
                 patch.object(reliability, "run_stage") as run:
                output = Path(temporary) / "report"
                code = main(["--full-workspace", "--rust-only", "--group", group,
                             "--rust-packages-json", "[]", "--output", str(output)])
                report = json.loads((output / "report.json").read_text(encoding="utf-8"))
            self.assertEqual(code, 0)
            self.assertIn("empty_reason", report)
            capture.assert_not_called()
            run.assert_not_called()

    def test_failed_build_stops_before_enumeration_and_tests_and_keeps_evidence(self):
        failed = ("compilation failed", {"status": "failed", "exit_code": 101, "log": "desktop-build.log"})
        def capture_result(stage, *args):
            return failed if stage.name == "desktop-build" else self.fake_capture(stage, *args)

        with tempfile.TemporaryDirectory() as temporary, \
             patch.object(reliability, "windows_runtime_supported", return_value=True), \
             patch.object(reliability, "capture_stage", side_effect=capture_result) as capture, \
             patch.object(reliability, "run_stage") as run:
            output = Path(temporary) / "report"
            code = main(["--full-workspace", "--rust-only", "--group", "desktop", "--output", str(output)])
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
        self.assertEqual(code, 1)
        self.assertEqual(report["stages"][-1], failed[1])
        self.assertEqual([call.args[0].name for call in capture.call_args_list], ["desktop-metadata", "desktop-build"])
        run.assert_not_called()

    def test_failed_desktop_batch_stops_subsequent_batches_and_doctests(self):
        checks = tuple(Stage(f"batch-{i}", (), ROOT, ()) for i in range(3))
        with tempfile.TemporaryDirectory() as temporary, \
             patch.object(reliability, "windows_runtime_supported", return_value=True), \
             patch.object(reliability, "prepare_desktop", return_value=checks), \
             patch.object(reliability, "run_stage", return_value={"status": "failed"}) as run:
            code = main(["--full-workspace", "--rust-only", "--group", "desktop", "--output",
                         str(Path(temporary) / "report")])
        self.assertEqual(code, 1)
        run.assert_called_once()

    def test_missing_or_ignored_required_behavior_prevents_any_batch_execution(self):
        args = argparse.Namespace(cycles=4, cargo_wrapper=None, node_wrapper=None, frontend_dist=None,
                                  shard_index=1, shard_count=4)
        for required in ("fixture::missing", "fixture::ignored"):
            baseline = list(stages())
            baseline[2] = Stage("desktop", (), ROOT, (required,))
            with self.subTest(required=required), tempfile.TemporaryDirectory() as temporary, \
                 patch.object(reliability, "capture_stage", side_effect=self.fake_capture), \
                 patch.object(reliability, "stages", return_value=tuple(baseline)), \
                 self.assertRaisesRegex(ValueError, "missing required test groups"):
                reliability.prepare_desktop(args, None, Path(temporary), {"stages": []})

    def test_failed_listing_propagates_before_execution(self):
        def capture(stage, *args):
            if stage.name == "desktop-list-1":
                return "listing failed", {"status": "failed", "exit_code": 101, "log": "desktop-list-1.log"}
            return self.fake_capture(stage, *args)

        with tempfile.TemporaryDirectory() as temporary, \
             patch.object(reliability, "windows_runtime_supported", return_value=True), \
             patch.object(reliability, "capture_stage", side_effect=capture) as calls, \
             patch.object(reliability, "run_stage") as run:
            output = Path(temporary) / "report"
            code = main(["--full-workspace", "--rust-only", "--group", "desktop", "--output", str(output)])
            report = json.loads((output / "report.json").read_text(encoding="utf-8"))
        self.assertEqual(code, 1)
        self.assertEqual([call.args[0].name for call in calls.call_args_list],
                         ["desktop-metadata", "desktop-build", "desktop-list-1"])
        self.assertEqual(report["stages"][-1]["exit_code"], 101)
        run.assert_not_called()

    def test_invalid_group_and_shard_combinations_rejected_without_execution(self):
        for args in (["--group", "core"], ["--shard-count", "4"],
                     ["--full-workspace", "--rust-only", "--group", "desktop", "--shard-index", "5"]):
            with self.subTest(args=args), redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as raised:
                main([*args, "--plan"])
            self.assertEqual(raised.exception.code, 2)


if __name__ == "__main__":
    unittest.main()
