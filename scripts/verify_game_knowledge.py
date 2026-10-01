"""Check bundled source manifests offline; never fetch bodies or advance review dates."""
from __future__ import annotations

import argparse
import datetime as dt
import ipaddress
import json
import re
import stat
import sys
import tomllib
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
MAX_FILE_BYTES = 256 * 1024
REVIEW_AFTER_DAYS = 30
IDENTIFIER = re.compile(r"[a-z0-9-]{1,64}\Z")
MANIFEST_FIELDS = {"schema_version", "module_id", "scope", "gaps", "sources"}
SOURCE_REQUIRED = {
    "id", "title", "authority", "kind", "seeds", "allowed_prefixes",
    "discover_links", "max_pages", "authority_evidence", "license_note", "reviewed_on",
}
SOURCE_OPTIONAL = {"content_selector", "discovery_selector", "sitemaps", "license_url", "reference_only"}


def text(value: object, limit: int) -> bool:
    return (isinstance(value, str) and bool(value.strip())
            and len(value.encode("utf-8")) <= limit
            and not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in value))


def identifier(value: object) -> bool:
    return isinstance(value, str) and IDENTIFIER.fullmatch(value) is not None


def source_url(value: object, *, directory: bool = False) -> bool:
    """Static screening; the downloader must separately validate DNS and redirects."""
    if not text(value, 2048) or not isinstance(value, str):
        return False
    if any(c.isspace() or c in '\\\"<>' or ord(c) == 96 for c in value):
        return False
    try:
        parsed = urlsplit(value)
        host = parsed.hostname or ""
        labels = host.split(".")
        if (
            parsed.scheme != "https" or not value.startswith("https://")
            or parsed.username is not None or parsed.password is not None
            or parsed.port is not None or parsed.fragment or "#" in value
            or len(labels) < 2 or host != parsed.netloc
            or any(re.fullmatch(r"[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?", label) is None for label in labels)
            or labels[-1].isdigit()
            or host.endswith((".localhost", ".local", ".internal", ".test", ".invalid"))
        ):
            return False
        try:
            ipaddress.ip_address(host)
            return False
        except ValueError:
            pass
        if re.search(r"%(?![0-9A-Fa-f]{2})", value):
            return False
        decoded = unquote(value, errors="strict")
        if any(ord(c) < 32 or 127 <= ord(c) <= 159 or c == "\\" for c in decoded):
            return False
        if re.search(r"%(?:2f|5c|25)", parsed.path, re.IGNORECASE):
            return False
        if any(segment in {".", ".."} for segment in unquote(parsed.path).split("/")):
            return False
        return not directory or (parsed.path.endswith("/") and not parsed.query and "?" not in value)
    except (ValueError, UnicodeError):
        return False


def string_list(value: object, minimum: int, maximum: int, limit: int) -> bool:
    return (isinstance(value, list) and minimum <= len(value) <= maximum
            and all(text(item, limit) for item in value) and len(set(value)) == len(value))


