from __future__ import annotations

import argparse
from collections import Counter
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
from pathlib import PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile
from typing import BinaryIO


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
MANIFEST_FILENAME = "PUBLIC_SNAPSHOT_MANIFEST.json"
MAX_ARCHIVE_COMMAND_CHARACTERS = 12_000
WINDOWS_RESERVED_BASENAMES = frozenset(
    {
        "aux",
        "con",
        "conin$",
        "conout$",
        "nul",
        "prn",
        *(f"com{number}" for number in range(1, 10)),
        *(f"lpt{number}" for number in range(1, 10)),
        *(f"com{number}" for number in "¹²³"),
        *(f"lpt{number}" for number in "¹²³"),
    }
)
WINDOWS_FORBIDDEN_FILENAME_CHARACTERS = frozenset('<>:"\\|?*')
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts.verify_no_tracked_secrets import scan_paths
from scripts.open_source_product_policy import scan_product_surfaces
from scripts.third_party_asset_policy import (
    THIRD_PARTY_ASSET_LICENSES,
    verify_third_party_asset_bytes,
)


FIRST_PARTY_ASSET_ALLOWLIST = frozenset(
    {
        "assets/entrogenesis-dark.svg",
        "assets/entrogenesis.svg",
        "assets/lgsm-system-49eec728.webp",
        "assets/logo-dark.svg",
        "assets/logo.svg",
        "assets/wechat-official-account.jpg",
        "apps/desktop/src-tauri/icons/icon.ico",
        "apps/desktop/src-tauri/icons/icon.png",
        "apps/desktop/src-tauri/icons/icon.svg",
        "apps/desktop/src-tauri/installer/branding/header.bmp",
        "apps/desktop/src-tauri/installer/branding/header.svg",
        "apps/desktop/src-tauri/installer/branding/sidebar.bmp",
        "apps/desktop/src-tauri/installer/branding/sidebar.svg",
        "apps/desktop/src/assets/langame-logo-dark.svg",
        "apps/desktop/src/assets/langame-logo.svg",
        "apps/desktop/src/assets/minecraft-cover.png",
    }
)
PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS = frozenset(
    {
        "AGENTS.md",
        "apps/desktop/tests/managed-desktop-launcher.test.cjs",
        "apps/desktop/src-tauri/src/bin/hydrate_smoke_cache.rs",
        "apps/desktop/src-tauri/src/bin/smoke_full_chain.rs",
        "apps/desktop/src-tauri/tests/support/real_smoke.rs",
        "crates/app-runtime/examples/verify_all_real_instances.rs",
        "crates/app-storage/examples/provision_all_library_instances.rs",
        "scripts/audit_real_game_instances.py",
        "scripts/audit_real_instance_launch_readiness.py",
        "scripts/audit_real_instance_templates.py",
        "scripts/audit_scum_ledger.py",
        "scripts/bootstrap_real_game_roots.py",
        "scripts/close_langame_desktop.ps1",
        "scripts/prepare_desktop_cargo_target.ps1",
        "scripts/resolve-cargo-target-dir.bat",
        "scripts/resolve-cargo-target-dir.ps1",
        "scripts/smoke-path-policy.psm1",
        "scripts/smoke-support.psm1",
        "scripts/smoke.ps1",
        "scripts/start_langame_desktop_dev.ps1",
        "scripts/start_langame_desktop_local.ps1",
        "scripts/test-smoke-command-contract.ps1",
        "scripts/test-smoke-storage-policy.ps1",
        "scripts/verify_startup_performance_guards.ps1",
        "scripts/wait_langame_desktop_window.ps1",
        "start.bat",
    }
)
PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES = frozenset(
    {
        "apps/desktop/src-tauri/src/bin/smoke_full_chain/",
        "apps/desktop/src-tauri/tests/real_",
    }
)
PUBLIC_SNAPSHOT_ENTRYPOINT_PATHS = frozenset(
    {
        ".github/workflows/ci.yml",
        "CONTRIBUTING.md",
        "Cargo.toml",
        "README.md",
        "README.en.md",
        "apps/desktop/package.json",
        "apps/desktop/src-tauri/Cargo.toml",
    }
)
PUBLIC_SNAPSHOT_INTERNAL_REFERENCE_MARKERS = frozenset(
    {
        *(path.replace("\\", "/") for path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS),
        "hydrate_smoke_cache",
        "smoke_full_chain",
    }
)
PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS = frozenset(
    {
        "docs/game-config-acceptance/sources/ark-server-configuration-2026-07-13.wikitext",
    }
)
PUBLIC_SNAPSHOT_REQUIRED_PATHS = frozenset(
    {
        ".editorconfig",
        ".github/ISSUE_TEMPLATE/bug_report.yml",
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/feature_request.yml",
        ".github/ISSUE_TEMPLATE/server_request.yml",
        ".github/dependabot.yml",
        ".github/pull_request_template.md",
        ".github/workflows/ci.yml",
        "CODE_OF_CONDUCT.md",
        "CONTRIBUTING.md",
        "CONTRIBUTOR_AGREEMENT.md",
        "Cargo.lock",
        "Cargo.toml",
        "LICENSE",
        "NOTICE",
        "PRIVACY.md",
        "README.md",
        "README.en.md",
        "SECURITY.md",
        "THIRD_PARTY_NOTICES.md",
        "THIRD_PARTY_ASSETS/NOTICE",
        "assets/NOTICE",
        *THIRD_PARTY_ASSET_LICENSES,
        *THIRD_PARTY_ASSET_LICENSES.values(),
        "docs/game-config-source-ledger.csv",
        "docs/game-config-source-ledger.md",
        "docs/game-integration-validation.md",
        "docs/lan-directory-protocol-v2.md",
        "docs/player-center-capability-matrix.md",
        "rust-toolchain.toml",
        "apps/desktop/LICENSE",
        "apps/desktop/package-lock.json",
        "apps/desktop/package.json",
        "apps/desktop/public/THIRD_PARTY_LICENSES.txt",
        "apps/desktop/scripts/audit_i18n_surface.cjs",
        "apps/desktop/scripts/catalog-file-discovery.cjs",
        "apps/desktop/scripts/generate_i18n_schema_catalogs.cjs",
        "apps/desktop/scripts/generate_npm_third_party_licenses.mjs",
        "apps/desktop/scripts/typescript_source_tools.cjs",
        "apps/desktop/scripts/verify_i18n_catalogs.cjs",
        "apps/desktop/src-tauri/Cargo.toml",
        "apps/desktop/src-tauri/THIRD_PARTY_LICENSES-RUST.txt",
        "apps/desktop/src-tauri/tauri.conf.json",
        "apps/desktop/third-party-license-sources/npm/react-three-fiber-9.7.0-LICENSE",
        "apps/desktop/vite.config.ts",
        "scripts/generate_rust_third_party_licenses.py",
        "scripts/rust_license_policy.py",
        "scripts/project_license_policy.py",
        "scripts/third_party_asset_policy.py",
        "scripts/third_party_license_sources/rust/SPDX-Apache-2.0.txt",
        "scripts/third_party_license_sources/rust/SPDX-MIT.txt",
        "scripts/third_party_license_sources/rust/SPDX-MPL-2.0.txt",
        "scripts/third_party_license_sources/rust/dlopen2-cc80e4a0-LICENSE",
        "scripts/third_party_license_sources/rust/objc2-7f976f7e-LICENSE.md",
        "scripts/third_party_license_sources/rust/webview2-b74dc5e2-LICENSE",
    }
)
ASSET_AND_BINARY_SUFFIXES = frozenset(
    {
        ".7z",
        ".avif",
        ".bin",
        ".bmp",
        ".dll",
        ".dmg",
        ".doc",
        ".docx",
        ".exe",
        ".gif",
        ".gz",
        ".icns",
        ".ico",
        ".iso",
        ".jpeg",
        ".jpg",
        ".m4a",
        ".mkv",
        ".mov",
        ".mp3",
        ".mp4",
        ".msi",
        ".ogg",
        ".otf",
        ".pak",
        ".pdf",
        ".png",
        ".ppt",
        ".pptx",
        ".rar",
        ".so",
        ".svg",
        ".tar",
        ".tgz",
        ".ttf",
        ".uasset",
        ".wav",
        ".wasm",
        ".webm",
        ".webp",
        ".woff",
        ".woff2",
        ".xls",
        ".xlsx",
        ".xz",
        ".zip",
    }
)
SENSITIVE_SUFFIXES = frozenset(
    {
        ".bak",
        ".backup",
        ".cer",
        ".core",
        ".crt",
        ".db",
        ".db-journal",
        ".db-shm",
        ".db-wal",
        ".der",
        ".dmp",
        ".dump",
        ".jks",
        ".kdbx",
        ".key",
        ".keystore",
        ".log",
        ".mobileprovision",
        ".mdmp",
        ".ovpn",
        ".p12",
        ".pem",
        ".pfx",
        ".ppk",
        ".sqlite",
        ".sqlite-journal",
        ".sqlite-shm",
        ".sqlite-wal",
        ".sqlite3",
        ".sqlite3-journal",
        ".sqlite3-shm",
        ".sqlite3-wal",
        ".wer",
        ".hdmp",
    }
)
LOCAL_DEVELOPMENT_DIRECTORIES = frozenset({".codex", ".agents", ".claude"})
LOCAL_INSTRUCTION_FILENAMES = frozenset({"agents.md", "agents.override.md", "claude.md"})
EDITOR_TEMPORARY_SUFFIXES = ("~", ".orig", ".rej", ".swp", ".swo", ".tmp")
GENERATED_OR_PRIVATE_DIRECTORIES = frozenset(
    {
        ".cache",
        ".git",
        ".idea",
        ".playwright-cli",
        ".runtime-data",
        ".vscode",
        "artifacts",
        "build",
        "coverage",
        "dist",
        "logs",
        "node_modules",
        "out",
        "target",
        "temp",
        "test-results",
        "tmp",
        "vendor",
    }
)
SENSITIVE_FILENAMES = frozenset(
    {
        ".git-credentials",
        ".envrc",
        ".netrc",
        ".npmrc",
        ".pypirc",
        "application_default_credentials.json",
        "id_dsa",
        "id_ecdsa",
        "id_ed25519",
        "id_rsa",
    }
)


