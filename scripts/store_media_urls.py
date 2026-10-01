from __future__ import annotations

import json
from functools import lru_cache
from pathlib import Path
from urllib.parse import urlparse


POLICY_PATH = Path(__file__).resolve().parents[1] / "crates" / "app-network" / "official-sources.json"
EXISTING_STORE_MEDIA_HOSTS = frozenset({
    "cdn.akamai.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "video.akamai.steamstatic.com",
    "store-images.s-microsoft.com",
    "cdn.trailers.xboxservices.com",
})
MEDIA_PURPOSES = frozenset({"image", "video"})


@lru_cache(maxsize=1)
def _official_media_policy() -> dict:
    policy = json.loads(POLICY_PATH.read_text(encoding="utf-8"))
    if not isinstance(policy, dict) or policy.get("schemaVersion") != 1:
        raise ValueError("Unsupported official media source policy")
    return policy


def allowed_store_media_url(value: object) -> str | None:
    if not isinstance(value, str):
        return None
    candidate = value.strip()
    try:
        parsed = urlparse(candidate)
        if parsed.scheme != "https" or not parsed.hostname or parsed.netloc != parsed.hostname:
            return None
    except ValueError:
        return None
    if parsed.hostname in EXISTING_STORE_MEDIA_HOSTS:
        return candidate

    policy = _official_media_policy()
    origin = f"https://{parsed.hostname}"
    # This validates the original reference; it never generates or forwards a signed URL to another source.
    for group in policy.get("groups", []):
        if not MEDIA_PURPOSES.intersection(group.get("purposes", [])) or origin not in group.get("origins", []):
            continue
        if parsed.path in group.get("exactPaths", []) or any(
            parsed.path.startswith(prefix) for prefix in group.get("pathPrefixes", [])
        ):
            return candidate
    for resource in policy.get("exactResources", []):
        if MEDIA_PURPOSES.intersection(resource.get("purposes", [])) and candidate in resource.get("urls", []):
            return candidate
    return None
