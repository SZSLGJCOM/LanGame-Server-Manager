from __future__ import annotations

import codecs
from dataclasses import dataclass
import hashlib
from pathlib import Path
import re
import subprocess
import sys


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts.publication_source_io import SourceReadError, read_source_payload, source_is_present
from scripts.source_literal_scan import (
    SOURCE_CODE_SUFFIXES,
    SourceLiteralState,
    match_is_literal,
    source_literal_mask,
)


SCOPED_SECRET_NAME = (
    r"(?:aws[_-]?secret[_-]?access[_-]?key|aws[_-]?session[_-]?token|"
    r"client[_-]?secret|webhook[_-]?secret|signing[_-]?secret|api[_-]?key|"
    r"access[_-]?token|auth[_-]?token|refresh[_-]?token|server[_-]?token|"
    r"lan[_-]?token|private[_-]?key|encryption[_-]?key|rcon[_-]?password|"
    r"admin[_-]?password|join[_-]?password|password)"
)
BARE_SECRET_NAME = r"(?<![a-z0-9_-])(?:secret|token)(?![a-z0-9_-])"
SECRET_NAME = rf"(?:{SCOPED_SECRET_NAME}|{BARE_SECRET_NAME})"
SECRET_REFERENCE = (
    r"(?:\{\{[^{}\r\n]+\}\}|\$\{[^{}\r\n]+\}|"
    r"\{[a-z_][a-z0-9_.:-]*\[[^\]\r\n]+\]\}|"
    r"\{[a-z_][a-z0-9_.:-]*\}|%[a-z_][a-z0-9_]*%|"
    r"\$env:[a-z_][a-z0-9_]*)"
)
ASSIGNED_SECRET = re.compile(
    rf"(?i)(?P<name>{SECRET_NAME})\s*(?:\\?[\"'])?\s*[:=]\s*"
    rf"(?:(?P<reference>{SECRET_REFERENCE})|"
    rf"\\(?P<escaped_quote>[\"'`])(?P<escaped_quoted>[^\r\n]*?)"
    rf"\\(?P=escaped_quote)|"
    rf"(?P<quote>[\"'`])(?P<quoted>[^\r\n]*?)(?P=quote)|"
    rf"(?P<unquoted>[^\s\"'`,;}}\]]+))"
)
CLI_SECRET = re.compile(
    rf"(?i)--(?P<name>{SECRET_NAME.replace('[_-]?', '[-_]?')})(?:=|\s+)\s*"
    rf"(?:(?P<reference>{SECRET_REFERENCE})|"
    rf"\\(?P<escaped_quote>[\"'`])(?P<escaped_quoted>[^\r\n]*?)"
    rf"\\(?P=escaped_quote)|"
    rf"(?P<quote>[\"'`])(?P<quoted>[^\r\n]*?)(?P=quote)|"
    rf"(?P<unquoted>[^\s\"'`,;}}\]]+))"
)
LAN_TOKEN = re.compile(r"(?i)langameToken=[A-Za-z0-9_-]{24,}")
PEM_PRIVATE_KEY = re.compile(
    r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----"
)
PROVIDER_TOKEN = re.compile(
    r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b|"
    r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})\b|"
    r"\bsk-[A-Za-z0-9_-]{20,}\b|"
    r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b|"
    r"\bsk_live_[A-Za-z0-9]{16,}\b|"
    r"\bAIza[A-Za-z0-9_-]{35}\b|"
    r"(?<![A-Za-z0-9_-])eyJ[A-Za-z0-9_-]{8,}\."
    r"[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?![A-Za-z0-9_-])"
)
URL_CREDENTIAL = re.compile(
    r"(?i)\b(?:https?|postgres(?:ql)?|mysql|mariadb|mongodb(?:\+srv)?|"
    r"redis(?:s)?|mssql|sqlserver|cockroachdb|amqp(?:s)?)://"
    r"(?P<username>[^\s/:@\"'`]*):(?P<password>[^\s/@\"'`]+)@[^\s/\"'`]+"
)
HTTP_AUTHORIZATION = re.compile(
    r"(?i)\b(?:proxy[-_])?authorization\s*(?:\\?[\"'])?\s*[:=]\s*"
    r"(?:\\?[\"'`])?(?:bearer|basic)\s+"
    r"(?P<credential>[A-Za-z0-9+/_=.-]{8,})"
)
SAFE_PLACEHOLDERS = frozenset(
    {
        "<redacted>",
        "acceptance-admin",
        "acceptance-corekeeper",
        "acceptance-enshrouded-admin",
        "acceptance-minecraft-rcon",
        "acceptance-necesse",
        "acceptance-pass",
        "acceptance-pz",
        "acceptance-pz-admin",
        "acceptance-pz-rcon",
        "acceptance-rcon",
        "acceptance-rust-rcon",
        "acceptance-sotf",
        "acceptance-squad-rcon",
        "acceptance-valheim",
        "acceptance-vrising",
        "acceptance-vrising-rcon",
        "admin-pass",
        "admin password",
        "admin_password",
        "asa-admin",
        "asa-ai-smoke",
        "change-me",
        "change-me-admin",
        "change-me-corekeeper",
        "change-me-friend",
        "change-me-guest",
        "change-me-necesse",
        "change-me-rcon",
        "change-me-telnet",
        "change-me-visitor",
        "console-pass",
        "fixture-client-secret",
        "fixture-admin-password",
        "fixture-dst-safe-pass",
        "fixture-generated-valid-value",
        "fixture-observer-mode",
        "fixture-password",
        "fixture-pz-rcon-safe",
        "fixture-pz-safe-pass",
        "fixture-short",
        "fixture-server-password",
        "forest-pass",
        "greenleaf",
        "helper-pass",
        "join-pass",
        "horde-night",
        "lanadmin",
        "live-token-value",
        "moria-pass",
        "pal-admin-safe",
        "pal-safe-pass",
        "ban_after_wrong_password",
        "server_password",
        "smoke-admin",
        "smoke-world",
        "stonegate",
        "join password",
        "room password",
        "world-pass",
    }
)
SAFE_REFERENCE = re.compile(rf"(?i){SECRET_REFERENCE}")
SAFE_TEST_PLACEHOLDERS = frozenset(
    {"previous-key", "replacement-fixture-key", "synthetic-test-only", "synthetic-fixture"}
)
SAFE_TEST_URL_CREDENTIALS = frozenset(
    {("user", "password"), ("user", "pass"), ("user", "fixture")}
)
TEST_SOURCE_NAME = re.compile(
    r"(?:\.test\.(?:[cm]?js|tsx?)|(?:^|_)tests\.rs|^test_[a-z0-9_]+\.py)$", re.I
)
RIMWORLD_ACCEPTANCE_PASSWORD_DIGEST = hashlib.sha256(
    b"fixture-server-password"
).hexdigest().upper()
# Native password serialization is verified with known synthetic inputs. Only
# their exact native key, digest and repository-relative fixture path qualify.
NATIVE_PASSWORD_FIXTURE_DIGESTS = {
    "modules/rimworld/config-fixtures/2026-09-28-rimworld_together_latest_release_api.json":
        frozenset({RIMWORLD_ACCEPTANCE_PASSWORD_DIGEST}),
    "docs/game-config-acceptance/rimworld.md":
        frozenset({RIMWORLD_ACCEPTANCE_PASSWORD_DIGEST}),
    "crates/app-storage/src/templates_rimworld_tests.rs":
        frozenset({hashlib.sha256(b"abc").hexdigest().upper()}),
}
TRANSLATION_ENTRY = re.compile(
    r"^\s*(?P<quote>[\"'])(?P<key>[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)+)"
    r"(?P=quote)\s*:\s*"
)
KNOWN_TEXT_SUFFIXES = SOURCE_CODE_SUFFIXES | {
    ".bat",
    ".cfg",
    ".cmd",
    ".conf",
    ".csv",
    ".env",
    ".hbs",
    ".ini",
    ".json",
    ".md",
    ".properties",
    ".ps1",
    ".psd1",
    ".psm1",
    ".reg",
    ".sh",
    ".sql",
    ".toml",
    ".txt",
    ".xml",
    ".yaml",
    ".yml",
}