class SnapshotError(RuntimeError):
    pass


@dataclass(frozen=True)
class ExcludedPath:
    path: Path
    reason: str


@dataclass(frozen=True)
class SnapshotPlan:
    included: tuple[Path, ...]
    excluded: tuple[ExcludedPath, ...]


@dataclass(frozen=True)
class GitTreeEntry:
    path: Path
    mode: str
    object_type: str
    object_id: str


def _run_git(
    repository_root: Path,
    arguments: list[str],
    *,
    input_bytes: bytes | None = None,
    accepted_return_codes: frozenset[int] = frozenset({0}),
) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        cwd=repository_root,
        env=_git_environment(),
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode not in accepted_return_codes:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise SnapshotError(f"git {' '.join(arguments)} failed: {detail}")
    return result.stdout


def _git_environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    return environment


def _decode_git_paths(payload: bytes) -> tuple[Path, ...]:
    paths_by_name: dict[str, Path] = {}
    aliases: dict[str, str] = {}
    for raw_path in payload.split(b"\0"):
        if not raw_path:
            continue
        path_name = raw_path.decode("utf-8")
        relative_path = _validated_repository_path(path_name, context="repository")
        _register_filesystem_alias(relative_path, aliases, context="repository")
        paths_by_name.setdefault(path_name, relative_path)
    return tuple(sorted(paths_by_name.values(), key=lambda path: path.as_posix()))


