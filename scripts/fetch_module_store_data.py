from __future__ import annotations

import json
import re
import time
import urllib.request
from pathlib import Path
from urllib.parse import urlsplit

if __package__:
    from .store_media_urls import allowed_store_media_url as allowed_media_url
else:
    from store_media_urls import allowed_store_media_url as allowed_media_url


ROOT = Path(__file__).resolve().parents[1]
MODULES_DIR = ROOT / "modules"
OUTPUT_JSON = ROOT / "apps" / "desktop" / "src" / "data" / "module-store-data.json"
APPDETAILS_URL = "https://store.steampowered.com/api/appdetails?appids={app_id}&l=schinese&cc=cn"
MAX_SCREENSHOTS = 6
MAX_TRAILERS = 2
APP_ID_PATTERN = re.compile(r"^steam_app_id\s*=\s*(\d+)\s*$", re.MULTILINE)

COVER_APP_OVERRIDES: dict[str, list[int]] = {
    "abioticfactor": [427410, 2857200],
    "arksurvivalascended": [2399830, 2430930],
    "arksurvivalevolved": [346110, 376030],
    "astroneer": [361420, 728470],
    "barotrauma": [602960, 1026340],
    "conanexiles": [440900, 443030],
    "corekeeper": [1621690, 1963720],
    "dontstarve": [322330, 343050],
    "enshrouded": [1203620, 2278520],
    "humanitz": [1766060, 2728330],
    "necesse": [1169040, 1169370],
    "nightingale": [1928980, 3796810],
    "palworld": [1623730, 2394010],
    "projectzomboid": [108600, 380870],
    "returntomoria": [2933130, 3349480],
    "rimworld": [294100],
    "romestead": [1805320, 4763510],
    "runescapedragonwilds": [1374490, 4019830],
    "rust": [252490, 258550],
    "satisfactory": [526870, 1690800],
    "scum": [513710, 3792580],
    "sevendaystodie": [251570, 294420],
    "sonsoftheforest": [1326470, 2465200],
    "soulmask": [2646460, 3017310],
    "squad": [393380, 403240],
    "terraria": [105600],
    "theforest": [242760, 556450],
    "unturned": [304930, 1110390],
    "valheim": [892970, 896660],
    "vrising": [1604030, 1829350],
    "windrose": [3041230, 4129620],
}

MANUAL_STORE_ENTRIES: dict[str, dict] = {
    "minecraft": {
        "storeSource": "official",
        "storeAppId": None,
        "storeName": "Minecraft Java Edition",
        "coverUrl": "https://store-images.s-microsoft.com/image/apps.53301.14216416494490173.9772fa78-5a01-45ce-9b9e-6ec61a10f4e2.3157b812-e78d-42b5-8d1e-c705c8e99c20?q=90&w=1280&h=720",
        "shortDescription": "",
        "aboutParagraphs": [],
        "genres": ["Sandbox", "Survival"],
        "categories": ["Multiplayer", "Co-op", "Dedicated Server"],
        "developers": ["Mojang Studios"],
        "publishers": ["Mojang Studios", "Xbox Game Studios"],
        "releaseDate": "",
        "storeUrl": "https://www.minecraft.net/en-us/download/server",
        "screenshots": [
            {
                "id": 1,
                "label": "Screenshot 1",
                "sourceUrl": "https://store-images.s-microsoft.com/image/apps.53301.14216416494490173.9772fa78-5a01-45ce-9b9e-6ec61a10f4e2.3157b812-e78d-42b5-8d1e-c705c8e99c20?q=90&w=1280&h=720",
            },
            {
                "id": 2,
                "label": "Screenshot 2",
                "sourceUrl": "https://store-images.s-microsoft.com/image/apps.8991.14216416494490173.9772fa78-5a01-45ce-9b9e-6ec61a10f4e2.f1809072-ce05-4893-800d-47217b28d3ae?q=90&w=1280&h=720",
            },
            {
                "id": 3,
                "label": "Screenshot 3",
                "sourceUrl": "https://store-images.s-microsoft.com/image/apps.21926.14216416494490173.9772fa78-5a01-45ce-9b9e-6ec61a10f4e2.907ec983-11a4-4e05-a2ea-2a67ef17f604?q=90&w=1280&h=720",
            },
            {
                "id": 4,
                "label": "Screenshot 4",
                "sourceUrl": "https://store-images.s-microsoft.com/image/apps.64739.14216416494490173.9772fa78-5a01-45ce-9b9e-6ec61a10f4e2.29ea7f08-7cc9-4462-809e-e82a3d88e6d1?q=90&w=1280&h=720",
            },
        ],
        "trailers": [
            {
                "id": 1,
                "name": "Minecraft Launcher trailer",
                "posterUrl": "https://store-images.s-microsoft.com/image/apps.55291.14247769038588514.9bec5ad3-671a-4c3e-ab15-deaf758065b3.37302b52-5a12-47cf-86b8-1edb45ef42a1?q=90&w=1280&h=720",
                "streamUrl": "https://cdn.trailers.xboxservices.com/trailers/00000000-0000-0000-0000-000000000000/is/content/microsoftassets/bc7acd3e-6d60-49cb-8b07-0916e6f7febc-AVS.m3u8?packagedStreaming=true",
                "highlight": True,
            },
        ],
    }
}


