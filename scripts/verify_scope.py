"""Select verification from repository inputs and their actual consumers."""

from __future__ import annotations

import json
from pathlib import Path
import re
import subprocess
import tomllib

if __package__:
    from .export_public_snapshot import PUBLIC_SNAPSHOT_ENTRYPOINT_PATHS
else:
    from export_public_snapshot import PUBLIC_SNAPSHOT_ENTRYPOINT_PATHS

ROOT = Path(__file__).resolve().parents[1]
DESKTOP = ROOT / "apps/desktop"
PARALLEL_PACKAGES = ("app-core", "app-modules", "app-knowledge")
# These predate the *-browser.test.cjs convention. Runner unit tests remain quick.
BROWSER_EXCEPTIONS = {"knowledge-settings.test.cjs", "runtime-browser-navigation.test.cjs"}
FRONTEND_FULL_PREFIXES = (
    "apps/desktop/src-tauri/", "apps/desktop/scripts/",
    "crates/app-core/", "modules/", "migrations/",
)
NATIVE_RISK_PREFIXES = (
    "apps/desktop/src-tauri/", "crates/app-runtime/", "crates/app-platform-win/",
    "crates/app-core/", "crates/app-storage/", ".cargo/",
    "apps/desktop/tests/helpers/desktop-reliability.",
    "scripts/tests/test_desktop_installer_",
    "scripts/tests/fixtures/retired-package-resources/",
)
NATIVE_RISK_FILES = {"apps/desktop/scripts/verify_desktop_reliability.cjs",
                     "apps/desktop/tests/desktop-reliability-driver.test.cjs",
                     "scripts/prepare_desktop_installer_tools.ps1"}
SHARED_RISK_FILES = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "apps/desktop/package.json",
                     "apps/desktop/package-lock.json", "scripts/verify_project.py",
                     "scripts/verify_scope.py",
                     "scripts/verify_runtime_reliability.py", "scripts/build_desktop_update_artifacts.ps1",
                     "crates/app-knowledge/runtime_build.rs", ".github/workflows/ci.yml"}
# These implementations define how current and historical source is admitted to
# publication. Ordinary scanned content retains the working-tree checks.
PUBLICATION_RULE_FILES = frozenset({
    "scripts/verify_no_tracked_secrets.py", "scripts/verify_open_source_boundary.py",
    "scripts/verify_open_source_history.py", "scripts/export_public_snapshot.py",
    "scripts/publication_source_io.py", "scripts/source_literal_scan.py",
    "scripts/open_source_product_policy.py", "scripts/project_license_policy.py",
    "scripts/third_party_asset_policy.py",
})