def _validated_repository_path(path_name: str, *, context: str) -> Path:
    if not path_name or "\\" in path_name:
        raise SnapshotError(f"unsafe {context} path: {path_name}")
    raw_parts = path_name.split("/")
    if any(part in {"", ".", ".."} for part in raw_parts):
        raise SnapshotError(f"unsafe {context} path: {path_name}")
    pure_path = PurePosixPath(path_name)
    if pure_path.is_absolute() or pure_path.as_posix() != path_name:
        raise SnapshotError(f"unsafe {context} path: {path_name}")
    relative_path = Path(*pure_path.parts)
    _validate_relative_path(relative_path, context=context)
    return relative_path


def _validate_relative_path(relative_path: Path, *, context: str) -> None:
    if (
        relative_path.is_absolute()
        or bool(relative_path.drive)
        or bool(relative_path.root)
        or relative_path == Path(".")
        or ".." in relative_path.parts
    ):
        raise SnapshotError(f"unsafe {context} path: {relative_path}")
    for component in relative_path.parts:
        if (
            component.endswith((" ", "."))
            or any(
                character in WINDOWS_FORBIDDEN_FILENAME_CHARACTERS
                or ord(character) < 32
                for character in component
            )
            or component.partition(".")[0].rstrip(" ").casefold()
            in WINDOWS_RESERVED_BASENAMES
        ):
            raise SnapshotError(f"unsafe {context} path: {relative_path}")


def _filesystem_alias_key(relative_path: Path) -> str:
    return "/".join(component.casefold() for component in relative_path.parts)


def _register_filesystem_alias(
    relative_path: Path,
    aliases: dict[str, str],
    *,
    context: str,
) -> None:
    path_name = relative_path.as_posix()
    alias = _filesystem_alias_key(relative_path)
    existing = aliases.get(alias)
    if existing is not None and existing != path_name:
        raise SnapshotError(
            f"{context} paths collide on Windows: {existing} and {path_name}"
        )
    aliases[alias] = path_name


def _is_reserved_manifest_path(relative_path: Path) -> bool:
    return _filesystem_alias_key(relative_path) == MANIFEST_FILENAME.casefold()


def _validated_commit_id(source_commit: str) -> str:
    normalized_commit = source_commit.strip().lower()
    if len(normalized_commit) not in {40, 64} or any(
        character not in "0123456789abcdef" for character in normalized_commit
    ):
        raise SnapshotError("source commit id must be a complete Git object id")
    return normalized_commit


def capture_head_commit(repository_root: Path) -> str:
    payload = _run_git(repository_root, ["rev-parse", "--verify", "HEAD^{commit}"])
    return _validated_commit_id(payload.decode("ascii", errors="strict"))


def discover_head_candidate_paths(
    repository_root: Path, source_commit: str
) -> tuple[Path, ...]:
    source_commit = _validated_commit_id(source_commit)
    payload = _run_git(
        repository_root,
        ["ls-tree", "-r", "-z", "--name-only", source_commit],
    )
    return _decode_git_paths(payload)