def validate_manifest(manifest: object, module_id: str, today: dt.date) -> tuple[list[str], list[dict[str, object]]]:
    errors: list[str] = []
    report: list[dict[str, object]] = []
    if not isinstance(manifest, dict) or set(manifest) != MANIFEST_FIELDS:
        return [f"{module_id}: invalid manifest fields"], report
    if (type(manifest.get("schema_version")) is not int or manifest["schema_version"] != 1
            or manifest.get("module_id") != module_id or not identifier(module_id)):
        errors.append(f"{module_id}: schema version or module binding mismatch")
    if not text(manifest.get("scope"), 2048):
        errors.append(f"{module_id}: invalid scope")
    if not string_list(manifest.get("gaps"), 0, 32, 2048):
        errors.append(f"{module_id}: invalid gaps")
    sources = manifest.get("sources")
    if not isinstance(sources, list) or not 1 <= len(sources) <= 16:
        return errors + [f"{module_id}: expected 1-16 sources"], report
    seen: set[str] = set()
    for index, source in enumerate(sources):
        prefix = f"{module_id}.sources[{index}]"
        if (not isinstance(source, dict) or not SOURCE_REQUIRED <= set(source)
                or set(source) - SOURCE_REQUIRED - SOURCE_OPTIONAL):
            errors.append(f"{prefix}: invalid source fields")
            continue
        key = source.get("id")
        if not identifier(key) or key in seen:
            errors.append(f"{prefix}: invalid or duplicate id")
        else:
            seen.add(key)
        for field, bound in (("title", 512), ("authority", 512), ("license_note", 4096)):
            if not text(source.get(field), bound):
                errors.append(f"{prefix}: invalid {field}")
        if not isinstance(source.get("kind"), str) or source["kind"] not in {"official", "official_community", "community"}:
            errors.append(f"{prefix}: invalid kind")
        if type(source.get("discover_links")) is not bool:
            errors.append(f"{prefix}: discover_links must be boolean")
        if type(source.get("max_pages")) is not int or not 1 <= source["max_pages"] <= 512:
            errors.append(f"{prefix}: max_pages must be an integer in 1-512")
        if "reference_only" in source and type(source["reference_only"]) is not bool:
            errors.append(f"{prefix}: reference_only must be a boolean")
        for field, minimum, maximum in (("seeds", 1, 256), ("allowed_prefixes", 0, 64), ("sitemaps", 0, 16)):
            values = source.get(field, [])
            if not string_list(values, minimum, maximum, 2048):
                errors.append(f"{prefix}: invalid {field} list")
                continue
            if any(not source_url(value, directory=field == "allowed_prefixes") for value in values):
                errors.append(f"{prefix}: invalid HTTPS URL in {field}")
        seeds, prefixes = source.get("seeds", []), source.get("allowed_prefixes", [])
        if isinstance(seeds, list) and isinstance(prefixes, list):
            origins = {urlsplit(url).netloc for url in seeds if source_url(url)}
            if any(source_url(url, directory=True) and urlsplit(url).netloc not in origins for url in prefixes):
                errors.append(f"{prefix}: discovery prefix has no seed on the same origin")
        if source.get("discover_links") is True and not prefixes:
            errors.append(f"{prefix}: link discovery requires a bounded directory prefix")
        if "discovery_selector" in source and source.get("discover_links") is not True:
            errors.append(f"{prefix}: discovery_selector requires link discovery")
        for field in ("authority_evidence", "license_url"):
            if field in source and not source_url(source[field]):
                errors.append(f"{prefix}: invalid {field} URL")
        for field in ("content_selector", "discovery_selector"):
            if field in source and not text(source[field], 1024):
                errors.append(f"{prefix}: invalid {field}")
        reviewed = source.get("reviewed_on")
        try:
            if not isinstance(reviewed, str) or re.fullmatch(r"\d{4}-\d{2}-\d{2}", reviewed) is None:
                raise ValueError("non-canonical date")
            day = dt.date.fromisoformat(reviewed)
            if day < dt.date(1970, 1, 1) or day > today:
                raise ValueError("out-of-range date")
            age = (today - day).days
            report.append({"module": module_id, "source": key, "sourceKind": source.get("kind"),
                           "directoryReview": "overdue" if age > REVIEW_AFTER_DAYS else "recent",
                           "reviewedOn": reviewed, "ageDays": age,
                           "seedCount": len(seeds) if isinstance(seeds, list) else 0})
        except (ValueError, TypeError):
            errors.append(f"{prefix}: reviewed_on requires a valid nonfuture YYYY-MM-DD date")
    return errors, report


def linked(path: Path) -> bool:
    details = path.lstat()
    return path.is_symlink() or bool(getattr(details, "st_file_attributes", 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def read_toml(path: Path) -> dict[str, object]:
    if linked(path) or not path.is_file():
        raise ValueError("expected a regular local file")
    with path.open("rb") as handle:
        raw = handle.read(MAX_FILE_BYTES + 1)
    if len(raw) > MAX_FILE_BYTES:
        raise ValueError("file exceeds 256 KiB")
    return tomllib.loads(raw.decode("utf-8"))


def validate_module(module: Path, today: dt.date) -> tuple[list[str], list[dict[str, object]]]:
    try:
        if linked(module):
            raise ValueError("module must be a regular local directory")
        descriptor = read_toml(module / "module.toml")
        if descriptor.get("id") != module.name:
            raise ValueError("module.toml id does not match its directory")
        return validate_manifest(read_toml(module / "knowledge-sources.toml"), module.name, today)
    except (OSError, ValueError, UnicodeError) as error:
        return [f"{module.name}: {error}"], []


def verify_catalog(root: Path, today: dt.date) -> tuple[list[str], list[dict[str, object]], int]:
    errors: list[str] = []
    reports: list[dict[str, object]] = []
    try:
        modules_root = root / "modules"
        if linked(modules_root):
            raise ValueError("modules must be a regular local directory")
        modules = sorted(path.parent for path in modules_root.glob("*/module.toml"))
        if not modules:
            errors.append("No game modules were found")
        expected = {path.name for path in modules}
        for path in modules_root.glob("*/knowledge-sources.toml"):
            if path.parent.name not in expected:
                errors.append(f"{path.parent.name}: orphan source manifest without module.toml")
        for module in modules:
            failures, records = validate_module(module, today)
            errors.extend(failures)
            reports.extend(records)
        return errors, reports, len(modules)
    except (OSError, ValueError) as error:
        return [str(error)], reports, 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Validate only (also the default).")
    parser.add_argument("--report-stale", action="store_true", help="Report overdue directory reviews, not cached-body freshness.")
    parser.add_argument("--today", type=dt.date.fromisoformat, default=dt.datetime.now(dt.timezone.utc).date(), help="UTC YYYY-MM-DD override for reproducible checks.")
    args = parser.parse_args(argv)
    errors, reports, module_count = verify_catalog(ROOT, args.today)
    if errors:
        print("Game knowledge source verification failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    overdue = [row for row in reports if row["directoryReview"] == "overdue"]
    print(f"Game knowledge source coverage: {module_count}/{module_count} modules; "
          f"{len(reports)} sources; {sum(row['seedCount'] for row in reports)} exact seeds; "
          f"{len(overdue)} overdue directory reviews. "
          "Offline schema/coverage validation is not a successful fetch or document freshness guarantee.")
    if args.report_stale:
        print(json.dumps(overdue, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
