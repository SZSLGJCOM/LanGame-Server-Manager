"""Run isolated runtime reliability checks and retain their actual test evidence."""

from __future__ import annotations

import argparse
import codecs
from dataclasses import dataclass
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import threading
import time

if __package__:
    from .verify_scope import PARALLEL_PACKAGES, parse_rust_packages, rust_workspace
else:
    from verify_scope import PARALLEL_PACKAGES, parse_rust_packages, rust_workspace

ROOT = Path(__file__).resolve().parents[1]
LOG_PROGRESS_INTERVAL_SECONDS = 10
LOG_PROGRESS_MAX_BYTES = 64 * 1024
QUICK_STORAGE_FILTERS = ("backups", "migration", "instance_creation", "instance_file_patch",
                         "instance_details_tests", "combined_runtime_read")


@dataclass(frozen=True)
class Stage:
    name: str
    command: tuple[str, ...]
    cwd: Path
    required: tuple[str, ...]
    kind: str = "rust"
    metrics_required: bool = False
    browser_required: bool = False
    exact_tests: tuple[str, ...] = ()
    allow_empty: bool = False


def rust_arguments(package: str, target: tuple[str, ...], filters: tuple[str, ...]) -> tuple[str, ...]:
    return ("test", "-p", package, *target, "--all-features", "--locked", "--",
            *filters, "--test-threads=1", "--show-output")


def stages(full_workspace: bool = False, include_web: bool = True,
           packages: list[str] | None = None) -> tuple[Stage, ...]:
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
        evidence = [stage for package, stage in (("app-runtime", selected[0]), ("langame-desktop", selected[2]))
                    if packages is None or package in packages]
        required = tuple(dict.fromkeys(selector for stage in evidence for selector in stage.required))
        core = tuple(argument for package in PARALLEL_PACKAGES for argument in ("-p", package))
        exclusions = tuple(argument for package in (*PARALLEL_PACKAGES, "app-storage") for argument in ("--exclude", package))
        stateful = None if packages is None else sorted(set(packages) - {*PARALLEL_PACKAGES, "app-storage"})
        workspace = (("--workspace", *exclusions) if stateful is None else
                     tuple(argument for package in stateful for argument in ("-p", package)))
        storage_full = packages is None or "app-storage" in packages
        storage = (("--all-features", "--locked", "--", "--test-threads=2", "--show-output") if storage_full else
                   ("--lib", "--all-features", "--locked", "--", *QUICK_STORAGE_FILTERS,
                    "--test-threads=1", "--show-output"))
        # Storage fixtures own separate roots; bound I/O concurrency to two tests.
        # Process and desktop suites retain their process-wide isolation.
        return (
            Stage("core", ("test", *core, "--all-features", "--locked", "--",
                           "--test-threads=4", "--show-output"), ROOT, ()),
            Stage("storage", ("test", "-p", "app-storage", *storage), ROOT, selected[1].required if storage_full else ()),
            *((Stage("workspace", ("test", *workspace, "--all-features", "--locked", "--",
                                   "--test-threads=1", "--show-output"), ROOT, required,
                      metrics_required=packages is None or "app-runtime" in packages),) if workspace else ()),
            *((selected[-1],) if include_web else ()),
        )
    return selected if include_web else selected[:-1]


def grouped_stages(group: str, packages: list[str] | None) -> tuple[Stage, ...]:
    if group in ("core", "storage"):
        return tuple(stage for stage in stages(True, False, packages) if stage.name == group)
    if group == "desktop":
        return ()  # Enumerated after compilation, before any test is executed.
    selected = sorted(set(rust_workspace() if packages is None else packages)
                      - {*PARALLEL_PACKAGES, "app-storage", "langame-desktop"})
    if not selected:
        return ()
    process_evidence = stages()[0]
    runtime = "app-runtime" in selected
    arguments = tuple(argument for package in selected for argument in ("-p", package))
    return (Stage("services", ("test", *arguments, "--all-features", "--locked", "--",
                              "--test-threads=1", "--show-output"), ROOT,
                  process_evidence.required if runtime else (), metrics_required=runtime),)


def desktop_target_selector(target: dict) -> tuple[str, ...]:
    kind = target["kind"]
    selector = {"lib": ("--lib",), "bin": ("--bin", target["name"]),
                "test": ("--test", target["name"]), "example": ("--example", target["name"])}
    if len(kind) != 1 or kind[0] not in selector:
        raise ValueError(f"Unsupported desktop test target: {target['name']} ({kind})")
    return selector[kind[0]]


