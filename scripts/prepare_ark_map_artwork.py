"""Prepare a local, unpublished ARK map-artwork package without network access.

Checks JPEG marker structure and SOF dimensions, not compressed-pixel decoding.
The selected original images must also have been decoded and visually reviewed.
No credentials, uploads, commits, or remote URL checks are performed.
"""
from __future__ import annotations

import argparse
from datetime import date
import hashlib
import json
import math
from pathlib import Path
import re
import stat
from urllib.parse import urlsplit


REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CATALOG = REPOSITORY_ROOT / "apps/desktop/src/data/ark-map-artwork.json"
MAX_IMAGE_BYTES = 16 * 1024 * 1024
MAX_CATALOG_BYTES = 256 * 1024
SOF_MARKERS = frozenset((0xC0, 0xC1, 0xC2, 0xC3, 0xC5, 0xC6, 0xC7,
                         0xC9, 0xCA, 0xCB, 0xCD, 0xCE, 0xCF))
EDITIONS = frozenset(("arksurvivalevolved", "arksurvivalascended"))
GITHUB_RAW = "https://raw.githubusercontent.com/SZSLGJCOM/LanGame-Server-Manager/master/"
GITCODE_RAW = "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/raw/"


def plain_path(path: Path) -> Path:
    """Reject symlinks/junctions, including ancestors and missing-target links."""
    absolute = path.absolute()
    if ".." in absolute.parts:
        raise ValueError("paths must not contain parent traversal")
    for component in reversed((absolute, *absolute.parents)):
        try:
            info = component.lstat()
        except FileNotFoundError:
            continue
        if (stat.S_ISLNK(info.st_mode)
                or getattr(info, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT):
            raise ValueError(f"symlinks and reparse points are not allowed: {component}")
    return absolute.resolve()


def bounded_read(path: Path, limit: int) -> bytes:
    path = plain_path(path)
    info = path.stat()
    if not stat.S_ISREG(info.st_mode):
        raise ValueError(f"input must be a regular file: {path.name}")
    if info.st_size > limit:
        raise ValueError(f"{path.name}: input exceeds {'16 MiB' if limit == MAX_IMAGE_BYTES else 'catalog'} limit")
    with path.open("rb") as stream:
        value = stream.read(limit + 1)
    if len(value) > limit:
        raise ValueError(f"{path.name}: input grew beyond its size limit")
    return value


def jpeg_dimensions(raw: bytes) -> tuple[int, int]:
    """Read JPEG framing without claiming to decode entropy-coded pixels."""
    if not raw.startswith(b"\xff\xd8"):
        raise ValueError("image is not a JPEG (missing SOI)")
    offset = 2
    dimensions = None
    saw_scan = False
    while offset < len(raw):
        if raw[offset] != 0xFF:
            raise ValueError("invalid JPEG marker boundary")
        while offset < len(raw) and raw[offset] == 0xFF:
            offset += 1
        if offset == len(raw):
            break
        marker = raw[offset]
        offset += 1
        if marker == 0xD9:
            if dimensions is None or not saw_scan or offset != len(raw):
                raise ValueError("incomplete JPEG or trailing bytes after EOI")
            return dimensions
        if marker == 0x01:  # Standalone TEM marker.
            continue
        if marker in (0x00, 0xD8) or 0xD0 <= marker <= 0xD7:
            raise ValueError("unexpected standalone JPEG marker")
        if offset + 2 > len(raw):
            break
        length = int.from_bytes(raw[offset:offset + 2], "big")
        if length < 2 or offset + length > len(raw):
            raise ValueError("truncated or invalid JPEG segment")
        segment = raw[offset + 2:offset + length]
        offset += length
        if marker in SOF_MARKERS:
            if dimensions is not None or len(segment) < 6:
                raise ValueError("invalid or repeated JPEG SOF")
            height = int.from_bytes(segment[1:3], "big")
            width = int.from_bytes(segment[3:5], "big")
            components = segment[5]
            if (not width or not height or not 1 <= segment[0] <= 16
                    or not 1 <= components <= 4 or len(segment) != 6 + 3 * components):
                raise ValueError("invalid JPEG SOF dimensions/components")
            dimensions = (width, height)
        elif marker == 0xDA:
            if (dimensions is None or not segment or not 1 <= segment[0] <= 4
                    or len(segment) != 4 + 2 * segment[0]):
                raise ValueError("invalid JPEG scan header")
            saw_scan = True
            # Skip stuffed FF bytes and restart markers; progressive JPEGs can
            # have more table/scan segments before the final EOI.
            while offset < len(raw):
                boundary = raw.find(b"\xff", offset)
                if boundary < 0:
                    raise ValueError("JPEG scan has no closing marker")
                cursor = boundary + 1
                while cursor < len(raw) and raw[cursor] == 0xFF:
                    cursor += 1
                if cursor == len(raw):
                    raise ValueError("truncated JPEG scan marker")
                if raw[cursor] == 0x00 or 0xD0 <= raw[cursor] <= 0xD7:
                    offset = cursor + 1
                else:
                    offset = boundary
                    break
    raise ValueError("JPEG is missing its end marker")


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def read_catalog(path: Path) -> tuple[str, list[dict], str]:
    raw = bounded_read(path, MAX_CATALOG_BYTES)
    catalog = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object)
    if not isinstance(catalog, dict):
        raise ValueError("catalog must be an object")
    version = catalog.get("verifiedOn")
    if (not isinstance(version, str) or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", version)
            or date.fromisoformat(version).isoformat() != version):
        raise ValueError("catalog verifiedOn must be an ISO date")
    maps = catalog.get("maps")
    if not isinstance(maps, list) or not 1 <= len(maps) <= 100:
        raise ValueError("catalog must contain 1 to 100 map entries")
    seen_images, seen_maps = set(), set()
    for item in maps:
        if not isinstance(item, dict):
            raise ValueError("each catalog map must be an object")
        image = item.get("image")
        if (not isinstance(image, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_-]*\.jpg", image)
                or re.match(r"(?i)^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])\.", image)):
            raise ValueError("image must be a plain, portable JPEG filename")
        edition, map_name = item.get("edition"), item.get("mapName")
        if (not isinstance(edition, str) or edition not in EDITIONS or not isinstance(map_name, str)
                or not re.fullmatch(r"[A-Za-z0-9_]+", map_name)):
            raise ValueError("invalid catalog edition or mapName")
        if image.casefold() in seen_images or (edition, map_name.casefold()) in seen_maps:
            raise ValueError("duplicate catalog image or edition/map identity")
        seen_images.add(image.casefold())
        seen_maps.add((edition, map_name.casefold()))
        if any(type(item.get(key)) is not int or not 1 <= item[key] <= 65535 for key in ("width", "height")):
            raise ValueError("catalog dimensions must be positive JPEG dimensions")
        bounds = item.get("bounds")
        if (not isinstance(bounds, dict) or set(bounds) != {"minX", "maxX", "minY", "maxY"}
                or any(type(value) not in (int, float) or not math.isfinite(value) for value in bounds.values())
                or bounds["minX"] >= bounds["maxX"] or bounds["minY"] >= bounds["maxY"]):
            raise ValueError("invalid catalog coordinate bounds")
        source = item.get("coordinateSource")
        if not isinstance(source, str):
            raise ValueError("coordinateSource must identify the reviewed ARK Wiki revision")
        parsed = urlsplit(source)
        if (parsed.scheme != "https" or parsed.netloc != "ark.wiki.gg"
                or not parsed.path.startswith("/wiki/Data:Maps/") or parsed.fragment
                or not re.fullmatch(r"oldid=[1-9]\d*", parsed.query)):
            raise ValueError("coordinateSource must identify the reviewed ARK Wiki revision")
    return version, maps, hashlib.sha256(raw).hexdigest()


