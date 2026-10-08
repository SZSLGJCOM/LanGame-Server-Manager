"""Refresh small static update manifests from one public GitCode release pointer.

No credentials, installer downloads, Nginx reloads, or signature verification are
performed here. The publishing pipeline verifies the signed installers before it
publishes the pointer; clients still verify the installer signature themselves.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import re
import time
from typing import Callable, Iterator
from urllib.error import HTTPError, URLError
from urllib.request import HTTPRedirectHandler, ProxyHandler, Request, build_opener
import uuid

if __package__:
    from .prepare_desktop_update_service import (
        bounded_read, gitcode_artifact_urls, json_bytes, read_json, regional_manifest,
    )
else:
    from prepare_desktop_update_service import (
        bounded_read, gitcode_artifact_urls, json_bytes, read_json, regional_manifest,
    )


# The documented raw API serves JSON bytes; the website's raw preview rejects
# this release pointer with HTTP 403 even when the file is publicly readable.
POINTER_URL = ("https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases"
               "/raw/updates/server-manager/release.json?ref=main")
GITHUB_REPOSITORY = "SZSLGJCOM/LanGame-Server-Manager"
MAX_POINTER_BYTES = 256 * 1024
HEX_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


@dataclass(frozen=True)
class ValidatedRelease:
    version: str
    identity: str
    pointer: bytes
    global_manifest: bytes
    cn_manifest: bytes


def release_asset_names(version: str) -> set[str]:
    small = f"LanGame.Server.Manager_{version}_x64-setup.exe"
    offline = f"LanGame.Server.Manager_{version}_x64-offline-setup.exe"
    return {small, small + ".sig", offline, offline + ".sig",
            "latest.json", "SHA256SUMS", "SHA256SUMS.offline"}


def validate_release_pointer(pointer: dict, public_key: str) -> ValidatedRelease:
    """Shared producer/consumer schema. Checks records, not remote installer bytes.

    Call this in the mirror publisher immediately before committing release.json.
    All assets must already have passed download/hash/Minisign verification there.
    """
    fields = {"schema_version", "github", "version", "manifest", "manifest_text", "gitcode_url", "assets"}
    if not isinstance(pointer, dict) or set(pointer) != fields or type(pointer["schema_version"]) is not int:
        raise ValueError("release pointer must contain exactly the schema-version-1 fields")
    if pointer["schema_version"] != 1:
        raise ValueError("unsupported release pointer schema")
    version = pointer["version"]
    if not isinstance(version, str) or len(version) > 32:
        raise ValueError("release pointer version must be a bounded stable version")
    manifest = pointer["manifest"]
    if not isinstance(manifest, dict) or manifest.get("version") != version:
        raise ValueError("release pointer and manifest versions must match")
    # Reuses the exact HTTPS origin/repository/version/name allowlist and the
    # public-key-ID/Minisign-format validation from offline preparation.
    cn = regional_manifest(manifest, pointer["gitcode_url"], public_key)
    github = pointer["github"]
    if (not isinstance(github, dict) or set(github) != {"repository", "release_id", "tag"}
            or github["repository"] != GITHUB_REPOSITORY or github["tag"] != f"v{version}"
            or type(github["release_id"]) is not int or github["release_id"] <= 0):
        raise ValueError("release pointer GitHub repository, release ID and tag must match")
    text = pointer["manifest_text"]
    if not isinstance(text, str) or text.startswith("\ufeff"):
        raise ValueError("manifest_text must be the original UTF-8 JSON without BOM")
    raw_manifest = text.encode("utf-8")
    if len(raw_manifest) > MAX_POINTER_BYTES or read_json(raw_manifest) != manifest:
        raise ValueError("manifest_text must decode to exactly the manifest object")
    assets = pointer["assets"]
    expected_names = release_asset_names(version)
    if not isinstance(assets, list) or len(assets) != len(expected_names):
        raise ValueError("release pointer must describe all seven release assets")
    indexed = {}
    for asset in assets:
        if not isinstance(asset, dict) or set(asset) != {"name", "size", "sha256", "url"}:
            raise ValueError("asset records require exactly name, size, sha256 and url")
        name, size, sha256 = asset["name"], asset["size"], asset["sha256"]
        if not isinstance(name, str) or name not in expected_names or name in indexed:
            raise ValueError("asset names must be unique members of the seven-file release")
        if type(size) is not int or not 0 < size <= 2 * 1024 * 1024 * 1024:
            raise ValueError("asset size must be a positive GitHub release asset byte count")
        if not isinstance(sha256, str) or not HEX_SHA256.fullmatch(sha256):
            raise ValueError("asset sha256 must be lowercase hexadecimal")
        if asset["url"] not in gitcode_artifact_urls(version, name):
            raise ValueError("asset URL must use the official fixed-version GitCode release path")
        indexed[name] = asset
    latest = indexed["latest.json"]
    if latest["size"] != len(raw_manifest) or latest["sha256"] != hashlib.sha256(raw_manifest).hexdigest():
        raise ValueError("manifest_text must match the latest.json asset size and SHA-256")
    small = indexed[f"LanGame.Server.Manager_{version}_x64-setup.exe"]
    if pointer["gitcode_url"] != small["url"]:
        raise ValueError("gitcode_url must identify the verified small installer asset")
    canonical = dict(pointer)
    canonical["assets"] = [indexed[name] for name in sorted(indexed)]
    encoded = json.dumps(canonical, sort_keys=True, ensure_ascii=False, separators=(",", ":"),
                         allow_nan=False).encode("utf-8")
    if len(encoded) > MAX_POINTER_BYTES:
        raise ValueError("release pointer exceeds 256 KiB")
    return ValidatedRelease(version, hashlib.sha256(encoded).hexdigest(), encoded + b"\n",
                            raw_manifest, json_bytes(cn))


class NoRedirects(HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        raise ValueError("public metadata redirects are not allowed; retain the last verified pointer")


def fetch_pointer() -> bytes:
    """One anonymous, bounded request; no environment proxy or alternate URL."""
    opener = build_opener(ProxyHandler({}), NoRedirects())
    request = Request(POINTER_URL, headers={"Accept": "application/json, text/plain",
                      "Accept-Encoding": "identity", "User-Agent": "LGSM-metadata-refresh/1"})
    deadline = time.monotonic() + 30
    with opener.open(request, timeout=15) as response:
        if response.status != 200:
            raise ValueError("public metadata must return HTTP 200")
        length = response.headers.get("Content-Length")
        if length is not None and not 0 < int(length) <= MAX_POINTER_BYTES:
            raise ValueError("public metadata Content-Length exceeds 256 KiB or is empty")
        chunks = []
        total = 0
        while True:
            if time.monotonic() >= deadline:
                raise TimeoutError("public metadata exceeded its download deadline")
            # read1 returns after one buffered/socket read, so a peer sending
            # occasional bytes cannot hide the overall deadline inside read(n).
            chunk = response.read1(min(16 * 1024, MAX_POINTER_BYTES + 1 - total))
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)
            if total > MAX_POINTER_BYTES:
                raise ValueError("public metadata body exceeds 256 KiB")
    return b"".join(chunks)


@contextmanager
def refresh_lock(root: Path) -> Iterator[None]:
    lock_path = root / ".refresh.lock"
    if lock_path.is_symlink():
        raise ValueError("refresh lock cannot be a symlink")
    flags = os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0)
    with os.fdopen(os.open(lock_path, flags, 0o644), "r+b") as lock:
        if os.name == "nt":  # Enables the same filesystem tests on Windows.
            import msvcrt
            if os.fstat(lock.fileno()).st_size == 0:
                lock.write(b"0"); lock.flush()
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        yield


def revision_files(release: ValidatedRelease) -> dict[str, bytes]:
    return {"release.json": release.pointer, "latest-global.json": release.global_manifest,
            "latest-cn.json": release.cn_manifest}


def verify_revision(directory: Path, release: ValidatedRelease) -> None:
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("revision must be a real directory")
    for name, expected in revision_files(release).items():
        path = directory / name
        if path.is_symlink() or bounded_read(path, MAX_POINTER_BYTES + 1) != expected:
            raise ValueError("existing revision differs from its verified release pointer")


def current_release(root: Path, public_key: str) -> ValidatedRelease | None:
    current = root / "current"
    if not current.is_symlink():
        if current.exists():
            raise ValueError("current must be an owned relative symlink, not a directory/file")
        return None
    target = os.readlink(current).replace("\\", "/")
    if not re.fullmatch(r"revisions/[0-9]+\.[0-9]+\.[0-9]+-[0-9a-f]{64}", target):
        raise ValueError("current symlink points outside the immutable release layout")
    directory = root / target
    if directory.is_symlink() or not directory.resolve().is_relative_to(root / "revisions"):
        raise ValueError("current revision escapes the state directory")
    pointer_path = directory / "release.json"
    if pointer_path.is_symlink():
        raise ValueError("saved release pointer cannot be a symlink")
    release = validate_release_pointer(read_json(bounded_read(pointer_path, MAX_POINTER_BYTES + 1)), public_key)
    if directory.name != f"{release.version}-{release.identity}":
        raise ValueError("current directory identity does not match its saved pointer")
    verify_revision(directory, release)
    return release


def publish_release(root: Path, release: ValidatedRelease) -> None:
    revisions = root / "revisions"
    if revisions.is_symlink():
        raise ValueError("revisions directory cannot be a symlink")
    revisions.mkdir(exist_ok=True)
    name = f"{release.version}-{release.identity}"
    destination = revisions / name
    stage = revisions / (".prepare-" + uuid.uuid4().hex)
    link = root / (".current-" + uuid.uuid4().hex)
    created: list[Path] = []
    try:
        if destination.exists():
            verify_revision(destination, release)
        else:
            stage.mkdir()
            for filename, data in revision_files(release).items():
                path = stage / filename
                with path.open("xb") as output:
                    created.append(path)
                    output.write(data)
                    output.flush()
                    os.fsync(output.fileno())
            stage.rename(destination)
            created.clear()
        os.symlink(f"revisions/{name}", link, target_is_directory=True)
        os.replace(link, root / "current")
    finally:
        if link.is_symlink():
            link.unlink()
        for path in reversed(created):
            path.unlink(missing_ok=True)
        if stage.exists():
            stage.rmdir()


def refresh_once(state_root: Path, public_key: str,
                 fetch: Callable[[], bytes] = fetch_pointer) -> dict:
    if state_root.is_symlink():
        raise ValueError("state directory cannot be a symlink")
    root = state_root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    with refresh_lock(root):
        raw = fetch()
        if len(raw) > MAX_POINTER_BYTES:
            raise ValueError("release pointer exceeds 256 KiB")
        release = validate_release_pointer(read_json(raw), public_key)
        current = current_release(root, public_key)
        if current is not None:
            old = tuple(int(part) for part in current.version.split("."))
            new = tuple(int(part) for part in release.version.split("."))
            if new < old:
                raise ValueError("refusing a release version downgrade")
            if new == old:
                if current.identity != release.identity:
                    raise ValueError("same version has a different release identity")
                return {"status": "unchanged", "version": release.version, "identity": release.identity}
        publish_release(root, release)
        return {"status": "published", "version": release.version, "identity": release.identity}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, default=Path("/var/lib/langame/desktop-updates"))
    parser.add_argument("--public-key-file", type=Path, required=True,
                        help="root-owned copy of the application's public updater key (never a private key)")
    args = parser.parse_args()
    try:
        public_key = bounded_read(args.public_key_file, 16384).decode("utf-8-sig").strip()
        result = refresh_once(args.state_dir, public_key)
    except (OSError, ValueError, KeyError, TypeError, HTTPError, URLError) as error:
        parser.exit(1, f"update metadata refresh failed; current retained: {error}\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