def discover_head_tree_entries(
    repository_root: Path, source_commit: str
) -> dict[Path, GitTreeEntry]:
    source_commit = _validated_commit_id(source_commit)
    payload = _run_git(repository_root, ["ls-tree", "-r", "-z", source_commit])
    entries: dict[Path, GitTreeEntry] = {}
    path_names: set[str] = set()
    aliases: dict[str, str] = {}
    for record in payload.split(b"\0"):
        if not record:
            continue
        try:
            metadata, raw_path = record.split(b"\t", 1)
            raw_mode, raw_type, raw_object_id = metadata.split(b" ", 2)
        except ValueError as error:
            raise SnapshotError("git ls-tree returned malformed output") from error
        path_name = raw_path.decode("utf-8")
        relative_path = _validated_repository_path(path_name, context="repository")
        if path_name in path_names:
            raise SnapshotError(f"duplicate path in git tree: {path_name}")
        _register_filesystem_alias(relative_path, aliases, context="repository")
        path_names.add(path_name)
        entries[relative_path] = GitTreeEntry(
            path=relative_path,
            mode=raw_mode.decode("ascii", errors="strict"),
            object_type=raw_type.decode("ascii", errors="strict"),
            object_id=raw_object_id.decode("ascii", errors="strict"),
        )
    return entries


def discover_candidate_paths(repository_root: Path) -> tuple[Path, ...]:
    payload = _run_git(
        repository_root,
        ["ls-files", "-z", "--cached"],
    )
    return _decode_git_paths(payload)


def ensure_clean_worktree(repository_root: Path) -> None:
    status = _run_git(
        repository_root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )
    if status:
        raise SnapshotError(
            "public snapshot export requires a clean worktree; review and commit or remove all staged, unstaged, and untracked changes first"
        )


def discover_ignored_paths(
    repository_root: Path, candidate_paths: tuple[Path, ...]
) -> frozenset[str]:
    if not candidate_paths:
        return frozenset()
    input_bytes = b"\0".join(
        path.as_posix().encode("utf-8") for path in candidate_paths
    ) + b"\0"
    payload = _run_git(
        repository_root,
        ["check-ignore", "--no-index", "-z", "--stdin"],
        input_bytes=input_bytes,
        accepted_return_codes=frozenset({0, 1}),
    )
    return frozenset(path.as_posix() for path in _decode_git_paths(payload))


def discover_head_ignored_paths(
    repository_root: Path,
    candidate_paths: tuple[Path, ...],
    tree_entries: dict[Path, GitTreeEntry],
) -> frozenset[str]:
    ignore_entries = tuple(
        entry
        for path, entry in tree_entries.items()
        if path.name.casefold() == ".gitignore"
    )
    if not ignore_entries or not candidate_paths:
        return frozenset()

    with tempfile.TemporaryDirectory(prefix="langame-public-ignore-policy-") as directory:
        policy_root = Path(directory) / "worktree"
        empty_template = Path(directory) / "empty-git-template"
        policy_root.mkdir()
        empty_template.mkdir()
        _run_git(
            policy_root,
            ["init", "--quiet", f"--template={empty_template.as_posix()}"],
        )

        for entry in ignore_entries:
            if entry.object_type != "blob" or entry.mode not in {"100644", "100755"}:
                raise SnapshotError(
                    f"committed ignore policy is not a regular file: {entry.path}"
                )
            destination = policy_root / entry.path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(
                _run_git(repository_root, ["cat-file", "blob", entry.object_id])
            )

        input_bytes = b"\0".join(
            path.as_posix().encode("utf-8") for path in candidate_paths
        ) + b"\0"
        disabled_global_ignore = Path(directory) / "disabled-global-ignore"
        payload = _run_git(
            policy_root,
            [
                "-c",
                f"core.excludesFile={disabled_global_ignore.as_posix()}",
                "check-ignore",
                "--no-index",
                "-z",
                "--stdin",
            ],
            input_bytes=input_bytes,
            accepted_return_codes=frozenset({0, 1}),
        )
        return frozenset(path.as_posix() for path in _decode_git_paths(payload))


