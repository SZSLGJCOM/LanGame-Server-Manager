from pathlib import Path
import unittest

from scripts.fetch_module_store_data import COVER_APP_OVERRIDES, MANUAL_STORE_ENTRIES, build_entry


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


class StoreCatalogTests(unittest.TestCase):
    def test_store_overrides_belong_to_available_modules(self):
        module_ids = {
            path.parent.name
            for path in (REPOSITORY_ROOT / "modules").glob("*/module.toml")
        }
        for catalog in (COVER_APP_OVERRIDES, MANUAL_STORE_ENTRIES):
            self.assertEqual(set(catalog) - module_ids, set())


class StoreTrailerTests(unittest.TestCase):
    HLS = "https://video.akamai.steamstatic.com/store_trailers/322330/1/hls_264_master.m3u8?t=1"
    DASH = "https://video.akamai.steamstatic.com/store_trailers/322330/1/dash_h264.mpd"
    MP4 = "https://cdn.akamai.steamstatic.com/steam/apps/1/movie_max.mp4?t=1"
    WEBM = "https://cdn.akamai.steamstatic.com/steam/apps/1/movie_max.webm?t=1"
    POSTER = "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/322330/header.jpg"

    def trailers(self, *movies: dict) -> list[dict]:
        return build_entry("dontstarve", 322330, {"movies": list(movies)})["trailers"]

    def test_prefers_hls_and_preserves_trailer_metadata(self):
        self.assertEqual(self.trailers({
            "id": 42, "name": "Launch trailer", "thumbnail": self.POSTER, "highlight": True,
            "hls_h264": self.HLS, "dash_h264": self.DASH,
            "mp4": {"max": self.MP4}, "webm": {"max": self.WEBM},
        }), [{
            "id": 42, "name": "Launch trailer", "posterUrl": self.POSTER,
            "streamUrl": self.HLS, "highlight": True,
        }])

    def test_excludes_dash_only_trailers(self):
        for movie in ({"dash_h264": self.DASH}, {"dash_av1": self.DASH}):
            with self.subTest(movie=movie):
                self.assertEqual(self.trailers(movie), [])

    def test_retains_native_mp4_and_webm_when_hls_is_unavailable(self):
        for movie, expected in (
            ({"mp4": {"max": self.MP4, "480": self.MP4.replace("max", "480")}, "webm": {"max": self.WEBM}}, self.MP4),
            ({"mp4": {"480": self.MP4}}, self.MP4),
            ({"webm": {"max": self.WEBM, "480": self.WEBM.replace("max", "480")}}, self.WEBM),
            ({"webm": {"480": self.WEBM}}, self.WEBM),
            ({"dash_h264": self.DASH, "mp4": {"max": self.MP4}}, self.MP4),
        ):
            with self.subTest(movie=movie):
                self.assertEqual([item["streamUrl"] for item in self.trailers(movie)], [expected])

    def test_rejects_manifest_or_image_urls_mislabeled_as_playable_formats(self):
        for movie in (
            {"hls_h264": self.DASH},
            {"hls_h264": self.POSTER},
            {"mp4": {"max": self.DASH}},
            {"webm": {"max": self.POSTER}},
        ):
            with self.subTest(movie=movie):
                self.assertEqual(self.trailers(movie), [])

    def test_invalid_primary_sources_do_not_hide_valid_native_fallbacks(self):
        for hls in (
            self.DASH,
            self.HLS.replace("https:", "http:"),
            self.HLS.replace("video.akamai.steamstatic.com", "unapproved.example.invalid"),
            self.HLS.replace("https://", "https://user:password@"),
            self.HLS.replace(".m3u8", ".m3u8.exe"),
        ):
            with self.subTest(hls=hls):
                self.assertEqual(
                    [item["streamUrl"] for item in self.trailers({"hls_h264": hls, "mp4": {"max": self.MP4}})],
                    [self.MP4],
                )

    def test_malformed_progressive_sources_do_not_hide_valid_webm(self):
        for mp4 in (None, "invalid", [], {"max": None}, {"max": self.DASH}):
            with self.subTest(mp4=mp4):
                self.assertEqual(
                    [item["streamUrl"] for item in self.trailers({"mp4": mp4, "webm": {"480": self.WEBM}})],
                    [self.WEBM],
                )

    def test_movie_limit_counts_only_playable_trailers(self):
        movies = self.trailers(
            {"id": 1, "dash_h264": self.DASH},
            {"id": 2, "hls_h264": self.POSTER},
            {"id": 3, "hls_h264": self.HLS},
            {"id": 4, "webm": {"480": self.WEBM}},
            {"id": 5, "mp4": {"max": self.MP4}},
        )
        self.assertEqual([(item["id"], item["streamUrl"]) for item in movies], [(3, self.HLS), (4, self.WEBM)])