def package_notices(manifest: dict) -> tuple[bytes, bytes]:
    readme = f"""# ARK map artwork publication candidate

This local package is **not published**. Its proposed URLs have not been fetched
or verified. Preparing it does not grant permission to upload or redistribute it.

- Catalog version: {manifest['catalogVerifiedOn']}
- Images: {manifest['imageCount']}; original image bytes: {manifest['imageBytes']}
- Copy destination, if this exact batch is approved: the same relative paths in
  `SZSLGJCOM/LanGame-Server-Manager` (`master`, GitHub) and
  `SZSLGJCOM/LanGame-Server-Manager-Releases` (`main`, GitCode).
- `manifest.json` records exact SHA-256, byte counts, dimensions, GPS bounds,
  original sources, license-source links, and proposed anonymous raw URLs.

Image bytes are unchanged. JPEG marker/SOF checks are not a full pixel decode or
a coordinate-alignment test; the original images require those separate reviews.
The project's source-code license does not cover these third-party images.
Retain NOTICE.md and each original file-page attribution/permission notice.

Before publishing, obtain the user's final confirmation for this exact batch and
review the per-file terms for the intended redistribution. After publishing,
download every mirror anonymously and verify its bytes, SHA-256, format and
dimensions before enabling those URLs. These proposed branch URLs are not
immutable: never replace bytes under this version directory. A changed image
requires a new version and a newly reviewed manifest.
"""
    notice = ["# Third-party ARK map artwork sources", "",
              "These images are not covered by the project's source-code license.",
              "Copyright and other rights remain with their respective rights holders.",
              "ARK Wiki file pages carry differing game-artwork, fair-use or public-domain",
              "notices; this package neither replaces those notices nor grants a new license.",
              "Keep the original source and file-page links with every redistributed image.", "",
              "| Image | Original image | File-page notice / attribution |",
              "| --- | --- | --- |"]
    for item in manifest["images"]:
        notice.append(f"| {item['image']} | [ARK Wiki image]({item['sourceURL']}) | "
                      f"[Original file page]({item['licenseSourceURL']}) |")
    return readme.encode("utf-8"), ("\n".join(notice) + "\n").encode("utf-8")