def exclusion_reason(relative_path: Path, ignored_paths: frozenset[str]) -> str | None:
    normalized_path = relative_path.as_posix()
    lowercase_parts = tuple(part.casefold() for part in relative_path.parts)
    lowercase_name = relative_path.name.casefold()

    if _is_reserved_manifest_path(relative_path):
        return "reserved-output-path"
    if normalized_path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS or any(
        normalized_path.startswith(prefix)
        for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
    ):
        return "internal-lifecycle"
    if normalized_path in PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS:
        return "third-party-source-material"
    if normalized_path in ignored_paths:
        return "git-ignored"
    if (
        normalized_path in FIRST_PARTY_ASSET_ALLOWLIST
        or normalized_path in THIRD_PARTY_ASSET_LICENSES
    ):
        return None
    if lowercase_name in LOCAL_INSTRUCTION_FILENAMES or any(
        part in LOCAL_DEVELOPMENT_DIRECTORIES for part in lowercase_parts[:-1]
    ):
        return "local-development-metadata"
    if any(part in GENERATED_OR_PRIVATE_DIRECTORIES for part in lowercase_parts[:-1]):
        return "generated-or-private-directory"
    if relative_path.suffix.casefold() in ASSET_AND_BINARY_SUFFIXES:
        return "unlicensed-asset-or-binary"
    if relative_path.suffix.casefold() in SENSITIVE_SUFFIXES:
        return "sensitive-file-type"
    if lowercase_name in SENSITIVE_FILENAMES:
        return "credential-file"
    if lowercase_name.startswith(("client_secret", "service-account")) and (
        relative_path.suffix.casefold() == ".json"
    ):
        return "credential-file"
    if (
        (".aws" in lowercase_parts and lowercase_name == "credentials")
        or (
            ".cargo" in lowercase_parts
            and lowercase_name in {"credentials", "credentials.toml"}
        )
        or (".kube" in lowercase_parts and lowercase_name == "config")
        or (".docker" in lowercase_parts and lowercase_name == "config.json")
    ):
        return "credential-file"
    if lowercase_name == ".env" or lowercase_name.startswith(".env."):
        return "environment-file"
    if lowercase_name.startswith(("credential.", "credentials.", "secret.", "secrets.")):
        return "credential-file"
    if lowercase_name.endswith(EDITOR_TEMPORARY_SUFFIXES):
        return "editor-or-temporary-file"
    return None


def build_snapshot_plan(repository_root: Path) -> SnapshotPlan:
    candidate_paths = discover_candidate_paths(repository_root)
    ignored_paths = discover_ignored_paths(repository_root, candidate_paths)
    included: list[Path] = []
    excluded: list[ExcludedPath] = []

    for relative_path in candidate_paths:
        reason = exclusion_reason(relative_path, ignored_paths)
        source_path = repository_root / relative_path
        if reason is None and source_path.is_symlink():
            reason = "symbolic-link"
        if reason is None and not source_path.exists():
            reason = "missing-from-working-tree"
        if reason is None and not source_path.is_file():
            reason = "non-regular-file"
        if reason is None:
            included.append(relative_path)
        else:
            excluded.append(ExcludedPath(path=relative_path, reason=reason))

    return SnapshotPlan(included=tuple(included), excluded=tuple(excluded))


def build_head_snapshot_plan(
    repository_root: Path, source_commit: str
) -> SnapshotPlan:
    candidate_paths = discover_head_candidate_paths(repository_root, source_commit)
    tree_entries = discover_head_tree_entries(repository_root, source_commit)
    if set(candidate_paths) != set(tree_entries):
        raise SnapshotError("git ls-tree candidate and metadata listings disagree")
    reserved_manifest = next(
        (path for path in candidate_paths if _is_reserved_manifest_path(path)),
        None,
    )
    if reserved_manifest is not None:
        raise SnapshotError(
            f"source commit contains reserved snapshot path: {reserved_manifest}"
        )
    ignored_paths = discover_head_ignored_paths(
        repository_root,
        candidate_paths,
        tree_entries,
    )
    included: list[Path] = []
    excluded: list[ExcludedPath] = []

    for relative_path in candidate_paths:
        reason = exclusion_reason(relative_path, ignored_paths)
        entry = tree_entries[relative_path]
        if reason is None and entry.mode == "120000":
            reason = "symbolic-link"
        if reason is None and (
            entry.object_type != "blob" or entry.mode not in {"100644", "100755"}
        ):
            reason = "non-regular-file"
        if reason is None:
            included.append(relative_path)
        else:
            excluded.append(ExcludedPath(path=relative_path, reason=reason))

    return SnapshotPlan(included=tuple(included), excluded=tuple(excluded))


def validate_public_snapshot_plan(plan: SnapshotPlan) -> None:
    included = {path.as_posix() for path in plan.included}
    excluded = {item.path.as_posix(): item.reason for item in plan.excluded}

    missing_required_paths = sorted(PUBLIC_SNAPSHOT_REQUIRED_PATHS - included)
    if missing_required_paths:
        raise SnapshotError(
            "public snapshot is missing required path(s): "
            + ", ".join(missing_required_paths)
        )

    expected_reasons = {
        **{
            path: "internal-lifecycle"
            for path in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS
        },
        **{
            path: "third-party-source-material"
            for path in PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS
        },
    }
    policy_leaks = sorted(path for path in expected_reasons if path in included)
    policy_leaks.extend(
        sorted(
            path
            for path in included
            if any(
                path.startswith(prefix)
                for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
            )
        )
    )
    if policy_leaks:
        raise SnapshotError(
            "public snapshot includes private publication path(s): "
            + ", ".join(policy_leaks)
        )

    wrong_reasons = sorted(
        f"{path} ({excluded[path]})"
        for path, expected_reason in expected_reasons.items()
        if path in excluded and excluded[path] != expected_reason
    )
    wrong_reasons.extend(
        sorted(
            f"{path} ({reason})"
            for path, reason in excluded.items()
            if reason != "internal-lifecycle"
            and any(
                path.startswith(prefix)
                for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
            )
        )
    )
    if wrong_reasons:
        raise SnapshotError(
            "public snapshot exclusion policy returned an unexpected reason: "
            + ", ".join(wrong_reasons)
        )