def desktop_metadata_targets(output: str) -> tuple[list[tuple[str, ...]], bool]:
    messages = [json.loads(line) for line in output.splitlines() if line.startswith("{")]
    metadata = [message for message in messages if "packages" in message and "workspace_members" in message]
    if len(metadata) != 1:
        raise ValueError("Cargo did not report one workspace metadata document")
    packages = [package for package in metadata[0]["packages"] if package["name"] == "langame-desktop"
                and package["id"] in metadata[0]["workspace_members"]]
    if len(packages) != 1:
        raise ValueError("Cargo metadata does not identify the desktop workspace package")
    package = packages[0]
    targets, doctests = set(), False
    for target in package["targets"]:
        # --all-features enables every declared feature, but Cargo still skips
        # targets requiring a feature that the package does not provide.
        if not set(target.get("required-features", [])) <= set(package["features"]):
            continue
        if "lib" in target["kind"] and target["doctest"]:
            doctests = True
        if target["test"]:
            targets.add(desktop_target_selector(target))
    if not targets:
        raise ValueError("Cargo metadata has no enabled desktop test targets")
    return sorted(targets), doctests


def desktop_targets(output: str) -> tuple[list[tuple[str, ...]], bool]:
    """Cargo JSON is interleaved with build diagnostics and managed-wrapper logs."""
    targets = set()
    doctests = False
    finished = False
    for line in output.splitlines():
        if not line.startswith("{"):
            continue
        message = json.loads(line)
        if message.get("reason") == "build-finished":
            finished = message.get("success") is True
        if (message.get("reason") != "compiler-artifact" or
                not re.search(r"(?:^|[# ])langame-desktop(?:@| )", message.get("package_id", ""))):
            continue
        target = message["target"]
        kind = target["kind"]
        if "lib" in kind and target.get("doctest", True):
            doctests = True
        if not message.get("profile", {}).get("test") or not message.get("executable"):
            continue
        targets.add(desktop_target_selector(target))
    if not finished or not targets:
        raise ValueError("Cargo did not report a successful desktop test build with executable targets")
    return sorted(targets), doctests


def listed_tests(output: str) -> list[str]:
    names = re.findall(r"^(\S+): test\s*$", output, re.MULTILINE)
    summaries = re.findall(r"^(\d+) tests?, (\d+) benchmarks?\s*$", output, re.MULTILINE)
    if (summaries != [(str(len(names)), "0")] or len(names) != len(set(names))
            or re.search(r"^\S+: benchmark\s*$", output, re.MULTILINE)):
        raise ValueError("Desktop inventory is incomplete, duplicated or contains unsupported benchmarks")
    return sorted(names)


def desktop_partition(inventory: dict[tuple[str, ...], list[str]], index: int, count: int
                      ) -> dict[tuple[str, ...], list[str]]:
    if not 1 <= index <= count <= 4:
        raise ValueError("Desktop shard requires 1 <= index <= count <= 4")
    # Round-robin sorted names spreads each large test family across independent
    # runners while keeping every process serial and every assignment repeatable.
    ordered = sorted((target, name) for target, names in inventory.items() for name in names)
    selected: dict[tuple[str, ...], list[str]] = {}
    for target, name in ordered[index - 1::count]:
        selected.setdefault(target, []).append(name)
    return selected


def desktop_batches(target: tuple[str, ...], names: list[str], limit: int = 24000,
                    *, managed: bool = False) -> list[tuple[str, ...]]:
    prefix = ("test", "-p", "langame-desktop", *target, "--all-features", "--locked", "--",
              "--exact", "--test-threads=1", "--show-output", "--format=pretty")
    batches = []
    batch: list[str] = []

    def command_size(arguments: tuple[str, ...]) -> int:
        native = len(subprocess.list2cmdline(["cargo", *arguments]).encode("utf-16-le")) // 2
        if not managed:
            return native
        # The managed guard encodes JSON as UTF-8/Base64. ASCII escaping bounds
        # Unicode expansion too; 16 KiB JSON leaves >10 KiB for its other fields,
        # paths and PowerShell arguments within the 32767-character limit.
        encoded = len(json.dumps(arguments, ensure_ascii=True).encode("utf-8"))
        return max(native, encoded)

    if managed:
        limit = min(limit, 16000)
    for name in names:
        # Leave headroom below CreateProcess's 32767 UTF-16-unit command limit.
        length = command_size((*prefix, *batch, name))
        if batch and length > limit:
            batches.append(tuple(batch))
            batch = []
        if command_size((*prefix, name)) > limit:
            raise ValueError(f"Desktop test name exceeds the command length budget: {name}")
        batch.append(name)
    if batch:
        batches.append(tuple(batch))
    return [(*prefix, *batch) for batch in batches]


