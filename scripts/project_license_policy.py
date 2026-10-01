from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path, PurePosixPath
import tomllib

from scripts.publication_source_io import SourceReadError, read_source_payload


LICENSE_TITLE = "LanGame Source-Available License 1.0"
LICENSE_SECTIONS = (
    "1. Scope",
    "2. Permitted noncommercial use",
    "3. Commercial use requires separate permission",
    "4. No independent modified releases",
    "5. Contributions",
    "6. Notices and reserved rights",
    "7. Termination",
    "8. Disclaimer",
)
NPM_LICENSE = "SEE LICENSE IN LICENSE"


@dataclass(frozen=True)
class ProjectLicenseFinding:
    rule: str
    path: str
    message: str


def _read(
    root: Path, path: str, rule: str, findings: list[ProjectLicenseFinding]
) -> bytes | None:
    try:
        return read_source_payload(root / path, repository_root=root)
    except SourceReadError as error:
        findings.append(ProjectLicenseFinding(rule, path, error.rule))
        return None


def _document(
    root: Path, path: str, rule: str, findings: list[ProjectLicenseFinding]
) -> dict[str, object] | None:
    payload = _read(root, path, rule, findings)
    if payload is None:
        return None
    try:
        text = payload.decode("utf-8")
        document = tomllib.loads(text) if path.endswith(".toml") else json.loads(text)
    except (UnicodeDecodeError, tomllib.TOMLDecodeError, json.JSONDecodeError):
        findings.append(ProjectLicenseFinding(rule, path, "invalid UTF-8 document"))
        return None
    if not isinstance(document, dict):
        findings.append(ProjectLicenseFinding(rule, path, "document must be an object"))
        return None
    return document


def _verify_cargo(root: Path, findings: list[ProjectLicenseFinding]) -> None:
    document = _document(root, "Cargo.toml", "cargo-license", findings)
    if document is None:
        return
    workspace = document.get("workspace")
    package = workspace.get("package") if isinstance(workspace, dict) else None
    if (
        not isinstance(package, dict)
        or package.get("license-file") != "LICENSE"
        or "license" in package
    ):
        findings.append(ProjectLicenseFinding(
            "cargo-license", "Cargo.toml",
            "workspace.package must declare license-file = LICENSE without license",
        ))
    members = workspace.get("members") if isinstance(workspace, dict) else None
    if not isinstance(members, list) or not members:
        findings.append(ProjectLicenseFinding(
            "cargo-license", "Cargo.toml", "workspace.members must list the local packages",
        ))
        return
    for member in members:
        if (
            not isinstance(member, str)
            or not member
            or PurePosixPath(member).is_absolute()
            or ".." in PurePosixPath(member).parts
            or any(character in member for character in "\\:*?[]")
        ):
            findings.append(ProjectLicenseFinding(
                "cargo-member-license", "Cargo.toml", "workspace member must be a relative literal path",
            ))
            continue
        path = (PurePosixPath(member) / "Cargo.toml").as_posix()
        manifest = _document(root, path, "cargo-member-license", findings)
        if manifest is None:
            continue
        package = manifest.get("package")
        inheritance = package.get("license-file") if isinstance(package, dict) else None
        if (
            not isinstance(package, dict)
            or not isinstance(inheritance, dict)
            or set(inheritance) != {"workspace"}
            or inheritance["workspace"] is not True
            or "license" in package
        ):
            findings.append(ProjectLicenseFinding(
                "cargo-member-license", path,
                "package must inherit license-file.workspace = true without license",
            ))


def _verify_npm(root: Path, findings: list[ProjectLicenseFinding]) -> None:
    path = "apps/desktop/package.json"
    package = _document(root, path, "npm-license", findings)
    if package is not None and (
        package.get("license") != NPM_LICENSE or package.get("private") is not True
    ):
        findings.append(ProjectLicenseFinding(
            "npm-license", path, f"package must be private with license = {NPM_LICENSE}",
        ))
    path = "apps/desktop/package-lock.json"
    lock = _document(root, path, "npm-license", findings)
    if lock is not None:
        packages = lock.get("packages")
        package = packages.get("") if isinstance(packages, dict) else None
        if not isinstance(package, dict) or package.get("license") != NPM_LICENSE:
            findings.append(ProjectLicenseFinding(
                "npm-license", path, f"root package license must be {NPM_LICENSE}",
            ))


def verify_project_license(root: Path) -> list[ProjectLicenseFinding]:
    findings: list[ProjectLicenseFinding] = []
    payload = _read(root, "LICENSE", "root-license", findings)
    if payload is not None:
        try:
            lines = [line.strip() for line in payload.decode("utf-8").splitlines() if line.strip()]
        except UnicodeDecodeError:
            lines = []
        if (
            not lines
            or lines[0] != LICENSE_TITLE
            or any(section not in lines for section in LICENSE_SECTIONS)
        ):
            findings.append(ProjectLicenseFinding(
                "root-license", "LICENSE", "source-available license title or required sections are missing",
            ))
    npm_payload = _read(root, "apps/desktop/LICENSE", "npm-license-copy", findings)
    if payload is not None and npm_payload is not None and payload != npm_payload:
        findings.append(ProjectLicenseFinding(
            "npm-license-copy", "apps/desktop/LICENSE", "must be byte-for-byte identical to the root LICENSE",
        ))
    _verify_cargo(root, findings)
    _verify_npm(root, findings)
    return findings