def scan_snapshot_entrypoint_references(
    repository_root: Path, relative_paths: tuple[Path, ...]
) -> None:
    included = {path.as_posix() for path in relative_paths}
    findings: list[str] = []
    for entrypoint in sorted(PUBLIC_SNAPSHOT_ENTRYPOINT_PATHS & included):
        try:
            text = (repository_root / entrypoint).read_text(encoding="utf-8")
        except (OSError, UnicodeError) as error:
            raise SnapshotError(
                f"cannot inspect public snapshot entrypoint {entrypoint}: {error}"
            ) from error
        normalized_text = text.replace("\\", "/")
        for marker in sorted(PUBLIC_SNAPSHOT_INTERNAL_REFERENCE_MARKERS):
            if marker in normalized_text:
                findings.append(f"{entrypoint} -> {marker}")
    if findings:
        raise SnapshotError(
            "public entrypoint references excluded internal lifecycle content: "
            + ", ".join(findings)
        )


def scan_snapshot_files(repository_root: Path, relative_paths: tuple[Path, ...]) -> None:
    asset_findings = verify_third_party_asset_bytes(repository_root, relative_paths)
    if asset_findings:
        asset_details = "\n".join(
            f"{finding.path}: {finding.rule}: {finding.message}"
            for finding in asset_findings
        )
        raise SnapshotError("public snapshot third-party asset policy failed:\n" + asset_details)

    findings = scan_paths(
        [repository_root / path for path in relative_paths], repository_root=repository_root
    )
    details = []
    for finding in findings:
        try:
            display_path = finding.path.relative_to(repository_root)
        except ValueError:
            display_path = finding.path
        details.append(f"{display_path}:{finding.line}: {finding.rule}")
    if details:
        raise SnapshotError(
            "public snapshot secret scan failed:\n" + "\n".join(details)
        )

    product_findings = scan_product_surfaces(repository_root, relative_paths)
    if product_findings:
        product_details = "\n".join(
            f"{finding.path}:{finding.line}: {finding.rule}"
            for finding in product_findings
        )
        raise SnapshotError(
            "public snapshot product policy failed:\n" + product_details
        )


def validate_output_path(repository_root: Path, output_path: Path) -> Path:
    resolved_repository = repository_root.resolve()
    resolved_output = output_path.resolve()
    try:
        resolved_output.relative_to(resolved_repository)
    except ValueError:
        pass
    else:
        raise SnapshotError("output directory must be outside the repository workspace")
    if resolved_output.exists():
        raise SnapshotError("output directory must not already exist")
    if not resolved_output.parent.is_dir():
        raise SnapshotError("output parent directory must already exist")
    return resolved_output


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _validated_export_paths(relative_paths: tuple[Path, ...]) -> tuple[Path, ...]:
    validated_paths: dict[str, Path] = {}
    aliases: dict[str, str] = {}
    for relative_path in relative_paths:
        _validate_relative_path(relative_path, context="export")
        if _is_reserved_manifest_path(relative_path):
            raise SnapshotError(f"reserved export path: {relative_path}")
        _register_filesystem_alias(relative_path, aliases, context="export")
        validated_paths.setdefault(relative_path.as_posix(), relative_path)
    return tuple(sorted(validated_paths.values(), key=lambda path: path.as_posix()))


def _validated_archive_member_path(member_name: str) -> Path:
    normalized_name = member_name[:-1] if member_name.endswith("/") else member_name
    return _validated_repository_path(normalized_name, context="git archive")


def _archive_path_batches(relative_paths: tuple[Path, ...]) -> tuple[tuple[str, ...], ...]:
    batches: list[tuple[str, ...]] = []
    current: list[str] = []
    current_characters = 0
    for relative_path in relative_paths:
        pathspec = f":(literal){relative_path.as_posix()}"
        estimated_characters = len(pathspec) + 3
        if current and (
            current_characters + estimated_characters
            > MAX_ARCHIVE_COMMAND_CHARACTERS
        ):
            batches.append(tuple(current))
            current = []
            current_characters = 0
        current.append(pathspec)
        current_characters += estimated_characters
    if current:
        batches.append(tuple(current))
    return tuple(batches)


