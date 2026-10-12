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
    from .verify_runtime_reliability import PARALLEL_PACKAGES, tauri_frontend_config, validate_frontend_dist
else:
    from verify_runtime_reliability import PARALLEL_PACKAGES, tauri_frontend_config, validate_frontend_dist

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps/desktop"
# These predate the *-browser.test.cjs convention. Runner unit tests remain quick.
BROWSER_EXCEPTIONS = {"knowledge-settings.test.cjs", "runtime-browser-navigation.test.cjs"}
FULL_RUST_PACKAGES = {"app-core", "app-network", "app-platform-win", "app-runtime", "app-steamcmd", "app-storage"}
KNOWN_RUST_PACKAGES = FULL_RUST_PACKAGES | set(PARALLEL_PACKAGES)
RUST_RISK_PREFIXES = tuple(f"crates/{package}/" for package in sorted(FULL_RUST_PACKAGES)) + (
    "apps/desktop/src-tauri/", "modules/", "migrations/",
    ".cargo/", "apps/desktop/src/api",
)
FRONTEND_FULL_PREFIXES = (
    "apps/desktop/src-tauri/", "apps/desktop/scripts/",
    "crates/app-core/", "modules/", "migrations/",
)
NATIVE_RISK_PREFIXES = (
    "apps/desktop/src-tauri/", "crates/app-runtime/", "crates/app-platform-win/",
    "crates/app-core/", "crates/app-storage/", ".cargo/",
    "apps/desktop/tests/helpers/desktop-reliability.",
)
NATIVE_RISK_FILES = {"apps/desktop/scripts/verify_desktop_reliability.cjs",
                     "apps/desktop/tests/desktop-reliability-driver.test.cjs",
                     "scripts/tests/test_desktop_installer_guard.ps1"}
SHARED_RISK_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "apps/desktop/package.json",
                     "apps/desktop/package-lock.json", "scripts/verify_project.py",
                     "scripts/verify_runtime_reliability.py", "scripts/build_desktop_update_artifacts.ps1",
                     "crates/app-knowledge/runtime_build.rs", ".github/workflows/ci.yml"}

QUICK_BROWSER_FILES = tuple(f"apps/desktop/tests/{name}-browser.test.cjs" for name in (
    "instance-create", "server-detail-navigation", "restored-configuration", "configuration-controls",
    "maintenance-world-save", "desktop-exit", "runtime-reliability", "lan-transport",
))
# Existing behavioral families, not one rule per test. Overlapping families are intentional.
BROWSER_GROUPS = {
    "configuration": ("configuration-", "restored-", "dragonwilds-", "dst-", "enshrouded-", "moria-",
                      "romestead-", "satisfactory-", "scum-", "valheim-", "ark-", "asa-"),
    "mods": ("mod-", "manual-mod-", "workshop-", "asa-", "ark-", "dst-", "moria-", "server-archives-"),
    "players": ("player-", "manual-player-", "seven-days-", "enshrouded-", "moria-", "system-player-", "server-archives-"),
    "maintenance": ("maintenance-", "server-archives-", "server-inline-", "instance-retirement-",
                    "program-cleanup-", "program-storage-", "scum-", "ark-cluster-", "moria-", "gm-tool-"),
    "runtime": ("runtime-", "desktop-exit-", "instance-unavailable-", "server-card-", "server-tools-",
                "operation-notices-", "activity-notices-", "server-archives-", "ark-cluster-maps-", "dst-four-shards-",
                "instance-retirement-", "desktop-layout-"),
    "updates": ("app-update-", "program-update-", "library-updates-", "steamcmd-dependency-"),
    "system": ("system-", "desktop-main-pages-", "desktop-layout-", "instance-retirement-", "app-update-"),
    "library": ("library-", "instance-create-", "instance-inventory-", "steamcmd-dependency-",
                "desktop-main-pages-", "desktop-layout-"),
    "assistant": ("assistant-", "knowledge-", "privacy-disclosure-"),
}
# These subtrees have bounded consumers. Shared settings infrastructure, shell,
# API contracts, hooks and unclassified sources deliberately retain full coverage.
FRONTEND_FEATURE_PATHS = {
    "views/system/": ("system",), "views/SystemView.tsx": ("system",), "views/SystemView.css": ("system",),
    "views/servers/player-center/": ("players",), "views/servers/PlayerCenterWorkbench.tsx": ("players",),
    "views/servers/ModWorkbench.tsx": ("mods",),
    "views/servers/RuntimeSurfaceWorkbench.tsx": ("runtime",),
    "views/servers/BackupTable.tsx": ("maintenance",),
    "views/servers/ImmediateWorldSave.tsx": ("maintenance",),
    "views/servers/useInstanceArchives.ts": ("maintenance",),
    "views/servers/ProgramUpdatePolicyEditor.tsx": ("updates", "maintenance"),
    "views/servers/program-update-policy.ts": ("updates", "maintenance"),
}


