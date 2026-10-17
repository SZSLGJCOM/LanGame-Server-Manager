import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.publish_enshrouded_control import publish


def manifest(version="1.0.0", revision=1):
    return {"verified": True, "schema": 1, "module_id": "enshrouded", "protocol": 1, "target": "windows-x86_64",
            "version": version, "revision": revision,
            "profiles": [{"game_build_id": "25988414", "executable_sha256": "b" * 64}],
            "dll": {"asset_name": f"Lgsm.Enshrouded.Control_{version}_windows-x86_64.dll", "sha256": "a" * 64, "size": 123}}


def package_files(current):
    return {current["dll"]["asset_name"]: b"DLL", current["dll"]["asset_name"] + ".minisig": b"DLL proof",
            "manifest.json": json.dumps({key: value for key, value in current.items() if key != "verified"}).encode(),
            "manifest.json.minisig": f"proof:{current['version']}:{current['revision']}".encode(),
            "LICENSE.txt": b"Project license", "LICENSE.minhook.txt": b"MinHook/HDE notice", "SHA256SUMS": b"sums"}


def verified_feed(contents, node):
    # Cryptographic behavior is exercised by the Node/Rust signed-vector tests;
    # these tests model purpose-bound validation to isolate publication recovery.
    if set(contents) != {"manifest.json", "manifest.json.minisig"}:
        raise ValueError("Incomplete signed feed")
    parsed = json.loads(contents["manifest.json"])
    if contents["manifest.json.minisig"] != f"proof:{parsed['version']}:{parsed['revision']}".encode():
        raise ValueError("Invalid signed feed pair")
    return {"verified": True, **parsed}


class FakeClient:
    def __init__(self):
        self.releases = {}
        self.contents = {}
        self.events = []
        self.next_asset = 1
        self.fail_name = None
        self.fail_after_store = False
        self.fail_starter = False

    def release(self, tag):
        return self.releases.get(tag)

    def create(self, tag, commit, body):
        self.events.append(("create", tag))
        release = {"id": len(self.releases) + 1, "tag_name": tag, "prerelease": True, "draft": True,
                   "html_url": f"https://github.com/example/releases/tag/{tag}"}
        self.releases[tag] = release
        return release

    def assets(self, release):
        return self.contents.setdefault(release["id"], {})

    def download(self, asset):
        return asset["content"]

    def upload(self, release, name, content):
        self.events.append(("upload", release["tag_name"], name))
        failing = release["tag_name"] == "enshrouded-control" and name == self.fail_name
        asset = {"id": self.next_asset, "content": content, "size": len(content), "state": "uploaded"}
        self.next_asset += 1
        if failing and self.fail_starter:
            self.assets(release)[name] = {**asset, "content": b"", "size": 0, "state": "starter"}
        elif not failing or self.fail_after_store:
            self.assets(release)[name] = asset
        if failing:
            self.fail_name = None
            raise RuntimeError("Injected interrupted upload")
        return asset

    def request(self, url, method="GET"):
        if method != "DELETE":
            raise AssertionError("Unexpected request")
        asset_id = int(url.rsplit("/", 1)[1])
        for release_id, assets in self.contents.items():
            for name, asset in list(assets.items()):
                if asset["id"] == asset_id:
                    del assets[name]
                    self.events.append(("delete", release_id, name))
                    return
        raise AssertionError("Unknown asset")

    def publish(self, release, body):
        self.events.append(("publish", release["tag_name"]))
        release["draft"] = False
        return release


class PublicationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory(prefix="enshrouded-publisher-test-")
        self.addCleanup(directory.cleanup)
        self.intent = Path(directory.name) / "publish-intent.json"
        validator = patch("scripts.publish_enshrouded_control.verify_manifest_files", side_effect=verified_feed)
        validator.start()
        self.addCleanup(validator.stop)

    def run_publish(self, client, current):
        return publish(client, current, package_files(current), "0" * 40, "node", self.intent)

    def add_feed(self, client, current):
        pointer = client.create("enshrouded-control", "0" * 40, "")
        for name, content in package_files(current).items():
            if name in {"manifest.json", "manifest.json.minisig"}:
                client.upload(pointer, name, content)
        pointer["draft"] = False
        client.events.clear()
        return pointer

    def test_immutable_version_is_public_before_moving_feed(self):
        client = FakeClient()
        result = self.run_publish(client, manifest())
        self.assertTrue(result["published"])
        immutable_public = client.events.index(("publish", "enshrouded-control-v1.0.0"))
        feed_first_upload = client.events.index(("upload", "enshrouded-control", "manifest.json"))
        self.assertLess(immutable_public, feed_first_upload)
        self.assertTrue(all(release["prerelease"] for release in client.releases.values()))
        self.assertFalse(self.intent.exists())

    def test_lower_revision_and_same_revision_relabel_are_rejected_before_upload(self):
        for new in [manifest("1.0.1", 1), manifest("1.0.2", 2)]:
            client = FakeClient()
            self.add_feed(client, manifest("1.0.1", 2))
            with self.assertRaises(ValueError):
                self.run_publish(client, new)
            self.assertEqual(client.events, [])

    def test_new_revision_cannot_drop_verified_profile(self):
        client = FakeClient()
        self.add_feed(client, manifest())
        new = manifest("1.0.1", 2)
        new["profiles"][0]["game_build_id"] = "25990000"
        with self.assertRaisesRegex(ValueError, "retain"):
            self.run_publish(client, new)

    def test_published_immutable_asset_can_never_be_replaced(self):
        client = FakeClient()
        current = manifest()
        release = client.create("enshrouded-control-v1.0.0", "0" * 40, "")
        for name, content in package_files(current).items():
            client.upload(release, name, content)
        release["draft"] = False
        client.upload(release, "manifest.json", b"previous")
        with self.assertRaisesRegex(ValueError, "different bytes"):
            self.run_publish(client, current)

    def test_concurrent_feed_advancement_is_detected_before_pointer_write(self):
        client = FakeClient()
        self.add_feed(client, manifest())
        with patch("scripts.publish_enshrouded_control.old_manifest", side_effect=[manifest(), manifest("1.0.2", 3)]):
            with self.assertRaisesRegex(ValueError, "changed during"):
                self.run_publish(client, manifest("1.0.1", 2))
        self.assertNotIn(("upload", "enshrouded-control", "manifest.json"), client.events)

    def test_interrupted_upgrade_recovers_missing_mixed_and_complete_target_pairs(self):
        scenarios = [("manifest.json", False, False), ("manifest.json", True, False),
                     ("manifest.json.minisig", False, False), ("manifest.json.minisig", True, False),
                     ("manifest.json", False, True)]
        for name, after_store, starter in scenarios:
            with self.subTest(name=name, after_store=after_store, starter=starter):
                client = FakeClient()
                pointer = self.add_feed(client, manifest())
                client.fail_name = name
                client.fail_after_store = after_store
                client.fail_starter = starter
                with self.assertRaisesRegex(RuntimeError, "interrupted"):
                    self.run_publish(client, manifest("1.0.1", 2))
                self.assertTrue(self.intent.exists())
                result = self.run_publish(client, manifest("1.0.1", 2))
                self.assertTrue(result["published"])
                pair = {asset_name: client.download(asset) for asset_name, asset in client.assets(pointer).items()}
                self.assertEqual(verified_feed(pair, "node")["revision"], 2)
                self.assertFalse(self.intent.exists())

    def test_interrupted_first_publication_recovers_without_an_original_pair(self):
        client = FakeClient()
        client.fail_name = "manifest.json.minisig"
        with self.assertRaises(RuntimeError):
            self.run_publish(client, manifest())
        self.assertEqual(json.loads(self.intent.read_bytes())["original"], {})
        self.assertTrue(self.run_publish(client, manifest())["published"])

    def test_recovery_cannot_overwrite_a_newer_independent_publisher(self):
        client = FakeClient()
        pointer = self.add_feed(client, manifest())
        client.fail_name = "manifest.json.minisig"
        with self.assertRaises(RuntimeError):
            self.run_publish(client, manifest("1.0.1", 2))
        for name, content in package_files(manifest("1.0.2", 3)).items():
            if name in {"manifest.json", "manifest.json.minisig"}:
                client.upload(pointer, name, content)
        client.events.clear()
        with self.assertRaisesRegex(ValueError, "outside the original/target"):
            self.run_publish(client, manifest("1.0.1", 2))
        self.assertFalse(any(event[0] in {"delete", "upload"} for event in client.events))

    def test_recovery_requires_same_target_and_release_identity(self):
        client = FakeClient()
        pointer = self.add_feed(client, manifest())
        client.fail_name = "manifest.json.minisig"
        with self.assertRaises(RuntimeError):
            self.run_publish(client, manifest("1.0.1", 2))
        with self.assertRaisesRegex(ValueError, "different target"):
            self.run_publish(client, manifest("1.0.2", 3))
        pointer["id"] = 999
        with self.assertRaisesRegex(ValueError, "different target"):
            self.run_publish(client, manifest("1.0.1", 2))


if __name__ == "__main__":
    unittest.main()
