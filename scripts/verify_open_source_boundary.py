from __future__ import annotations

from dataclasses import dataclass
import fnmatch
from pathlib import Path
import re
import subprocess
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts.export_public_snapshot import (
    ASSET_AND_BINARY_SUFFIXES,
    EDITOR_TEMPORARY_SUFFIXES,
    FIRST_PARTY_ASSET_ALLOWLIST,
    LOCAL_DEVELOPMENT_DIRECTORIES,
    LOCAL_INSTRUCTION_FILENAMES,
    PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PATHS,
    PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES,
    PUBLIC_SNAPSHOT_REQUIRED_PATHS,
    PUBLIC_SNAPSHOT_THIRD_PARTY_SOURCE_PATHS,
    THIRD_PARTY_ASSET_LICENSES,
    exclusion_reason,
)
from scripts.open_source_product_policy import scan_product_payload
from scripts.project_license_policy import verify_project_license
from scripts.publication_source_io import SourceReadError, read_source_payload, source_is_present
from scripts.third_party_asset_policy import verify_third_party_asset_bytes


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
        ".mdmp",
        ".mobileprovision",
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
SENSITIVE_FILENAMES = frozenset(
    {
        ".envrc",
        ".git-credentials",
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
PRIVATE_PATH_GLOBS = (
    "$log",
    "artifacts/**",
    "build_*_audit_doc.py",
    "design-qa.md",
    "docs/ai-assistant-architecture.md",
    "docs/ark-creature-source-audit-*.md",
    "docs/dst-real-config-gap-audit-*.md",
    "docs/dst-workbench-ia-*.md",
    "docs/game-config-i18n-translation-backlog-*.md",
    "docs/game-mod-workflow-audit-*",
    "docs/game-module-support-audit-*.md",
    "docs/hydrate-module-results-*.csv",
    "docs/instance_template_audit.*",
    "docs/library-instance-provisioning-*.json",
    "docs/manual-issues.tsv",
    "docs/module-config-depth-audit-*.md",
    "docs/module-integration-audit.md",
    "docs/product-focus-roadmap.md",
    "docs/real-instance-startup-smoke-*.json",
    "docs/real-instance-template-audit-*.json",
    "docs/real-instance-template-audit-*.md",
    "docs/runescape-dragonwilds-smoke-*.md",
    "docs/runtime-surface-architecture-*.md",
    "docs/superpowers/**",
    "*.local.bat",
    "local-game-data/**",
    "open-langame-lan-client.bat",
    "pic/**",
    "prompt.txt",
    "scripts/tmp_*",
)
DEVELOPER_MACHINE_PATH = re.compile(
    rb"(?i)(?:[A-Z]:[\\/]+Users[\\/]+[^\\/\s]+|"
    rb"D:[\\/]+LanGame[\\/]+(?:projects|scripts)(?:[\\/]+|\b)|"
    rb"D:[\\/]+LanGameTemp(?:[\\/]+|\b))"
)


@dataclass(frozen=True)
class BoundaryFinding:
    rule: str
    path: str
    detail: str


def _git_paths(
    repository_root: Path,
    arguments: list[str],
    *,
    input_bytes: bytes | None = None,
    accepted_return_codes: frozenset[int] = frozenset({0}),
) -> tuple[Path, ...]:
    result = subprocess.run(
        ["git", *arguments],
        cwd=repository_root,
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode not in accepted_return_codes:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"git {' '.join(arguments)} failed: {detail}")
    paths = {
        Path(raw_path.decode("utf-8"))
        for raw_path in result.stdout.split(b"\0")
        if raw_path
    }
    return tuple(sorted(paths, key=lambda path: path.as_posix()))


def tracked_worktree_paths(repository_root: Path) -> tuple[Path, ...]:
    tracked_paths = _git_paths(repository_root, ["ls-files", "-z"])
    return tuple(
        path
        for path in tracked_paths
        if source_is_present(repository_root / path)
    )


def ignored_tracked_paths(
    repository_root: Path, tracked_paths: tuple[Path, ...]
) -> frozenset[str]:
    if not tracked_paths:
        return frozenset()
    input_bytes = b"\0".join(
        path.as_posix().encode("utf-8") for path in tracked_paths
    ) + b"\0"
    return frozenset(
        path.as_posix()
        for path in _git_paths(
            repository_root,
            ["check-ignore", "--no-index", "-z", "--stdin"],
            input_bytes=input_bytes,
            accepted_return_codes=frozenset({0, 1}),
        )
    )


def is_unapproved_asset_path(path: Path) -> bool:
    return (
        path.suffix.casefold() in ASSET_AND_BINARY_SUFFIXES
        and path.as_posix() not in FIRST_PARTY_ASSET_ALLOWLIST
        and path.as_posix() not in THIRD_PARTY_ASSET_LICENSES
    )


def is_sensitive_or_private_path(path: Path) -> bool:
    normalized_path = path.as_posix()
    lowercase_path = normalized_path.casefold()
    lowercase_parts = tuple(part.casefold() for part in path.parts)
    lowercase_name = path.name.casefold()
    if lowercase_name in LOCAL_INSTRUCTION_FILENAMES or any(
        part in LOCAL_DEVELOPMENT_DIRECTORIES for part in lowercase_parts[:-1]
    ):
        return True
    if lowercase_name.endswith(EDITOR_TEMPORARY_SUFFIXES):
        return True
    if lowercase_name == ".env" or lowercase_name.startswith(".env."):
        return True
    if lowercase_name in SENSITIVE_FILENAMES:
        return True
    if lowercase_name.startswith(("client_secret", "service-account")) and (
        path.suffix.casefold() == ".json"
    ):
        return True
    if (
        (".aws" in lowercase_parts and lowercase_name == "credentials")
        or (
            ".cargo" in lowercase_parts
            and lowercase_name in {"credentials", "credentials.toml"}
        )
        or (".kube" in lowercase_parts and lowercase_name == "config")
        or (".docker" in lowercase_parts and lowercase_name == "config.json")
    ):
        return True
    if lowercase_name.startswith(("credential.", "credentials.", "secret.", "secrets.")):
        return True
    if path.suffix.casefold() in SENSITIVE_SUFFIXES:
        return True
    return any(
        fnmatch.fnmatch(lowercase_path, pattern.casefold())
        for pattern in PRIVATE_PATH_GLOBS
    )


def verify_boundary(
    repository_root: Path,
    tracked_paths: tuple[Path, ...],
    ignored_paths: frozenset[str],
) -> list[BoundaryFinding]:
    findings = [
        BoundaryFinding(item.rule, item.path, item.message)
        for item in verify_project_license(repository_root)
    ]

    notice_path = repository_root / "NOTICE"
    if not notice_path.is_file():
        findings.append(BoundaryFinding("asset-notice", "NOTICE", "file is missing"))
    else:
        notice_text = notice_path.read_text(encoding="utf-8")
        for asset_path in sorted(FIRST_PARTY_ASSET_ALLOWLIST):
            if asset_path not in notice_text:
                findings.append(
                    BoundaryFinding(
                        "asset-notice",
                        "NOTICE",
                        f"first-party asset is not declared: {asset_path}",
                    )
                )
    third_party_notice = "THIRD_PARTY_ASSETS/NOTICE"
    if not (repository_root / third_party_notice).is_file():
        findings.append(
            BoundaryFinding("asset-notice", third_party_notice, "file is missing")
        )
    else:
        notice_text = (repository_root / third_party_notice).read_text(encoding="utf-8")
        declared_paths = set(THIRD_PARTY_ASSET_LICENSES) | set(THIRD_PARTY_ASSET_LICENSES.values())
        for declared_path in sorted(declared_paths):
            if declared_path not in notice_text:
                findings.append(
                    BoundaryFinding(
                        "asset-notice",
                        third_party_notice,
                        f"third-party asset or license is not declared: {declared_path}",
                    )
                )

    conflicting_license = repository_root / "license" / "license.txt"
    if conflicting_license.exists():
        findings.append(
            BoundaryFinding(
                "conflicting-license",
                "license/license.txt",
                "retired license file conflicts with the root project license",
            )
        )

    tracked_path_names = {path.as_posix() for path in tracked_paths}
    declared_asset_paths = (
        ("first-party-asset", FIRST_PARTY_ASSET_ALLOWLIST),
        ("third-party-asset", THIRD_PARTY_ASSET_LICENSES),
        ("third-party-asset-license", set(THIRD_PARTY_ASSET_LICENSES.values())),
    )
    for rule, asset_paths in declared_asset_paths:
        for asset_path in sorted(asset_paths):
            if not (repository_root / asset_path).is_file():
                findings.append(
                    BoundaryFinding(
                        rule,
                        asset_path,
                        "declared build asset or license is missing from the working tree",
                    )
                )
            elif asset_path not in tracked_path_names:
                findings.append(
                    BoundaryFinding(
                        rule,
                        asset_path,
                        "declared build asset or license is not tracked",
                    )
                )

    existing_findings = {(finding.rule, finding.path) for finding in findings}
    findings.extend(
        BoundaryFinding(finding.rule, finding.path, finding.message)
        for finding in verify_third_party_asset_bytes(repository_root, tracked_paths)
        if (finding.rule, finding.path) not in existing_findings
    )

    for path in tracked_paths:
        normalized_path = path.as_posix()
        if is_unapproved_asset_path(path):
            findings.append(
                BoundaryFinding(
                    "tracked-media",
                    normalized_path,
                    "asset or binary is not approved for source distribution",
                )
            )
        if normalized_path in ignored_paths:
            findings.append(
                BoundaryFinding(
                    "tracked-ignored-path",
                    normalized_path,
                    "path is tracked even though repository ignore rules classify it as local",
                )
            )
        elif is_sensitive_or_private_path(path):
            findings.append(
                BoundaryFinding(
                    "tracked-sensitive-path",
                    normalized_path,
                    "sensitive or private artifact remains tracked",
                )
            )
        source_path = repository_root / path
        try:
            payload = read_source_payload(source_path, repository_root=repository_root)
        except SourceReadError as error:
            findings.append(
                BoundaryFinding(error.rule, normalized_path, "source could not be safely inspected")
            )
            continue
        if b"\0" not in payload and DEVELOPER_MACHINE_PATH.search(payload):
            findings.append(
                BoundaryFinding(
                    "developer-machine-path",
                    normalized_path,
                    "source contains a developer-specific absolute filesystem path",
                )
            )
        for finding in scan_product_payload(path, payload):
            findings.append(
                BoundaryFinding(
                    "prohibited-product-surface",
                    finding.path.as_posix(),
                    f"{finding.rule} at line {finding.line}",
                )
            )
    return findings


def verify_public_snapshot_contract(
    tracked_paths: tuple[Path, ...],
) -> list[BoundaryFinding]:
    findings: list[BoundaryFinding] = []
    tracked_path_names = {path.as_posix() for path in tracked_paths}

    for required_path in sorted(PUBLIC_SNAPSHOT_REQUIRED_PATHS):
        if required_path not in tracked_path_names:
            findings.append(
                BoundaryFinding(
                    "public-snapshot-required-path",
                    required_path,
                    "required public snapshot path is not tracked",
                )
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
    for path, expected_reason in sorted(expected_reasons.items()):
        if path not in tracked_path_names:
            continue
        actual_reason = exclusion_reason(Path(path), frozenset())
        if actual_reason != expected_reason:
            findings.append(
                BoundaryFinding(
                    "public-snapshot-exclusion",
                    path,
                    f"expected {expected_reason!r}, found {actual_reason!r}",
                )
            )

    for path in sorted(tracked_path_names):
        if not any(
            path.startswith(prefix)
            for prefix in PUBLIC_SNAPSHOT_INTERNAL_LIFECYCLE_PREFIXES
        ):
            continue
        actual_reason = exclusion_reason(Path(path), frozenset())
        if actual_reason != "internal-lifecycle":
            findings.append(
                BoundaryFinding(
                    "public-snapshot-exclusion",
                    path,
                    f"expected 'internal-lifecycle', found {actual_reason!r}",
                )
            )

    return findings


def main() -> int:
    try:
        tracked_paths = tracked_worktree_paths(REPOSITORY_ROOT)
        ignored_paths = ignored_tracked_paths(REPOSITORY_ROOT, tracked_paths)
        findings = verify_boundary(REPOSITORY_ROOT, tracked_paths, ignored_paths)
        findings.extend(verify_public_snapshot_contract(tracked_paths))
    except (OSError, RuntimeError) as error:
        print(f"source-publication boundary verification failed: {error}", file=sys.stderr)
        return 1

    for finding in findings:
        print(f"{finding.path}: {finding.rule}: {finding.detail}")
    if findings:
        print(f"source-publication boundary verification failed with {len(findings)} finding(s)")
        return 1
    print("source-publication boundary verification passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