def prepare_desktop(args, packages: list[str] | None, output: Path, report: dict) -> tuple[Stage, ...]:
    if packages is not None and "langame-desktop" not in packages:
        report["empty_reason"] = "langame-desktop is outside the selected complete packages"
        return ()

    def capture(name: str, command: tuple[str, ...]) -> str:
        text, result = capture_stage(Stage(name, command, ROOT, ()), output, args.cycles,
                                     args.cargo_wrapper, args.node_wrapper, args.frontend_dist)
        report["stages"].append(result)
        write_report(output, report)
        if result["exit_code"] != 0:
            raise ValueError(f"{name} failed; see {result['log']}")
        return text

    prefix = ("test", "-p", "langame-desktop", "--all-features", "--locked")
    if args.cargo_wrapper is None:
        targets, doctests = desktop_metadata_targets(capture("desktop-metadata", (
            "metadata", "--no-deps", "--all-features", "--locked", "--format-version", "1")))
    else:
        # The managed workstation wrapper admits Cargo test, but not metadata.
        # Keep its supported discovery path; CI avoids the redundant prebuild.
        targets, doctests = desktop_targets(capture("desktop-build", (*prefix, "--no-run", "--message-format=json")))
    inventory, ignored = {}, {}
    for number, target in enumerate(targets, 1):
        # Pretty listing adds a count footer, including for truly empty targets.
        listing = (*prefix, *target, "--", "--list", "--format=pretty")
        names = listed_tests(capture(f"desktop-list-{number}", listing))
        skipped = listed_tests(capture(f"desktop-ignored-{number}", (*listing, "--ignored")))
        if not set(skipped) <= set(names):
            raise ValueError("Ignored desktop tests are absent from the complete inventory")
        inventory[target] = sorted(set(names) - set(skipped))
        ignored[target] = skipped
    missing = [selector for selector in stages()[2].required
               if not any(selector in name for names in inventory.values() for name in names)]
    if missing:
        raise ValueError(f"Desktop inventory is missing required test groups: {missing}")
    selected = desktop_partition(inventory, args.shard_index, args.shard_count)
    report["desktop_partition"] = {
        "index": args.shard_index, "count": args.shard_count, "doctests": doctests and args.shard_index == 1,
        "targets": [{"target": list(target), "tests": inventory[target], "ignored": ignored[target],
                     "selected": selected.get(target, [])} for target in targets],
    }
    checks = []
    for target, names in selected.items():
        for command in desktop_batches(target, names, managed=args.cargo_wrapper is not None):
            exact = command[command.index("--format=pretty") + 1:]
            checks.append(Stage(f"desktop-batch-{len(checks) + 1}", command, ROOT, (), exact_tests=exact))
    if doctests and args.shard_index == 1:
        checks.append(Stage("desktop-doctests", (*prefix, "--doc", "--", "--test-threads=1", "--show-output"),
                            ROOT, (), allow_empty=True))
    return tuple(checks)


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
    summaries = []
    if stage.kind == "rust":
        summary_matches = list(re.finditer(r"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored", output))
        summaries = [match.groups() for match in summary_matches]
        names = re.findall(r"^test (\S+)(?: - should panic)? \.\.\. ok\s*$", output, re.MULTILINE)
        if stage.exact_tests:
            # Child stdout can interrupt the parent's progress line and contain
            # its own summaries. --show-output ends with the outer success list.
            summaries = summaries[-1:]
            before_summary = output[:summary_matches[-1].start()] if summary_matches else ""
            successes = re.search(r"(?:^|\n)successes:\r?\n((?: {4}\S+\r?\n)+)\s*\Z", before_summary)
            names = successes[1].split() if successes else []
        passed, failed, skipped = (sum(int(row[i]) for row in summaries) for i in range(3))
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
    if stage.exact_tests and (sorted(stage.exact_tests) != sorted(names) or passed != len(stage.exact_tests) or skipped):
        evidence_errors.append("Completed tests do not match the exact shard batch")
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
        "status": "passed" if returncode == 0 and (passed > 0 or stage.allow_empty and bool(summaries))
                  and failed == 0 and not missing and not evidence_errors else "failed",
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