@dataclass(frozen=True)
class Finding:
    path: Path
    line: int
    rule: str


def _secret_value(match: re.Match[str]) -> tuple[str, int]:
    for group_name in ("reference", "escaped_quoted", "quoted", "unquoted"):
        if match.group(group_name) is not None:
            value = match.group(group_name).strip()
            value = re.split(
                r"\\[rn]|\?(?=[A-Za-z][A-Za-z0-9_-]*=)",
                value,
                maxsplit=1,
            )[0]
            return value, match.start(group_name)
    raise ValueError("secret match did not capture a value")


def _is_safe_secret_value(path: Path, value: str) -> bool:
    return (
        value.casefold() in SAFE_PLACEHOLDERS
        or SAFE_REFERENCE.fullmatch(value) is not None
        # Exact assertion fixtures are safe only in test source, never deployment data.
        or (
            value in SAFE_TEST_PLACEHOLDERS
            and TEST_SOURCE_NAME.search(path.name) is not None
        )
    )


def _is_concrete_literal_secret(
    path: Path, line: str, match: re.Match[str], literal_mask: list[bool]
) -> bool:
    value, value_index = _secret_value(match)
    normalized_name = match.group("name").casefold().replace("_", "").replace("-", "")
    minimum_length = 8 if normalized_name.endswith("password") else 12
    return (
        len(value) >= minimum_length
        and not _is_safe_secret_value(path, value)
        and not (
            match.group("name") == "Password"
            and (
                match.start("name") == 0
                or re.fullmatch(r"[A-Za-z0-9_-]", line[match.start("name") - 1]) is None
            )
            and value in NATIVE_PASSWORD_FIXTURE_DIGESTS.get(path.as_posix(), ())
        )
        and match_is_literal(path, line, value_index, literal_mask)
    )


