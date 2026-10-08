"""Mirror one immutable GitHub desktop release, then publish its verified pointer.

Only --execute writes GitCode. No deletion, asset replacement or mutation retry.
An interrupted run re-reads remote assets and verifies bytes before proceeding.
GitCode has public pre/latest releases, not documented draft releases. Clients
must consume the final main-branch pointer, never an incomplete pre-release.

API contracts: https://docs.gitcode.com/docs/apis/ (header authentication),
get-api-v-5-repos-owner-repo-releases-tag-upload-url (GET then signed PUT),
put-api-v-5-repos-owner-repo-contents-path (blob SHA compare-and-swap).
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
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

if __package__:
    from .refresh_desktop_update_service import validate_release_pointer
else:
    from refresh_desktop_update_service import validate_release_pointer

ROOT = Path(__file__).resolve().parents[1]
GITHUB_REPO = "SZSLGJCOM/LanGame-Server-Manager"
GITCODE_REPO = "SZSLGJCOM/LanGame-Server-Manager-Releases"
GH_API = "https://api.github.com/repos/" + GITHUB_REPO
GC_API = "https://api.gitcode.com/api/v5/repos/" + GITCODE_REPO
POINTER = "updates/server-manager/release.json"
MAX_ASSET = 1024 * 1024 * 1024
MAX_SMALL_INSTALLER = 256 * 1024 * 1024


class SyncError(Exception):
    """Sanitized operational failure; never include server bodies or URLs."""


def require(value, message):
    if not value:
        raise SyncError(message)


def version_from_tag(tag):
    require(isinstance(tag, str) and re.fullmatch(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", tag), "Expected stable vMAJOR.MINOR.PATCH tag")
    return tag[1:]


def asset_names(version):
    small = f"LanGame.Server.Manager_{version}_x64-setup.exe"
    offline = f"LanGame.Server.Manager_{version}_x64-offline-setup.exe"
    return (small, small + ".sig", offline, offline + ".sig", "SHA256SUMS", "SHA256SUMS.offline", "latest.json")


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "Duplicate JSON key")
        result[key] = value
    return result


def parse_json(raw):
    return json.loads(raw.decode("utf-8-sig"), object_pairs_hook=unique_object)


def file_hash(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def public_key():
    return parse_json((ROOT / "apps/desktop/src-tauri/tauri.conf.json").read_bytes())["plugins"]["updater"]["pubkey"]


def https_url(url):
    p = urllib.parse.urlsplit(url)
    require(p.scheme == "https" and p.hostname and not p.username and not p.password
            and p.port in (None, 443) and not p.fragment and "\\" not in url
            and not any(c.isspace() for c in url), "Invalid HTTPS address")
    return p


def public_url(url, provider):
    p = https_url(url)
    hosts = {"github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"}
    allowed = p.hostname in hosts if provider == "github" else (
        p.hostname in {"gitcode.com", "api.gitcode.com", "raw.gitcode.com"}
        or p.hostname.endswith((".gitcode.com", ".myhuaweicloud.com")))
    require(allowed, "Unexpected public download origin")
    return url


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise SyncError("Authenticated API or upload redirected; request not repeated")


class PublicRedirect(urllib.request.HTTPRedirectHandler):
    def __init__(self, provider):
        self.provider = provider

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        public_url(newurl, self.provider)
        require(not req.has_header("Authorization") and not req.has_header("Private-token"), "Credential on public download")
        return super().redirect_request(req, fp, code, msg, headers, newurl)


class Transport:
    def __init__(self, github_token="", gitcode_token=""):
        self.tokens = {"github": github_token, "gitcode": gitcode_token}

    def api(self, provider, method, suffix, payload=None, missing_ok=False, timeout=60):
        base = GH_API if provider == "github" else GC_API
        require(suffix.startswith("/") and not suffix.startswith("//"), "Invalid API path")
        headers = {"Accept": "application/json", "User-Agent": "LGSM-release-mirror"}
        token = self.tokens[provider]
        if token:
            headers["Authorization"] = "Bearer " + token
        data = None if payload is None else json.dumps(payload).encode()
        if data is not None:
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(base + suffix, data=data, headers=headers, method=method)
        try:
            with urllib.request.build_opener(NoRedirect()).open(req, timeout=timeout) as response:
                raw = response.read(2 * 1024 * 1024 + 1)
                require(len(raw) <= 2 * 1024 * 1024, "API response exceeds limit")
                return parse_json(raw)
        except urllib.error.HTTPError as error:
            if missing_ok and error.code == 404:
                return None
            raise SyncError(f"{provider} {method} API returned HTTP {error.code}; inspect before repeating writes") from None
        except (urllib.error.URLError, TimeoutError, OSError, ValueError):
            raise SyncError(f"{provider} {method} API failed; write outcome may be unknown") from None

    def download(self, url, destination, size, digest, provider):
        public_url(url, provider)
        require(0 < size <= MAX_ASSET and re.fullmatch(r"[a-f0-9]{64}", digest), "Invalid download identity")
        if destination.exists():
            require(destination.is_file() and destination.stat().st_size == size and file_hash(destination) == digest,
                    "Existing local artifact conflicts with frozen release")
            return
        partial = destination.with_name(destination.name + ".partial")
        require(not partial.exists(), "Unfinished local download exists; inspect it first")
        deadline = time.monotonic() + 1800
        created_partial = False
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "LGSM-release-mirror"})
            with partial.open("xb") as output:
                created_partial = True
                with urllib.request.build_opener(PublicRedirect(provider)).open(req, timeout=60) as response:
                    require(response.status == 200, "Expected complete public artifact response")
                    received = 0
                    while chunk := response.read(1024 * 1024):
                        received += len(chunk)
                        require(received <= size and time.monotonic() < deadline, "Download exceeded size or time budget")
                        output.write(chunk)
            require(received == size and file_hash(partial) == digest, "Public artifact size or SHA-256 mismatch")
            partial.rename(destination)
        except BaseException:
            if created_partial:
                partial.unlink(missing_ok=True)
            raise

    def upload(self, ticket, source):
        p = https_url(ticket.get("url", ""))
        require(p.hostname.endswith((".myhuaweicloud.com", ".gitcode.com")), "Unexpected signed upload origin")
        headers = ticket.get("headers")
        require(isinstance(headers, dict) and headers and all(isinstance(k, str) and isinstance(v, str)
                and "\r" not in k + v and "\n" not in k + v for k, v in headers.items()), "Invalid upload headers")
        require(not any(k.lower() in {"authorization", "private-token", "cookie", "host"} for k in headers), "Unexpected credential or routing upload header")
        # Signed URL/OBS callback headers are never logged or persisted. This is
        # the documented large-file PUT, not the 20 MB repository upload API.
        with source.open("rb") as stream:
            request = urllib.request.Request(ticket["url"], data=stream, method="PUT",
                headers={**headers, "Content-Length": str(source.stat().st_size)})
            try:
                with urllib.request.build_opener(NoRedirect()).open(request, timeout=1800) as response:
                    require(response.status in (200, 201, 204), "Upload returned an unexpected status")
            except (urllib.error.URLError, TimeoutError, OSError):
                raise SyncError("GitCode upload outcome unknown; next run must inspect the release, not replay blindly") from None


def freeze_release(release, tag, release_id):
    version = version_from_tag(tag)
    require(release.get("id") == release_id and release.get("tag_name") == tag
            and release.get("draft") is False and release.get("prerelease") is False,
            "GitHub release identity/state mismatch")
    assets = release.get("assets", [])
    expected = asset_names(version)
    require(len(assets) == 7 and {a.get("name") for a in assets} == set(expected), "Expected exactly seven canonical release assets")
    frozen = {}
    for asset in assets:
        name = asset["name"]
        require(type(asset.get("id")) is int and asset["id"] > 0 and type(asset.get("size")) is int
                and 0 < asset["size"] <= MAX_ASSET and asset.get("state") == "uploaded", "Invalid GitHub asset identity")
        digest = asset.get("digest", "")
        require(isinstance(digest, str) and re.fullmatch(r"sha256:[a-f0-9]{64}", digest), "GitHub asset must supply SHA-256 digest")
        if not name.endswith(".exe"):
            require(asset["size"] <= (256 * 1024 if name == "latest.json" else 16 * 1024), "Release metadata exceeds size limit")
        elif name == expected[0]:
            require(asset["size"] <= MAX_SMALL_INSTALLER, "Online installer exceeds the client 256 MiB limit")
        url = f"https://github.com/{GITHUB_REPO}/releases/download/{tag}/{name}"
        require(asset.get("browser_download_url") == url, "Unexpected GitHub release asset URL")
        frozen[name] = {"id": asset["id"], "size": asset["size"], "sha256": digest[7:], "url": url}
    require(len({a["id"] for a in frozen.values()}) == 7, "Duplicate GitHub asset ID")
    return frozen


def checksums(path, expected, assets):
    found = {}
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  ([A-Za-z0-9._-]+)", line)
        require(match is not None and match[2] not in found, "Invalid or duplicated checksum entry")
        found[match[2]] = match[1]
    require(set(found) == set(expected), "Checksum inventory does not match canonical artifacts")
    require(all(found[n] == assets[n]["sha256"] for n in found), "Checksum differs from frozen GitHub digest")


def verify_signature(directory, name, version, node_command):
    result = subprocess.run([*node_command, str(ROOT / "scripts/verify_desktop_update_signature.cjs"),
        "--config", str(ROOT / "apps/desktop/src-tauri/tauri.conf.json"), "--installer", str(directory / name),
        "--signature", str(directory / (name + ".sig")), "--version", version], capture_output=True, text=True, timeout=180)
    require(result.returncode == 0, "Cryptographic installer/signature/trusted-version verification failed")
    reports = [parse_json(line.encode()) for line in result.stdout.splitlines() if line.startswith("{")]
    require(len(reports) == 1 and reports[0].get("verified") is True
            and reports[0].get("signedVersionVerified") is True and reports[0].get("version") == version
            and reports[0].get("installerSHA256") == file_hash(directory / name), "Signature verifier receipt does not bind this artifact/version")


def verify_payloads(directory, version, assets, node_command):
    names = asset_names(version)
    checksums(directory / "SHA256SUMS", (names[0], names[1], "latest.json"), assets)
    checksums(directory / "SHA256SUMS.offline", names[2:4], assets)
    manifest = parse_json((directory / "latest.json").read_bytes())
    require(manifest.get("version") == version and set(manifest.get("platforms", {})) == {"windows-x86_64"}, "Manifest version/platform mismatch")
    platform = manifest["platforms"]["windows-x86_64"]
    require(set(platform) == {"url", "signature"} and platform["url"] == assets[names[0]]["url"]
            and platform["signature"] == (directory / names[1]).read_text(encoding="utf-8-sig").strip(), "Manifest download or signature mismatch")
    for name in (names[0], names[2]):
        verify_signature(directory, name, version, node_command)
    return manifest


def gitcode_assets(release, tag):
    require(release.get("tag_name") == tag and release.get("release_status") in ("pre", "latest"), "Unexpected GitCode release identity/state")
    result = {}
    for asset in release.get("assets", []):
        name = asset.get("name")
        # GitCode also includes generated source archives; they are not uploaded
        # desktop assets and are never advertised by our pointer.
        if name not in asset_names(version_from_tag(tag)):
            require(isinstance(name, str) and name.endswith((".zip", ".tar", ".tar.gz", ".tar.bz2")), "Unexpected GitCode attachment")
            continue
        require(name not in result, "Duplicate GitCode attachment")
        # Presence comes from the authenticated release response. The documented
        # attachment-download API is our stable anonymous validation entry; do
        # not advertise an unverified browser URL or an expiring OBS address.
        result[name] = f"{GC_API}/releases/{tag}/attach_files/{name}/download"
    return result


def pointer_content(response):
    if response is None:
        return None
    require(response.get("type") == "file" and response.get("encoding") == "base64"
            and response.get("path") == POINTER and re.fullmatch(r"[a-f0-9]{40}", response.get("sha", "")), "Invalid existing pointer identity")
    raw = base64.b64decode(response["content"].replace("\n", ""), validate=True)
    require(len(raw) <= 256 * 1024, "Pointer exceeds limit")
    return parse_json(raw)


def wait_for_attachment(client, path, tag, name):
    """OBS callback registration may lag a successful PUT. Only GET is repeated."""
    deadline = time.monotonic() + 30
    for delay in (0, 2, 4, 8, 8, 8):
        if delay:
            time.sleep(min(delay, max(0, deadline - time.monotonic())))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        release = client.api("gitcode", "GET", path, timeout=min(8, remaining))
        if name in gitcode_assets(release, tag):
            return release
    raise SyncError("Uploaded attachment registration did not appear within 30 seconds; PUT was not repeated")


def publish_pointer(client, pointer):
    path = "/contents/" + POINTER
    previous = client.api("gitcode", "GET", path + "?ref=main", missing_ok=True)
    old = pointer_content(previous)
    if old == pointer:
        return "unchanged"
    if old:
        require(old.get("schema_version") == 1 and old.get("github", {}).get("repository") == GITHUB_REPO, "Unrecognized existing publication pointer")
        old_version = tuple(map(int, version_from_tag("v" + old["version"]).split(".")))
        new_version = tuple(map(int, pointer["version"].split(".")))
        require(old_version < new_version, "Refusing pointer downgrade or conflicting same-version publication")
    payload = {"branch": "main", "content": base64.b64encode((json.dumps(pointer, ensure_ascii=False, indent=2) + "\n").encode()).decode(),
               "message": "Publish verified desktop release " + pointer["github"]["tag"]}
    if previous:
        payload["sha"] = previous["sha"]
    client.api("gitcode", "PUT" if previous else "POST", path, payload)
    require(pointer_content(client.api("gitcode", "GET", path + "?ref=main")) == pointer, "Published pointer readback differs; do not overwrite")
    return "published"


def synchronize(client, tag, release_id, directory, node_command, execute=False):
    version = version_from_tag(tag)
    if release_id is None:
        release_id = client.api("github", "GET", "/releases/tags/" + tag)["id"]
    require(type(release_id) is int and release_id > 0, "Expected positive GitHub release ID")
    release = client.api("github", "GET", f"/releases/{release_id}")
    frozen = freeze_release(release, tag, release_id)
    directory.mkdir(parents=True, exist_ok=True)
    for name, asset in frozen.items():
        client.download(asset["url"], directory / name, asset["size"], asset["sha256"], "github")
    manifest = verify_payloads(directory, version, frozen, node_command)
    require(freeze_release(client.api("github", "GET", f"/releases/{release_id}"), tag, release_id) == frozen, "GitHub release assets changed during verification")
    print(json.dumps({"stage": "github_verified", "release_id": release_id, "tag": tag, "assets": 7}), flush=True)
    if not execute:
        return {"version": version, "release_id": release_id, "verified_assets": 7, "published": False}
    remote_path = "/releases/tags/" + tag
    remote = client.api("gitcode", "GET", remote_path, missing_ok=True)
    marker = f"GitHub source: https://github.com/{GITHUB_REPO}/releases/tag/{tag}\nGitHub release ID: {release_id}"
    if remote is None:
        remote = client.api("gitcode", "POST", "/releases", {"tag_name": tag, "name": tag,
            "body": marker, "target_commitish": "main", "release_status": "pre"})
    require(marker in remote.get("body", ""), "GitCode release belongs to a different source identity")
    verified = directory / "gitcode-verified"
    verified.mkdir(exist_ok=True)
    verified_urls = {}
    for name in asset_names(version):
        remote = client.api("gitcode", "GET", remote_path)
        existing = gitcode_assets(remote, tag)
        if name not in existing:
            require(remote["release_status"] == "pre", "Published GitCode release is incomplete; refuse mutation")
            ticket = client.api("gitcode", "GET", f"/releases/{tag}/upload_url?" + urllib.parse.urlencode({"file_name": name}))
            client.upload(ticket, directory / name)
            # Successful PUT is not proof that the callback attached the asset.
            remote = wait_for_attachment(client, remote_path, tag, name)
            existing = gitcode_assets(remote, tag)
        asset = frozen[name]
        # Always read remote bytes, even when this run directory was reused.
        destination = verified / name
        require(not destination.exists(), "Use a new work directory to reverify remote assets")
        client.download(existing[name], destination, asset["size"], asset["sha256"], "gitcode")
        verified_urls[name] = existing[name]
        print(json.dumps({"stage": "gitcode_asset_verified", "name": name}), flush=True)
    verify_payloads(verified, version, frozen, node_command)
    require(freeze_release(client.api("github", "GET", f"/releases/{release_id}"), tag, release_id) == frozen, "GitHub source changed before publication")
    urls = gitcode_assets(client.api("gitcode", "GET", remote_path), tag)
    require(urls == verified_urls, "GitCode asset URLs changed after verification")
    pointer = {"schema_version": 1, "github": {"repository": GITHUB_REPO, "release_id": release_id, "tag": tag},
        "version": version, "manifest": manifest,
        "manifest_text": (directory / "latest.json").read_bytes().decode("utf-8"),
        "gitcode_url": urls[asset_names(version)[0]],
        "assets": [{"name": name, "size": frozen[name]["size"], "sha256": frozen[name]["sha256"], "url": urls[name]} for name in asset_names(version)]}
    validate_release_pointer(pointer, public_key())
    # Only this CAS write can move the domestic feed. A later status failure
    # leaves a complete verified feed and is safely resumable on a new run.
    state = publish_pointer(client, pointer)
    if remote.get("release_status") != "latest":
        client.api("gitcode", "PATCH", "/releases/" + tag, {"name": tag, "body": marker, "release_status": "latest"})
    return {"version": version, "release_id": release_id, "verified_assets": 7, "published": True, "pointer": state}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--release-id", type=int)
    parser.add_argument("--work-dir", type=Path, required=True)
    parser.add_argument("--node-command-json", default='["node"]', help="Explicit Node command prefix; use the managed guard on managed Windows hosts")
    parser.add_argument("--execute", action="store_true")
    args = parser.parse_args()
    directory = args.work_dir.resolve()
    require(not directory.is_relative_to(ROOT), "Artifact work directory must be outside the repository")
    node_command = json.loads(args.node_command_json)
    require(isinstance(node_command, list) and node_command and all(isinstance(x, str) and x for x in node_command), "Invalid Node command prefix")
    token = os.environ.get("GITCODE_RELEASE_TOKEN", "")
    require(not args.execute or token, "GITCODE_RELEASE_TOKEN is required to mirror/publish")
    require(not directory.exists(), "Each execution requires a new artifact work directory; prior evidence is preserved")
    directory.mkdir(parents=True, exist_ok=False)
    result = synchronize(Transport(os.environ.get("GITHUB_TOKEN", ""), token), args.tag, args.release_id, directory, node_command, args.execute)
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    try:
        main()
    except SyncError as error:
        print("Release sync failed: " + str(error), file=sys.stderr)
        raise SystemExit(1) from None
    except (ValueError, TypeError, KeyError, OSError, subprocess.SubprocessError):
        # URLs can include signed OBS queries; never print arbitrary exceptions.
        print("Release sync failed; inspect the last completed stage and remote state before a new run.", file=sys.stderr)
        raise SystemExit(1) from None
