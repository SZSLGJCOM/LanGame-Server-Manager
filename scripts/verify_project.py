"""Shared local and CI checks, with quick feedback and a complete Windows tier."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

if __package__:
    from .verify_runtime_reliability import QUICK_STORAGE_FILTERS, tauri_frontend_config, validate_frontend_dist
    from .verify_scope import (DESKTOP, ROOT, PARALLEL_PACKAGES, browser_repository_files,
                               changed_paths, frontend_selection, frontend_tests, full_scopes,
                               parse_browser_files, parse_rust_packages, publication_full, rust_selection, rust_workspace)
else:
    from verify_runtime_reliability import QUICK_STORAGE_FILTERS, tauri_frontend_config, validate_frontend_dist
    from verify_scope import (DESKTOP, ROOT, PARALLEL_PACKAGES, browser_repository_files,
                              changed_paths, frontend_selection, frontend_tests, full_scopes,
                              parse_browser_files, parse_rust_packages, publication_full, rust_selection, rust_workspace)


def rust_test_matrix(packages: list[str]) -> dict:
    """Isolate slow suites by runner without sharing process-global test state."""
    desktop = "langame-desktop" in packages
    rows = [{"group": group, "shard": 1, "count": 1} for group in ("core", "storage")]
    if set(packages) - {*PARALLEL_PACKAGES, "app-storage", "langame-desktop"}:
        rows.append({"group": "services", "shard": 1, "count": 1})
    if desktop:
        rows.extend({"group": "desktop", "shard": index, "count": 4} for index in range(1, 5))
    for row in rows:
        # Desktop compiles all application dependencies. A quick core-only run
        # must not permanently occupy the immutable key with a partial cache.
        row["cache_writer"] = row["group"] == "desktop" and row["shard"] == 1
    return {"include": rows}

def validate_frontend_output(path: Path) -> Path:
    output = path.expanduser().resolve()
    if output == ROOT or ROOT in output.parents or output in ROOT.parents:
        raise ValueError("--frontend-output must be outside the repository and its ancestors")
    if output.exists():
        raise ValueError("--frontend-output must be a new directory; existing content is never emptied")
    return output


class Runner:
    def __init__(self, args: argparse.Namespace):
        self.args = args
        self.environment = os.environ.copy()
        if args.frontend_dist:
            self.environment["TAURI_CONFIG"] = tauri_frontend_config(
                self.environment.get("TAURI_CONFIG"), validate_frontend_dist(args.frontend_dist))

    def run(self, command: list[str], *, cwd: Path = ROOT) -> None:
        print(f"[verify] {subprocess.list2cmdline(command)}", flush=True)
        if self.args.plan:
            return
        started = time.monotonic()
        result = subprocess.run(command, cwd=cwd, env=self.environment, check=False)
        print(f"[verify] exit {result.returncode}; {time.monotonic() - started:.1f}s", flush=True)
        if result.returncode:
            raise subprocess.CalledProcessError(result.returncode, command)

    def python(self, *arguments: str) -> None:
        self.run([sys.executable, "-B", *arguments])

    def node(self, *arguments: str, browser: bool = False, cwd: Path = ROOT, profile: str | None = None) -> None:
        prefix = ["node"]
        if self.args.node_wrapper:
            prefix += [str(self.args.node_wrapper), "--profile", profile or ("e2e" if browser else "contracts"), "--"]
        self.run([*prefix, *arguments], cwd=cwd)

    def npm(self, *arguments: str) -> None:
        npm = shutil.which("npm.cmd" if os.name == "nt" else "npm") or "npm"
        if self.args.node_wrapper:
            cli = Path(npm).resolve().parent / "node_modules/npm/bin/npm-cli.js"
            if not cli.is_file():
                raise ValueError("The guarded build requires npm's node_modules/npm/bin/npm-cli.js beside npm.cmd")
            self.node(str(cli), "--prefix", str(DESKTOP), *arguments, profile="build")
        else:
            self.run([npm, "--prefix", str(DESKTOP), *arguments])

    def cargo(self, command: str, *arguments: str) -> None:
        if not self.args.cargo_wrapper:
            self.run(["cargo", command, *arguments])
            return
        self.environment["LGSM_VERIFY_CARGO_WRAPPER"] = str(self.args.cargo_wrapper)
        self.environment["LGSM_VERIFY_CARGO_COMMAND"] = command
        self.environment["LGSM_VERIFY_CARGO_ARGUMENTS"] = json.dumps(arguments)
        snapshot = "" if command in ("fmt", "generate-lockfile", "update") else " -SourceSnapshot"
        self.run([shutil.which("pwsh") or "powershell", "-NoLogo", "-NoProfile", "-NonInteractive",
                  "-Command", "$ErrorActionPreference = 'Stop'; "
                  "$checkArguments = @(ConvertFrom-Json $env:LGSM_VERIFY_CARGO_ARGUMENTS); "
                  f"& $env:LGSM_VERIFY_CARGO_WRAPPER -Project LanGameServerManager{snapshot} "
                  "-CargoCommand $env:LGSM_VERIFY_CARGO_COMMAND -CargoArguments $checkArguments; exit $LASTEXITCODE"])

    def publication(self) -> None:
        for script in ("verify_no_tracked_secrets.py", "verify_open_source_boundary.py"):
            self.python(f"scripts/{script}")
        if self.args.tier == "full":
            self.python("scripts/verify_open_source_history.py")

    def policy(self) -> None:
        self.python("-m", "unittest", "discover", "-s", "scripts/tests", "-p", "test_*.py")
        self.node("--test", "scripts/tests/enshrouded_control_package.test.cjs")
        self.publication()
        if self.args.tier == "full":
            self.python("scripts/export_public_snapshot.py", "--verbose")

    def frontend_build(self) -> None:
        self.node("apps/desktop/scripts/generate_npm_third_party_licenses.mjs", "--check")
        self.node("apps/desktop/scripts/audit_i18n_surface.cjs")
        self.node("apps/desktop/scripts/verify_i18n_catalogs.cjs")
        for script, arguments in (
            ("audit_dedicated_settings_pages.py", ("--strict",)),
            ("verify_game_config_provenance.py", ("--check",)),
            ("verify_game_knowledge.py", ("--check",)),
            ("verify_module_setting_coverage.py", ()),
            ("verify_game_config_acceptance.py", ("--require-all", "--check")),
            ("verify_library_media.py", ()),
        ):
            self.python(f"scripts/{script}", *arguments)
        self.node("--test", "scripts/tests/verify_desktop_update_signature.test.cjs")
        if self.args.frontend_dist and not self.args.frontend_output:
            print(f"[verify] Reusing supplied frontend build: {self.args.frontend_dist}", flush=True)
        else:
            if self.args.frontend_output:
                # Vite empties its destination. Recheck immediately before invoking it.
                validate_frontend_output(self.args.frontend_output)
            build_arguments = ("--", "--outDir", str(self.args.frontend_output)) if self.args.frontend_output else ()
            self.npm("run", "build", *build_arguments)
            if self.args.frontend_output and not self.args.plan:
                self.args.frontend_dist = self.args.frontend_output
                self.environment = Runner(self.args).environment

    def frontend_suite(self, browser: bool) -> None:
        files = ([str(Path(path).relative_to("apps/desktop")) for path in self.args.browser_files]
                 if browser and self.args.browser_files is not None else frontend_tests(browser))
        if browser:
            files = files[self.args.shard_index - 1::self.args.shard_count]
        if not files:
            if browser:
                print(f"[verify] browser: 0 test files in shard {self.args.shard_index}/{self.args.shard_count}")
                return
            raise ValueError("The selected frontend test suite is empty")
        print(f"[verify] {'browser' if browser else 'unit/contract'}: {len(files)} test files", flush=True)
        # Windows command lines are bounded; batches preserve the same file suite.
        for offset in range(0, len(files), 80):
            self.node("--test", f"--test-concurrency={1 if browser else 4}", *files[offset:offset + 80],
                      browser=browser, cwd=DESKTOP)

    def rust_checks(self) -> None:
        self.cargo("fmt", "--all", "--", "--check")
        # Clippy performs the workspace compile/type check as well as linting.
        self.cargo("clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings")
        if self.args.cargo_wrapper:
            # The verifier downloads checksum-locked archives itself; the managed
            # Cargo admission intentionally does not expose a fetch command.
            self.python("scripts/generate_rust_third_party_licenses.py", "--check")
        else:
            self.cargo("fetch", "--locked")
            self.python("scripts/generate_rust_third_party_licenses.py", "--check", "--offline")

    def rust_tests(self) -> None:
        selected = getattr(self.args, "rust_packages", None)
        group = getattr(self.args, "rust_group", None)
        if self.args.tier == "quick" and not selected and not group:
            packages = tuple(arg for package in PARALLEL_PACKAGES for arg in ("-p", package))
            self.cargo("test", *packages, "--all-features", "--locked", "--", "--test-threads=4")
            # Keep retained-data regressions in ordinary feedback, even without a risky diff.
            self.cargo("test", "-p", "app-storage", "--lib", "--all-features", "--locked", "--",
                       *QUICK_STORAGE_FILTERS, "--test-threads=1")
        else:
            arguments = ["scripts/verify_runtime_reliability.py", "--full-workspace", "--rust-only",
                         "--output", str(self.args.output / "runtime")]
            if selected is not None and self.args.tier != "full":
                arguments += ["--rust-packages-json", json.dumps(selected)]
            if group:
                arguments += ["--group", group, "--shard-index", str(self.args.shard_index),
                              "--shard-count", str(self.args.shard_count)]
            for name in ("cargo_wrapper", "node_wrapper", "frontend_dist"):
                value = getattr(self.args, name)
                if value:
                    arguments += ["--" + name.replace("_", "-"), str(value)]
            self.python(*arguments)

    def rust(self) -> None:
        self.rust_checks()
        self.rust_tests()

    def native(self) -> None:
        if self.args.native_executable:
            executable = self.args.native_executable
        elif self.args.cargo_wrapper:
            raise ValueError("Managed native acceptance requires --native-executable from its build receipt")
        else:
            self.cargo("build", "-p", "langame-desktop", "--features", "desktop-reliability", "--locked")
            executable = ROOT / "target/debug/langame-desktop.exe"
        self.node("apps/desktop/scripts/verify_desktop_reliability.cjs", "--executable", str(executable),
                  "--output", str(self.args.output / "desktop/report.json"), browser=True)
        if self.args.nsis_root:
            self.run([shutil.which("pwsh") or "powershell", "-NoProfile", "-File",
                      "scripts/tests/test_desktop_installer_guard.ps1", "-NsisRoot", str(self.args.nsis_root)])
        else:
            print("[verify] NSIS guard fixture not selected; supply --nsis-root. Real signed installation, "
                  "upgrade and retained-data acceptance remain release requirements.", flush=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tier", choices=("quick", "full"), default="quick")
    parser.add_argument("--component", choices=("all", "policy", "frontend", "frontend-build",
                        "frontend-tests", "frontend-browser", "rust", "rust-checks", "rust-tests",
                        "native", "scope"), default="all")
    parser.add_argument("--plan", action="store_true")
    parser.add_argument("--base", help="Commit to compare for CI risk selection")
    parser.add_argument("--event", help="CI event; manual dispatch honors an explicitly selected quick tier")
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--output", type=Path, help="New evidence root outside the repository")
    parser.add_argument("--cargo-wrapper", type=Path)
    parser.add_argument("--node-wrapper", type=Path)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--frontend-output", type=Path, help="External frontend build destination")
    parser.add_argument("--native-executable", type=Path)
    parser.add_argument("--nsis-root", type=Path)
    parser.add_argument("--shard-index", type=int, default=1)
    parser.add_argument("--shard-count", type=int, default=1)
    parser.add_argument("--browser-files-json", help="JSON list of repository-relative browser tests selected by scope")
    parser.add_argument("--rust-packages-json", help="Complete selected packages in addition to the quick baseline")
    parser.add_argument("--rust-group", choices=("core", "storage", "services", "desktop"),
                        help="Run one isolated CI group using the same local verification rules")
    args = parser.parse_args(argv)
    if not 1 <= args.shard_index <= args.shard_count <= 8:
        parser.error("Require 1 <= shard-index <= shard-count <= 8")
    if args.rust_group and args.component != "rust-tests":
        parser.error("--rust-group requires --component rust-tests")
    if args.rust_group and args.rust_group != "desktop" and args.shard_count != 1:
        parser.error("Only the desktop Rust group supports sharding")
    if args.rust_group == "desktop" and args.shard_count > 4:
        parser.error("The desktop Rust group supports at most four shards")
    if args.rust_group and args.rust_packages_json is None and not args.base and args.tier == "quick":
        args.rust_packages_json = "[]"
    if args.base and args.browser_files_json is not None:
        parser.error("--base and --browser-files-json cannot be combined")
    if args.rust_packages_json is not None and (args.base or args.tier == "full"):
        parser.error("--rust-packages-json cannot be combined with --base or the complete full tier")
    try:
        args.browser_files = (parse_browser_files(args.browser_files_json, full=args.tier == "full")
                              if args.browser_files_json is not None else None)
        args.rust_packages = parse_rust_packages(args.rust_packages_json) if args.rust_packages_json is not None else None
    except ValueError as error:
        parser.error(str(error))
    if args.component == "scope":
        full = args.tier == "full" or args.event == "schedule"
        manual = args.event == "workflow_dispatch"
        full = full or not manual and (not args.base or set(args.base) == {"0"})
        frontend_full = rust_full = native_full = full
        paths = []
        if not full and not manual:
            paths = changed_paths(args.base)
            full = paths is None
            paths = paths or []
            frontend_full, rust_full, native_full = (True, True, True) if full else full_scopes(paths)
        policy_full = full or publication_full(paths)
        full = frontend_full or rust_full or native_full or policy_full
        frontend_files = browser_repository_files() if frontend_full else frontend_selection(paths)
        rust_packages = (sorted(rust_workspace()) if rust_full and not paths else rust_selection(paths))
        frontend_changed = bool(frontend_files)
        print(f"[verify] frontend_full={frontend_full}; rust_full={rust_full}; native_full={native_full}; "
              f"frontend_changed={frontend_changed}; {len(frontend_files)} browser files; {len(paths)} changed paths; "
              f"complete Rust packages={','.join(rust_packages) or 'quick baseline'}")
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as destination:
                for name, value in (("full", full), ("frontend_full", frontend_full), ("rust_full", rust_full),
                                    ("native_full", native_full), ("frontend_changed", frontend_changed)):
                    destination.write(f"{name}={str(value).lower()}\n")
                destination.write(f"frontend_files={json.dumps(frontend_files, separators=(',', ':'))}\n")
                destination.write(f"rust_packages={json.dumps(rust_packages, separators=(',', ':'))}\n")
                destination.write(f"policy_full={str(policy_full).lower()}\n")
                destination.write(f"rust_matrix={json.dumps(rust_test_matrix(rust_packages), separators=(',', ':'))}\n")
        return 0
    if args.component == "frontend-browser" and args.base and args.tier != "full":
        paths = changed_paths(args.base)
        args.browser_files = browser_repository_files() if paths is None else frontend_selection(paths)
    if args.component in ("all", "rust", "rust-tests") and args.base and args.tier != "full":
        paths = changed_paths(args.base)
        args.rust_packages = sorted(rust_workspace()) if paths is None else rust_selection(paths)
    requires_output = args.component == "native" or (
        (args.tier == "full" or args.rust_packages or args.rust_group) and args.component in ("all", "rust", "rust-tests"))
    if requires_output and not args.output:
        parser.error("--output is required for full Rust/native checks")
    if args.output:
        args.output = args.output.expanduser().resolve()
        if args.output == ROOT or ROOT in args.output.parents:
            parser.error("--output must be outside the repository")
    if args.frontend_output:
        try:
            args.frontend_output = validate_frontend_output(args.frontend_output)
        except ValueError as error:
            parser.error(str(error))
    for name in ("cargo_wrapper", "node_wrapper", "native_executable"):
        value = getattr(args, name)
        if value:
            if not value.is_file():
                parser.error(f"--{name.replace('_', '-')} must be an existing file")
            setattr(args, name, value.resolve())
    try:
        runner = Runner(args)
        if args.component in ("all", "policy"):
            runner.policy()
        if args.component == "frontend":
            runner.publication()
        if args.component in ("all", "frontend", "frontend-build"):
            runner.frontend_build()
        if args.component in ("all", "frontend", "frontend-tests"):
            runner.frontend_suite(False)
            if args.tier == "full":
                runner.frontend_suite(True)
        if args.component == "frontend-browser":
            runner.frontend_suite(True)
        if args.component in ("all", "rust"):
            runner.rust()
        if args.component == "rust-checks":
            runner.rust_checks()
        if args.component == "rust-tests":
            runner.rust_tests()
        if args.component == "native" or (args.component == "all" and args.tier == "full"):
            runner.native()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"[verify] failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
