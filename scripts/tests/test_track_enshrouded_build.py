import json
import unittest
from unittest.mock import patch

from scripts.track_enshrouded_build import ensure_issue, steam_public_build


def metadata(build="25988414"):
    return f'''Steam Console Client (c) Valve Corporation
Connecting anonymously to Steam Public...OK
"2278520"
{{
  "common" {{ "name" "Enshrouded Dedicated Server" }}
  "depots" {{
    "2278521" {{ "manifests" {{ "public" {{ "gid" "123" }} }} }}
    "branches" {{ "public" {{ "buildid" "{build}" "timeupdated" "1792195200" }} }}
  }}
}}
Unloading Steam API...OK
'''


class SteamMetadataTests(unittest.TestCase):
    def test_reads_only_public_dedicated_server_branch(self):
        self.assertEqual(steam_public_build(metadata())["game_build_id"], "25988414")
        self.assertEqual(steam_public_build(metadata())["steam_app_id"], "2278520")

    def test_stale_login_or_unexpected_metadata_cannot_claim_compatibility(self):
        for text in ["Steam login failed", metadata().replace('"2278520"', '"1203620"'),
                     metadata(""), metadata("025988414"), metadata().replace('"public" { "buildid"', '"preview" { "buildid"'),
                     metadata()[:metadata().rfind("}")]]:
            with self.subTest(text=text[:60]), self.assertRaises(ValueError):
                steam_public_build(text)

    def test_rejects_duplicate_build_and_oversized_metadata(self):
        with self.assertRaises(ValueError):
            steam_public_build(metadata().replace('"buildid" "25988414"', '"buildid" "25988414" "buildid" "25967645"'))
        with self.assertRaises(ValueError):
            steam_public_build(metadata() + "x" * (4 * 1024 * 1024))

    def test_closed_compatibility_issue_is_deduplicated(self):
        build = steam_public_build(metadata())
        issue = {"body": "<!-- langame-enshrouded-build:25988414 -->", "html_url": "https://github.com/example/issues/1", "state": "closed"}
        with patch("scripts.track_enshrouded_build.request_bytes", return_value=json.dumps([issue]).encode()) as request:
            result = ensure_issue(build, {"revision": 1, "version": "1.0.0"}, "fixture-generated-valid-value")
        self.assertFalse(result["created"])
        self.assertEqual(request.call_count, 1)

    def test_new_build_issue_is_explicitly_a_validation_request(self):
        build = steam_public_build(metadata())
        with patch("scripts.track_enshrouded_build.request_bytes", side_effect=[b"[]", b'{"html_url":"https://github.com/example/issues/2"}']) as request:
            result = ensure_issue(build, {"revision": 1, "version": "1.0.0"}, "fixture-generated-valid-value")
        self.assertTrue(result["created"])
        body = request.call_args.kwargs["data"]["body"]
        self.assertIn("has not enabled this binary", body)
        self.assertIn("kick, ban and unban", body)


if __name__ == "__main__":
    unittest.main()