def is_browser_test(path: Path) -> bool:
    return path.name.endswith("-browser.test.cjs") or path.name in BROWSER_EXCEPTIONS


def shared_risk(paths: list[str]) -> bool:
    return any(path in SHARED_RISK_FILES or path.startswith(".github/actions/")
               or path.startswith("crates/") and Path(path).name in ("Cargo.toml", "build.rs") for path in paths)


def full_scopes(paths: list[str]) -> tuple[bool, bool, bool]:
    shared = shared_risk(paths)
    # Quick already runs every test in app-knowledge and app-modules. New crates
    # remain conservative until their execution and integration boundaries are known.
    rust = shared or any(path.startswith(RUST_RISK_PREFIXES)
                         or path.startswith("crates/") and path.split("/")[1] not in KNOWN_RUST_PACKAGES
                         for path in paths)
    frontend = frontend_selection(paths) == browser_repository_files()
    native = shared or any(path in NATIVE_RISK_FILES or path.startswith(NATIVE_RISK_PREFIXES) for path in paths)
    return frontend, rust, native


def needs_full(paths: list[str]) -> bool:
    return any(full_scopes(paths))


def frontend_tests(browser: bool) -> list[str]:
    return [str(path.relative_to(DESKTOP)) for path in sorted((DESKTOP / "tests").glob("*.test.cjs"))
            if is_browser_test(path) == browser]


def browser_repository_files() -> list[str]:
    return [f"apps/desktop/{Path(path).as_posix()}" for path in frontend_tests(True)]


def private_fixture_owners(path: str, suite: list[str]) -> list[str]:
    fixture = ROOT / path
    if fixture.suffix not in (".html", ".tsx") or not fixture.is_file():
        return []
    html = fixture.with_suffix(".html")
    if not html.is_file():
        return []
    if fixture.suffix == ".tsx":
        # A module imported by another fixture is shared, even if it has its own HTML.
        for peer in fixture.parent.iterdir():
            if peer.is_file() and peer.stem != fixture.stem and peer.suffix in (".ts", ".tsx", ".html", ".cjs"):
                if fixture.stem in peer.read_text(encoding="utf-8"):
                    return []
    return [test for test in suite if html.name in (ROOT / test).read_text(encoding="utf-8")]


def frontend_selection(paths: list[str]) -> list[str]:
    suite = browser_repository_files()
    if shared_risk(paths):
        return suite
    selected: set[str] = set()
    groups: set[str] = set()
    for path in paths:
        if path.startswith(FRONTEND_FULL_PREFIXES) or (
                path.startswith("apps/desktop/") and Path(path).name.startswith(("vite.config", "tsconfig"))):
            return suite
        if path.startswith("apps/desktop/src/"):
            relative = path.removeprefix("apps/desktop/src/")
            matched = [group for prefix, owners in FRONTEND_FEATURE_PATHS.items()
                       if prefix.endswith("/") and relative.startswith(prefix) for group in owners]
            matched += list(FRONTEND_FEATURE_PATHS.get(relative, ()))
            if not matched:
                return suite
            groups.update(matched)
        elif path.startswith("apps/desktop/tests/helpers/"):
            owners = private_fixture_owners(path, suite)
            if not owners:
                return suite
            selected.update(owners)
        elif path.startswith("apps/desktop/tests/fixtures/"):
            return suite
        elif Path(path).parent.as_posix() == "apps/desktop/tests" and is_browser_test(Path(path)):
            if path not in suite:
                return suite
            selected.add(path)
        elif path.startswith("apps/desktop/") and not (
                Path(path).suffix == ".md" or path.startswith("apps/desktop/tests/") and path.endswith(".test.cjs")):
            # HTML entry points, public assets and new frontend infrastructure
            # must not silently disappear from checks merely because they moved.
            return suite
    for path in selected:
        groups.update(group for group, prefixes in BROWSER_GROUPS.items() if Path(path).name.startswith(prefixes))
    if selected or groups:
        selected.update(QUICK_BROWSER_FILES)
        prefixes = tuple(prefix for group in groups for prefix in BROWSER_GROUPS[group])
        selected.update(path for path in suite if Path(path).name.startswith(prefixes))
    return [path for path in suite if path in selected]