def _assignment_scan_line(path: Path, line: str) -> str:
    is_translation_source = any(
        part.casefold() == "i18n" for part in path.parts
    ) or path.name.casefold().startswith("i18n-")
    if not is_translation_source:
        return line
    entry = TRANSLATION_ENTRY.match(line)
    # A dotted translation key names a UI label. Its value, surrounding source,
    # and CLI arguments must still be scanned for actual credential assignments.
    if entry is None:
        return line
    return " " * entry.end() + line[entry.end():]


def _decode_text_payload(path: Path, payload: bytes) -> tuple[str | None, str | None]:
    # UTF-32 LE starts with the UTF-16 LE BOM, so check the longer marker first.
    if payload.startswith((codecs.BOM_UTF32_LE, codecs.BOM_UTF32_BE)):
        return payload.decode("utf-32", errors="replace"), None
    if payload.startswith((b"\xff\xfe", b"\xfe\xff")):
        return payload.decode("utf-16", errors="replace"), None
    if b"\0" in payload:
        if path.suffix.casefold() in KNOWN_TEXT_SUFFIXES:
            return None, "nul-byte-text"
        return None, None
    return payload.decode("utf-8-sig", errors="replace"), None


def scan_payload(path: Path, payload: bytes) -> list[Finding]:
    findings: list[Finding] = []
    text, decode_finding = _decode_text_payload(path, payload)
    if decode_finding is not None:
        findings.append(Finding(path=path, line=1, rule=decode_finding))
    if text is None:
        return findings

    literal_state = SourceLiteralState()
    for line_number, line in enumerate(text.splitlines(), start=1):
        literal_mask = source_literal_mask(path, line, literal_state)
        if any(
            _is_concrete_literal_secret(path, line, match, literal_mask)
            for match in CLI_SECRET.finditer(line)
        ):
            findings.append(Finding(path=path, line=line_number, rule="cli-secret"))
        elif any(
            _is_concrete_literal_secret(path, line, match, literal_mask)
            for match in ASSIGNED_SECRET.finditer(_assignment_scan_line(path, line))
        ):
            findings.append(Finding(path=path, line=line_number, rule="assigned-secret"))
        if LAN_TOKEN.search(line):
            findings.append(Finding(path=path, line=line_number, rule="lan-token"))
        if PEM_PRIVATE_KEY.search(line):
            findings.append(Finding(path=path, line=line_number, rule="private-key"))
        if PROVIDER_TOKEN.search(line):
            findings.append(Finding(path=path, line=line_number, rule="provider-token"))
        # URL rejection tests use these exact synthetic pairs. Other values and
        # the same pairs in production/configuration files remain findings.
        if any(
            not (
                TEST_SOURCE_NAME.search(path.name) is not None
                and (match.group("username"), match.group("password"))
                in SAFE_TEST_URL_CREDENTIALS
            )
            for match in URL_CREDENTIAL.finditer(line)
        ):
            findings.append(Finding(path=path, line=line_number, rule="url-credential"))
        if any(
            not _is_safe_secret_value(path, match.group("credential"))
            and match_is_literal(path, line, match.start("credential"), literal_mask)
            for match in HTTP_AUTHORIZATION.finditer(line)
        ):
            findings.append(Finding(path=path, line=line_number, rule="http-authorization"))
    return findings