def write_new(path: Path, payload: bytes, created: list[Path]) -> None:
    plain_path(path)
    with path.open("xb") as stream:
        created.append(path)
        stream.write(payload)


def rollback_created(output: Path, files: list[Path], directories: list[Path]) -> None:
    # No recursive deletion: only this invocation's exact files and empty dirs.
    # Recheck ancestry before removing anything; unexpected contents are retained.
    errors = []
    for path in (*reversed(files), *reversed(directories)):
        try:
            checked = plain_path(path)
            if checked != output and not checked.is_relative_to(output):
                raise ValueError("created path escaped the output directory")
            if path in files:
                path.unlink()
            else:
                path.rmdir()
        except (OSError, ValueError) as error:
            errors.append(error)
    if errors:
        raise OSError(f"incomplete package retained at {output}: {errors[0]}") from errors[0]


def prepare_bundle(images_dir: Path, output_dir: Path, catalog_path: Path = DEFAULT_CATALOG) -> dict:
    images_dir, output = plain_path(images_dir), plain_path(output_dir)
    if not images_dir.is_dir():
        raise ValueError("images directory must exist")
    if output.is_relative_to(REPOSITORY_ROOT.resolve()):
        raise ValueError("output must be outside the source repository")
    if output.is_relative_to(images_dir) or images_dir.is_relative_to(output):
        raise ValueError("output must not overlap the images directory")
    if output.exists():
        raise ValueError("output directory already exists; refusing to overwrite")
    if not output.parent.is_dir():
        raise ValueError("output parent directory must already exist")
    version, maps, catalog_sha = read_catalog(catalog_path)
    records = []
    for item in maps:
        payload = bounded_read(images_dir / item["image"], MAX_IMAGE_BYTES)
        if jpeg_dimensions(payload) != (item["width"], item["height"]):
            raise ValueError(f"{item['image']}: JPEG dimensions differ from the catalog")
        relative = f"assets/ark-map-artwork/{version}/{item['image']}"
        records.append({key: item[key] for key in ("edition", "mapName", "image", "width", "height", "bounds",
                                                   "coordinateSource")} | {
            "relativePath": relative, "size": len(payload), "sha256": hashlib.sha256(payload).hexdigest(),
            "sourceURL": "https://ark.wiki.gg/images/" + item["image"],
            "licenseSourceURL": "https://ark.wiki.gg/wiki/File:" + item["image"],
            "proposedURLs": {"github": GITHUB_RAW + relative, "gitcode": GITCODE_RAW + relative + "?ref=main"},
        })
    manifest = {"schemaVersion": 1, "catalogVerifiedOn": version, "catalogSha256": catalog_sha,
                "published": False, "remoteValidation": "not-performed",
                "imageValidation": "JPEG-marker-structure-and-SOF-dimensions; not-pixel-decoding",
                "imageCount": len(records), "imageBytes": sum(item["size"] for item in records), "images": records}
    readme, notice = package_notices(manifest)
    manifest_bytes = (json.dumps(manifest, ensure_ascii=False, indent=2, allow_nan=False) + "\n").encode("utf-8")
    files, directories = [], []
    output.mkdir(exist_ok=False)
    directories.append(output)
    try:
        folder = output
        for segment in ("assets", "ark-map-artwork", version):
            folder /= segment
            plain_path(folder).mkdir(exist_ok=False)
            directories.append(folder)
        for item in records:
            payload = bounded_read(images_dir / item["image"], MAX_IMAGE_BYTES)
            if len(payload) != item["size"] or hashlib.sha256(payload).hexdigest() != item["sha256"]:
                raise ValueError(f"{item['image']}: source changed after validation")
            destination = output / item["relativePath"]
            write_new(destination, payload, files)
            if hashlib.sha256(bounded_read(destination, MAX_IMAGE_BYTES)).hexdigest() != item["sha256"]:
                raise ValueError(f"{item['image']}: written image failed integrity verification")
        write_new(output / "README.md", readme, files)
        write_new(output / "NOTICE.md", notice, files)
        # Last file is the completion record; interrupted partial output has no manifest.
        write_new(output / "manifest.json", manifest_bytes, files)
    except BaseException:
        rollback_created(output, files, directories)
        raise
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--images-dir", type=Path, required=True, help="directory containing reviewed original JPEGs")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory outside the repository and images")
    parser.add_argument("--catalog", type=Path, default=DEFAULT_CATALOG)
    args = parser.parse_args()
    try:
        manifest = prepare_bundle(args.images_dir, args.output_dir, args.catalog)
    except (OSError, ValueError, TypeError, KeyError) as error:
        parser.exit(1, f"ARK artwork preparation failed: {error}\n")
    print(json.dumps({"output": str(args.output_dir.absolute()), "images": manifest["imageCount"],
                      "imageBytes": manifest["imageBytes"], "published": False, "remoteValidation": "not-performed"}))


if __name__ == "__main__":
    main()