def _materialize_archive_batch(
    repository_root: Path,
    source_commit: str,
    staging_path: Path,
    pathspecs: tuple[str, ...],
    included_by_name: dict[str, Path],
    materialized: set[Path],
) -> None:
    process = subprocess.Popen(
        ["git", "archive", "--format=tar", source_commit, "--", *pathspecs],
        cwd=repository_root,
        env=_git_environment(),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if process.stdout is None or process.stderr is None:
        process.kill()
        process.wait()
        raise SnapshotError("failed to open git archive pipes")

    try:
        with tarfile.open(fileobj=process.stdout, mode="r|") as archive:
            for member in archive:
                relative_path = _validated_archive_member_path(member.name)
                expected_path = included_by_name.get(relative_path.as_posix())
                if expected_path is None:
                    continue
                if expected_path in materialized:
                    raise SnapshotError(
                        f"duplicate path in git archive: {expected_path}"
                    )
                if not member.isfile():
                    raise SnapshotError(
                        f"git archive entry is not a regular file: {expected_path}"
                    )
                source = archive.extractfile(member)
                if source is None:
                    raise SnapshotError(
                        f"cannot read git archive entry: {expected_path}"
                    )
                destination = staging_path / expected_path
                destination.parent.mkdir(parents=True, exist_ok=True)
                with source, destination.open("wb") as target:
                    shutil.copyfileobj(source, target)
                materialized.add(expected_path)

        # A streaming tar reader stops at the end-of-archive marker and may leave
        # Git's record padding in the pipe. On Windows that remainder can fill the
        # pipe and keep `git archive` blocked forever. Drain both pipes together so
        # the producer can always finish before its return code is inspected.
        _, stderr = process.communicate()
        detail = stderr.decode("utf-8", errors="replace").strip()
        return_code = process.returncode
        if return_code != 0:
            raise SnapshotError(f"git archive failed: {detail}")
    except (tarfile.TarError, OSError) as error:
        if process.poll() is None:
            process.kill()
            process.wait()
        raise SnapshotError(f"cannot read git archive stream: {error}") from error
    except BaseException:
        if process.poll() is None:
            process.kill()
            process.wait()
        raise
    finally:
        for stream in (process.stdout, process.stderr):
            if not stream.closed:
                stream.close()


def _materialize_head_files(
    repository_root: Path,
    source_commit: str,
    staging_path: Path,
    relative_paths: tuple[Path, ...],
) -> None:
    source_commit = _validated_commit_id(source_commit)
    included_by_name = {path.as_posix(): path for path in relative_paths}
    materialized: set[Path] = set()
    for pathspecs in _archive_path_batches(relative_paths):
        _materialize_archive_batch(
            repository_root,
            source_commit,
            staging_path,
            pathspecs,
            included_by_name,
            materialized,
        )

    missing = set(relative_paths).difference(materialized)
    if missing:
        missing_list = ", ".join(path.as_posix() for path in sorted(missing))
        raise SnapshotError(f"git archive is missing selected path(s): {missing_list}")


def _read_exact(stream: BinaryIO, size: int) -> bytes:
    chunks: list[bytes] = []
    remaining = size
    while remaining > 0:
        chunk = stream.read(remaining)
        if not chunk:
            raise SnapshotError("git cat-file ended before returning the complete blob")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)


