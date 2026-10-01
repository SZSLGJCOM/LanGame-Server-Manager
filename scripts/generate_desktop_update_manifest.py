"""Prepare a static Tauri update manifest and optional local release assets.

This tool never uploads files. Signature validation checks encoding and key identity;
Tauri must still verify the signature cryptographically before installing an update.
"""
from __future__ import annotations

import argparse
import base64
import binascii
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path
import re
import shutil
from urllib.parse import unquote, urlsplit


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
STABLE_VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\Z")


def validate_version(version: str) -> str:
    if not STABLE_VERSION.fullmatch(version):
        raise ValueError("version must be a stable major.minor.patch version without a v prefix")
    return version


def decode_base64(value: str, label: str) -> bytes:
    try:
        return base64.b64decode(value, validate=True)
    except (ValueError, binascii.Error) as error:
        raise ValueError(f"{label} must be valid base64") from error


def validate_signature(signature: str, public_key: str) -> str:
    """Reject malformed/wrong-key inputs, without claiming cryptographic verification."""
    signature = signature.lstrip("\ufeff").strip()
    if not signature or len(signature) > 16384:
        raise ValueError("signature must contain a bounded Tauri .sig value")
    try:
        lines = decode_base64(signature, "signature").decode("utf-8").splitlines()
        key_lines = decode_base64(public_key.strip(), "updater public key").decode("utf-8").splitlines()
    except UnicodeDecodeError as error:
        raise ValueError("signature and public key must contain UTF-8 minisign records") from error
    if (len(lines) != 4 or not lines[0].startswith("untrusted comment: ")
            or not lines[2].startswith("trusted comment: ")):
        raise ValueError("signature must contain the four-line Tauri minisign record")
    if len(key_lines) != 2 or not key_lines[0].startswith("untrusted comment: "):
        raise ValueError("updater public key must contain a minisign public-key record")
    packet = decode_base64(lines[1], "signature packet")
    global_signature = decode_base64(lines[3], "signature comment packet")
    key_packet = decode_base64(key_lines[1], "updater public-key packet")
    if len(packet) != 74 or packet[:2] not in (b"Ed", b"ED") or len(global_signature) != 64:
        raise ValueError("signature has an invalid minisign packet")
    if len(key_packet) != 42 or key_packet[:2] != b"Ed":
        raise ValueError("updater public key has an invalid minisign packet")
    if packet[2:10] != key_packet[2:10]:
        raise ValueError("signature key ID does not match the configured updater public key")
    return signature


def public_artifact_name(source_name: str) -> str:
    name = source_name.replace(" ", ".")
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", name) or name.endswith("."):
        raise ValueError("public artifact names must use ASCII letters, digits, dots, underscores or hyphens")
    return name


def validate_artifact_url(url: str, expected_name: str) -> str:
    if expected_name != public_artifact_name(expected_name):
        raise ValueError("artifact URL must use the public artifact name")
    parsed = urlsplit(url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.username is not None
            or parsed.password is not None or parsed.query or parsed.fragment
            or any(character.isspace() for character in url) or "\\" in url):
        raise ValueError("artifact URL must be an absolute HTTPS URL without credentials, query or fragment")
    if any(part in (".", "..") for part in unquote(parsed.path).split("/")):
        raise ValueError("artifact URL cannot contain traversal segments")
    if unquote(parsed.path.rsplit("/", 1)[-1]) != expected_name:
        raise ValueError("artifact URL filename must match the current version's x64 NSIS installer")
    return url


