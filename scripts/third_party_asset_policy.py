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

# Reference game media keeps its upstream rights notice, separately from licensed
# build assets. This records provenance and bytes; it does not grant a license.
ARK_MAP_ARTWORK_NOTICE = "assets/ark-map-artwork/2026-10-25/NOTICE.md"
REFERENCE_MEDIA_SHA256 = {
    "assets/ark-map-artwork/2026-10-25/Genesis_Part_2_Map.jpg": "9059904fce44211652ce98148c3dc7e1510d474f0cfa7b37aa4ac9b8a265abe6",
    "assets/ark-map-artwork/2026-10-25/The_Island_Topographic_Map.jpg": "55e2b99e2c6b39c0332460304a5827c078164a1319c58c4d70f349719cf25965",
    "assets/ark-map-artwork/2026-10-25/Scorched_Earth_Topographic_Map.jpg": "43d6c3a4e5ac96ad1779158892c492d86650ccce576ccb5f16515f8f6317e9b7",
    "assets/ark-map-artwork/2026-10-25/The_Center_Topographic_Map.jpg": "bb94b36a0494749b62af288085fb19f9454a8b122974a35a9b281c6b4a6de694",
    "assets/ark-map-artwork/2026-10-25/Ragnarok_Ocean_Topographic_Map.jpg": "281de05c8263282617275f318800c94acc6e3e43f62ae1deddded9b9b0437d36",
    "assets/ark-map-artwork/2026-10-25/Aberration_Topographic_Map.jpg": "48d64aad2b5e2f9e880af449b8fe1da2ac1c973716ca9f8be132f978c69a50f8",
    "assets/ark-map-artwork/2026-10-25/Extinction_Topographic_Map.jpg": "4f9f4e0c27de54a20e82bf9817b131778e560104581580e09c3798c3a065668b",
    "assets/ark-map-artwork/2026-10-25/Valguero_Topographic_Map.jpg": "2496b196d8517dac6211c97be7f75bd911be476f2eab94979b4457eece16f6e3",
    "assets/ark-map-artwork/2026-10-25/Crystal_Isles_Topographic_Map.jpg": "30a4c8d304a52938de1818e1bd891df1df35a3a08499e6b873542af82d2add66",
    "assets/ark-map-artwork/2026-10-25/Lost_Island_Map.jpg": "104cba48f310725e8a56ad9eafbe064d724ef46e08e3fceb5fd93ce076859fe5",
    "assets/ark-map-artwork/2026-10-25/The_Island_map_ASA.jpg": "c5938891e42e0d639dd1f2732236769f935bae827684e262871ded0d08524d50",
    "assets/ark-map-artwork/2026-10-25/Scorched_Earth_map_ASA.jpg": "5d5c1ce1f9c8b36cd5ebb0fc3f196181242592a0d1da8429826ef6006cf0f60c",
    "assets/ark-map-artwork/2026-10-25/The_Center_map_ASA.jpg": "591c54c198bdc5f2899f716d52a9095f0bebc5fcb4485a1def14b1314b480bd1",
    "assets/ark-map-artwork/2026-10-25/Ragnarok_map_ASA.jpg": "a9c7ea7e60d0ccc2117e3867bc010174b90b4c75c32049a98d91312d63144283",
    "assets/ark-map-artwork/2026-10-25/Aberration_map_ASA.jpg": "56f8b34f5f014cdf6fa7b4593fe5ebfafc227d1022f53e3b16ce368059a20913",
    "assets/ark-map-artwork/2026-10-25/Extinction_map_ASA.jpg": "ba2898f380595597fd45c4dabe5309061ff492430b2f748c3373c85aa3dd3caf",
    "assets/ark-map-artwork/2026-10-25/Valguero_map_ASA.jpg": "7c82e6435a84bf39e7870f6059706d64409fa2e0b4a0d6264b29a7ca6e80786d",
    "assets/ark-map-artwork/2026-10-25/Astraeos_Map_ASA.jpg": "6d8b4b1589f50846b26e56c8fd165f5015a949afd02af46e1ddddef79e9fa08e",
    "assets/ark-map-artwork/2026-10-25/Lost_Colony_Map_ASA.jpg": "5b0ac6bbb822f8c592a8eb656c9f5b0d3a9880c83aff3efd243255910af2eefa",
}
THIRD_PARTY_REFERENCE_MEDIA = frozenset(REFERENCE_MEDIA_SHA256)
REFERENCE_MEDIA_SHA256[ARK_MAP_ARTWORK_NOTICE] = "52bee1ff5d06036ff6c94dec39e40df803fc1cfbdca691136e5aa1975015a326"