def _restore_canonical_git_blob_bytes(
    repository_root: Path,
    staging_path: Path,
    entries: tuple[GitTreeEntry, ...],
) -> None:
    if not entries:
        return
    process = subprocess.Popen(
        ["git", "cat-file", "--batch"],
        cwd=repository_root,
        env=_git_environment(),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if process.stdin is None or process.stdout is None or process.stderr is None:
        process.kill()
        raise SnapshotError("failed to open git cat-file pipes")

    try:
        for entry in entries:
            process.stdin.write(entry.object_id.encode("ascii") + b"\n")
            process.stdin.flush()
            header = process.stdout.readline().decode("ascii", errors="strict").strip()
            parts = header.split()
            if len(parts) != 3 or parts[0] != entry.object_id or parts[1] != "blob":
                raise SnapshotError(
                    f"git cat-file returned invalid metadata for {entry.path}: {header}"
                )
            try:
                blob_size = int(parts[2])
            except ValueError as error:
                raise SnapshotError(
                    f"git cat-file returned an invalid blob size for {entry.path}"
                ) from error
            blob = _read_exact(process.stdout, blob_size)
            if process.stdout.read(1) != b"\n":
                raise SnapshotError(
                    f"git cat-file returned malformed blob framing for {entry.path}"
                )
            (staging_path / entry.path).write_bytes(blob)

        process.stdin.close()
        return_code = process.wait()
        if return_code != 0:
            detail = process.stderr.read().decode("utf-8", errors="replace").strip()
            raise SnapshotError(f"git cat-file failed: {detail}")
    except BaseException:
        if process.poll() is None:
            process.kill()
            process.wait()
        raise
    finally:
        for stream in (process.stdin, process.stdout, process.stderr):
            if not stream.closed:
                stream.close()


def write_snapshot_from_head(
    repository_root: Path,
    output_path: Path,
    source_commit: str,
    relative_paths: tuple[Path, ...],
) -> None:
    output_path = validate_output_path(repository_root, output_path)
    source_commit = _validated_commit_id(source_commit)
    relative_paths = _validated_export_paths(relative_paths)
    tree_entries = discover_head_tree_entries(repository_root, source_commit)
    reserved_manifest = next(
        (path for path in tree_entries if _is_reserved_manifest_path(path)),
        None,
    )
    if reserved_manifest is not None:
        raise SnapshotError(
            f"source commit contains reserved snapshot path: {reserved_manifest}"
        )
    selected_entries: list[GitTreeEntry] = []
    for relative_path in relative_paths:
        entry = tree_entries.get(relative_path)
        if entry is None:
            raise SnapshotError(
                f"selected path is missing from source commit: {relative_path}"
            )
        if entry.object_type != "blob" or entry.mode not in {"100644", "100755"}:
            raise SnapshotError(f"selected path is not a regular file: {relative_path}")
        selected_entries.append(entry)
    staging_path = Path(
        tempfile.mkdtemp(prefix=f".{output_path.name}.", dir=output_path.parent)
    )
    try:
        _materialize_head_files(
            repository_root,
            source_commit,
            staging_path,
            relative_paths,
        )
        _restore_canonical_git_blob_bytes(
            repository_root,
            staging_path,
            tuple(selected_entries),
        )
        scan_snapshot_files(staging_path, relative_paths)
        scan_snapshot_entrypoint_references(staging_path, relative_paths)
        manifest = {
            "schema_version": 1,
            "source_commit": source_commit,
            "files": [
                {
                    "path": path.as_posix(),
                    "sha256": _sha256(staging_path / path),
                }
                for path in relative_paths
            ],
        }
        (staging_path / MANIFEST_FILENAME).write_text(
            json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        staging_path.rename(output_path)
    except BaseException as export_error:
        try:
            shutil.rmtree(staging_path)
        except OSError as cleanup_error:
            raise SnapshotError(
                "snapshot export failed and temporary staging cleanup also failed; "
                f"remove this path manually: {staging_path}: {cleanup_error}"
            ) from export_error
        raise


def _print_plan(plan: SnapshotPlan, *, verbose: bool) -> None:
    print(f"included files: {len(plan.included)}")
    print(f"excluded files: {len(plan.excluded)}")
    for reason, count in sorted(Counter(item.reason for item in plan.excluded).items()):
        print(f"  {reason}: {count}")
    if verbose:
        for item in plan.excluded:
            print(f"  excluded [{item.reason}] {item.path.as_posix()}")


def parse_arguments(arguments: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Plan or export a history-free public source snapshot."
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="new directory outside the repository; required with --execute",
    )
    parser.add_argument(
        "--execute",
        action="store_true",
        help="write an immutable snapshot from the current clean HEAD commit; without this flag the command previews the tracked working tree",
    )
    parser.add_argument(
        "--verbose", action="store_true", help="list every excluded repository path"
    )
    return parser.parse_args(arguments)


def main(arguments: list[str] | None = None) -> int:
    options = parse_arguments(arguments)
    if options.execute and options.output is None:
        print("error: --execute requires an explicit --output path", file=sys.stderr)
        return 2

    proposed_output = options.output or (
        REPOSITORY_ROOT.parent / f"{REPOSITORY_ROOT.name}-public-snapshot"
    )
    try:
        if options.execute:
            ensure_clean_worktree(REPOSITORY_ROOT)
            source_commit = capture_head_commit(REPOSITORY_ROOT)
            output_path = validate_output_path(REPOSITORY_ROOT, proposed_output)
            plan = build_head_snapshot_plan(REPOSITORY_ROOT, source_commit)
            validate_public_snapshot_plan(plan)
            print(f"source commit: {source_commit}")
            _print_plan(plan, verbose=options.verbose)
            write_snapshot_from_head(
                REPOSITORY_ROOT,
                output_path,
                source_commit,
                plan.included,
            )
            print("secret and product policy scans: passed")
            print(f"public snapshot exported to: {output_path}")
            return 0

        output_path = validate_output_path(REPOSITORY_ROOT, proposed_output)
        plan = build_snapshot_plan(REPOSITORY_ROOT)
        validate_public_snapshot_plan(plan)
        scan_snapshot_files(REPOSITORY_ROOT, plan.included)
        scan_snapshot_entrypoint_references(REPOSITORY_ROOT, plan.included)
        print("secret and product policy scans: passed")
        _print_plan(plan, verbose=options.verbose)
        print("mode: dry-run (no files written)")
        print("source: tracked working tree preview")
        print(f"proposed output: {output_path}")
        return 0
    except SnapshotError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
