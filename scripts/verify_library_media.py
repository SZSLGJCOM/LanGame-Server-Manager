from __future__ import annotations

import json
import sys
from pathlib import Path

if __package__:
    from .store_media_urls import allowed_store_media_url
else:
    from store_media_urls import allowed_store_media_url


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
PUBLIC_DIR = ROOT / "apps" / "desktop" / "public"
STORE_DATA_PATH = ROOT / "apps" / "desktop" / "src" / "data" / "module-store-data.json"
LOCAL_MEDIA_DIRS = (
    PUBLIC_DIR / "game-covers",
    PUBLIC_DIR / "game-media",
    PUBLIC_DIR / "game-assets",
)


def load_store_data() -> dict[str, dict]:
    return json.loads(STORE_DATA_PATH.read_text(encoding="utf-8"))


def list_module_ids() -> list[str]:
    return sorted(module_toml.parent.name for module_toml in MODULES_DIR.glob("*/module.toml"))


def is_allowed_media_url(value: object) -> bool:
    return allowed_store_media_url(value) is not None


def validate_entry(module_id: str, entry: dict) -> list[str]:
    issues: list[str] = []
    store_url = str(entry.get("storeUrl") or "").strip()
    store_app_id = entry.get("storeAppId")
    store_source = str(entry.get("storeSource") or "").strip().lower()

    if store_source == "steam":
        if not isinstance(store_app_id, int) or store_app_id <= 0:
            issues.append("missing or invalid Steam app id")
        if not store_url.startswith("https://store.steampowered.com/app/"):
            issues.append("missing or invalid Steam storeUrl")
        if not is_allowed_media_url(entry.get("coverUrl")):
            issues.append("missing or untrusted remote coverUrl")
    elif store_source == "official":
        if not store_url.startswith("https://"):
            issues.append("missing or invalid official storeUrl")
        cover_url = entry.get("coverUrl")
        if cover_url is not None and not is_allowed_media_url(cover_url):
            issues.append("untrusted official coverUrl")
    else:
        issues.append(f"unsupported storeSource {store_source!r}")

    if str(entry.get("shortDescription") or "").strip():
        issues.append("copied store shortDescription must not be vendored")
    if entry.get("aboutParagraphs") not in ([], None):
        issues.append("copied store aboutParagraphs must not be vendored")

    screenshots = entry.get("screenshots")
    if not isinstance(screenshots, list):
        issues.append("missing screenshots list")
    else:
        for index, screenshot in enumerate(screenshots, start=1):
            screenshot_data = screenshot if isinstance(screenshot, dict) else {}
            if "path" in screenshot_data:
                issues.append(f"local screenshot path #{index} is forbidden")
            if not is_allowed_media_url(screenshot_data.get("sourceUrl")):
                issues.append(f"missing or untrusted screenshot sourceUrl #{index}")

    trailers = entry.get("trailers")
    if not isinstance(trailers, list):
        issues.append("missing trailers list")
    else:
        for index, trailer in enumerate(trailers, start=1):
            trailer_data = trailer if isinstance(trailer, dict) else {}
            if "poster" in trailer_data:
                issues.append(f"local trailer poster #{index} is forbidden")
            poster_url = trailer_data.get("posterUrl")
            if poster_url is not None and not is_allowed_media_url(poster_url):
                issues.append(f"untrusted trailer posterUrl #{index}")
            if not is_allowed_media_url(trailer_data.get("streamUrl")):
                issues.append(f"missing or untrusted trailer streamUrl #{index}")

    return issues


def main() -> int:
    failures: list[str] = []
    store_data = load_store_data()

    for media_dir in LOCAL_MEDIA_DIRS:
        if media_dir.exists() and any(path.is_file() for path in media_dir.rglob("*")):
            failures.append(f"{media_dir.relative_to(ROOT)}: vendored game media is forbidden")

    module_ids = list_module_ids()
    for module_id in module_ids:
        entry = store_data.get(module_id)
        if not isinstance(entry, dict):
            failures.append(f"{module_id}: missing store data entry")
            continue
        failures.extend(f"{module_id}: {issue}" for issue in validate_entry(module_id, entry))

    unknown_modules = sorted(set(store_data) - set(module_ids))
    failures.extend(f"{module_id}: store metadata has no module" for module_id in unknown_modules)

    if failures:
        print("library media verification failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print(f"library remote media metadata verified for {len(module_ids)} modules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
