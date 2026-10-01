from __future__ import annotations

from dataclasses import dataclass
import fnmatch
from pathlib import Path
import re

from scripts.publication_source_io import SourceReadError, read_source_payload


PROHIBITED_PRODUCT_PATH_GLOBS = (
    "apps/desktop/src/auth/**",
    "apps/desktop/src/**/auth/**",
    "apps/desktop/src/auth-core.*",
    "apps/desktop/src/**/auth-core.*",
    "apps/desktop/src/login/**",
    "apps/desktop/src/**/login/**",
    "apps/desktop/src/**/*auth*context*",
    "apps/desktop/src/**/*auth*login*",
    "apps/desktop/src/**/*vip*access*",
    "apps/desktop/src/assets/*login*",
    "apps/desktop/src/assets/*vip*",
    "apps/desktop/src/assets/privacy.md",
    "apps/desktop/src/assets/terms.md",
    "apps/desktop/src-tauri/src/auth_service.rs",
    "apps/desktop/src-tauri/src/auth/**",
    "apps/desktop/tests/*vip*",
    "artifacts/**/*login*",
    "services/auth-server/**",
    "services/account-service/**",
    "services/payment-service/**",
    "docs/auth-system*.md",
    "docs/langame-account-service*.md",
    "docs/*account-authority*.md",
    "docker-compose.auth.yml",
)
PRODUCT_SOURCE_PREFIXES = (
    "apps/desktop/src/",
    "apps/desktop/src-tauri/src/",
    "crates/",
    "docs/",
    "migrations/",
    "services/",
)
PROHIBITED_PRODUCT_MARKERS = (
    re.compile(rb"(?i)\bcloud_account_id\b"),
    re.compile(rb"(?i)\bauth_login_screen\b|\bAuthLoginScreen\b"),
    re.compile(rb"(?i)\bauth_account_settings\b|\bAuthAccountSettingsCard\b"),
    re.compile(rb"(?i)\b(?:auth_service|account_service|account_verification)\b"),
    re.compile(rb"(?i)\b(?:vip|vip_access|is_vip|vip_tier|vip_expires_at)\b"),
    re.compile(
        rb"(?i)\b(?:has_membership|is_member|membership_tier|membership_level|membership_expires_at)\b"
    ),
    re.compile(rb"(?i)\b(?:recharge_dialog|recharge_order|payment_order|wechat_login)\b"),
)


@dataclass(frozen=True)
class ProductPolicyFinding:
    path: Path
    line: int
    rule: str


def _matches_prohibited_path(path: Path) -> bool:
    normalized = path.as_posix().casefold()
    return any(
        fnmatch.fnmatch(normalized, pattern.casefold())
        for pattern in PROHIBITED_PRODUCT_PATH_GLOBS
    )


def scan_product_payload(
    relative_path: Path, payload: bytes
) -> list[ProductPolicyFinding]:
    if _matches_prohibited_path(relative_path):
        return [ProductPolicyFinding(relative_path, 0, "prohibited-product-path")]

    normalized = relative_path.as_posix()
    if not normalized.casefold().startswith(PRODUCT_SOURCE_PREFIXES) or b"\0" in payload:
        return []

    for pattern in PROHIBITED_PRODUCT_MARKERS:
        match = pattern.search(payload)
        if match is not None:
            return [
                ProductPolicyFinding(
                    relative_path,
                    payload.count(b"\n", 0, match.start()) + 1,
                    "prohibited-product-marker",
                )
            ]
    return []


def scan_product_surfaces(
    repository_root: Path, relative_paths: tuple[Path, ...]
) -> list[ProductPolicyFinding]:
    findings: list[ProductPolicyFinding] = []
    for relative_path in relative_paths:
        try:
            payload = read_source_payload(
                repository_root / relative_path, repository_root=repository_root
            )
        except SourceReadError as error:
            findings.append(ProductPolicyFinding(relative_path, 1, error.rule))
            continue
        findings.extend(scan_product_payload(relative_path, payload))
    return findings