def validate_pub_date(value: str) -> str:
    if not value:
        return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as error:
        raise ValueError("pub-date must be an RFC 3339 timestamp with timezone") from error
    if "T" not in value or parsed.tzinfo is None:
        raise ValueError("pub-date must be an RFC 3339 timestamp with timezone")
    return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_outputs(output: Path, manifest: dict, artifact: Path | None, signature: Path) -> None:
    output = output.resolve()
    if output.is_relative_to(REPOSITORY_ROOT):
        raise ValueError("output must be outside the source repository")
    payloads = {}
    if artifact is not None:
        artifact_name = public_artifact_name(artifact.name)
        signature_name = artifact_name + ".sig"
        validate_artifact_url(manifest["platforms"]["windows-x86_64"]["url"], artifact_name)
        if signature.name != artifact.name + ".sig":
            raise ValueError("signature filename must match the input installer")
        payloads = {artifact_name: artifact, signature_name: signature}
    names = [output.name, *payloads]
    if artifact is not None:
        names += ["SHA256SUMS", "desktop-update-artifacts.json"]
    if len(set(names)) != len(names):
        raise ValueError("output names must be distinct")
    for name in names:
        if (output.parent / name).exists():
            raise ValueError(f"refusing to overwrite existing release output: {name}")
    source_hashes = {name: sha256_file(source) for name, source in payloads.items()}
    output.parent.mkdir(parents=True, exist_ok=True)
    created: list[Path] = []
    try:
        for name, source in payloads.items():
            destination = output.parent / name
            with destination.open("xb") as target:
                created.append(destination)
                with source.open("rb") as stream:
                    shutil.copyfileobj(stream, target, length=1024 * 1024)
            if sha256_file(destination) != source_hashes[name]:
                raise ValueError(f"release input changed while copying: {name}")
        if artifact is not None:
            copied_signature = (output.parent / signature_name).read_text(encoding="utf-8-sig").strip()
            if copied_signature != manifest["platforms"]["windows-x86_64"]["signature"]:
                raise ValueError("signature changed after validation; exported manifest would not match")
            if (output.parent / artifact_name).stat().st_size == 0:
                raise ValueError("exported installer must not be empty")
        with output.open("x", encoding="utf-8", newline="\n") as target:
            created.append(output)
            json.dump(manifest, target, ensure_ascii=False, indent=2)
            target.write("\n")
        if artifact is not None:
            assets = [
                {"name": path.name, "bytes": path.stat().st_size, "sha256": sha256_file(path)}
                for path in created
            ]
            checksum = output.parent / "SHA256SUMS"
            with checksum.open("x", encoding="utf-8", newline="\n") as target:
                created.append(checksum)
                target.writelines(f"{item['sha256']}  {item['name']}\n" for item in assets)
            summary = output.parent / "desktop-update-artifacts.json"
            with summary.open("x", encoding="utf-8", newline="\n") as target:
                created.append(summary)
                json.dump({
                    "version": manifest["version"], "artifact_count": len(assets),
                    "artifacts": assets, "published": False,
                    "signature_validation": "format-and-key-id-only",
                    "artifact_build_configuration": "not-inspected",
                }, target, ensure_ascii=False, indent=2)
                target.write("\n")
    except BaseException:
        for path in reversed(created):
            path.unlink(missing_ok=True)
        raise


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--artifact-url", required=True)
    parser.add_argument("--signature-file", type=Path, required=True)
    parser.add_argument("--artifact-file", type=Path)
    parser.add_argument("--tauri-config", type=Path,
                        default=REPOSITORY_ROOT / "apps/desktop/src-tauri/tauri.conf.json")
    notes = parser.add_mutually_exclusive_group()
    notes.add_argument("--notes", default="")
    notes.add_argument("--notes-file", type=Path)
    parser.add_argument("--pub-date", default="")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        version = validate_version(args.version)
        config = json.loads(args.tauri_config.read_text(encoding="utf-8-sig"))
        if config["version"] != version:
            raise ValueError("manifest version must match the Tauri configuration")
        name = f"{config['productName']}_{version}_x64-setup.exe"
        url = validate_artifact_url(args.artifact_url, public_artifact_name(name))
        if args.signature_file.stat().st_size > 16384:
            raise ValueError("signature file is too large")
        signature = validate_signature(args.signature_file.read_text(encoding="utf-8-sig"),
                                       config["plugins"]["updater"]["pubkey"])
        if args.artifact_file is not None:
            if args.artifact_file.name != name or args.signature_file.name != name + ".sig":
                raise ValueError("artifact and signature filenames must match the current version")
            if args.artifact_file.stat().st_size == 0:
                raise ValueError("installer must not be empty")
        release_notes = (args.notes_file.read_text(encoding="utf-8-sig")
                         if args.notes_file is not None else args.notes)
        manifest = {
            "version": version, "notes": release_notes, "pub_date": validate_pub_date(args.pub_date),
            "platforms": {"windows-x86_64": {"signature": signature, "url": url}},
        }
        write_outputs(args.output, manifest, args.artifact_file, args.signature_file)
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