# Unchanged upstream test archives, not downloaded game mods. Exact package
# paths and redistribution terms are recorded in the companion source ledger.
ARCHIVE_FIXTURE_ROOT = "apps/desktop/src-tauri/src/test_fixtures/mod_archives"
ARCHIVE_FIXTURE_SHA256 = {
    f"{ARCHIVE_FIXTURE_ROOT}/lzma.7z": "570606adc4626105bd471b8dfb866e4fe8d06e73ea916a2b5ed365eeb030ce8b",
    f"{ARCHIVE_FIXTURE_ROOT}/solid.7z": "f3baded396d9ce01a2c591585db5bc168916de70de4a83644a41273befd6ec20",
    f"{ARCHIVE_FIXTURE_ROOT}/ppmd.7z": "58b8804b1b11d8c688b376858832cb047446023cfdd2e34f98f62d984cc49228",
    f"{ARCHIVE_FIXTURE_ROOT}/encrypted.7z": "ea3e538426be3fe908ed804f31d169610322ec87a1a9ad4901a526a2d632a50e",
    f"{ARCHIVE_FIXTURE_ROOT}/rar4.rar": "2c595ee9aaea2c837bd0e11a659b9b62b46251f18ab7b6675748da0fadd34460",
    f"{ARCHIVE_FIXTURE_ROOT}/rar5-solid.rar": "23da48de916e4feb7a0f0fbd6ba520dae3492d04e98d061225b0e923668e48f4",
    f"{ARCHIVE_FIXTURE_ROOT}/rar5-unicode.rar": "404c872a59c60f5040586065a4f0b30478139a405284154479cc9b57f7bf2815",
    f"{ARCHIVE_FIXTURE_ROOT}/encrypted.rar": "3215288892bfae2e641786952ab2547ea72cdd58c9c544604cd63e242fa07318",
}
THIRD_PARTY_ARCHIVE_FIXTURES = frozenset(ARCHIVE_FIXTURE_SHA256)
ARCHIVE_FIXTURE_COMPANION_SHA256 = {
    f"{ARCHIVE_FIXTURE_ROOT}/README.md": "ec2269ab8f4afd804aeb73cd2f291068565015f98f613fd64f6c8379eb2f8473",
    f"{ARCHIVE_FIXTURE_ROOT}/sources.json": "0922ca4b870eec325cc75bd017f9f2c5c94cbda7871523528f3816198256ded6",
    f"{ARCHIVE_FIXTURE_ROOT}/LICENSE-sevenz-APACHE": "43070e2d4e532684de521b885f385d0841030efa2b1a20bafb76133a5e1379c1",
    f"{ARCHIVE_FIXTURE_ROOT}/LICENSE-unrar-MIT": "23f18e03dc49df91622fe2a76176497404e46ced8a715d9d2b67a7446571cca3",
    f"{ARCHIVE_FIXTURE_ROOT}/LICENSE-unrar-APACHE": "62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a",
    f"{ARCHIVE_FIXTURE_ROOT}/LICENSE-unrar-rs": "97dfd1950dfb0901afbf6e51cae496e12281e97f670fdb2a2aab3252d2561086",
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
    if selected & THIRD_PARTY_REFERENCE_MEDIA and ARK_MAP_ARTWORK_NOTICE not in selected:
        findings.append(ThirdPartyAssetFinding(
            "third-party-asset-notice", ARK_MAP_ARTWORK_NOTICE,
            "selected ARK reference map requires its upstream rights notice in the same snapshot",
        ))
    if selected & THIRD_PARTY_ARCHIVE_FIXTURES:
        for companion in sorted(ARCHIVE_FIXTURE_COMPANION_SHA256.keys() - selected):
            findings.append(ThirdPartyAssetFinding(
                "third-party-fixture-notice", companion,
                "selected archive fixture requires its provenance and license files in the same snapshot",
            ))
    reviewed_bytes = (
        THIRD_PARTY_ASSET_SHA256 | REFERENCE_MEDIA_SHA256
        | ARCHIVE_FIXTURE_SHA256 | ARCHIVE_FIXTURE_COMPANION_SHA256
    )
    for path in sorted(selected & reviewed_bytes.keys()):
        try:
            payload = read_source_payload(repository_root / path, repository_root=repository_root)
        except SourceReadError as error:
            findings.append(ThirdPartyAssetFinding(error.rule, path, "asset could not be safely inspected"))
            continue
        if hashlib.sha256(payload).hexdigest() != reviewed_bytes[path]:
            findings.append(ThirdPartyAssetFinding(
                "third-party-asset-integrity", path,
                "file bytes differ from the reviewed third-party asset or license",
            ))
    return findings
