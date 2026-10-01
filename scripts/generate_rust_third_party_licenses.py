from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import tarfile
import tempfile
import time
import tomllib
from typing import Callable
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen

SCRIPT_ROOT = Path(__file__).resolve().parent
if str(SCRIPT_ROOT) not in sys.path:
    sys.path.insert(0, str(SCRIPT_ROOT))

from rust_license_policy import AUDITED_SOURCES, MISSING_LICENSE_RULES


REPOSITORY_ROOT = SCRIPT_ROOT.parent
LOCK_PATH = REPOSITORY_ROOT / "Cargo.lock"
OUTPUT_PATH = (
    REPOSITORY_ROOT
    / "apps"
    / "desktop"
    / "src-tauri"
    / "THIRD_PARTY_LICENSES-RUST.txt"
)
AUDITED_SOURCE_ROOT = Path(__file__).with_name("third_party_license_sources") / "rust"
CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
MAX_ARCHIVE_BYTES = 100 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
CODE_SUFFIXES = frozenset(
    {
        ".c",
        ".cc",
        ".cpp",
        ".h",
        ".hpp",
        ".js",
        ".json",
        ".lock",
        ".mjs",
        ".ps1",
        ".py",
        ".rs",
        ".sh",
        ".toml",
        ".ts",
        ".tsx",
        ".yaml",
        ".yml",
    }
)
LEGAL_PREFIX = re.compile(r"^(?:licen[cs]e|unlicense|copying)(?:$|[-_.])", re.I)
NOTICE_PREFIX = re.compile(r"^(?:notice|copyright)(?:$|[-_.])", re.I)


class LicenseGenerationError(RuntimeError):
    pass


@dataclass(frozen=True)
class LicenseSource:
    label: str
    origin: str
    payload: bytes

    @property
    def sha256(self) -> str:
        return hashlib.sha256(self.payload).hexdigest()


@dataclass
class CrateRecord:
    archive_checksum: str
    authors: tuple[str, ...]
    declared_license: str
    license_sources: list[LicenseSource]
    member_payloads: dict[str, bytes]
    name: str
    package_id: str
    repository: str | None
    source: str
    vcs_sha1: str | None
    version: str
    selected_license: str | None = None
    used_override: bool = False


@dataclass
class GenerationResult:
    document: str
    downloaded_packages: tuple[str, ...] = field(default_factory=tuple)
    records: tuple[CrateRecord, ...] = field(default_factory=tuple)


def fail(message: str) -> None:
    raise LicenseGenerationError(message)


def sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def decode_text(payload: bytes, source: str) -> str:
    if len(payload) > MAX_TEXT_BYTES:
        fail(f"license source is unexpectedly large: {source}")
    if b"\0" in payload:
        fail(f"license source is not text: {source}")
    try:
        return payload.decode("utf-8")
    except UnicodeDecodeError as error:
        fail(f"license source is not UTF-8: {source}: {error}")


def package_id(name: str, version: str) -> str:
    return f"{name}@{version}"


def cargo_home_from_environment() -> Path:
    configured = os.environ.get("CARGO_HOME")
    return Path(configured).expanduser() if configured else Path.home() / ".cargo"


def load_locked_registry_packages(lock_path: Path = LOCK_PATH) -> list[dict[str, object]]:
    try:
        lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot read {lock_path}: {error}")
    packages = []
    for item in lock.get("package", []):
        source = item.get("source")
        if source is None:
            continue
        if source != CRATES_IO_SOURCE:
            fail(f"unsupported non-crates.io dependency source for {item.get('name')}: {source}")
        checksum = item.get("checksum")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            fail(f"missing Cargo.lock checksum for {item.get('name')}@{item.get('version')}")
        packages.append(item)
    packages.sort(key=lambda item: package_id(str(item["name"]), str(item["version"])))
    return packages


