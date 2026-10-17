"""Track Steam's public dedicated-server build against the signed published feed.

Detection creates one issue per build. It never edits profiles, releases or trust
roots and never treats an unseen binary as compatible.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
REPO = "SZSLGJCOM/LanGame-Server-Manager"
APP_ID = "2278520"
FEED = f"https://github.com/{REPO}/releases/download/enshrouded-control/manifest.json"
MAX_RESPONSE = 4 * 1024 * 1024


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def tokenize(text: str):
    index = 0
    while index < len(text):
        if text[index].isspace():
            index += 1
        elif text.startswith("//", index):
            end = text.find("\n", index)
            index = len(text) if end < 0 else end + 1
        elif text[index] in "{}":
            yield text[index]
            index += 1
        elif text[index] == '"':
            index += 1
            value = []
            while index < len(text) and text[index] != '"':
                if text[index] == "\\":
                    index += 1
                    require(index < len(text), "Truncated Steam VDF escape")
                value.append(text[index])
                index += 1
            require(index < len(text), "Unterminated Steam VDF string")
            index += 1
            yield "".join(value)
        else:
            end = index
            while end < len(text) and not text[end].isspace() and text[end] not in '{}"':
                end += 1
            require(end > index, "Invalid Steam VDF token")
            yield text[index:end]
            index = end


def steam_public_build(output: str) -> dict:
    require(len(output.encode("utf-8")) <= MAX_RESPONSE, "Steam metadata exceeds read budget")
    # SteamCMD prints login/progress lines before the app's VDF root.
    marker = re.search(r'(?m)^\s*"2278520"\s*\n?\s*\{', output)
    require(marker is not None, "SteamCMD did not return app 2278520 metadata")
    tokens = iter(tokenize(output[marker.start():]))
    require(next(tokens, None) == APP_ID and next(tokens, None) == "{", "Invalid Steam app root")

    def object_value(depth: int = 0) -> dict:
        require(depth < 32, "Steam VDF nesting exceeds budget")
        result = {}
        while True:
            key = next(tokens, None)
            require(key is not None, "Truncated Steam app metadata")
            if key == "}":
                return result
            require(key != "{" and key not in result, "Duplicate or invalid Steam VDF key")
            value = next(tokens, None)
            require(value is not None and value != "}", "Missing Steam VDF value")
            result[key] = object_value(depth + 1) if value == "{" else value

    app = object_value()
    public = app.get("depots", {}).get("branches", {}).get("public", {})
    build = public.get("buildid")
    require(isinstance(build, str) and re.fullmatch(r"[1-9]\d{0,19}", build), "Steam public build ID is missing")
    return {"steam_app_id": APP_ID, "branch": "public", "game_build_id": build,
            "timeupdated": public.get("timeupdated", "")}


def request_bytes(url: str, *, limit: int = MAX_RESPONSE, method: str = "GET", data=None, token: str = "") -> bytes:
    headers = {"User-Agent": "LanGame-Enshrouded-Compatibility-Tracker", "Accept": "application/vnd.github+json"}
    if token:
        require(url.startswith(f"https://api.github.com/repos/{REPO}/"), "Token is only sent to the fixed repository API")
        headers["Authorization"] = f"Bearer {token}"
        headers["X-GitHub-Api-Version"] = "2022-11-28"
    encoded = json.dumps(data).encode() if data is not None else None
    if encoded is not None:
        headers["Content-Type"] = "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(url, data=encoded, headers=headers, method=method), timeout=45) as response:
            length = response.headers.get("Content-Length")
            require(length is None or int(length) <= limit, "Response exceeds read budget")
            result = response.read(limit + 1)
            require(len(result) <= limit, "Response exceeds read budget")
            return result
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"GitHub request failed with HTTP {error.code}") from None
    except urllib.error.URLError:
        raise RuntimeError("GitHub request failed; check network connectivity") from None


def published_manifest(node: str) -> dict:
    with tempfile.TemporaryDirectory(prefix="langame-enshrouded-feed-") as temporary:
        directory = Path(temporary)
        manifest = directory / "manifest.json"
        signature = directory / "manifest.json.minisig"
        manifest.write_bytes(request_bytes(FEED, limit=65536))
        signature.write_bytes(request_bytes(FEED + ".minisig", limit=8192))
        result = subprocess.run([node, str(ROOT / "scripts/enshrouded_control_package.cjs"), "verify-manifest",
                                 "--manifest", str(manifest), "--signature", str(signature)],
                                capture_output=True, text=True, timeout=30, check=False)
        require(result.returncode == 0, "Published extension manifest signature/contract is invalid")
        return json.loads(result.stdout)


def ensure_issue(build: dict, manifest: dict, token: str) -> dict:
    require(bool(token), "GITHUB_TOKEN is required to create compatibility issues")
    marker = f"<!-- langame-enshrouded-build:{build['game_build_id']} -->"
    base = f"https://api.github.com/repos/{REPO}"
    for page in range(1, 101):
        issues = json.loads(request_bytes(f"{base}/issues?state=all&per_page=100&page={page}", token=token))
        require(isinstance(issues, list), "Invalid issue-list response")
        for issue in issues:
            if not issue.get("pull_request") and marker in (issue.get("body") or ""):
                return {"created": False, "url": issue["html_url"]}
        if len(issues) < 100:
            break
    else:
        raise ValueError("Issue deduplication exceeded its page budget; refusing a possible duplicate")
    body = (f"{marker}\nSteamCMD reports **public build {build['game_build_id']}** for Enshrouded Dedicated Server "
            f"(app {APP_ID}). The verified published extension manifest revision {manifest['revision']} "
            f"(version {manifest['version']}) does not include this build.\n\n"
            "Automatic detection has not enabled this binary. Obtain the official server in isolation, verify its "
            "SHA256 and native ABI/entry points, run the native tests, and verify real kick, ban and unban before "
            "publishing a new signed independent extension. Keep the previous verified profiles in the release.\n\n"
            f"Signed feed: {FEED}\nSteam metadata timeupdated: {build['timeupdated'] or 'not supplied'}\n")
    issue = json.loads(request_bytes(f"{base}/issues", method="POST", token=token,
                                    data={"title": f"Enshrouded compatibility: Steam public build {build['game_build_id']}", "body": body}))
    return {"created": True, "url": issue["html_url"]}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--steamcmd", type=Path)
    source.add_argument("--steam-log", type=Path, help="Parse already captured metadata without contacting Steam")
    parser.add_argument("--manifest-directory", type=Path, help="Verify a staged feed instead of downloading the published one")
    parser.add_argument("--node", default="node")
    parser.add_argument("--report", type=Path)
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--issue", action="store_true")
    args = parser.parse_args()
    if args.steam_log:
        require(args.steam_log.stat().st_size <= MAX_RESPONSE, "Steam metadata exceeds read budget")
        output = args.steam_log.read_text(encoding="utf-8-sig")
    else:
        result = subprocess.run([str(args.steamcmd.resolve()), "+login", "anonymous", "+app_info_update", "1",
                                 "+app_info_print", APP_ID, "+quit"], capture_output=True,
                                encoding="utf-8", errors="replace", timeout=240, check=False)
        require(result.returncode == 0, "SteamCMD metadata refresh failed")
        output = result.stdout
    build = steam_public_build(output)
    if args.manifest_directory:
        staged = args.manifest_directory.resolve()
        result = subprocess.run([args.node, str(ROOT / "scripts/enshrouded_control_package.cjs"), "verify-manifest",
                                 "--manifest", str(staged / "manifest.json"), "--signature", str(staged / "manifest.json.minisig")],
                                capture_output=True, text=True, timeout=30, check=False)
        require(result.returncode == 0, "Staged manifest verification failed")
        manifest = json.loads(result.stdout)
    else:
        manifest = published_manifest(args.node)
    supported = any(profile["game_build_id"] == build["game_build_id"] for profile in manifest["profiles"])
    report = {**build, "supported": supported, "extension_version": manifest["version"], "extension_revision": manifest["revision"]}
    if not supported and args.issue:
        report["issue"] = ensure_issue(build, manifest, os.environ.get("GITHUB_TOKEN", ""))
    if args.report:
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as destination:
            destination.write(f"build_id={build['game_build_id']}\nsupported={str(supported).lower()}\n")
    print(json.dumps(report))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError) as error:
        raise SystemExit(f"Enshrouded compatibility tracking failed: {error}") from None
