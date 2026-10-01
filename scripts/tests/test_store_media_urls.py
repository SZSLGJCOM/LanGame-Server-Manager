from __future__ import annotations

import unittest
from unittest.mock import patch

from scripts.fetch_module_store_data import allowed_media_url, build_entry
from scripts.store_media_urls import allowed_store_media_url
from scripts.verify_library_media import is_allowed_media_url, validate_entry


class StoreMediaUrlTests(unittest.TestCase):
    def test_fetch_and_verifier_accept_the_same_official_media_references(self):
        urls = [
            "https://shared.cdn.steamchina.queniuam.com/store_item_assets/steam/apps/322330/header.jpg?t=123",
            "https://video.fastly.steamstatic.com/store_trailers/123/movie.m3u8",
            "https://video.cdn.steamchina.queniuam.com/store_trailers/123/movie.m3u8?t=4",
            "https://images.steamusercontent.com/ugc/123/image.jpg?imw=400",
            "https://steamuserimages-a.akamaihd.net/ugc/123/image.jpg",
            "https://cdn.akamai.steamstatic.com/steam/apps/322330/header.jpg",
            "https://store-images.s-microsoft.com/image/apps.example?q=90&w=1280",
            "https://cdn.trailers.xboxservices.com/trailers/example/master.m3u8",
        ]
        for url in urls:
            with self.subTest(url=url):
                self.assertEqual(allowed_media_url(f" {url} "), url)
                self.assertTrue(is_allowed_media_url(url))

    def test_original_trusted_hosts_remain_allowed_without_rewriting(self):
        for host in ["cdn.akamai.steamstatic.com", "shared.akamai.steamstatic.com", "shared.fastly.steamstatic.com",
                     "video.akamai.steamstatic.com", "store-images.s-microsoft.com", "cdn.trailers.xboxservices.com"]:
            url = f"https://{host}/existing/path?signature=preserved#original"
            self.assertEqual(allowed_store_media_url(url), url)

    def test_new_sources_are_limited_to_media_purposes_and_declared_paths(self):
        for value in [
            None, 123, "http://shared.cdn.steamchina.queniuam.com/store_item_assets/image.jpg",
            "https://user@shared.cdn.steamchina.queniuam.com/store_item_assets/image.jpg",
            "https://shared.cdn.steamchina.queniuam.com:443/store_item_assets/image.jpg",
            "https://shared.cdn.steamchina.queniuam.com.evil.invalid/store_item_assets/image.jpg",
            "https://shared.cdn.steamchina.queniuam.com/private/image.jpg",
            "https://video.fastly.steamstatic.com/private/movie.m3u8",
            "https://images.steamusercontent.com/private/image.jpg",
            "https://api.steamchina.com/ISteamNews/GetNewsForApp/v2/",
            "https://media.steampowered.com/installer/steamcmd.zip",
            "https://[invalid/asset.jpg",
        ]:
            with self.subTest(value=value):
                self.assertIsNone(allowed_media_url(value))
                self.assertFalse(is_allowed_media_url(value))

    def test_policy_exact_resources_are_consumed_without_creating_a_second_mapping(self):
        url = "https://official.invalid/exact/movie.mp4"
        policy = {"schemaVersion": 1, "groups": [], "exactResources": [
            {"urls": [url], "purposes": ["video"]},
            {"urls": ["https://official.invalid/file.zip"], "purposes": ["download"]},
        ]}
        with patch("scripts.store_media_urls._official_media_policy", return_value=policy):
            self.assertEqual(allowed_media_url(url), url)
            self.assertTrue(is_allowed_media_url(url))
            self.assertFalse(is_allowed_media_url(f"{url}?different=1"))
            self.assertFalse(is_allowed_media_url("https://official.invalid/file.zip"))

    def test_store_payload_fixture_round_trips_new_sources_without_fetching_or_rewriting(self):
        image = "https://shared.cdn.steamchina.queniuam.com/store_item_assets/steam/apps/322330/header.jpg?t=123"
        video = "https://video.fastly.steamstatic.com/store_trailers/123/movie.m3u8"
        payload = {"name": "Fixture", "header_image": image, "screenshots": [{"path_full": image}],
                   "movies": [{"hls_h264": video, "thumbnail": image}]}
        with patch("urllib.request.urlopen", side_effect=AssertionError("fixture must not access the network")):
            entry = build_entry("dontstarve", 322330, payload)
            self.assertEqual(validate_entry("dontstarve", entry), [])
        self.assertEqual(entry["coverUrl"], image)
        self.assertEqual(entry["screenshots"][0]["sourceUrl"], image)
        self.assertEqual(entry["trailers"][0]["streamUrl"], video)


if __name__ == "__main__":
    unittest.main()