def find_cached_archive(
    cargo_home: Path, name: str, version: str, expected_checksum: str
) -> bytes | None:
    cache_root = cargo_home / "registry" / "cache"
    if not cache_root.is_dir():
        return None
    filename = f"{name}-{version}.crate"
    candidates = sorted(cache_root.glob(f"*/{filename}"))
    matches = []
    for candidate in candidates:
        try:
            payload = candidate.read_bytes()
        except OSError as error:
            fail(f"cannot read Cargo registry archive {candidate}: {error}")
        if sha256(payload) == expected_checksum:
            matches.append(payload)
    if candidates and not matches:
        fail(f"cached archives for {name}@{version} do not match Cargo.lock checksum")
    return matches[0] if matches else None


def source_archive_url(name: str, version: str) -> str:
    encoded_name = quote(name, safe="")
    encoded_version = quote(version, safe="")
    return (
        f"https://static.crates.io/crates/{encoded_name}/"
        f"{encoded_name}-{encoded_version}.crate"
    )


def download_crate_archive(name: str, version: str) -> bytes:
    request = Request(
        source_archive_url(name, version), headers={"User-Agent": "LanGame-license-generator/1"}
    )
    last_error: BaseException | None = None
    for attempt in range(3):
        try:
            with urlopen(request, timeout=30) as response:
                expected_length = response.headers.get("Content-Length")
                if expected_length and int(expected_length) > MAX_ARCHIVE_BYTES:
                    fail(f"crate archive exceeds size limit: {name}@{version}")
                payload = response.read(MAX_ARCHIVE_BYTES + 1)
                if len(payload) > MAX_ARCHIVE_BYTES:
                    fail(f"crate archive exceeds size limit: {name}@{version}")
                return payload
        except (HTTPError, URLError, OSError, TimeoutError) as error:
            last_error = error
            if attempt < 2:
                time.sleep(0.5 * (attempt + 1))
    fail(f"cannot download checksum-locked crate {name}@{version}: {last_error}")


def archive_payload(
    package: dict[str, object],
    *,
    cargo_home: Path,
    offline: bool,
    download: Callable[[str, str], bytes] = download_crate_archive,
) -> tuple[bytes, bool]:
    name = str(package["name"])
    version = str(package["version"])
    checksum = str(package["checksum"])
    payload = find_cached_archive(cargo_home, name, version, checksum)
    downloaded = False
    if payload is None:
        if offline:
            fail(f"checksum-locked crate is not cached: {name}@{version}; run cargo fetch --locked")
        payload = download(name, version)
        downloaded = True
    actual_checksum = sha256(payload)
    if actual_checksum != checksum:
        fail(
            f"crate checksum mismatch for {name}@{version}: "
            f"expected {checksum}, found {actual_checksum}"
        )
    return payload, downloaded


def safe_members(archive: tarfile.TarFile, expected_root: str) -> list[tarfile.TarInfo]:
    members = []
    for member in archive.getmembers():
        path = PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not path.parts:
            fail(f"unsafe member path in {expected_root}.crate: {member.name}")
        if path.parts[0] != expected_root:
            fail(f"unexpected archive root in {expected_root}.crate: {member.name}")
        if member.isfile():
            members.append(member)
    return members


def member_bytes(archive: tarfile.TarFile, member: tarfile.TarInfo) -> bytes:
    if member.size > MAX_TEXT_BYTES:
        fail(f"legal or metadata file is unexpectedly large: {member.name}")
    stream = archive.extractfile(member)
    if stream is None:
        fail(f"cannot read archive member: {member.name}")
    return stream.read()


def is_legal_filename(name: str) -> bool:
    path = PurePosixPath(name)
    return path.suffix.casefold() not in CODE_SUFFIXES and bool(LEGAL_PREFIX.match(path.name))


def is_notice_filename(name: str) -> bool:
    path = PurePosixPath(name)
    return path.suffix.casefold() not in CODE_SUFFIXES and bool(NOTICE_PREFIX.match(path.name))