QUICK_BROWSER_FILES = tuple(f"apps/desktop/tests/{name}-browser.test.cjs" for name in (
    "instance-create", "server-detail-navigation", "restored-configuration", "configuration-controls",
    "maintenance-world-save", "desktop-exit", "runtime-reliability", "lan-transport",
))
# Existing behavioral families, not one rule per test. Overlapping families are intentional.
BROWSER_GROUPS = {
    "configuration": ("configuration-", "restored-", "dragonwilds-", "dst-", "enshrouded-", "moria-",
                      "romestead-", "satisfactory-", "scum-", "valheim-", "ark-", "asa-"),
    "mods": ("mod-", "manual-mod-", "workshop-", "steam-workshop-", "asa-", "ark-", "dst-", "moria-", "server-archives-"),
    "players": ("player-", "manual-player-", "seven-days-", "enshrouded-", "moria-", "system-player-", "server-archives-"),
    "maintenance": ("maintenance-", "server-archives-", "server-inline-", "instance-retirement-",
                    "program-cleanup-", "program-storage-", "steam-server-install-", "scum-", "ark-cluster-", "moria-", "gm-tool-"),
    "runtime": ("runtime-", "desktop-exit-", "instance-unavailable-", "server-card-", "server-tools-",
                "operation-notices-", "activity-notices-", "server-archives-", "ark-cluster-maps-", "dst-four-shards-",
                "instance-retirement-", "desktop-layout-", "steam-workshop-"),
    "updates": ("app-update-", "program-update-", "library-updates-", "steamcmd-dependency-", "steam-server-install-"),
    "system": ("system-", "desktop-main-pages-", "desktop-layout-", "instance-retirement-", "app-update-"),
    "library": ("library-", "instance-create-", "instance-inventory-", "steamcmd-dependency-", "steam-server-install-",
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


def publication_full(paths: list[str]) -> bool:
    """Recheck history and the snapshot when their rules change, including deletions."""
    # Nested ignore rules affect admitted paths; attributes affect Git archives.
    # Snapshot entrypoints also need the excluded-lifecycle reference check.
    return any(path in PUBLICATION_RULE_FILES or path in PUBLIC_SNAPSHOT_ENTRYPOINT_PATHS
               or Path(path).name in (".gitignore", ".gitattributes")
               for path in paths)


def full_scopes(paths: list[str]) -> tuple[bool, bool, bool]:
    shared = shared_risk(paths)
    rust = bool(rust_selection(paths))
    frontend = frontend_selection(paths) == browser_repository_files()
    native = shared or any((path in NATIVE_RISK_FILES or path.startswith(NATIVE_RISK_PREFIXES))
                           and not rust_test_only(path) for path in paths)
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
        if rust_test_only(path):
            continue
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


def rust_test_only(path: str) -> bool:
    # The inner attribute is enforced by Rust, even if a module is referenced by
    # a production parent. A filename containing "test" alone proves nothing.
    source = ROOT / path
    if source.suffix != ".rs" or not source.is_file():
        return False
    return re.match(r"\s*#!\[cfg\(test\)\]", source.read_text(encoding="utf-8")) is not None


def rust_workspace() -> dict[str, tuple[str, set[str]]]:
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    members = {}
    for member in manifest["workspace"]["members"]:
        for directory in ROOT.glob(member):
            data = tomllib.loads((directory / "Cargo.toml").read_text(encoding="utf-8"))
            members[directory.resolve()] = (data["package"]["name"], data)
    packages = {}
    for directory, (name, data) in members.items():
        dependencies = set()
        tables = [data, *data.get("target", {}).values()]
        for table in tables:
            for kind in ("dependencies", "build-dependencies", "dev-dependencies"):
                for dependency_name, dependency in table.get(kind, {}).items():
                    relative_to = directory
                    if isinstance(dependency, dict) and dependency.get("workspace"):
                        dependency = manifest["workspace"].get("dependencies", {}).get(dependency_name, {})
                        relative_to = ROOT
                    if isinstance(dependency, dict) and "path" in dependency:
                        target = (relative_to / dependency["path"]).resolve()
                        if target in members:
                            dependencies.add(members[target][0])
        packages[name] = (directory.relative_to(ROOT).as_posix() + "/", dependencies)
    return packages


def rust_selection(paths: list[str]) -> list[str]:
    """Complete affected packages and consumers; the quick baseline still runs."""
    packages = rust_workspace()
    if shared_risk(paths):
        return sorted(packages)
    selected: set[str] = set()
    changed: set[str] = set()
    for path in paths:
        owners = [name for name, (prefix, _) in packages.items() if path.startswith(prefix)]
        if owners:
            owner = owners[0]
            test_only = rust_test_only(path)
            if owner in PARALLEL_PACKAGES and test_only:
                # Their full test suites already run in the quick baseline.
                continue
            selected.add(owner)
            if not test_only:
                changed.add(owner)
        elif path.startswith(("crates/", "apps/desktop/src-tauri/", "modules/", "migrations/",
                              ".cargo/", "apps/desktop/src/api")):
            return sorted(packages)
    while True:
        consumers = {name for name, (_, dependencies) in packages.items() if dependencies & changed}
        if consumers <= changed:
            return sorted(selected | changed)
        changed.update(consumers)


def parse_rust_packages(value: str) -> list[str]:
    try:
        packages = json.loads(value)
    except json.JSONDecodeError as error:
        raise ValueError("--rust-packages-json must be a JSON list of workspace package names") from error
    workspace = rust_workspace()
    if (not isinstance(packages, list) or any(not isinstance(name, str) or name not in workspace for name in packages)
            or len(packages) != len(set(packages))):
        raise ValueError("--rust-packages-json requires unique workspace package names")
    return sorted(packages)
