"""Publish verified immutable extension assets, then advance the signed feed.

Uses GITHUB_TOKEN or Git's configured credential helper without printing either.
The extension releases are prereleases so they cannot replace the desktop's
releases/latest update pointer. A dry run verifies local signatures only.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
REPO = "SZSLGJCOM/LanGame-Server-Manager"
API = f"https://api.github.com/repos/{REPO}"
FEED_TAG = "enshrouded-control"


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


class ApiError(RuntimeError):
    def __init__(self, status: int):
        super().__init__(f"GitHub request failed with HTTP {status}")
        self.status = status


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, new_url):
        redirected = super().redirect_request(request, fp, code, message, headers, new_url)
        if redirected and urllib.parse.urlsplit(request.full_url).netloc != urllib.parse.urlsplit(new_url).netloc:
            redirected.remove_header("Authorization")
        require(urllib.parse.urlsplit(new_url).scheme == "https", "Refusing a non-HTTPS redirect")
        return redirected


def credential() -> str:
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    if not token:
        result = subprocess.run(["git", "credential", "fill"], input="protocol=https\nhost=github.com\n\n",
                                capture_output=True, text=True, timeout=30, check=False,
                                env={**os.environ, "GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "never"})
        require(result.returncode == 0, "GitHub credentials are unavailable; set GITHUB_TOKEN")
        values = dict(line.split("=", 1) for line in result.stdout.splitlines() if "=" in line)
        token = values.get("password")
    require(bool(token) and not any(char.isspace() for char in token), "GitHub token is unavailable or invalid")
    return token


class Client:
    def __init__(self, token: str):
        self.token = token
        self.opener = urllib.request.build_opener(SafeRedirect())

    def request(self, url: str, *, method: str = "GET", data=None, binary: bool = False, limit: int = 4 * 1024 * 1024):
        host = urllib.parse.urlsplit(url).hostname
        require(host in {"api.github.com", "uploads.github.com"}, "Unexpected authenticated API host")
        headers = {"User-Agent": "LanGame-Enshrouded-Control-Publisher", "X-GitHub-Api-Version": "2022-11-28",
                   "Authorization": f"Bearer {self.token}",
                   "Accept": "application/octet-stream" if binary else "application/vnd.github+json"}
        encoded = data if isinstance(data, bytes) else json.dumps(data).encode() if data is not None else None
        if encoded is not None:
            headers["Content-Type"] = "application/octet-stream" if isinstance(data, bytes) else "application/json"
        try:
            with self.opener.open(urllib.request.Request(url, headers=headers, data=encoded, method=method), timeout=90) as response:
                result = response.read(limit + 1)
                require(len(result) <= limit, "GitHub response exceeds read budget")
                return result if binary else json.loads(result) if result else None
        except urllib.error.HTTPError as error:
            raise ApiError(error.code) from None
        except urllib.error.URLError:
            raise RuntimeError("GitHub connection failed; check network/proxy settings") from None

    def release(self, tag: str):
        try:
            return self.request(f"{API}/releases/tags/{tag}")
        except ApiError as error:
            if error.status == 404:
                return None
            raise

    def create(self, tag: str, commit: str, body: str):
        return self.request(f"{API}/releases", method="POST", data={"tag_name": tag, "target_commitish": commit,
                            "name": tag, "body": body, "draft": True, "prerelease": True, "make_latest": "false"})

    def assets(self, release: dict) -> dict:
        assets = self.request(f"{API}/releases/{release['id']}/assets?per_page=100")
        require(isinstance(assets, list) and len(assets) < 100, "Release asset budget exceeded")
        result = {asset["name"]: asset for asset in assets}
        require(len(result) == len(assets), "Duplicate release asset names")
        return result

    def download(self, asset: dict) -> bytes:
        require(type(asset.get("id")) is int and asset["id"] > 0, "Invalid asset ID")
        require(type(asset.get("size")) is int and 0 < asset["size"] <= 2 * 1024 * 1024, "Invalid asset size")
        result = self.request(f"{API}/releases/assets/{asset['id']}", binary=True, limit=2 * 1024 * 1024)
        require(len(result) == asset["size"], "Downloaded asset size mismatch")
        return result

    def upload(self, release: dict, name: str, content: bytes):
        url = f"https://uploads.github.com/repos/{REPO}/releases/{release['id']}/assets?name={urllib.parse.quote(name, safe='')}"
        result = self.request(url, method="POST", data=content)
        require(result.get("name") == name and result.get("size") == len(content), "Uploaded asset identity mismatch")
        require(self.download(result) == content, "Uploaded asset readback differs from verified local content")
        return result

    def publish(self, release: dict, body: str):
        return self.request(f"{API}/releases/{release['id']}", method="PATCH",
                            data={"body": body, "draft": False, "prerelease": True, "make_latest": "false"})


def node_verify(node: str, directory: Path, manifest_only: bool = False) -> dict:
    command = [node, str(ROOT / "scripts/enshrouded_control_package.cjs")]
    if manifest_only:
        command += ["verify-manifest", "--manifest", str(directory / "manifest.json"),
                    "--signature", str(directory / "manifest.json.minisig")]
    else:
        command += ["verify", "--directory", str(directory)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30, check=False)
    require(result.returncode == 0, "Extension package cryptographic verification failed")
    return json.loads(result.stdout)


def frozen_files(directory: Path, manifest: dict) -> dict[str, bytes]:
    names = [manifest["dll"]["asset_name"], manifest["dll"]["asset_name"] + ".minisig", "manifest.json", "manifest.json.minisig",
             "LICENSE.txt", "LICENSE.minhook.txt", "SHA256SUMS"]
    result = {}
    for name in names:
        file = directory / name
        require(file.is_file() and not file.is_symlink() and 0 < file.stat().st_size <= 2 * 1024 * 1024, "Invalid package input file")
        result[name] = file.read_bytes()
    expected = "".join(f"{hashlib.sha256(result[name]).hexdigest()}  {name}\n" for name in names[:-1])
    require(result["SHA256SUMS"].decode("ascii") == expected, "Checksum inventory differs from verified package")
    require(json.loads(result["manifest.json"]) == {key: value for key, value in manifest.items() if key != "verified"},
            "Package manifest changed after signature verification")
    dll = result[manifest["dll"]["asset_name"]]
    require(len(dll) == manifest["dll"]["size"] and hashlib.sha256(dll).hexdigest() == manifest["dll"]["sha256"],
            "Package DLL changed after signature verification")
    return result


def verify_manifest_files(contents: dict[str, bytes], node: str) -> dict:
    require(set(contents) == {"manifest.json", "manifest.json.minisig"}, "Expected a complete signed feed pair")
    with tempfile.TemporaryDirectory(prefix="langame-enshrouded-pointer-") as temporary:
        directory = Path(temporary)
        for name, content in contents.items():
            (directory / name).write_bytes(content)
        return node_verify(node, directory, manifest_only=True)


def feed_files(client: Client, release: dict) -> tuple[dict, dict]:
    assets = client.assets(release)
    require(set(assets).issubset({"manifest.json", "manifest.json.minisig"}), "Existing feed has unexpected assets")
    contents = {}
    for name, asset in assets.items():
        # GitHub can retain a zero-byte 'starter' asset after a failed upload.
        # It authorizes no content and is treated as a missing file on recovery.
        contents[name] = None if asset.get("state") == "starter" and asset.get("size") == 0 else client.download(asset)
    return assets, contents


def old_manifest(client: Client, release: dict, node: str) -> dict | None:
    _, contents = feed_files(client, release)
    if not contents and release.get("draft"):
        return None
    require(set(contents) == {"manifest.json", "manifest.json.minisig"} and all(contents.values()), "Existing feed is incomplete")
    return verify_manifest_files(contents, node)


def intent_target(manifest: dict, files: dict[str, bytes], release_id: int) -> dict:
    return {"feed_release_id": release_id, "version": manifest["version"], "revision": manifest["revision"],
            "sha256": {name: hashlib.sha256(files[name]).hexdigest() for name in ("manifest.json", "manifest.json.minisig")}}


def read_intent(path: Path, manifest: dict, files: dict[str, bytes], pointer: dict | None, node: str) -> tuple[dict, dict | None]:
    require(path.is_file() and not path.is_symlink() and 0 < path.stat().st_size <= 128 * 1024, "Invalid publication intent file")
    intent = json.loads(path.read_bytes())
    require(set(intent) == {"schema", "target", "original"} and intent["schema"] == 1, "Invalid publication intent schema")
    require(pointer is not None and intent["target"] == intent_target(manifest, files, pointer["id"]),
            "Publication intent is bound to a different target or feed release")
    original = intent["original"]
    require(isinstance(original, dict) and set(original).issubset({"manifest.json", "manifest.json.minisig"}), "Invalid original feed intent")
    decoded = {}
    for name, entry in original.items():
        require(isinstance(entry, dict) and set(entry) == {"base64", "sha256"}, "Invalid original feed evidence")
        try:
            content = base64.b64decode(entry["base64"], validate=True)
        except (ValueError, TypeError):
            raise ValueError("Invalid original feed evidence encoding") from None
        require(hashlib.sha256(content).hexdigest() == entry["sha256"], "Original feed evidence checksum differs")
        decoded[name] = content
    previous = verify_manifest_files(decoded, node) if decoded else None
    return decoded, previous


def save_intent(path: Path, manifest: dict, files: dict[str, bytes], release_id: int, original: dict[str, bytes]) -> None:
    intent = {"schema": 1, "target": intent_target(manifest, files, release_id),
              "original": {name: {"base64": base64.b64encode(content).decode("ascii"),
                                    "sha256": hashlib.sha256(content).hexdigest()} for name, content in original.items()}}
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        with temporary.open("x", encoding="utf-8", newline="\n") as destination:
            destination.write(json.dumps(intent, indent=2) + "\n")
            destination.flush()
            os.fsync(destination.fileno())
        require(temporary.parent.resolve() == path.parent.resolve(), "Unexpected publication intent parent")
        # Windows rename refuses an existing destination. POSIX link provides
        # the same exclusive atomic publication without rename's overwrite.
        if os.name == "nt":
            os.rename(temporary, path)
        else:
            os.link(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def publish(client: Client, manifest: dict, files: dict[str, bytes], commit: str, node: str, intent_path: Path) -> dict:
    tag = f"enshrouded-control-v{manifest['version']}"
    pointer = client.release(FEED_TAG)
    recovering = intent_path.exists()
    original, previous = read_intent(intent_path, manifest, files, pointer, node) if recovering else ({}, old_manifest(client, pointer, node) if pointer else None)
    if previous:
        require(manifest["revision"] >= previous["revision"], "Refusing to move the extension feed revision backward")
        if manifest["revision"] == previous["revision"]:
            require({k: v for k, v in manifest.items() if k != "verified"} == {k: v for k, v in previous.items() if k != "verified"},
                    "An existing revision cannot be relabeled or replaced")
        else:
            previous_version = tuple(int(part) for part in previous["version"].split("."))
            next_version = tuple(int(part) for part in manifest["version"].split("."))
            require(next_version > previous_version, "A new revision requires a newer immutable extension version")
            old_profiles = {(profile["game_build_id"], profile["executable_sha256"]) for profile in previous["profiles"]}
            new_profiles = {(profile["game_build_id"], profile["executable_sha256"]) for profile in manifest["profiles"]}
            require(old_profiles.issubset(new_profiles), "New extension releases must retain previously verified profiles")
    body = (f"Independent signed Enshrouded player-management extension {manifest['version']} (revision {manifest['revision']}).\n\n"
            "Verified dedicated-server builds: " + ", ".join(profile["game_build_id"] for profile in manifest["profiles"]) + ".\n"
            "The manager authenticates the signed manifest and DLL before stopped-instance installation. No game binaries are included.\n\n"
            f"Licenses: [LanGame](https://github.com/{REPO}/releases/download/{tag}/LICENSE.txt), "
            f"[MinHook/HDE](https://github.com/{REPO}/releases/download/{tag}/LICENSE.minhook.txt).\n")
    release = client.release(tag) or client.create(tag, commit, body)
    require(release.get("tag_name") == tag and release.get("prerelease") is True, "Versioned extension release has unexpected identity/channel")
    existing = client.assets(release)
    require(set(existing).issubset(files), "Versioned release contains unexpected assets")
    for name, content in files.items():
        if name in existing:
            require(client.download(existing[name]) == content, "Immutable versioned asset already exists with different bytes")
        else:
            require(release.get("draft") is True, "Published versioned releases cannot be repaired or mutated")
            client.upload(release, name, content)
    if release.get("draft"):
        release = client.publish(release, body)
    # A version is public and verified before the moving pointer names it.
    pointer = pointer or client.create(FEED_TAG, commit, "Signed Enshrouded extension compatibility feed.")
    require(pointer.get("prerelease") is True, "The extension feed must not occupy the desktop stable release channel")
    if not recovering:
        # Reread immediately before mutation to reject another publisher's advancement.
        current = old_manifest(client, pointer, node)
        require(current == previous, "Extension feed changed during publication; retry against the new revision")
        _, original = feed_files(client, pointer)
        require(not original or all(original.values()), "Existing feed is incomplete before publication")
        require((verify_manifest_files(original, node) if original else None) == previous,
                "Extension feed changed before recovery evidence was captured")
        save_intent(intent_path, manifest, files, pointer["id"], original)
    # Every immutable target asset has now been authenticated/read back. Resume
    # only this exact local target and release, without accepting unrelated bytes.
    existing, remote = feed_files(client, pointer)
    for name, content in remote.items():
        require(content is None or content == files[name] or content == original.get(name),
                "Remote feed contains bytes outside the original/target publication intent; refusing recovery")
    for name in ("manifest.json", "manifest.json.minisig"):
        if name in existing:
            if remote[name] == files[name]:
                continue
            client.request(f"{API}/releases/assets/{existing[name]['id']}", method="DELETE")
        client.upload(pointer, name, files[name])
    client.publish(pointer, body)
    verified_pointer = old_manifest(client, pointer, node)
    require(verified_pointer == manifest, "Published feed differs from the verified package")
    intent_path.unlink()
    return {"published": True, "version": manifest["version"], "revision": manifest["revision"],
            "release_url": release["html_url"],
            "feed_url": f"https://github.com/{REPO}/releases/download/{FEED_TAG}/manifest.json"}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--source-commit", help="Exact source commit for the new release tags")
    parser.add_argument("--node", default="node")
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    directory = args.directory.resolve()
    manifest = node_verify(args.node, directory)
    files = frozen_files(directory, manifest)
    # Verify the exact in-memory upload snapshot, including both signatures.
    # A concurrent local writer cannot substitute files between checks/upload.
    with tempfile.TemporaryDirectory(prefix="langame-enshrouded-frozen-") as temporary:
        frozen = Path(temporary)
        for name, content in files.items():
            (frozen / name).write_bytes(content)
        require(node_verify(args.node, frozen) == manifest, "Frozen package differs from the verified input")
    if not args.execute:
        print(json.dumps({"verified": True, "execute": False, "version": manifest["version"], "revision": manifest["revision"], "assets": list(files)}))
        return
    require(bool(args.source_commit) and re.fullmatch(r"[a-f0-9]{40}", args.source_commit), "--execute requires the exact 40-character --source-commit")
    intent_path = directory.parent / f".{directory.name}.publish-intent.json"
    print(json.dumps(publish(Client(credential()), manifest, files, args.source_commit, args.node, intent_path)))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"Enshrouded extension publication failed: {error}") from None