def parse_crate(package: dict[str, object], payload: bytes) -> CrateRecord:
    name = str(package["name"])
    version = str(package["version"])
    expected_root = f"{name}-{version}"
    try:
        archive_context = tarfile.open(fileobj=io.BytesIO(payload), mode="r:gz")
    except tarfile.TarError as error:
        fail(f"cannot parse checksum-locked crate {name}@{version}: {error}")
    with archive_context as archive:
        members = safe_members(archive, expected_root)
        manifests = [
            member
            for member in members
            if PurePosixPath(member.name) == PurePosixPath(expected_root) / "Cargo.toml"
        ]
        if len(manifests) != 1:
            fail(f"{name}@{version} must contain one root Cargo.toml")
        manifest_payload = member_bytes(archive, manifests[0])
        try:
            metadata = tomllib.loads(manifest_payload.decode("utf-8"))["package"]
        except (UnicodeDecodeError, KeyError, tomllib.TOMLDecodeError) as error:
            fail(f"cannot parse packaged Cargo.toml for {name}@{version}: {error}")
        if metadata.get("name") != name or str(metadata.get("version")) != version:
            fail(f"packaged Cargo.toml does not match Cargo.lock for {name}@{version}")
        declared_license = metadata.get("license")
        if not isinstance(declared_license, str) or not declared_license.strip():
            fail(f"{name}@{version} has no SPDX license declaration")
        repository = metadata.get("repository")
        if repository is not None and not isinstance(repository, str):
            fail(f"{name}@{version} has invalid repository metadata")
        authors_value = metadata.get("authors", [])
        if not isinstance(authors_value, list) or not all(
            isinstance(author, str) for author in authors_value
        ):
            fail(f"{name}@{version} has invalid authors metadata")

        vcs_members = [
            member
            for member in members
            if PurePosixPath(member.name)
            == PurePosixPath(expected_root) / ".cargo_vcs_info.json"
        ]
        vcs_sha1 = None
        if vcs_members:
            if len(vcs_members) != 1:
                fail(f"{name}@{version} has duplicate Cargo VCS metadata")
            try:
                vcs_data = json.loads(member_bytes(archive, vcs_members[0]))
                vcs_sha1 = vcs_data["git"]["sha1"]
            except (KeyError, TypeError, json.JSONDecodeError) as error:
                fail(f"cannot parse Cargo VCS metadata for {name}@{version}: {error}")
            if not isinstance(vcs_sha1, str) or not re.fullmatch(r"[0-9a-f]{40}", vcs_sha1):
                fail(f"invalid Cargo VCS commit for {name}@{version}")

        legal_members = [member for member in members if is_legal_filename(member.name)]
        notice_members = [member for member in members if is_notice_filename(member.name)]
        selected_members = legal_members + notice_members if legal_members else notice_members
        reviewed_member_names = {
            specification["member_name"]
            for specification in MISSING_LICENSE_RULES.get(package_id(name, version), {}).get(
                "sources", []
            )
            if specification["kind"] == "archive-member"
        }
        reviewed_members = [
            member
            for member in members
            if PurePosixPath(member.name).name in reviewed_member_names
        ]
        selected_paths = {member.name for member in selected_members}
        payload_members = {member.name: member for member in selected_members + reviewed_members}
        sources = []
        member_payloads = {}
        for member in sorted(payload_members.values(), key=lambda item: item.name):
            source_payload = member_bytes(archive, member)
            decode_text(source_payload, f"{name}@{version}/{member.name}")
            member_payloads[member.name] = source_payload
            if member.name in selected_paths:
                sources.append(
                    LicenseSource(
                        label=f"checksum-verified crate file {member.name}",
                        origin=f"crate:{name}@{version}:{member.name}",
                        payload=source_payload,
                    )
                )
        return CrateRecord(
            archive_checksum=str(package["checksum"]),
            authors=tuple(authors_value),
            declared_license=declared_license,
            license_sources=sources,
            member_payloads=member_payloads,
            name=name,
            package_id=package_id(name, version),
            repository=repository,
            source=str(package["source"]),
            vcs_sha1=vcs_sha1,
            version=version,
        )


