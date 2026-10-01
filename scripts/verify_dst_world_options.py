"""Check the tracked DST inventory; optionally compare or refresh an owned package."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys
import zipfile

from dst_world_options import MODULE, PAGES, compare_inventory, compare_schema, extract_inventory, validate_inventory, write_inventory


def main(argv: list[str] | None = None, *, root: Path | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install-root", type=Path, help="Read version.txt and data/databundles/scripts.zip without running the game")
    parser.add_argument("--write", action="store_true", help="Refresh only modules/dontstarve/world-options.json; requires --install-root")
    args = parser.parse_args(argv)
    if args.write and args.install_root is None:
        parser.error("--write requires --install-root")
    root = root or Path(__file__).resolve().parents[1]
    inventory_path = root / MODULE / "world-options.json"
    try:
        schema = json.loads((root / MODULE / "schema.json").read_text(encoding="utf-8"))
        actual = extract_inventory(args.install_root) if args.install_root else None
        if actual is not None:
            validate_inventory(actual)
        if args.write:
            write_inventory(inventory_path, actual)
            tracked = actual
        else:
            tracked = json.loads(inventory_path.read_text(encoding="utf-8"))
            validate_inventory(tracked)
        errors = compare_schema(tracked, schema)
        if actual is not None and not args.write:
            errors.extend(compare_inventory(tracked, actual))
    except (OSError, ValueError, TypeError, KeyError, zipfile.BadZipFile) as error:
        print(f"DST world options: {error}", file=sys.stderr)
        return 1
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    counts = [sum(option["category"] == category and location in option["locations"] and (master or not option["masterControlled"]) for option in tracked["options"]) for category, location, master in PAGES.values()]
    print(f"DST {tracked['gameVersion']}: {len(tracked['options'])} native options; Master gen/settings {counts[0]}/{counts[1]}, Caves gen/settings {counts[2]}/{counts[3]}; schema matches.")
    if actual is not None:
        print("Native package inventory refreshed." if args.write else "Native package version, hash, groups, order, locations and enums match.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