def scan_paths(
    paths: list[Path], *, repository_root: Path | None = None
) -> list[Finding]:
    findings: list[Finding] = []
    for path in paths:
        try:
            payload = read_source_payload(path, repository_root=repository_root)
        except SourceReadError as error:
            findings.append(Finding(path=path, line=1, rule=error.rule))
            continue
        scan_path = path
        if repository_root is not None:
            try:
                scan_path = path.absolute().relative_to(repository_root.absolute())
            except ValueError:
                pass
        findings.extend(
            Finding(path=path, line=finding.line, rule=finding.rule)
            for finding in scan_payload(scan_path, payload)
        )
    return findings


def tracked_paths(
    repository_root: Path = REPOSITORY_ROOT, *, include_untracked: bool = False
) -> list[Path]:
    repository_root = repository_root.resolve()
    arguments = ["git", "ls-files", "-z"]
    if include_untracked:
        arguments.extend(["--cached", "--others", "--exclude-standard"])
    result = subprocess.run(
        arguments,
        cwd=repository_root,
        check=True,
        stdout=subprocess.PIPE,
    )
    return [
        repository_root / Path(raw.decode("utf-8"))
        for raw in result.stdout.split(b"\0")
        if raw
    ]


def run_scan(repository_root: Path = REPOSITORY_ROOT) -> int:
    repository_root = repository_root.resolve()
    paths = [
        path for path in tracked_paths(repository_root, include_untracked=True)
        if source_is_present(path)
    ]
    findings = scan_paths(paths, repository_root=repository_root)
    for finding in findings:
        try:
            display_path = finding.path.relative_to(repository_root)
        except ValueError:
            display_path = finding.path
        print(f"{display_path}:{finding.line}: {finding.rule}")
    if findings:
        print(
            f"working-tree secret scan failed with {len(findings)} finding(s) "
            f"across {len(paths)} working-tree file(s)"
        )
        return 1
    print(
        f"working-tree secret scan passed "
        f"({len(paths)} tracked and nonignored untracked files)"
    )
    return 0


def main() -> int:
    return run_scan()


if __name__ == "__main__":
    sys.exit(main())