def audited_license_sources() -> dict[str, LicenseSource]:
    expected_files = {str(item["path"]) for item in AUDITED_SOURCES.values()}
    actual_files = {path.name for path in AUDITED_SOURCE_ROOT.iterdir() if path.is_file()}
    if actual_files != expected_files:
        fail(
            "audited Rust license source set drifted: "
            f"expected {sorted(expected_files)}, found {sorted(actual_files)}"
        )
    sources = {}
    for source_id, item in AUDITED_SOURCES.items():
        path = AUDITED_SOURCE_ROOT / str(item["path"])
        payload = path.read_bytes()
        actual_hash = sha256(payload)
        if actual_hash != item["sha256"]:
            fail(
                f"audited source {path} has SHA-256 {actual_hash}; "
                f"expected {item['sha256']}"
            )
        decode_text(payload, str(path))
        sources[source_id] = LicenseSource(
            label=f"reviewed upstream source {item['url']}",
            origin=f"audited:{source_id}",
            payload=payload,
        )
    return sources


def apply_missing_license_rules(records: list[CrateRecord]) -> None:
    by_id = {record.package_id: record for record in records}
    audited_sources = audited_license_sources()
    used_rules = set()
    used_audited_sources = set()
    for record in records:
        has_packaged_license = any(
            source.origin.startswith("crate:")
            and is_legal_filename(source.origin.rsplit(":", 1)[-1])
            for source in record.license_sources
        )
        if has_packaged_license:
            continue
        rule = MISSING_LICENSE_RULES.get(record.package_id)
        if rule is None:
            fail(
                f"{record.package_id} has no packaged license text and no reviewed exact-version rule"
            )
        if rule["declared_license"] != record.declared_license:
            fail(f"license declaration changed for {record.package_id}")
        if rule["repository"] != record.repository:
            fail(f"repository metadata changed for {record.package_id}")
        if rule["vcs_sha1"] != record.vcs_sha1:
            fail(f"VCS commit changed for {record.package_id}")
        record.selected_license = str(rule["selected_license"])
        record.used_override = True
        used_rules.add(record.package_id)
        for specification in rule["sources"]:
            kind = specification["kind"]
            if kind == "audited":
                source_id = specification["source_id"]
                record.license_sources.append(audited_sources[source_id])
                used_audited_sources.add(source_id)
            elif kind == "archive-member":
                member_name = specification["member_name"]
                matches = [
                    payload
                    for path, payload in record.member_payloads.items()
                    if PurePosixPath(path).name == member_name
                ]
                if len(matches) != 1:
                    fail(f"reviewed archive source disappeared for {record.package_id}: {member_name}")
                payload = matches[0]
                record.license_sources.append(
                    LicenseSource(
                        label=f"reviewed checksum-verified crate file {member_name}",
                        origin=f"reviewed-crate:{record.package_id}:{member_name}",
                        payload=payload,
                    )
                )
            elif kind == "sibling":
                donor_id = specification["package_id"]
                member_name = specification["member_name"]
                donor = by_id.get(donor_id)
                if donor is None:
                    fail(f"reviewed sibling crate is no longer locked: {donor_id}")
                if record.repository is not None and donor.repository != record.repository:
                    fail(f"reviewed sibling repository changed for {record.package_id}")
                matches = [
                    source
                    for source in donor.license_sources
                    if source.origin.startswith("crate:")
                    and PurePosixPath(source.origin.rsplit(":", 1)[-1]).name
                    == member_name
                ]
                if len(matches) != 1:
                    fail(
                        f"reviewed sibling license disappeared for {record.package_id}: "
                        f"{donor_id}/{member_name}"
                    )
                source = matches[0]
                record.license_sources.append(
                    LicenseSource(
                        label=(
                            f"repository-shared license from checksum-verified "
                            f"{donor_id}/{member_name}"
                        ),
                        origin=f"sibling:{donor_id}:{member_name}",
                        payload=source.payload,
                    )
                )
            else:
                fail(f"unknown reviewed source kind for {record.package_id}: {kind}")
        unique_sources = {source.origin: source for source in record.license_sources}
        record.license_sources = sorted(unique_sources.values(), key=lambda source: source.origin)
    if used_rules != set(MISSING_LICENSE_RULES):
        stale = sorted(set(MISSING_LICENSE_RULES) - used_rules)
        fail(f"reviewed missing-license rules are stale or unused: {stale}")
    if used_audited_sources != set(AUDITED_SOURCES):
        stale = sorted(set(AUDITED_SOURCES) - used_audited_sources)
        fail(f"audited Rust license sources are stale or unused: {stale}")


