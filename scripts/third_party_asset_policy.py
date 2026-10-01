from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path

from scripts.publication_source_io import SourceReadError, read_source_payload


# Unmodified Inter 4.1 release files; provenance and terms are in THIRD_PARTY_ASSETS/NOTICE.
INTER_LICENSE_PATH = "apps/desktop/public/fonts/inter/OFL.txt"
THIRD_PARTY_ASSET_LICENSES = {
    "apps/desktop/src/assets/fonts/InterVariable.woff2": INTER_LICENSE_PATH,
    "apps/desktop/src/assets/fonts/InterVariable-Italic.woff2": INTER_LICENSE_PATH,
}
THIRD_PARTY_ASSET_SHA256 = {
    "apps/desktop/src/assets/fonts/InterVariable.woff2": "693b77d4f32ee9b8bfc995589b5fad5e99adf2832738661f5402f9978429a8e3",
    "apps/desktop/src/assets/fonts/InterVariable-Italic.woff2": "e564f652916db6c139570fefb9524a77c4d48f30c92928de9db19b6b5c7a262a",
    INTER_LICENSE_PATH: "262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a",
}


@dataclass(frozen=True)
class ThirdPartyAssetFinding:
    rule: str
    path: str
    message: str


def verify_third_party_asset_bytes(
    repository_root: Path, relative_paths: tuple[Path, ...]
) -> list[ThirdPartyAssetFinding]:
    """Verify selected assets and legal texts, without requiring unrelated build assets."""
    selected = {path.as_posix() for path in relative_paths}
    findings: list[ThirdPartyAssetFinding] = []
    missing_licenses = {
        license_path
        for asset_path, license_path in THIRD_PARTY_ASSET_LICENSES.items()
        if asset_path in selected and license_path not in selected
    }
    for license_path in sorted(missing_licenses):
        findings.append(ThirdPartyAssetFinding(
            "third-party-asset-license", license_path,
            "selected third-party font requires its license in the same snapshot",
        ))
    for path in sorted(selected & THIRD_PARTY_ASSET_SHA256.keys()):
        try:
            payload = read_source_payload(repository_root / path, repository_root=repository_root)
        except SourceReadError as error:
            findings.append(ThirdPartyAssetFinding(error.rule, path, "asset could not be safely inspected"))
            continue
        if hashlib.sha256(payload).hexdigest() != THIRD_PARTY_ASSET_SHA256[path]:
            findings.append(ThirdPartyAssetFinding(
                "third-party-asset-integrity", path,
                "file bytes differ from the reviewed third-party asset or license",
            ))
    return findings
