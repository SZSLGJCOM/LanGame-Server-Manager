from __future__ import annotations

import argparse
from collections import defaultdict
from dataclasses import dataclass
import hashlib
import os
from pathlib import Path
import subprocess
import sys
from typing import BinaryIO, TextIO


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts.open_source_product_policy import scan_product_payload
from scripts.verify_no_tracked_secrets import scan_payload
from scripts.third_party_asset_policy import ARK_MAP_ARTWORK_NOTICE, REFERENCE_MEDIA_SHA256, THIRD_PARTY_REFERENCE_MEDIA
from scripts.verify_open_source_boundary import (
    DEVELOPER_MACHINE_PATH,
    ignored_tracked_paths,
    is_sensitive_or_private_path,
    is_unapproved_asset_path,
)

# Reviewed synthetic credentials and embedded console prompts in ca33c10,
# plus the loopback URL redaction fixture in 6bd28c4.
# Current fixtures use recognized placeholders. Historical exceptions apply only
# to these immutable blobs and assigned-secret locations, never other findings.
SYNTHETIC_FIXTURE_ASSIGNMENTS = {
    ("apps/desktop/tests/runtime-browser-startup.test.cjs", "82def1fe05dbae1b104a3850a7d700e55b8bebc2"): {10},
    ("apps/desktop/src-tauri/src/steam_workshop_account_sessions_tests.rs", "a485bf8b0e406cd02ed2ee87727a2d9a79eb5b45"): {287},
    ("crates/app-runtime/src/interactive_console_tests.rs", "06624529f340dc9687ebc02b0534beee06cf9e8d"): {100},
    ("crates/app-steamcmd/src/steamcmd_account_bridge_tests.rs", "77ea25a15d1fe4383604702a5918fc20c6b1a943"): {81},
    ("crates/app-steamcmd/src/steamcmd_account_session_tests.rs", "491c0bc06d77359ee4d69e934954ab9401e3e716"): {20, 94, 479},
    ("crates/app-storage/src/native_bans_tests.rs", "13fc79752771f262e3287237c4b261d7562b9ab2"): {411},
}

# Two non-secret scratch paths in a superseded documentation command example.
# Current documentation uses portable paths; changed blobs receive no exception.
HISTORICAL_DOCUMENTATION_PATH_EXAMPLES = {
    ("docs/ark-map-artwork-distribution.md", "4fb5d2764ec2809a941ee519d84cb4ec70809450"): {30, 31},
}


@dataclass(frozen=True)
class HistoricalFileVersion:
    commit: str
    path: Path
    object_id: str


@dataclass(frozen=True)
class HistoryFinding:
    commit: str
    path: Path
    line: int
    rule: str


@dataclass(frozen=True)
class HistoryScanResult:
    commit_count: int
    file_version_count: int
    findings: tuple[HistoryFinding, ...]


def _git_environment() -> dict[str, str]:
    environment = os.environ.copy()
    # Publication checks must inspect stored objects, not local replacement views.
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    return environment


