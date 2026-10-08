"""Prepare an offline, static Nginx update-metadata deployment bundle.

The release pipeline must verify installer hashes and signatures before calling
this tool. This tool checks structure/key identity only and never fetches URLs,
verifies installer signatures, uploads files, or reloads a server.
"""
from __future__ import annotations

import argparse
from collections import Counter
from copy import deepcopy
from datetime import date, datetime
import hashlib
import ipaddress
import json
from pathlib import Path, PurePosixPath
import re

if __package__:
    from .generate_desktop_update_manifest import validate_signature, validate_version
else:
    from generate_desktop_update_manifest import validate_signature, validate_version


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
PLATFORM = "windows-x86_64"
PUBLIC_PATH = "/updates/server-manager/latest.json"
GITHUB_RELEASES = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download"
GITCODE_RELEASES = "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases"
MAX_APNIC_AGE_DAYS = 31
Network = ipaddress.IPv4Network | ipaddress.IPv6Network


def gitcode_artifact_url(version: str, name: str) -> str:
    """Documented attachment API path; existence/anonymous access is checked upstream."""
    return f"{GITCODE_RELEASES}/v{version}/attach_files/{name}/download"


def gitcode_artifact_urls(version: str, name: str) -> tuple[str, str]:
    return (gitcode_artifact_url(version, name),
            "https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases"
            f"/releases/download/v{version}/{name}")


def bounded_read(path: Path, limit: int) -> bytes:
    with path.open("rb") as source:
        value = source.read(limit + 1)
    if len(value) > limit:
        raise ValueError(f"{path.name}: input exceeds the size limit")
    return value


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result: dict = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("JSON must not contain duplicate object keys")
        result[key] = value
    return result


def read_json(raw: bytes) -> dict:
    value = json.loads(raw.decode("utf-8-sig"), object_pairs_hook=unique_object)
    if not isinstance(value, dict):
        raise ValueError("JSON input must be an object")
    return value


def regional_manifest(manifest: dict, gitcode_url: str, public_key: str) -> dict:
    version = manifest.get("version")
    if not isinstance(version, str):
        raise ValueError("manifest version must be a string")
    validate_version(version)
    platforms = manifest.get("platforms")
    if not isinstance(platforms, dict) or set(platforms) != {PLATFORM}:
        raise ValueError("manifest must contain exactly the supported windows-x86_64 platform")
    platform = platforms[PLATFORM]
    if not isinstance(platform, dict) or set(platform) != {"url", "signature"}:
        raise ValueError("platform must contain only url and signature")
    signature = platform["signature"]
    if not isinstance(signature, str) or validate_signature(signature, public_key) != signature:
        raise ValueError("manifest signature must be a canonical Tauri signature")
    name = f"LanGame.Server.Manager_{version}_x64-setup.exe"
    suffix = f"/v{version}/{name}"
    # Exact public release paths exclude credentials, arbitrary origins, ports,
    # queries, redirects supplied by an operator, traversal, and encoded segments.
    if platform["url"] != GITHUB_RELEASES + suffix:
        raise ValueError("global URL must be the official fixed-version GitHub small installer")
    if gitcode_url not in gitcode_artifact_urls(version, name):
        raise ValueError("GitCode URL must match the official repository, version and installer filename")
    result = deepcopy(manifest)
    result["platforms"][PLATFORM]["url"] = gitcode_url
    return result