def normalized_text(payload: bytes, label: str) -> str:
    return decode_text(payload, label).replace("\r\n", "\n").replace("\r", "\n").rstrip("\n")


def render_document(records: list[CrateRecord]) -> str:
    all_sources = [source for record in records for source in record.license_sources]
    catalog: dict[str, dict[str, object]] = {}
    for source in all_sources:
        entry = catalog.setdefault(source.sha256, {"payload": source.payload, "labels": set()})
        if entry["payload"] != source.payload:
            fail(f"SHA-256 collision while cataloging license text: {source.sha256}")
        entry["labels"].add(source.label)
    override_count = sum(record.used_override for record in records)
    lines = [
        "LanGame Server Manager — Rust Dependency Licenses",
        "LanGame Server Manager — Rust 依赖许可证",
        "",
        "This deterministic inventory intentionally includes every crates.io package in",
        "Cargo.lock. It may therefore include target-specific, development, build, or optional",
        "crates that are not linked into the Windows executable; inclusion is not proof of linkage.",
        "Each crate archive is accepted only after its SHA-256 matches Cargo.lock. License and",
        "notice texts are copied from that archive whenever present. Exact-version reviewed rules",
        "supply checksum-pinned upstream or repository-shared terms only for omitted license files.",
        "",
        "本清单有意收录 Cargo.lock 中全部 crates.io 软件包，因此可能包含未链接到 Windows",
        "可执行文件的目标平台、开发、构建或可选依赖；收录本身不表示实际链接。每个 crate",
        "归档均须先通过 Cargo.lock SHA-256 校验。发布包缺少许可证正文时，仅允许使用绑定",
        "精确版本的审计规则，以及经哈希校验的上游正文或同仓库共享许可证。",
        "",
        "SOURCE AVAILABILITY / 对应源码获取",
        "Each Source archive URL below identifies the exact crates.io source package version.",
        "Download the .crate file, verify its Archive SHA-256, then extract it as a gzip tar",
        "archive to obtain the upstream source and its notices. Repository and VCS fields",
        "provide additional provenance; the archive is the checksum-locked build input.",
        "MPL-covered source in these archives is available as Source Code Form under MPL-2.0.",
        "When MPL-covered code is distributed in executable form, recipients must be told",
        "how to obtain its corresponding Source Code Form. These upstream links do not supply",
        "a distributor's modified MPL-covered files: any such files must also be made",
        "available under MPL-2.0, with their retrieval location stated for that release.",
        "The LanGame project license does not restrict recipients' MPL source rights.",
        "See Mozilla Public License 2.0 sections 3.1-3.3: https://www.mozilla.org/MPL/2.0/",
        "",
        "下列每项源码归档链接指向 crates.io 上对应的精确版本。下载 .crate 文件，核对该项",
        "Archive SHA-256 后按 gzip tar 归档解压，即可取得上游源码及其声明。仓库与提交字段",
        "提供额外溯源信息；归档本身是由校验和锁定的构建输入。",
        "其中受 MPL 覆盖的源码继续以 MPL-2.0 提供。分发含 MPL 代码的可执行形式时，须告知",
        "接收者如何取得对应源码。若分发者修改了 MPL 文件，这些上游链接不包含其修改；",
        "分发者还须以 MPL-2.0 提供修改后的对应源码，并在该版本中注明取得位置。",
        "LanGame 项目许可不限制接收者依 MPL 获得的源码权利。",
        "条款见 Mozilla Public License 2.0 第 3.1-3.3 节：https://www.mozilla.org/MPL/2.0/",
        "",
        f"Locked external crates / 锁定外部 crate: {len(records)}",
        f"Reviewed missing-file rules / 缺失文件审计规则: {override_count}",
        f"Unique legal texts / 唯一法律文本: {len(catalog)}",
        "",
        "PACKAGE INVENTORY / 软件包清单",
        "=" * 80,
    ]
    for record in records:
        lines.extend(
            [
                f"Package / 软件包: {record.package_id}",
                f"Declared license / 声明许可证: {record.declared_license}",
                f"Selected reviewed terms / 选用审计条款: {record.selected_license or 'packaged upstream texts'}",
                f"Archive SHA-256: {record.archive_checksum}",
                f"Source archive / 源码归档: {source_archive_url(record.name, record.version)}",
                f"Repository / 仓库: {record.repository or 'not declared'}",
                f"VCS commit / 版本提交: {record.vcs_sha1 or 'not included in crate metadata'}",
            ]
        )
        if record.authors:
            lines.append(f"Authors metadata / 作者元数据: {'; '.join(record.authors)}")
        for source in sorted(record.license_sources, key=lambda item: (item.sha256, item.label)):
            lines.append(f"Legal text / 法律文本: {source.sha256} — {source.label}")
        lines.append("-" * 80)
    lines.extend(["", "LEGAL TEXT CATALOG / 法律文本目录", "=" * 80])
    for text_hash in sorted(catalog):
        entry = catalog[text_hash]
        lines.append(f"Text SHA-256 / 文本 SHA-256: {text_hash}")
        for label in sorted(entry["labels"]):
            lines.append(f"Source / 来源: {label}")
        lines.extend(["-" * 80, normalized_text(entry["payload"], text_hash), ""])
    return "\n".join(lines).rstrip("\n") + "\n"