def read_steam_app_id(module_toml: Path) -> int | None:
    match = APP_ID_PATTERN.search(module_toml.read_text(encoding="utf-8"))
    return int(match.group(1)) if match else None


def candidate_app_ids(module_id: str, steam_app_id: int | None) -> list[int]:
    candidates = [*COVER_APP_OVERRIDES.get(module_id, [])]
    if steam_app_id is not None:
        candidates.append(steam_app_id)
    return list(dict.fromkeys(candidates))


def fetch_json(url: str) -> dict:
    request = urllib.request.Request(url, headers={"User-Agent": "LanGame-StoreSync/1.0"})
    last_error: Exception | None = None
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return json.load(response)
        except Exception as exc:  # noqa: BLE001
            last_error = exc
            time.sleep(0.6 * (attempt + 1))
    raise RuntimeError(f"failed to fetch json: {url}: {last_error}")


def choose_store_payload(module_id: str, steam_app_id: int | None) -> tuple[int | None, dict | None]:
    for app_id in candidate_app_ids(module_id, steam_app_id):
        payload = fetch_json(APPDETAILS_URL.format(app_id=app_id))
        app_payload = payload.get(str(app_id), {})
        data = app_payload.get("data") if app_payload.get("success") else None
        if isinstance(data, dict):
            return app_id, data
    return None, None


def choose_trailer_stream_url(trailer: dict) -> str | None:
    hls = allowed_media_url(trailer.get("hls_h264"))
    if hls and urlsplit(hls).path.lower().endswith(".m3u8"):
        return hls
    # The desktop supports HLS and native video files, but has no DASH player.
    for video_format in ("mp4", "webm"):
        sources = trailer.get(video_format)
        if not isinstance(sources, dict):
            continue
        for quality in ("max", "480"):
            source = allowed_media_url(sources.get(quality))
            if source and urlsplit(source).path.lower().endswith(f".{video_format}"):
                return source
    return None


def build_entry(module_id: str, app_id: int, data: dict) -> dict:
    screenshots = []
    for index, screenshot in enumerate(data.get("screenshots", [])[:MAX_SCREENSHOTS], start=1):
        source_url = allowed_media_url(screenshot.get("path_full")) or allowed_media_url(screenshot.get("path_thumbnail"))
        if source_url:
            screenshots.append({
                "id": screenshot.get("id", index),
                "label": f"Screenshot {index}",
                "sourceUrl": source_url,
            })

    trailers = []
    for index, trailer in enumerate(data.get("movies", []), start=1):
        stream_url = choose_trailer_stream_url(trailer)
        if stream_url:
            trailers.append({
                "id": trailer.get("id", index),
                "name": trailer.get("name") or f"Trailer {index}",
                "posterUrl": allowed_media_url(trailer.get("thumbnail")),
                "streamUrl": stream_url,
                "highlight": bool(trailer.get("highlight")),
            })
            if len(trailers) == MAX_TRAILERS:
                break

    cover_url = next(
        (
            candidate
            for candidate in (
                allowed_media_url(data.get("header_image")),
                allowed_media_url(data.get("capsule_image")),
                allowed_media_url(data.get("capsule_imagev5")),
            )
            if candidate
        ),
        None,
    )

    return {
        "storeSource": "steam",
        "storeAppId": app_id,
        "storeName": data.get("name") or module_id,
        "coverUrl": cover_url,
        "shortDescription": "",
        "aboutParagraphs": [],
        "genres": [item.get("description") for item in data.get("genres", []) if item.get("description")],
        "categories": [item.get("description") for item in data.get("categories", []) if item.get("description")],
        "developers": data.get("developers", []),
        "publishers": data.get("publishers", []),
        "releaseDate": (data.get("release_date") or {}).get("date") or "",
        "storeUrl": f"https://store.steampowered.com/app/{app_id}/",
        "screenshots": screenshots,
        "trailers": trailers,
    }


def main() -> int:
    entries: dict[str, dict] = {}
    for module_toml in sorted(MODULES_DIR.glob("*/module.toml")):
        module_id = module_toml.parent.name
        app_id, payload = choose_store_payload(module_id, read_steam_app_id(module_toml))
        if app_id is not None and payload is not None:
            entries[module_id] = build_entry(module_id, app_id, payload)

    for module_id, entry in MANUAL_STORE_ENTRIES.items():
        if (MODULES_DIR / module_id / "module.toml").exists():
            entries[module_id] = entry

    OUTPUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_JSON.write_text(json.dumps(entries, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"synced factual store metadata and remote media URLs for {len(entries)} modules")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