def china_networks(raw: bytes, *, today: date | None = None) -> tuple[list[Network], dict]:
    """Read the complete APNIC delegated format, including its record counts.

    Country denotes initial allocation, not proven current geolocation. IPv4
    values are address counts (possibly non-CIDR); IPv6 values are prefix lengths.
    """
    today = today or date.today()
    lines = [line.strip() for line in raw.decode("utf-8").splitlines()
             if line.strip() and not line.lstrip().startswith("#")]
    if not lines:
        raise ValueError("APNIC input is empty")
    header = [part.strip() for part in lines[0].split("|")]
    if len(header) != 7 or header[:2] != ["2", "apnic"] or not header[2].isdigit():
        raise ValueError("APNIC input requires the standard version-2 apnic header")
    expected_count = int(header[3])
    try:
        end_date = datetime.strptime(header[5], "%Y%m%d").date()
    except ValueError as error:
        raise ValueError("APNIC header has an invalid end date") from error
    if not 0 <= (today - end_date).days <= MAX_APNIC_AGE_DAYS:
        raise ValueError("APNIC input is future-dated or older than 31 days")
    summaries: dict[str, int] = {}
    counts: Counter[str] = Counter()
    networks: dict[int, list[Network]] = {4: [], 6: []}
    for line in lines[1:]:
        fields = [part.strip() for part in line.split("|")]
        if len(fields) == 6 and fields[1] == "*" and fields[-1] == "summary":
            registry, _, kind, start, count, _ = fields
            if (registry != "apnic" or kind not in {"asn", "ipv4", "ipv6"}
                    or start != "*" or kind in summaries):
                raise ValueError("APNIC summary is malformed or duplicated")
            summaries[kind] = int(count)
            continue
        if len(fields) < 7 or fields[0] != "apnic" or fields[2] not in {"asn", "ipv4", "ipv6"}:
            raise ValueError("APNIC input contains an invalid resource record")
        _, country, kind, start, value, _, status, *_ = fields
        counts[kind] += 1
        if country != "CN" or kind == "asn" or status not in {"allocated", "assigned"}:
            continue
        if kind == "ipv4":
            first = ipaddress.IPv4Address(start)
            count = int(value)
            if count <= 0:
                raise ValueError("APNIC IPv4 address count must be positive")
            last = ipaddress.IPv4Address(int(first) + count - 1)
            selected = list(ipaddress.summarize_address_range(first, last))
        else:
            selected = [ipaddress.IPv6Network(f"{start}/{value}", strict=True)]
        if any(not block.network_address.is_global or not block.broadcast_address.is_global
               for block in selected):
            raise ValueError("APNIC CN allocations must contain only globally routable addresses")
        networks[4 if kind == "ipv4" else 6].extend(selected)
    if sum(counts.values()) != expected_count or dict(counts) != summaries:
        raise ValueError("APNIC record/summary count mismatch; input may be truncated")
    if not networks[4] or not networks[6]:
        raise ValueError("APNIC input must contain both CN IPv4 and IPv6 allocations")
    collapsed = [*ipaddress.collapse_addresses(networks[4]),
                 *ipaddress.collapse_addresses(networks[6])]
    return collapsed, {
        "serial": header[2], "end_date": end_date.isoformat(),
        "records": expected_count,
        "ipv4_cidrs": sum(block.version == 4 for block in collapsed),
        "ipv6_cidrs": sum(block.version == 6 for block in collapsed),
        "country_semantics": "initial-allocation-country-not-current-geolocation",
    }


def nginx_configs(deploy_root: str, manifest_root: str | None = None) -> tuple[str, str]:
    manifest_root = manifest_root or deploy_root
    for directory in (deploy_root, manifest_root):
        if (not re.fullmatch(r"/(?:[A-Za-z0-9_.-]+/)*[A-Za-z0-9_.-]+", directory)
                or any(part in {".", ".."} for part in directory.split("/"))
                or str(PurePosixPath(directory)) != directory):
            raise ValueError("deployment paths must be canonical absolute Linux directories without interpolation")
    http = f"""# Include once in http {{}}. See deploy/desktop-updates/README.md.
# $remote_addr must already be the authenticated connection's client address.
geo $remote_addr $lgsm_update_cn {{
    default 0;
    include {deploy_root}/china-cidrs.conf;
}}
map $lgsm_update_cn $lgsm_update_manifest {{
    default {manifest_root}/latest-global.json;
    1 {manifest_root}/latest-cn.json;
}}
"""
    location = f"""# Include in the existing langame.cn HTTPS server {{}} only.
location = {PUBLIC_PATH} {{
    alias $lgsm_update_manifest;
    types {{ }}
    default_type application/json;
    add_header Cache-Control "private, no-store" always;
    expires off;
    etag off;
    if_modified_since off;
    open_file_cache off;
    max_ranges 0;
    limit_except GET {{ deny all; }}
}}
"""
    return http, location