def console_text(text: str) -> str:
    # Keep the UTF-8 evidence intact when a redirected Windows console uses a legacy code page.
    encoding = getattr(sys.stdout, "encoding", None) or "utf-8"
    return text.encode(encoding, errors="backslashreplace").decode(encoding)


class _StageLogProgress:
    def __init__(self, name: str, log: Path, started: float):
        self.name = name
        self.log = log
        self.started = started
        self.offset = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")(errors="replace")
        self.stop = threading.Event()
        self.error: Exception | None = None
        self.thread = threading.Thread(target=self.observe, name=f"reliability-log-{name}")

    def emit(self, *, final: bool = False) -> None:
        with self.log.open("rb") as source:
            size = source.seek(0, os.SEEK_END)
            if size < self.offset:
                raise OSError("The stage log was truncated during observation")
            omitted = max(0, size - self.offset - LOG_PROGRESS_MAX_BYTES)
            if omitted:
                self.offset += omitted
                self.decoder.reset()
            source.seek(self.offset)
            payload = source.read(min(size - self.offset, LOG_PROGRESS_MAX_BYTES))
        self.offset += len(payload)
        text = self.decoder.decode(payload, final=final)
        print(f"[reliability] {self.name}: elapsed {time.monotonic() - self.started:.1f}s; "
              f"log bytes {self.offset}/{size}" +
              (f"; {omitted} bytes omitted from console; complete output remains in {self.log.name}" if omitted else ""),
              flush=True)
        if text:
            print(console_text(text), end="" if text.endswith("\n") else "\n", flush=True)

    def observe(self) -> None:
        try:
            while not self.stop.wait(LOG_PROGRESS_INTERVAL_SECONDS):
                self.emit()
            self.emit(final=True)
        except Exception as error:
            self.error = error

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, exception_type, _exception, _traceback) -> None:
        self.stop.set()
        self.thread.join()
        if self.error is not None and exception_type is None:
            raise OSError(f"Stage log observation failed: {self.error}") from self.error


