"""Prepare immutable ARK dependencies; only --execute appends GitCode assets.

The canonical package identities live in modules/ark-tools/dependencies.json.
Reuse the same work directory after interruption: an unresolved mutation is
reconciled by remote reads, never automatically replayed. No desktop pointer,
release promotion, deletion, attachment replacement, or task configuration.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile

if __package__:
    from .sync_desktop_release_to_gitcode import (
        GC_API, GITCODE_REPO, NoRedirect, SyncError, Transport, file_hash,
        parse_json, provider_opener, require,
    )
else:
    from sync_desktop_release_to_gitcode import (
        GC_API, GITCODE_REPO, NoRedirect, SyncError, Transport, file_hash,
        parse_json, provider_opener, require,
    )

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "modules/ark-tools/dependencies.json"
MAX_LICENSE = 128 * 1024
PACKAGE_KINDS = {
    "ase": ("arksurvivalevolved", "ArkApi", "AseApi"),
    "asa": ("arksurvivalascended", "AsaApi", "AsaApi"),
}
# Attribution references, not replacements for the canonical binary identities.
NOTICE_SOURCES = (
    ("ASE Permissions upstream project", "ArkServerApi/ASE-Plugins/7aeea5dfe5f87cad9f94c304ceeba423d9b19cb6/LICENSE.TXT"),
    ("ASA Permissions upstream project", "ArkServerApi/ASA-Plugins/e82ff85b7219b56701c69c5ef7ebd3566833f878/LICENSE"),
    ("ASA loader upstream project", "ArkServerApi/AsaApiLoader/4e04dc8982e1473920278f199638b29dc511d539/LICENSE"),
    ("OpenSSL 3.2.1", "openssl/openssl/a7e992847de83aa36be0c399c89db3fb827b0be2/LICENSE.txt"),
    ("fmt 12.1.0 (fixed ASA source baseline)", "fmtlib/fmt/407c905e45ad75fc29bf0f9bb7c5c2fd3475976f/LICENSE"),
    ("Poco 1.14.1 (fixed ASA source baseline)", "pocoproject/poco/fee4dccb44396911e9559c6875e88146f64b7f55/LICENSE"),
    ("Detours (fixed ASA source baseline)", "microsoft/Detours/9764cebcb1a75940e68fa83d6730ffaf0f669401/LICENSE.md"),
    ("zlib 1.3.1 (fixed ASA source baseline)", "madler/zlib/51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf/LICENSE"),
    ("minizip 1.3.1 (fixed ASA source baseline)", "madler/zlib/51b7f2abdade71cd9bb0e7a373ef2610ec6f9daf/contrib/minizip/MiniZip64_info.txt"),
)


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")


def load_manifest(path=MANIFEST):
    raw = path.read_bytes()
    require(len(raw) <= 16 * 1024, "Dependency manifest exceeds limit")
    manifest = parse_json(raw)
    require(isinstance(manifest, dict) and set(manifest) == {
        "schemaVersion", "mirrorRepository", "mirrorTag", "packages"}, "Invalid dependency manifest fields")
    require(type(manifest["schemaVersion"]) is int and manifest["schemaVersion"] == 1
            and manifest["mirrorRepository"] == GITCODE_REPO, "Unexpected manifest version or repository")
    packages = manifest["packages"]
    require(isinstance(packages, list) and len(packages) == 2, "Expected ASE and ASA packages")
    versions, seen = {}, set()
    for package in packages:
        require(isinstance(package, dict) and set(package) == {
            "id", "moduleId", "version", "fileName", "upstreamUrl", "mirrorUrl", "size", "sha256", "licenseUrl"},
            "Invalid dependency package fields")
        package_id = package["id"]
        require(isinstance(package_id, str) and package_id in PACKAGE_KINDS and package_id not in seen,
                "Unexpected or duplicate package ID")
        seen.add(package_id)
        module, prefix, upstream = PACKAGE_KINDS[package_id]
        version = package["version"]
        require(isinstance(version, str) and re.fullmatch(re.escape(prefix) + r" [0-9]+\.[0-9]+", version),
                "Invalid fixed dependency version")
        version = version.split(" ")[1]
        versions[package_id] = version
        name = f"{prefix}_{version}.zip"
        require(package["moduleId"] == module and package["fileName"] == name
                and package["upstreamUrl"] == f"https://github.com/ArkServerApi/{upstream}/releases/download/{version}/{name}",
                "Dependency source identity mismatch")
        require(type(package["size"]) is int and 0 < package["size"] <= 32 * 1024 * 1024
                and isinstance(package["sha256"], str) and re.fullmatch(r"[a-f0-9]{64}", package["sha256"]),
                "Invalid dependency size or SHA-256")
        require(isinstance(package["licenseUrl"], str) and re.fullmatch(
            rf"https://raw\.githubusercontent\.com/ArkServerApi/{upstream}/[a-f0-9]{{40}}/LICENSE",
            package["licenseUrl"]), "Dependency license must use the fixed upstream commit")
    tag = f"ark-tools-dependencies-ase-{versions['ase']}-asa-{versions['asa']}"
    require(manifest["mirrorTag"] == tag, "Dependency tag differs from its package versions")
    for package in packages:
        require(package["mirrorUrl"] == attachment_url(tag, package["fileName"]), "Unexpected mirror address")
    return manifest, raw


def attachment_url(tag, name):
    return f"{GC_API}/releases/{tag}/attach_files/{name}/download"


def write_immutable(path, content):
    require(not path.is_symlink(), "Symbolic-link artifact refused")
    if path.exists():
        require(path.is_file() and path.read_bytes() == content, "Prepared file differs; preserve it and inspect the work directory")
        return
    with path.open("xb") as stream:
        stream.write(content)
        stream.flush()
        os.fsync(stream.fileno())


def atomic_json(path, value):
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    write_immutable(temporary, json_bytes(value))
    os.replace(temporary, path)


def fetch_license(url):
    require(re.fullmatch(r"https://raw\.githubusercontent\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/[a-f0-9]{40}/[A-Za-z0-9_./-]+", url),
            "License source must be an immutable public GitHub file")
    request = urllib.request.Request(url, headers={"User-Agent": "LGSM-ark-dependency-publisher"})
    try:
        with provider_opener("github", NoRedirect()).open(request, timeout=30) as response:
            require(response.status == 200, "License download did not return complete content")
            content = response.read(MAX_LICENSE + 1)
        require(0 < len(content) <= MAX_LICENSE, "License document exceeds limit")
        content.decode("utf-8-sig")
        return content
    except (urllib.error.URLError, TimeoutError, OSError, UnicodeError):
        raise SyncError("Immutable upstream license download failed") from None


def archive_members(path):
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        require(0 < len(entries) <= 512, "Unexpected dependency archive inventory")
        names = set()
        for entry in entries:
            name = entry.filename
            require(name not in names and len(name) <= 300 and "\\" not in name
                    and not PurePosixPath(name).is_absolute() and ".." not in PurePosixPath(name).parts,
                    "Invalid dependency archive path")
            names.add(name)
        return sorted(names)


def prepare(client, manifest, manifest_raw, directory, license_reader=fetch_license):
    directory.mkdir(parents=True, exist_ok=True)
    artifacts, archives, sources = [], {}, []
    notices = [
        "LanGame ARK tools dependency mirror - third-party notices\n",
        "These are byte-for-byte copies of the identified upstream release archives.\n"
        "The SDK MIT licenses apply to their respective upstream projects; they do\n"
        "not relicense every binary in the archives or the proprietary game.\n"
        "The original archives have no LICENSE/NOTICE document. The references\n"
        "below preserve the reviewed project notices and identify bundled vendors;\n"
        "they are not a claim of a reproduced upstream binary build.\n",
        "Microsoft components: msdia140.dll (both archives), msvcp140.dll (ASA).\n"
        "These remain Microsoft components under Microsoft's applicable terms.\n"
        "Visual Studio's redistribution list identifies the DIA SDK DLL and C++\n"
        "runtime files for unmodified redistribution with a program, subject to\n"
        "the corresponding product license. No MIT license is asserted for them.\n"
        "https://learn.microsoft.com/en-us/visualstudio/releases/2022/redistribution\n"
        "https://www.microsoft.com/licensing/guidance/Visual-Studio\n",
        "ASA's libcrypto-3-x64.dll identifies OpenSSL 3.2.1 (30 Jan 2024); its\n"
        "libssl-3-x64.dll accompanies it. The complete upstream license is below.\n"
        "Other dependencies declared by the fixed ASA source's vcpkg baseline\n"
        "ed8445dd2a9ba44e0e3ace4e952b2e33130082d9 include fmt 12.1.0, Poco 1.14.1,\n"
        "Detours, zlib 1.3.1 and minizip 1.3.1. Their complete license documents\n"
        "are preserved below. A declared source version is not a claim of a\n"
        "reproduced binary build or the embedded version of every component.\n"
        "Baseline source: https://github.com/microsoft/vcpkg/tree/"
        "ed8445dd2a9ba44e0e3ace4e952b2e33130082d9/ports\n",
    ]
    for package in manifest["packages"]:
        name = package["fileName"]
        print(json.dumps({"stage": "prepare_dependency", "name": name}), flush=True)
        client.download(package["upstreamUrl"], directory / name, package["size"], package["sha256"], "github")
        archives[name] = archive_members(directory / name)
        artifacts.append(name)
        license_name = package["id"].upper() + "-LICENSE.txt"
        license_bytes = license_reader(package["licenseUrl"])
        require(b"MIT License" in license_bytes and b"Permission is hereby granted" in license_bytes,
                "Expected upstream SDK MIT license")
        write_immutable(directory / license_name, license_bytes)
        artifacts.append(license_name)
        sources.append({"name": license_name, "url": package["licenseUrl"], "sha256": hashlib.sha256(license_bytes).hexdigest()})
        notices.append(f"{package['version']}\nSource: {package['upstreamUrl']}\n"
                       f"Archive SHA-256: {package['sha256']}\nSDK license: {package['licenseUrl']}\n"
                       "Archive members:\n" + "\n".join(archives[name]) + "\n")
        if package["id"] == "asa":
            notices.append("ASA dependency declaration: " + package["licenseUrl"].rsplit("/", 1)[0] + "/AsaApi/vcpkg.json\n")
    for title, reference in NOTICE_SOURCES:
        url = "https://raw.githubusercontent.com/" + reference
        content = license_reader(url)
        digest = hashlib.sha256(content).hexdigest()
        sources.append({"name": title, "url": url, "sha256": digest})
        notices.append(f"{title}\nSource: {url}\nDocument SHA-256: {digest}\n\n" + content.decode("utf-8-sig"))
    notices.append("Additional notices preserved from LanGame's incorporated SDK headers\n\n" +
                   (ROOT / "modules/ark-tools/THIRD_PARTY_NOTICES.txt").read_text(encoding="utf-8-sig"))
    write_immutable(directory / "THIRD_PARTY_NOTICES.txt", ("\n\n".join(notices) + "\n").encode("utf-8"))
    write_immutable(directory / "dependencies.json", manifest_raw)
    artifacts.extend(("THIRD_PARTY_NOTICES.txt", "dependencies.json"))
    checksums = "".join(f"{file_hash(directory / name)}  {name}\n" for name in sorted(artifacts))
    write_immutable(directory / "SHA256SUMS", checksums.encode("ascii"))
    artifacts.append("SHA256SUMS")
    prepared = {"schemaVersion": 1, "repository": manifest["mirrorRepository"], "tag": manifest["mirrorTag"],
                "manifestSha256": hashlib.sha256(manifest_raw).hexdigest(),
                "assets": [{"name": name, "size": (directory / name).stat().st_size,
                            "sha256": file_hash(directory / name)} for name in sorted(artifacts)],
                "licenseSources": sources, "archiveMembers": archives}
    write_immutable(directory / "prepared.json", json_bytes(prepared))
    return prepared


def remote_assets(release, prepared, marker):
    require(isinstance(release, dict) and release.get("tag_name") == prepared["tag"]
            and release.get("release_status") == "pre" and marker in release.get("body", ""),
            "Dependency release identity/state differs; no mutation allowed")
    expected = {asset["name"] for asset in prepared["assets"]}
    entries = release.get("assets")
    require(isinstance(entries, list), "Invalid remote attachment inventory")
    names = set()
    for asset in entries:
        require(isinstance(asset, dict), "Invalid remote attachment")
        name = asset.get("name")
        require(isinstance(name, str) and name not in names, "Invalid or duplicate remote attachment name")
        # GitCode includes generated source archives. They are left untouched.
        require(name in expected or name.endswith((".zip", ".tar", ".tar.gz", ".tar.bz2")), "Unrecognized remote attachment")
        names.add(name)
    return names & expected


def wait_for_asset(client, path, prepared, marker, name):
    deadline = time.monotonic() + 30
    for delay in (0, 2, 4, 8, 8, 8):
        if delay:
            time.sleep(min(delay, max(0, deadline - time.monotonic())))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        release = client.api("gitcode", "GET", path, timeout=min(8, remaining))
        if name in remote_assets(release, prepared, marker):
            return release
    raise SyncError("Attachment outcome remains unknown after bounded readback; upload was not repeated")


def publish(client, prepared, directory):
    for asset in prepared["assets"]:
        source = directory / asset["name"]
        require(not source.is_symlink() and source.is_file() and source.stat().st_size == asset["size"]
                and file_hash(source) == asset["sha256"], "Prepared artifact changed before publication")
    identity = file_hash(directory / "prepared.json")
    marker = f"Dependency manifest SHA-256: {prepared['manifestSha256']}\nPrepared inventory SHA-256: {identity}"
    path = "/releases/tags/" + prepared["tag"]
    state_path = directory / "publish-state.json"
    state = parse_json(state_path.read_bytes()) if state_path.exists() else {
        "schemaVersion": 1, "preparedSha256": identity, "status": "ready", "pending": None}
    require(isinstance(state, dict) and state.get("schemaVersion") == 1 and state.get("preparedSha256") == identity
            and state.get("status") in {"ready", "running", "unknown", "failed", "verified"}, "Publishing state differs; inspect it first")
    attempt = directory / ("attempt-" + uuid.uuid4().hex)
    attempt.mkdir()
    write_immutable(attempt / "intent.json", json_bytes({"preparedSha256": identity, "repository": prepared["repository"],
                    "tag": prepared["tag"], "releaseStatus": "pre", "assets": prepared["assets"]}))
    assets = {asset["name"]: asset for asset in prepared["assets"]}
    verified = set()

    def checkpoint(pending=None, status="running"):
        state.update(status=status, pending=pending)
        atomic_json(state_path, state)

    def verify(name):
        asset = assets[name]
        client.download(attachment_url(prepared["tag"], name), attempt / name, asset["size"], asset["sha256"], "gitcode")
        verified.add(name)

    try:
        release = client.api("gitcode", "GET", path, missing_ok=True)
        existing = remote_assets(release, prepared, marker) if release is not None else set()
        # Check every same-name artifact before any append. Fresh paths force an
        # anonymous network download even when local originals already exist.
        for name in sorted(existing):
            verify(name)
        pending = state.get("pending")
        if pending is not None:
            require(isinstance(pending, dict) and pending.get("kind") in {"create_release", "upload"}, "Invalid pending mutation state")
            resolved = release is not None if pending["kind"] == "create_release" else pending.get("name") in verified
            require(resolved, "Previous mutation is unresolved; inspect remote state, do not replay it")
        checkpoint()
        if release is None:
            checkpoint({"kind": "create_release"})
            payload = {"tag_name": prepared["tag"], "name": prepared["tag"], "target_commitish": "main",
                       "release_status": "pre", "body": "Fixed upstream ARK tools dependencies for LanGame.\n\n" + marker}
            try:
                client.api("gitcode", "POST", "/releases", payload)
            except SyncError:
                # A timeout can follow a committed POST; query, never POST again.
                pass
            release = client.api("gitcode", "GET", path, missing_ok=True)
            require(release is not None, "Release creation outcome unknown; creation was not repeated")
            remote_assets(release, prepared, marker)
            checkpoint()
        for name in sorted(assets, key=lambda item: (assets[item]["size"], item)):
            if name in verified:
                continue
            release = client.api("gitcode", "GET", path)
            if name not in remote_assets(release, prepared, marker):
                ticket = client.api("gitcode", "GET", f"/releases/{prepared['tag']}/upload_url?" + urllib.parse.urlencode({"file_name": name}))
                checkpoint({"kind": "upload", "name": name})
                print(json.dumps({"stage": "upload_dependency_attachment", "name": name, "bytes": assets[name]["size"]}), flush=True)
                try:
                    client.upload(ticket, directory / name)
                except SyncError:
                    # A lost response is reconciled by GET and hash verification.
                    pass
                wait_for_asset(client, path, prepared, marker, name)
            verify(name)
            checkpoint()
        require(set(assets) <= remote_assets(client.api("gitcode", "GET", path), prepared, marker), "Final attachment inventory is incomplete")
        receipt = {"published": True, "repository": prepared["repository"], "tag": prepared["tag"],
                   "releaseStatus": "pre", "preparedSha256": identity, "verifiedAssets": len(verified),
                   "assets": [{**assets[name], "url": attachment_url(prepared["tag"], name)} for name in sorted(verified)]}
        write_immutable(attempt / "receipt.json", json_bytes(receipt))
        checkpoint(status="verified")
        return receipt
    except BaseException:
        checkpoint(state.get("pending"), "unknown" if state.get("pending") else "failed")
        write_immutable(attempt / "incomplete.json", json_bytes(state))
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, required=True, help="Persistent preparation/evidence directory outside the repository")
    parser.add_argument("--execute", action="store_true", help="Append missing verified assets to the dedicated pre-release")
    args = parser.parse_args()
    directory = args.work_dir.resolve()
    require(not directory.is_relative_to(ROOT), "Artifact work directory must be outside the repository")
    token = os.environ.get("GITCODE_RELEASE_TOKEN", "") if args.execute else ""
    require(not args.execute or token, "GITCODE_RELEASE_TOKEN is required for --execute")
    manifest, raw = load_manifest()
    directory.mkdir(parents=True, exist_ok=True)
    lock = directory / "publisher.lock"
    try:
        stream = lock.open("x", encoding="ascii")
    except FileExistsError:
        raise SyncError("Publisher lock exists; inspect its process and state before resuming") from None
    try:
        with stream:
            stream.write(str(os.getpid()) + "\n")
        client = Transport(gitcode_token=token)
        prepared = prepare(client, manifest, raw, directory)
        result = publish(client, prepared, directory) if args.execute else {
            "published": False, "tag": prepared["tag"], "preparedAssets": len(prepared["assets"]),
            "preparedSha256": file_hash(directory / "prepared.json"), "workDir": str(directory)}
        print(json.dumps(result, ensure_ascii=False), flush=True)
    finally:
        lock.unlink(missing_ok=True)


if __name__ == "__main__":
    try:
        main()
    except SyncError as error:
        print("ARK dependency publication stopped: " + str(error), file=sys.stderr)
        raise SystemExit(1) from None
    except (ValueError, TypeError, KeyError, OSError, zipfile.BadZipFile):
        print("ARK dependency publication stopped; inspect preserved preparation and mutation state before resuming.", file=sys.stderr)
        raise SystemExit(1) from None