def generate(
    *,
    cargo_home: Path | None = None,
    lock_path: Path = LOCK_PATH,
    offline: bool = False,
    download: Callable[[str, str], bytes] = download_crate_archive,
) -> GenerationResult:
    cargo_home = cargo_home or cargo_home_from_environment()
    records = []
    downloaded = []
    with tempfile.TemporaryDirectory(prefix="langame-rust-license-") as temporary_directory:
        temporary_root = Path(temporary_directory)
        for package in load_locked_registry_packages(lock_path):
            payload, was_downloaded = archive_payload(
                package, cargo_home=cargo_home, offline=offline, download=download
            )
            if was_downloaded:
                downloaded.append(package_id(str(package["name"]), str(package["version"])))
                temporary_path = temporary_root / f"{package['name']}-{package['version']}.crate"
                temporary_path.write_bytes(payload)
            records.append(parse_crate(package, payload))
    apply_missing_license_rules(records)
    records.sort(key=lambda record: record.package_id)
    return GenerationResult(
        document=render_document(records),
        downloaded_packages=tuple(downloaded),
        records=tuple(records),
    )


def parse_arguments(arguments: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate checksum-verified Rust dependency license notices."
    )
    parser.add_argument("--check", action="store_true", help="fail if the checked-in output drifted")
    parser.add_argument(
        "--offline",
        action="store_true",
        help="fail instead of downloading a Cargo.lock-pinned crate missing from the local cache",
    )
    return parser.parse_args(arguments)


def main(arguments: list[str] | None = None) -> int:
    options = parse_arguments(arguments)
    try:
        result = generate(offline=options.offline)
        if options.check:
            current = OUTPUT_PATH.read_text(encoding="utf-8")
            if current != result.document:
                fail(
                    f"{OUTPUT_PATH.relative_to(REPOSITORY_ROOT)} is stale; run "
                    "python -B scripts/generate_rust_third_party_licenses.py"
                )
            action = "verified"
        else:
            OUTPUT_PATH.write_text(result.document, encoding="utf-8", newline="\n")
            action = "generated"
        print(
            f"{action} {OUTPUT_PATH.relative_to(REPOSITORY_ROOT)} "
            f"({len(result.records)} crates, {len(result.downloaded_packages)} temporary downloads)"
        )
        return 0
    except (LicenseGenerationError, OSError) as error:
        print(f"Rust license generation failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
