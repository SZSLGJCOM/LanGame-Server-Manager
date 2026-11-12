"""Prepare one reviewed ASA symbol cache; --execute appends its immutable mirror.

Reuses the dependency publisher's authenticated transport, pending-mutation
reconciliation and full anonymous readback. Never promotes a desktop release,
replaces an attachment or publishes an arbitrary local server/cache directory.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import os
from pathlib import Path
import re
import struct
import sys
import time
import urllib.request
import urllib.parse
import zipfile

if __package__:
    from . import publish_ark_tools_dependencies as dependencies
else:
    import publish_ark_tools_dependencies as dependencies

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "modules/ark-tools/symbol-caches.json"
MAX_ARCHIVE = 768 * 1024 * 1024
MAX_MAP = 512 * 1024 * 1024
CDNS = {"cdn.pelayori.com", "cdn.shadowhunter.co.za", "cdn.shadowhunter-systems.co.za"}
COMMIT = "bd376a0422291b0b1bf15a0b7868e7e275605da9"
LICENSE_URL = f"https://raw.githubusercontent.com/ArkServerApi/AsaApi/{COMMIT}/LICENSE"
require = dependencies.require
SyncError = dependencies.SyncError


def load_manifest(path=MANIFEST):
    raw = path.read_bytes()
    require(len(raw) <= 256 * 1024, "Symbol manifest exceeds limit")
    manifest = dependencies.parse_json(raw)
    require(isinstance(manifest, dict) and set(manifest) == {
        "schemaVersion", "mirrorRepository", "packages"}, "Invalid symbol manifest fields")
    require(type(manifest["schemaVersion"]) is int and manifest["schemaVersion"] == 1
            and manifest["mirrorRepository"] == dependencies.GITCODE_REPO,
            "Invalid symbol publication identity")
    packages = manifest["packages"]
    require(isinstance(packages, list) and 0 < len(packages) <= 128, "Invalid symbol package count")
    seen = set()
    for package in packages:
        require(isinstance(package, dict) and set(package) == {
            "steamBuildId", "executableSha256", "fileName", "upstreamUrl", "mirrorTag", "mirrorUrl", "size", "sha256"},
            "Invalid symbol package fields")
        for key in ("executableSha256", "sha256"):
            require(isinstance(package[key], str) and re.fullmatch(r"[a-f0-9]{64}", package[key]), "Invalid symbol SHA-256")
        executable = package["executableSha256"]
        require(executable not in seen, "Duplicate symbol executable identity")
        seen.add(executable)
        require(isinstance(package["steamBuildId"], str) and re.fullmatch(r"[0-9]+", package["steamBuildId"]), "Invalid Steam build")
        require(package["fileName"] == executable + ".zip"
                and package["upstreamUrl"] == "https://cdn.pelayori.com/cache/" + package["fileName"],
                "Symbol source does not match its executable")
        require(type(package["size"]) is int and 0 < package["size"] <= MAX_ARCHIVE, "Invalid symbol archive size")
        require(package["mirrorTag"] == "ark-tools-symbols-asa-" + package["sha256"][:16]
                and package["mirrorUrl"] == dependencies.attachment_url(package["mirrorTag"], package["fileName"]),
                "Invalid immutable symbol mirror address")
    return manifest


def cdn_url(url):
    parsed = urllib.parse.urlsplit(url)
    require(parsed.scheme == "https" and parsed.hostname in CDNS and parsed.port in (None, 443)
            and not parsed.username and not parsed.password and not parsed.fragment
            and not parsed.query and re.fullmatch(r"/cache/[a-f0-9]{64}\.zip", parsed.path),
            "Unapproved symbol CDN address")
    return url


class CacheRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, url):
        cdn_url(url)
        require(not request.has_header("Authorization"), "Credential on symbol download")
        return super().redirect_request(request, response, code, message, headers, url)


def download_cache(package, destination):
    if destination.exists():
        require(not destination.is_symlink() and destination.is_file()
                and destination.stat().st_size == package["size"]
                and dependencies.file_hash(destination) == package["sha256"], "Existing symbol archive differs")
        return
    partial = destination.with_name(destination.name + ".partial")
    require(not partial.exists(), "Unfinished symbol download exists; inspect it first")
    request = urllib.request.Request(cdn_url(package["upstreamUrl"]), headers={"User-Agent": "LGSM-ASA-symbol-mirror"})
    deadline = time.monotonic() + 1800
    with partial.open("xb") as output:
        with urllib.request.build_opener(CacheRedirect()).open(request, timeout=60) as response:
            require(response.status == 200, "Expected complete symbol response")
            received = 0
            while chunk := response.read(1024 * 1024):
                received += len(chunk)
                require(received <= package["size"] and time.monotonic() < deadline, "Symbol download exceeded bounds")
                output.write(chunk)
        output.flush()
        os.fsync(output.fileno())
    require(received == package["size"] and dependencies.file_hash(partial) == package["sha256"],
            "Symbol download size or SHA-256 differs; partial retained")
    partial.rename(destination)


def map_entries(stream, size, width):
    remaining, count, keys = size, 0, set()
    digest = hashlib.sha256()

    def read(count):
        value = stream.read(count)
        require(len(value) == count, "Truncated symbol map")
        digest.update(value)
        return value

    while remaining:
        require(remaining >= 8, "Truncated symbol key length")
        length = struct.unpack("<Q", read(8))[0]
        remaining -= 8
        require(0 < length <= 1024 * 1024 and length + width <= remaining and count < 5_000_000,
                "Invalid symbol map entry")
        key = read(length)
        require(key not in keys, "Duplicate symbol map key")
        keys.add(key)
        read(width)
        remaining -= length + width
        count += 1
    require(count > 0 and not stream.read(1), "Empty or excess symbol map content")
    return count, digest.hexdigest()


def inspect_archive(path):
    members, names, total = [], set(), 0
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        require(2 <= len(entries) <= 4, "Unexpected symbol archive entry count")
        for entry in entries:
            name = entry.filename
            mode = entry.external_attr >> 16
            require(name in {"cached_offsets.cache", "cached_bitfields.cache", "cached_offsets.txt", "cached_key.cache"}
                    and name not in names and not entry.is_dir()
                    and mode & 0o170000 in (0, 0o100000)
                    and 0 < entry.file_size <= MAX_MAP and total + entry.file_size <= MAX_ARCHIVE,
                    "Unsafe, duplicate or oversized symbol archive member")
            names.add(name)
            total += entry.file_size
            item = {"name": name, "size": entry.file_size}
            with archive.open(entry) as raw:
                stream = io.BufferedReader(raw)
                if name in ("cached_offsets.cache", "cached_bitfields.cache"):
                    item["entries"], item["sha256"] = map_entries(stream, entry.file_size, 8 if name == "cached_offsets.cache" else 32)
                else:
                    digest, size = hashlib.sha256(), 0
                    while chunk := stream.read(1024 * 1024):
                        digest.update(chunk)
                        size += len(chunk)
                    require(size == entry.file_size, "Symbol member size differs")
                    item["sha256"] = digest.hexdigest()
            members.append(item)
    require({"cached_offsets.cache", "cached_bitfields.cache"} <= names, "Missing required symbol maps")
    return members


def prepare(package, directory, cache_reader=download_cache, license_reader=dependencies.fetch_license):
    directory.mkdir(parents=True, exist_ok=True)
    name = package["fileName"]
    cache_reader(package, directory / name)
    require(dependencies.file_hash(directory / name) == package["sha256"]
            and (directory / name).stat().st_size == package["size"], "Prepared symbol identity differs")
    members = inspect_archive(directory / name)
    license_bytes = license_reader(LICENSE_URL)
    require(b"MIT License" in license_bytes and b"Permission is hereby granted" in license_bytes, "Unexpected SDK license")
    notice = (
        "ASA executable-bound symbol cache mirror\n\n"
        f"Upstream: {package['upstreamUrl']}\nExecutable SHA-256: {package['executableSha256']}\n"
        f"ZIP SHA-256: {package['sha256']}\nSteam build: {package['steamBuildId']}\n\n"
        "The ZIP is an unmodified public maintainer cache: symbol names, native offsets and bitfield maps.\n"
        "It contains no game executable, game content, player world or local credentials.\n"
        "ArkServerApi is a community API project, not a Studio Wildcard distribution.\n"
        "The accompanying MIT license is the pinned AsaApi SDK project's license.\n"
        "It does not relicense the proprietary game or assert separate licensing of every derived symbol name.\n"
        f"SDK/cache implementation source: https://github.com/ArkServerApi/AsaApi/tree/{COMMIT}\n"
        f"SDK license source: {LICENSE_URL}\n"
    ).encode("utf-8")
    public_manifest = {"schemaVersion": 1, "mirrorRepository": dependencies.GITCODE_REPO, "packages": [package]}
    manifest_bytes = dependencies.json_bytes(public_manifest)
    for file_name, content in (("ASA-SDK-LICENSE.txt", license_bytes), ("THIRD_PARTY_NOTICES.txt", notice),
                               ("symbols.json", manifest_bytes)):
        dependencies.write_immutable(directory / file_name, content)
    assets = [name, "ASA-SDK-LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "symbols.json"]
    sums = "".join(f"{dependencies.file_hash(directory / item)}  {item}\n" for item in sorted(assets))
    dependencies.write_immutable(directory / "SHA256SUMS", sums.encode("ascii"))
    assets.append("SHA256SUMS")
    prepared = {"schemaVersion": 1, "repository": dependencies.GITCODE_REPO, "tag": package["mirrorTag"],
                "manifestSha256": hashlib.sha256(manifest_bytes).hexdigest(),
                "assets": [{"name": item, "size": (directory / item).stat().st_size,
                            "sha256": dependencies.file_hash(directory / item)} for item in sorted(assets)],
                "archiveMembers": members, "licenseSource": LICENSE_URL}
    dependencies.write_immutable(directory / "prepared.json", dependencies.json_bytes(prepared))
    return prepared


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--executable-sha256", required=True)
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    directory = args.work_dir.resolve()
    require(not directory.is_relative_to(ROOT), "Work directory must be outside the repository")
    packages = [p for p in load_manifest()["packages"] if p["executableSha256"] == args.executable_sha256]
    require(len(packages) == 1, "Executable has no reviewed symbol cache")
    token = os.environ.get("GITCODE_RELEASE_TOKEN", "") if args.execute else ""
    require(not args.execute or token, "GITCODE_RELEASE_TOKEN is required for --execute")
    directory.mkdir(parents=True, exist_ok=True)
    lock = directory / "publisher.lock"
    try:
        stream = lock.open("x", encoding="ascii")
    except FileExistsError:
        raise SyncError("Publisher lock exists; inspect its process and state before resuming") from None
    try:
        with stream:
            stream.write(str(os.getpid()) + "\n")
        prepared = prepare(packages[0], directory)
        result = dependencies.publish(dependencies.Transport(gitcode_token=token), prepared, directory) if args.execute else {
            "published": False, "tag": prepared["tag"], "preparedAssets": len(prepared["assets"]),
            "preparedSha256": dependencies.file_hash(directory / "prepared.json")}
        print(dependencies.json_bytes(result).decode(), end="", flush=True)
    finally:
        lock.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        main()
    except SyncError as error:
        print("ASA symbol publication stopped: " + str(error), file=sys.stderr)
        raise SystemExit(1) from None
    except (OSError, ValueError, TypeError, KeyError, zipfile.BadZipFile):
        print("ASA symbol publication stopped; inspect retained artifacts/state before resuming.", file=sys.stderr)
        raise SystemExit(1) from None