def _run_git(repository_root: Path, arguments: list[str]) -> bytes:
    result = subprocess.run(
        ["git", *arguments],
        cwd=repository_root,
        env=_git_environment(),
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        command = " ".join(arguments[:2])
        raise RuntimeError(
            f"git {command} failed with exit status {result.returncode}"
        )
    return result.stdout


def _reachable_commits(repository_root: Path) -> tuple[str, ...]:
    if _run_git(repository_root, ["rev-parse", "--is-shallow-repository"]).strip() == b"true":
        raise RuntimeError(
            "history verification requires a complete repository; shallow history cannot be verified"
        )
    output = _run_git(repository_root, ["rev-list", "--all"])
    return tuple(line.decode("ascii") for line in output.splitlines() if line)


def _tree_file_versions(
    repository_root: Path, commit: str
) -> tuple[HistoricalFileVersion, ...]:
    output = _run_git(
        repository_root,
        ["ls-tree", "-r", "-z", "--full-tree", commit],
    )
    versions: list[HistoricalFileVersion] = []
    for record in output.split(b"\0"):
        if not record:
            continue
        header, separator, raw_path = record.partition(b"\t")
        fields = header.split()
        if not separator or len(fields) != 3 or fields[1] != b"blob":
            continue
        versions.append(
            HistoricalFileVersion(
                commit=commit,
                path=Path(raw_path.decode("utf-8", errors="replace")),
                object_id=fields[2].decode("ascii"),
            )
        )
    return tuple(versions)


def _unique_file_versions(
    repository_root: Path, commits: tuple[str, ...]
) -> tuple[tuple[HistoricalFileVersion, ...], set[HistoryFinding]]:
    versions: dict[tuple[str, Path], HistoricalFileVersion] = {}
    findings: set[HistoryFinding] = set()
    for commit in commits:
        tree = _tree_file_versions(repository_root, commit)
        tree_paths = {version.path.as_posix() for version in tree}
        if tree_paths & THIRD_PARTY_REFERENCE_MEDIA and ARK_MAP_ARTWORK_NOTICE not in tree_paths:
            findings.add(HistoryFinding(commit, Path(ARK_MAP_ARTWORK_NOTICE), 0, "historical-reference-media-notice"))
        for version in tree:
            versions.setdefault((version.object_id, version.path), version)
    return tuple(versions.values()), findings


def _history_messages(
    repository_root: Path, commits: tuple[str, ...]
) -> tuple[tuple[str, str, bytes], ...]:
    messages: list[tuple[str, str, bytes]] = []
    for commit in commits:
        payload = _run_git(repository_root, ["cat-file", "commit", commit])
        messages.append((commit, "commit-message.txt", payload.partition(b"\n\n")[2]))

    tag_refs = _run_git(
        repository_root,
        ["for-each-ref", "--format=%(objecttype) %(objectname)", "refs/tags"],
    )
    pending = [
        fields[1].decode("ascii")
        for line in tag_refs.splitlines()
        if len(fields := line.split()) == 2 and fields[0] == b"tag"
    ]
    visited: set[str] = set()
    while pending:
        object_id = pending.pop()
        if object_id in visited:
            continue
        visited.add(object_id)
        payload = _run_git(repository_root, ["cat-file", "tag", object_id])
        header, separator, message = payload.partition(b"\n\n")
        if not separator:
            raise RuntimeError("Git tag object has no message boundary")
        messages.append((object_id, "tag-message.txt", message))
        fields = dict(line.split(b" ", 1) for line in header.splitlines() if b" " in line)
        # An annotated tag may point to another tag, not directly to a commit.
        if fields.get(b"type") == b"tag":
            pending.append(fields[b"object"].decode("ascii"))
    return tuple(messages)


def _scan_history_message(
    object_id: str, name: str, payload: bytes
) -> tuple[HistoryFinding, ...]:
    path = Path("git-metadata") / name
    findings = [
        HistoryFinding(object_id, path, finding.line, f"history-secret:{finding.rule}")
        for finding in scan_payload(path, payload)
    ]
    machine_path = DEVELOPER_MACHINE_PATH.search(payload)
    if machine_path is not None:
        findings.append(
            HistoryFinding(
                object_id,
                path,
                payload.count(b"\n", 0, machine_path.start()) + 1,
                "developer-machine-path",
            )
        )
    return tuple(findings)


class GitBlobReader:
    def __init__(self, repository_root: Path) -> None:
        self._process = subprocess.Popen(
            ["git", "cat-file", "--batch"],
            cwd=repository_root,
            env=_git_environment(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )

    def __enter__(self) -> GitBlobReader:
        return self

    def __exit__(
        self,
        error_type: type[BaseException] | None,
        _error: BaseException | None,
        _traceback: object,
    ) -> None:
        if self._process.stdin is not None:
            self._process.stdin.close()
        if error_type is not None and self._process.poll() is None:
            self._process.terminate()
        self._process.wait()
        if self._process.stdout is not None:
            self._process.stdout.close()

    def read(self, object_id: str) -> bytes:
        input_stream = self._required_stream(self._process.stdin, "stdin")
        output_stream = self._required_stream(self._process.stdout, "stdout")
        input_stream.write(object_id.encode("ascii") + b"\n")
        input_stream.flush()

        header = output_stream.readline().rstrip(b"\n")
        fields = header.split()
        if len(fields) != 3 or fields[0] != object_id.encode("ascii"):
            raise RuntimeError("git cat-file returned an invalid object header")
        if fields[1] != b"blob":
            raise RuntimeError("git cat-file returned a non-blob object")
        try:
            size = int(fields[2])
        except ValueError as error:
            raise RuntimeError("git cat-file returned an invalid object size") from error
        payload = output_stream.read(size)
        terminator = output_stream.read(1)
        if len(payload) != size or terminator != b"\n":
            raise RuntimeError("git cat-file returned a truncated blob")
        return payload

    @staticmethod
    def _required_stream(stream: BinaryIO | None, name: str) -> BinaryIO:
        if stream is None:
            raise RuntimeError(f"git cat-file {name} is unavailable")
        return stream


def _scan_file_version(
    version: HistoricalFileVersion,
    payload: bytes,
    ignored_paths: frozenset[str],
) -> tuple[HistoryFinding, ...]:
    findings: list[HistoryFinding] = []
    path = version.path
    normalized_path = path.as_posix()

    if normalized_path in ignored_paths or is_sensitive_or_private_path(path):
        findings.append(
            HistoryFinding(version.commit, path, 0, "historical-private-path")
        )

    if is_unapproved_asset_path(path):
        findings.append(HistoryFinding(version.commit, path, 0, "historical-media"))
    if normalized_path in REFERENCE_MEDIA_SHA256 and hashlib.sha256(payload).hexdigest() != REFERENCE_MEDIA_SHA256[normalized_path]:
        findings.append(HistoryFinding(version.commit, path, 0, "historical-reference-media-integrity"))

    for product_finding in scan_product_payload(path, payload):
        findings.append(
            HistoryFinding(
                version.commit,
                path,
                product_finding.line,
                product_finding.rule,
            )
        )

    if b"\0" not in payload:
        for machine_path in DEVELOPER_MACHINE_PATH.finditer(payload):
            line = payload.count(b"\n", 0, machine_path.start()) + 1
            if line in HISTORICAL_DOCUMENTATION_PATH_EXAMPLES.get((normalized_path, version.object_id), ()):
                continue
            findings.append(
                HistoryFinding(
                    version.commit,
                    path,
                    line,
                    "developer-machine-path",
                )
            )

    for secret_finding in scan_payload(path, payload):
        if (secret_finding.rule == "assigned-secret" and secret_finding.line in
                SYNTHETIC_FIXTURE_ASSIGNMENTS.get((normalized_path, version.object_id), ())):
            continue
        findings.append(
            HistoryFinding(
                version.commit,
                path,
                secret_finding.line,
                f"history-secret:{secret_finding.rule}",
            )
        )
    return tuple(findings)


def scan_reachable_history(repository_root: Path = REPOSITORY_ROOT) -> HistoryScanResult:
    repository_root = repository_root.resolve()
    commits = _reachable_commits(repository_root)
    versions, findings = _unique_file_versions(repository_root, commits)
    paths = tuple({version.path for version in versions})
    ignored_paths = ignored_tracked_paths(repository_root, paths)
    versions_by_object: dict[str, list[HistoricalFileVersion]] = defaultdict(list)
    for version in versions:
        versions_by_object[version.object_id].append(version)

    for object_id, name, payload in _history_messages(repository_root, commits):
        findings.update(_scan_history_message(object_id, name, payload))
    with GitBlobReader(repository_root) as reader:
        for object_id, object_versions in versions_by_object.items():
            payload = reader.read(object_id)
            for version in object_versions:
                findings.update(_scan_file_version(version, payload, ignored_paths))

    ordered_findings = tuple(
        sorted(
            findings,
            key=lambda finding: (
                finding.commit,
                finding.path.as_posix(),
                finding.line,
                finding.rule,
            ),
        )
    )
    return HistoryScanResult(
        commit_count=len(commits),
        file_version_count=len(versions),
        findings=ordered_findings,
    )


def run_scan(
    repository_root: Path = REPOSITORY_ROOT, output: TextIO | None = None
) -> int:
    stream = output if output is not None else sys.stdout
    result = scan_reachable_history(repository_root)
    for finding in result.findings:
        location = finding.path.as_posix()
        if finding.line:
            location = f"{location}:{finding.line}"
        print(
            f"{finding.commit[:12]}:{location}: {finding.rule}",
            file=stream,
        )
    if result.findings:
        print(
            "open-source history scan failed with "
            f"{len(result.findings)} finding(s) across "
            f"{result.commit_count} reachable commit(s)",
            file=stream,
        )
        return 1
    print(
        "open-source history scan passed "
        f"({result.commit_count} reachable commit(s), "
        f"{result.file_version_count} unique file version(s))",
        file=stream,
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Scan every reachable Git commit for open-source boundary risks."
    )
    parser.add_argument(
        "--repository",
        type=Path,
        default=REPOSITORY_ROOT,
        help="Git repository to scan (defaults to the project root).",
    )
    arguments = parser.parse_args()
    try:
        return run_scan(arguments.repository)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"open-source history scan could not run: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