def capture_stage(stage: Stage, output_dir: Path, cycles: int,
                  cargo_wrapper: Path | None = None, node_wrapper: Path | None = None,
                  frontend_dist: Path | None = None) -> tuple[str, dict]:
    command, environment = command_for(stage, cargo_wrapper, node_wrapper, frontend_dist)
    environment["LANGAME_RELIABILITY_CYCLES"] = str(cycles)
    log = output_dir / f"{stage.name}.log"
    started = time.monotonic()
    print(f"[reliability] {stage.name}: running; output: {log}", flush=True)
    # Each test owns its deadlines and cleanup. CI or workstation admission owns
    # the outer process-tree deadline; killing only this wrapper would strand it.
    with log.open("w", encoding="utf-8") as destination:
        with _StageLogProgress(stage.name, log, started):
            try:
                completed = subprocess.run(command, cwd=stage.cwd, env=environment,
                    stdout=destination, stderr=subprocess.STDOUT, check=False,
                    **({"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}))
            finally:
                destination.flush()
    return log.read_text(encoding="utf-8", errors="replace"), {
        "name": stage.name, "status": "passed" if completed.returncode == 0 else "failed",
        "exit_code": completed.returncode, "elapsed_seconds": round(time.monotonic() - started, 3), "log": log.name,
    }


def run_stage(stage: Stage, output_dir: Path, cycles: int,
              cargo_wrapper: Path | None = None, node_wrapper: Path | None = None,
              frontend_dist: Path | None = None) -> dict:
    text, execution = capture_stage(stage, output_dir, cycles, cargo_wrapper, node_wrapper, frontend_dist)
    result = {**execution, **inspect_result(stage, text, execution["exit_code"], cycles)}
    print(f"[reliability] {stage.name}: {result['status']}; "
          f"{result['passed']} passed, {result['failed']} failed, {result['skipped']} skipped", flush=True)
    for metrics in result["metrics"]:
        print("RELIABILITY_METRICS " + json.dumps(metrics), flush=True)
    for browser_report in result["browser_reports"]:
        print("RELIABILITY_BROWSER " + json.dumps(browser_report), flush=True)
    if result["status"] != "passed":
        print(json.dumps({key: result[key] for key in ("exit_code", "missing_test_groups", "evidence_errors")}), flush=True)
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
    parser.add_argument("--rust-only", action="store_true", help="Run Rust only when the shared frontend tier owns Web checks")
    parser.add_argument("--rust-packages-json", help="Complete affected packages, retaining the quick baseline")
    parser.add_argument("--group", choices=("core", "storage", "services", "desktop"),
                        help="One independently runnable Rust group; requires --full-workspace --rust-only")
    parser.add_argument("--shard-index", type=int, default=1, help="Desktop shard index (1-based)")
    parser.add_argument("--shard-count", type=int, default=1, help="Independent desktop runners (1..4)")
    parser.add_argument("--output", type=Path, help="New evidence directory outside the repository")
    parser.add_argument("--cycles", type=int, default=24, help="Isolated process lifecycle cycles (4..256)")
    parser.add_argument("--cargo-wrapper", type=Path, help="Managed workstation invoke-codex-cargo.ps1")
    parser.add_argument("--node-wrapper", type=Path, help="Managed workstation run-guarded-node-command.cjs")
    parser.add_argument("--frontend-dist", type=Path,
                        help="Existing absolute frontend distribution outside the repository for Rust asset embedding")
    args = parser.parse_args(argv)
    try:
        packages = parse_rust_packages(args.rust_packages_json) if args.rust_packages_json is not None else None
    except (ValueError, OSError) as error:
        parser.error(str(error))
    if packages is not None and not args.full_workspace:
        parser.error("--rust-packages-json requires --full-workspace")
    if args.group and not (args.full_workspace and args.rust_only):
        parser.error("--group requires --full-workspace --rust-only")
    if not 1 <= args.shard_index <= args.shard_count <= 4:
        parser.error("Shards require 1 <= --shard-index <= --shard-count <= 4")
    if args.group != "desktop" and (args.shard_index != 1 or args.shard_count != 1):
        parser.error("Shards require --group desktop")
    if not 4 <= args.cycles <= 256:
        parser.error("--cycles must be between 4 and 256")
    if args.frontend_dist is not None:
        try:
            args.frontend_dist = validate_frontend_dist(args.frontend_dist)
            tauri_frontend_config(os.environ.get("TAURI_CONFIG"), args.frontend_dist)
        except (OSError, ValueError) as error:
            parser.error(str(error))
    if args.plan:
        planned = (grouped_stages(args.group, packages) if args.group else
                   stages(args.full_workspace, not args.rust_only, packages))
        if args.group == "desktop" and (packages is None or "langame-desktop" in packages):
            print("desktop-build: cargo test -p langame-desktop --all-features --locked --no-run --message-format=json"
                  if args.cargo_wrapper else "desktop-metadata: cargo metadata --no-deps --all-features --locked --format-version 1")
            print(f"desktop: enumerate targets; exact serial batches for shard {args.shard_index}/{args.shard_count}; "
                  "library doctests on shard 1")
        elif not planned:
            print(f"{args.group}: empty; no complete packages selected for this group")
        for stage in planned:
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
        "scope": "synthetic Windows processes, temporary storage and loopback peers" +
                 ("" if args.rust_only else ", and frontend component lifecycles"),
        "stages": [],
    }
    if packages is not None:
        report["selected_complete_packages"] = packages
    if args.group:
        report.update(group=args.group, shard_index=args.shard_index, shard_count=args.shard_count)
    if args.frontend_dist is not None:
        report["frontend_dist"] = str(args.frontend_dist)
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    status = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True, check=True)
    report.update(git_head=head.stdout.strip(), working_tree_dirty=bool(status.stdout.strip()))
    write_report(output, report)
    try:
        if args.group == "desktop":
            checks = prepare_desktop(args, packages, output, report)
        else:
            checks = (grouped_stages(args.group, packages) if args.group else
                      stages(args.full_workspace, not args.rust_only, packages))
        if not checks:
            report.setdefault("empty_reason", "No tests assigned to this group or shard")
            print(f"[reliability] empty: {report['empty_reason']}", flush=True)
        for stage in checks:
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