def json_bytes(value: dict) -> bytes:
    # The saved pointer canonicalizes object keys; regenerating a regional
    # manifest from that pointer must preserve its original generated bytes.
    return (json.dumps(value, sort_keys=True, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")


def prepare_bundle(manifest_path: Path, apnic_path: Path, gitcode_url: str,
                   config_path: Path, output: Path, deploy_root: str,
                   manifest_root: str | None = None) -> dict:
    output = output.resolve()
    if output.is_relative_to(REPOSITORY_ROOT):
        raise ValueError("output must be outside the source repository")
    manifest_raw = bounded_read(manifest_path, 256 * 1024)
    apnic_raw = bounded_read(apnic_path, 16 * 1024 * 1024)
    manifest = read_json(manifest_raw)
    config = read_json(bounded_read(config_path, 256 * 1024))
    cn = regional_manifest(manifest, gitcode_url, config["plugins"]["updater"]["pubkey"])
    networks, apnic_info = china_networks(apnic_raw)
    http, location = nginx_configs(deploy_root, manifest_root)
    payloads = {
        "latest-global.json": manifest_raw,  # Preserve the published source bytes.
        "latest-cn.json": json_bytes(cn),
        "china-cidrs.conf": ("# APNIC CN allocated/assigned networks; generated, do not edit.\n"
                             + "".join(f"{block} 1;\n" for block in networks)).encode("ascii"),
        "http.conf": http.encode("ascii"),
        "server-location.conf": location.encode("ascii"),
    }
    receipt = {
        "version": manifest["version"], "endpoint": "https://langame.cn" + PUBLIC_PATH,
        "deploy_root": deploy_root, "manifest_root": manifest_root or deploy_root, "apnic": apnic_info,
        "input_sha256": {"manifest": hashlib.sha256(manifest_raw).hexdigest(),
                         "apnic": hashlib.sha256(apnic_raw).hexdigest()},
        "signature_validation": "format-and-key-id-only; cryptographic-verification-required-upstream",
        "remote_asset_validation": "not-performed; same-signed-installer-verification-required-upstream",
        "files": {name: {"bytes": len(value), "sha256": hashlib.sha256(value).hexdigest()}
                  for name, value in payloads.items()},
    }
    payloads["deployment.json"] = json_bytes(receipt)
    output.mkdir(parents=True, exist_ok=False)
    created: list[Path] = []
    try:
        for name, value in payloads.items():
            destination = output / name
            with destination.open("xb") as target:
                created.append(destination)
                target.write(value)
    except BaseException:
        for destination in reversed(created):
            destination.unlink(missing_ok=True)
        output.rmdir()
        raise
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--verified-manifest", type=Path, required=True,
                        help="published GitHub manifest whose installer the release pipeline has verified")
    parser.add_argument("--apnic-file", type=Path, required=True)
    parser.add_argument("--gitcode-url", required=True,
                        help="explicit observed permanent URL of the same verified fixed-version installer")
    parser.add_argument("--tauri-config", type=Path,
                        default=REPOSITORY_ROOT / "apps/desktop/src-tauri/tauri.conf.json")
    parser.add_argument("--deploy-root", required=True, help="versioned directory outside the server webroot")
    parser.add_argument("--manifest-root", help="separate manifest directory; use the refresher's current symlink")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory outside the repository")
    args = parser.parse_args()
    try:
        receipt = prepare_bundle(args.verified_manifest, args.apnic_file, args.gitcode_url,
                                 args.tauri_config, args.output_dir, args.deploy_root, args.manifest_root)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"update service preparation failed: {error}\n")
    print(json.dumps({"version": receipt["version"], "files": len(receipt["files"]),
                      "published": False}))


if __name__ == "__main__":
    main()
