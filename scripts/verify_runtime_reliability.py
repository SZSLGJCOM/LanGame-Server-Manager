"""Run isolated runtime reliability checks and retain their actual test evidence."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Stage:
    name: str
    command: tuple[str, ...]
    cwd: Path
    required: tuple[str, ...]
    kind: str = "rust"
    metrics_required: bool = False
    browser_required: bool = False


def rust_arguments(package: str, target: tuple[str, ...], filters: tuple[str, ...]) -> tuple[str, ...]:
    return ("test", "-p", package, *target, "--all-features", "--locked", "--",
            *filters, "--test-threads=1", "--show-output")


def stages(full_workspace: bool = False) -> tuple[Stage, ...]:
    selected = (
        Stage("processes", rust_arguments("app-runtime", ("--lib",), (
            "runtime_reliability", "owned_process_tree", "stdin_write", "stdin_dispatch",
            "windows_process_output", "runtime_supervisor",
        )), ROOT, ("runtime_reliability_repeated_stop_restart_releases_resources_after_warmup",
                   "runtime_reliability_large_unbroken_output", "runtime_reliability_native_launch_stdin",
                   "runtime_reliability_unicode_commands", "runtime_reliability_launcher_detection",
                   "owned_process_tree", "stdin_write", "stdin_dispatch", "windows_process_output"), metrics_required=True),
        Stage("storage", rust_arguments("app-storage", ("--lib",), (
            "managed_console_log", "atomic_file::tests",
        )), ROOT, ("reliability_rotation_journal", "reliability_locked_rotation", "reliability_retention_replays",
                   "managed_console_log::tests")),
        Stage("desktop", rust_arguments("langame-desktop", ("--bin", "langame-desktop"), (
            "reliability_", "runtime_log_stream", "live_players::log_capture", "desktop_app_log::tests",
            "runtime_reconciliation", "runtime_restart_", "commands_managed_save",
            "runtime_transport::deadline", "lan_host::tests", "state::timed_cache",
        )), ROOT, ("reliability_restart_limit_survives_years", "reliability_restart_due_boundary",
                   "reliability_restart_years_of_elapsed_time", "reliability_restart_cancelled_pending_ticket",
                   "reliability_restart_stopped_flight", "reliability_restart_duplicate_exit",
                   "reliability_timed_cache_expires", "reliability_timed_cache_years",
                   "reliability_cancelled_request", "reliability_failed_refresh",
                   "reliability_aborted_worker", "reliability_panicked_worker",
                   "reliability_lan_disconnects", "reliability_lan_incomplete_header",
                   "reliability_lan_overload", "reliability_lan_write_disconnect", "runtime_reconciliation",
                   "runtime_log_stream", "runtime_transport::deadline", "runtime_restart::tests",
                   "live_players::log_capture", "desktop_app_log::tests", "commands_managed_save")),
        Stage("web", ("--test", "--test-reporter=tap", "--test-concurrency=1",
            "tests/runtime-reliability-browser.test.cjs",
            "tests/runtime-reliability-subscription.test.cjs", "tests/runtime-log-subscription.test.cjs",
            "tests/runtime-log-stream.test.cjs", "tests/runtime-recovery.test.cjs",
            "tests/runtime-action-delivery.test.cjs"), ROOT / "apps/desktop", (
                "reliability: repeated workbench lifecycles", "reliability: a retired workbench",
                "reliability: sustained console bursts", "reliability: repeated startup shard events",
                "reliability real ReactDOM StrictMode lifecycle releases subscriptions and bounds the DOM tail",
                "reliability browser startup failure preserves its cause and unverified process-tree scratch",
            ), "node", browser_required=True),
    )
    if full_workspace:
        required = tuple(dict.fromkeys(selector for stage in selected[:3] for selector in stage.required))
        return (Stage("workspace", ("test", "--workspace", "--all-features", "--locked", "--", "--show-output"),
                      ROOT, required, metrics_required=True), selected[-1])
    return selected


def command_for(stage: Stage, cargo_wrapper: Path | None, node_wrapper: Path | None,
                frontend_dist: Path | None = None) -> tuple[list[str], dict[str, str]]:
    environment = os.environ.copy()
    if stage.kind == "node":
        prefix = ["node"]
        if node_wrapper:
            profile = "e2e" if stage.browser_required else "contracts"
            prefix += [str(node_wrapper), "--profile", profile, "--"]
        return [*prefix, *stage.command], environment
    if frontend_dist is not None:
        environment["TAURI_CONFIG"] = tauri_frontend_config(
            environment.get("TAURI_CONFIG"), validate_frontend_dist(frontend_dist)
        )
    if cargo_wrapper is None:
        return ["cargo", *stage.command], environment
    # Arguments travel as JSON data, never as interpolated PowerShell source.
    environment["LGSM_RELIABILITY_CARGO_WRAPPER"] = str(cargo_wrapper)
    environment["LGSM_RELIABILITY_CARGO_ARGUMENTS"] = json.dumps(stage.command[1:])
    shell = shutil.which("pwsh") or shutil.which("powershell")
    if not shell:
        raise ValueError("PowerShell is required for the configured Cargo wrapper")
    invocation = (
        "$ErrorActionPreference = 'Stop'; "
        "$testArguments = @(ConvertFrom-Json $env:LGSM_RELIABILITY_CARGO_ARGUMENTS); "
        "& $env:LGSM_RELIABILITY_CARGO_WRAPPER -Project LanGameServerManager "
        "-CargoCommand test -SourceSnapshot -CargoArguments $testArguments; exit $LASTEXITCODE"
    )
    return [shell, "-NoLogo", "-NoProfile", "-NonInteractive", "-Command", invocation], environment


def resource_evidence_errors(metrics: list, cycles: int | None) -> list[str]:
    if len(metrics) != 1 or not isinstance(metrics[0], dict):
        return ["exactly one resource measurement report is required"]
    report = metrics[0]
    count = report.get("measured_cycles")
    warmup_count = report.get("warmup_cycles")
    if (type(count) is not int or not 4 <= count <= 256 or (cycles is not None and count != cycles)
            or type(warmup_count) is not int or warmup_count < 4 or report.get("launches_per_cycle") != 4):
        return ["invalid resource cycle counts"]
    warmup, measured, limits = report.get("warmup"), report.get("measured"), report.get("limits")
    if (not isinstance(warmup, list) or len(warmup) != warmup_count
            or not isinstance(measured, list) or len(measured) != count or not isinstance(limits, dict)):
        return ["missing or incomplete resource samples"]
    keys = ("handles", "threads", "private_bytes", "owned_output_sinks_remaining")
    if report.get("schema_version") != 2:
        return ["unsupported resource measurement schema"]
    if report.get("reader_thread_exits_verified") != (count + warmup_count) * 4:
        return ["missing exact output-reader termination evidence"]
    for sample in [*warmup, *measured]:
        if not isinstance(sample, dict) or any(type(sample.get(key)) is not int or sample[key] < 0 for key in keys):
            return ["invalid resource measurements"]
    if type(limits.get("handles")) is not int or limits["handles"] < 0 or limits.get("owned_output_sinks_remaining") != 0:
        return ["invalid owned resource limits"]
    if (any(sample["owned_output_sinks_remaining"] != 0 for sample in warmup)
            or any(sample["handles"] > limits["handles"] or sample["owned_output_sinks_remaining"] != 0 for sample in measured)):
        return ["resource measurements exceed the recorded limits"]
    return []


def inspect_result(stage: Stage, output: str, returncode: int, cycles: int | None = None) -> dict:
    if stage.kind == "rust":
        summaries = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", output)
        passed, failed, skipped = (sum(int(row[i]) for row in summaries) for i in range(3))
        names = re.findall(r"^test (\S+) \.\.\. ok\s*$", output, re.MULTILINE)
    else:
        def total(label: str) -> int:
            matches = re.findall(rf"^# {label} (\d+)\s*$", output, re.MULTILINE)
            return int(matches[-1]) if matches else 0
        passed, failed, skipped = total("pass"), total("fail") + total("cancelled"), total("skipped")
        names = [name for name in re.findall(r"^ok \d+ - (.+)$", output, re.MULTILINE)
                 if not re.search(r"#\s*(?:SKIP|TODO)\b", name, re.IGNORECASE)]
    missing = [selector for selector in stage.required if not any(selector in name for name in names)]
    metrics = []
    evidence_errors = []
    for line in output.splitlines():
        if line.startswith("RELIABILITY_METRICS "):
            try:
                metrics.append(json.loads(line.removeprefix("RELIABILITY_METRICS ")))
            except json.JSONDecodeError:
                evidence_errors.append("invalid resource report JSON")
    if stage.metrics_required:
        evidence_errors.extend(resource_evidence_errors(metrics, cycles))
    browser_reports = []
    for payload in re.findall(r"^(?:# )?RELIABILITY_BROWSER (.+)$", output, re.MULTILINE):
        try:
            browser_reports.append(json.loads(payload))
        except json.JSONDecodeError:
            evidence_errors.append("invalid browser report JSON")
    if stage.browser_required:
        evidence_errors.extend(browser_evidence_errors(browser_reports))
    return {
        "status": "passed" if returncode == 0 and passed > 0 and failed == 0 and not missing and not evidence_errors else "failed",
        "exit_code": returncode, "passed": passed, "failed": failed, "skipped": skipped,
        "missing_test_groups": missing, "metrics": metrics, "evidence_errors": evidence_errors,
        "browser_reports": browser_reports,
        "source_snapshots": re.findall(r"Source snapshot captured: .*?input ([0-9a-f]+)", output),
    }


def browser_evidence_errors(reports: list) -> list[str]:
    if len(reports) != 1 or not isinstance(reports[0], dict):
        return ["exactly one real browser lifecycle report is required"]
    report = reports[0]
    expected_zero = ("listeners_remaining", "dom_nodes_after_unmount")
    counts = ("cycles", "renders", "strict_mode_replays", "listeners_created", "listeners_released", "max_tail_lines")
    if (report.get("status") != "passed" or report.get("browser_errors") != []
            or report.get("browser_exited") is not True or report.get("scratch_removed") is not True
            or report.get("browser_processes_remaining") != 0
            or type(report.get("browser_processes_observed")) is not int or report["browser_processes_observed"] < 2
            or any(type(report.get(key)) is not int or report[key] != 0 for key in expected_zero)
            or any(type(report.get(key)) is not int or report[key] <= 0 for key in counts)):
        return ["browser lifecycle or cleanup evidence is incomplete"]
    if (report["listeners_created"] != report["listeners_released"] or report["max_tail_lines"] > 400
            or report["strict_mode_replays"] < report["cycles"] or report["renders"] < report["cycles"] * 3):
        return ["browser lifecycle measurements violate the acceptance bounds"]
    return []


def run_stage(stage: Stage, output_dir: Path, cycles: int,
              cargo_wrapper: Path | None = None, node_wrapper: Path | None = None,
              frontend_dist: Path | None = None) -> dict:
    command, environment = command_for(stage, cargo_wrapper, node_wrapper, frontend_dist)
    environment["LANGAME_RELIABILITY_CYCLES"] = str(cycles)
    log = output_dir / f"{stage.name}.log"
    started = time.monotonic()
    print(f"[reliability] {stage.name}: running; output: {log}", flush=True)
    # Each test owns its deadlines and cleanup. CI or workstation admission owns
    # the outer process-tree deadline; killing only this wrapper would strand it.
    with log.open("w", encoding="utf-8") as destination:
        completed = subprocess.run(command, cwd=stage.cwd, env=environment,
            stdout=destination, stderr=subprocess.STDOUT, check=False,
            **({"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}))
    result = inspect_result(stage, log.read_text(encoding="utf-8", errors="replace"), completed.returncode, cycles)
    result.update(name=stage.name, elapsed_seconds=round(time.monotonic() - started, 3), log=log.name)
    print(f"[reliability] {stage.name}: {result['status']}; "
          f"{result['passed']} passed, {result['failed']} failed, {result['skipped']} skipped", flush=True)
    for metrics in result["metrics"]:
        print("RELIABILITY_METRICS " + json.dumps(metrics), flush=True)
    for browser_report in result["browser_reports"]:
        print("RELIABILITY_BROWSER " + json.dumps(browser_report), flush=True)
    if result["status"] != "passed":
        print(json.dumps({key: result[key] for key in ("exit_code", "missing_test_groups", "evidence_errors")}), flush=True)
        tail = "\n".join(log.read_text(encoding="utf-8", errors="replace").splitlines()[-60:])
        # Windows redirected consoles can use a legacy code page. Preserve the
        # UTF-8 log and escape only unrepresentable console characters so a
        # failed test still returns its result and the caller can write a report.
        console_encoding = getattr(sys.stdout, "encoding", None) or "utf-8"
        print(tail.encode(console_encoding, errors="backslashreplace").decode(console_encoding), flush=True)
    return result


def validate_output(path: Path) -> Path:
    output = path.expanduser().resolve()
    if output == ROOT or ROOT in output.parents:
        raise ValueError("Reliability output must be outside the repository")
    if output.exists():
        raise ValueError("Use a new output directory; previous evidence is never overwritten")
    return output


def validate_frontend_dist(path: Path) -> Path:
    expanded = path.expanduser()
    if not expanded.is_absolute():
        raise ValueError("Frontend distribution must be an absolute path")
    distribution = expanded.resolve(strict=True)
    if distribution == ROOT or ROOT in distribution.parents:
        raise ValueError("Frontend distribution must be outside the repository")
    if not distribution.is_dir() or not (distribution / "index.html").is_file():
        raise ValueError("Frontend distribution must be a directory containing index.html")
    return distribution


def tauri_frontend_config(existing: str | None, frontend_dist: Path) -> str:
    try:
        config = json.loads(existing) if existing is not None else {}
    except (ValueError, TypeError):
        raise ValueError("TAURI_CONFIG must contain a valid JSON object") from None
    if not isinstance(config, dict):
        raise ValueError("TAURI_CONFIG must contain a JSON object")
    build = config.setdefault("build", {})
    if not isinstance(build, dict):
        raise ValueError("TAURI_CONFIG build must contain a JSON object")
    distribution = str(frontend_dist)
    if os.name == "nt" and not distribution.startswith("\\\\?\\"):
        # Tauri's untagged FrontendDist tries URL before PathBuf; a drive-letter
        # path becomes a URL and silently skips embedding. Verbatim paths avoid
        # that ambiguity while remaining absolute inside the source snapshot.
        distribution = (
            "\\\\?\\UNC\\" + distribution[2:]
            if distribution.startswith("\\\\") else "\\\\?\\" + distribution
        )
    build["frontendDist"] = distribution
    return json.dumps(config, ensure_ascii=False, allow_nan=False)


def write_report(output: Path, report: dict) -> None:
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def windows_runtime_supported() -> bool:
    return os.name == "nt"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plan", action="store_true", help="Print the selected checks without executing")
    parser.add_argument("--full-workspace", action="store_true", help="Run the full Rust baseline plus the Web checks (CI)")
    parser.add_argument("--output", type=Path, help="New evidence directory outside the repository")
    parser.add_argument("--cycles", type=int, default=24, help="Isolated process lifecycle cycles (4..256)")
    parser.add_argument("--cargo-wrapper", type=Path, help="Managed workstation invoke-codex-cargo.ps1")
    parser.add_argument("--node-wrapper", type=Path, help="Managed workstation run-guarded-node-command.cjs")
    parser.add_argument("--frontend-dist", type=Path,
                        help="Existing absolute frontend distribution outside the repository for Rust asset embedding")
    args = parser.parse_args(argv)
    if not 4 <= args.cycles <= 256:
        parser.error("--cycles must be between 4 and 256")
    if args.frontend_dist is not None:
        try:
            args.frontend_dist = validate_frontend_dist(args.frontend_dist)
            tauri_frontend_config(os.environ.get("TAURI_CONFIG"), args.frontend_dist)
        except (OSError, ValueError) as error:
            parser.error(str(error))
    if args.plan:
        for stage in stages(args.full_workspace):
            print(f"{stage.name}: {'cargo' if stage.kind == 'rust' else 'node'} {' '.join(stage.command)}")
        return 0
    if not windows_runtime_supported():
        parser.error("The complete runtime acceptance suite requires Windows; other platforms cannot certify its process checks")
    if args.output is None:
        parser.error("--output is required")
    for wrapper in (args.cargo_wrapper, args.node_wrapper):
        if wrapper is not None and not wrapper.is_file():
            parser.error(f"Wrapper is not a file: {wrapper}")
    args.cargo_wrapper = args.cargo_wrapper.resolve() if args.cargo_wrapper else None
    args.node_wrapper = args.node_wrapper.resolve() if args.node_wrapper else None
    output = validate_output(args.output)
    if args.frontend_dist is not None and output.is_relative_to(args.frontend_dist):
        parser.error("Reliability evidence must be outside the frontend distribution to avoid embedding logs")
    output.mkdir(parents=True)
    report = {
        "format": "langame-runtime-reliability-v1", "status": "running",
        "started_at": datetime.now(timezone.utc).isoformat(), "cycles": args.cycles,
        "scope": "synthetic Windows processes, temporary storage, loopback peers and frontend component lifecycles",
        "stages": [],
    }
    if args.frontend_dist is not None:
        report["frontend_dist"] = str(args.frontend_dist)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    status = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True, check=True)
    report.update(git_head=head.stdout.strip(), working_tree_dirty=bool(status.stdout.strip()))
    write_report(output, report)
    try:
        for stage in stages(args.full_workspace):
            result = run_stage(stage, output, args.cycles, args.cargo_wrapper, args.node_wrapper,
                               args.frontend_dist)
            report["stages"].append(result)
            write_report(output, report)
            if result["status"] != "passed":
                report["status"] = "failed"
                break
        else:
            report["status"] = "passed"
    except (OSError, ValueError) as error:
        report.update(status="failed", error=str(error))
    except KeyboardInterrupt:
        report["status"] = "interrupted"
        raise
    finally:
        report["finished_at"] = datetime.now(timezone.utc).isoformat()
        write_report(output, report)
    print(f"[reliability] {report['status']}: {output / 'report.json'}", flush=True)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