def parse_browser_files(value: str, *, full: bool) -> list[str]:
    try:
        files = json.loads(value)
    except json.JSONDecodeError as error:
        raise ValueError("--browser-files-json must be a JSON list of browser test paths") from error
    suite = browser_repository_files()
    if (not isinstance(files, list) or not files or any(not isinstance(path, str) or path not in suite for path in files)
            or len(files) != len(set(files))):
        raise ValueError("--browser-files-json requires unique existing browser test paths relative to the repository")
    if full and set(files) != set(suite):
        raise ValueError("The full tier requires the complete browser suite")
    return [path for path in suite if path in files]


def changed_paths(base: str) -> list[str] | None:
    diff = subprocess.run(["git", "diff", "--name-only", "--no-renames", "-z", base, "HEAD"],
                          cwd=ROOT, capture_output=True, text=True, check=False)
    return [path for path in diff.stdout.split("\0") if path] if diff.returncode == 0 else None


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

    def rust(self) -> None:
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
        if self.args.tier == "quick":
            packages = tuple(arg for package in PARALLEL_PACKAGES for arg in ("-p", package))
            self.cargo("test", *packages, "--all-features", "--locked", "--", "--test-threads=4")
            # Keep retained-data regressions in ordinary feedback, even without a risky diff.
            self.cargo("test", "-p", "app-storage", "--lib", "--all-features", "--locked", "--",
                       "backups", "migration", "instance_creation", "instance_file_patch",
                       "instance_details_tests", "combined_runtime_read", "--test-threads=1")
        else:
            arguments = ["scripts/verify_runtime_reliability.py", "--full-workspace", "--rust-only",
                         "--output", str(self.args.output / "runtime")]
            for name in ("cargo_wrapper", "node_wrapper", "frontend_dist"):
                value = getattr(self.args, name)
                if value:
                    arguments += ["--" + name.replace("_", "-"), str(value)]
            self.python(*arguments)

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
                        "frontend-tests", "frontend-browser", "rust", "native", "scope"), default="all")
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
    args = parser.parse_args(argv)
    if not 1 <= args.shard_index <= args.shard_count <= 8:
        parser.error("Require 1 <= shard-index <= shard-count <= 8")
    if args.base and args.browser_files_json is not None:
        parser.error("--base and --browser-files-json cannot be combined")
    try:
        args.browser_files = (parse_browser_files(args.browser_files_json, full=args.tier == "full")
                              if args.browser_files_json is not None else None)
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
        full = frontend_full or rust_full or native_full or any(path.startswith("scripts/") for path in paths)
        frontend_files = browser_repository_files() if frontend_full else frontend_selection(paths)
        frontend_changed = bool(frontend_files)
        print(f"[verify] frontend_full={frontend_full}; rust_full={rust_full}; native_full={native_full}; "
              f"frontend_changed={frontend_changed}; {len(frontend_files)} browser files; {len(paths)} changed paths")
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as destination:
                for name, value in (("full", full), ("frontend_full", frontend_full), ("rust_full", rust_full),
                                    ("native_full", native_full), ("frontend_changed", frontend_changed)):
                    destination.write(f"{name}={str(value).lower()}\n")
                destination.write(f"frontend_files={json.dumps(frontend_files, separators=(',', ':'))}\n")
        return 0
    if args.component == "frontend-browser" and args.base and args.tier != "full":
        paths = changed_paths(args.base)
        args.browser_files = browser_repository_files() if paths is None else frontend_selection(paths)
    requires_output = args.component == "native" or (args.tier == "full" and args.component in ("all", "rust"))
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
        if args.component == "native" or (args.component == "all" and args.tier == "full"):
            runner.native()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"[verify] failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
