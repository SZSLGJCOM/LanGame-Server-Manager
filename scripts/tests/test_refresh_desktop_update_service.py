from copy import deepcopy
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import URLError

from scripts import refresh_desktop_update_service as service
from scripts.prepare_desktop_update_service import GITHUB_RELEASES, PLATFORM, gitcode_artifact_url
from scripts.tests.test_desktop_update_service import PUBLIC_KEY, manifest


def release_pointer(version: str = "0.1.0") -> dict:
    value = manifest()
    value["version"] = version
    name = f"LanGame.Server.Manager_{version}_x64-setup.exe"
    value["platforms"][PLATFORM]["url"] = f"{GITHUB_RELEASES}/v{version}/{name}"
    raw = (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()
    assets = []
    for filename in sorted(service.release_asset_names(version)):
        # The fixtures are not signed binaries. Only metadata structure is tested.
        content = raw if filename == "latest.json" else f"synthetic {filename}".encode()
        assets.append({"name": filename, "size": len(content), "sha256": hashlib.sha256(content).hexdigest(),
                       "url": gitcode_artifact_url(version, filename)})
    return {"schema_version": 1, "github": {"repository": service.GITHUB_REPOSITORY,
            "release_id": 12345, "tag": f"v{version}"}, "version": version,
            "manifest": value, "manifest_text": raw.decode(), "assets": assets,
            "gitcode_url": gitcode_artifact_url(version, name)}


def encoded(pointer: dict) -> bytes:
    return json.dumps(pointer, ensure_ascii=False).encode()


class ReleasePointerTests(unittest.TestCase):
    def test_pointer_roundtrip_preserves_every_saved_revision_byte(self):
        first = service.validate_release_pointer(release_pointer(), PUBLIC_KEY)
        reloaded = service.validate_release_pointer(service.read_json(first.pointer), PUBLIC_KEY)
        self.assertEqual(first, reloaded)
        with tempfile.TemporaryDirectory(prefix="lgsm-pointer-roundtrip-") as temporary:
            directory = Path(temporary)
            for name, data in service.revision_files(first).items():
                (directory / name).write_bytes(data)
            service.verify_revision(directory, reloaded)

    def test_verified_record_preserves_published_manifest_and_only_changes_cn_url(self):
        pointer = release_pointer()
        result = service.validate_release_pointer(pointer, PUBLIC_KEY)
        self.assertEqual(result.global_manifest, pointer["manifest_text"].encode())
        cn = json.loads(result.cn_manifest)
        self.assertEqual(cn["platforms"][PLATFORM]["url"], pointer["gitcode_url"])
        cn["platforms"][PLATFORM]["url"] = pointer["manifest"]["platforms"][PLATFORM]["url"]
        self.assertEqual(cn, pointer["manifest"])
        reordered = deepcopy(pointer)
        reordered["assets"].reverse()
        self.assertEqual(result.identity, service.validate_release_pointer(reordered, PUBLIC_KEY).identity)

    def test_manifest_text_cannot_disagree_with_original_asset_or_object(self):
        bad = []
        item = release_pointer(); item["manifest_text"] += " "; bad.append(item)
        item = release_pointer(); item["manifest"]["notes"] = "changed"; bad.append(item)
        item = release_pointer(); item["manifest_text"] = "\ufeff" + item["manifest_text"]; bad.append(item)
        item = release_pointer(); item["manifest_text"] = '{"version":"0.1.0","version":"0.2.0"}'; bad.append(item)
        for item in bad:
            with self.subTest(item=item), self.assertRaises(ValueError):
                service.validate_release_pointer(item, PUBLIC_KEY)

    def test_strict_identity_fields_reject_missing_extra_wrong_repo_tag_and_version(self):
        edits = [lambda p: p.update(schema_version=True), lambda p: p.update(schema_version=2),
                 lambda p: p.update(extra="not allowed"), lambda p: p.pop("manifest_text"),
                 lambda p: p.update(version="0.1.1"),
                 lambda p: p["github"].update(repository="Other/repo"),
                 lambda p: p["github"].update(tag="latest"),
                 lambda p: p["github"].update(release_id=True),
                 lambda p: p["github"].update(release_id=0)]
        for change in edits:
            item = release_pointer(); change(item)
            with self.subTest(item=item), self.assertRaises(ValueError):
                service.validate_release_pointer(item, PUBLIC_KEY)

    def test_all_seven_assets_are_unique_bounded_and_use_known_permanent_origins(self):
        edits = [lambda p: p["assets"].pop(), lambda p: p["assets"].append(p["assets"][0]),
                 lambda p: p["assets"].__setitem__(1, p["assets"][0]),
                 lambda p: p["assets"][0].update(sha256="0" * 63),
                 lambda p: p["assets"][0].update(sha256="A" * 64),
                 lambda p: p["assets"][0].update(size=True), lambda p: p["assets"][0].update(size=0),
                 lambda p: p["assets"][0].update(size=2 ** 31 + 1),
                 lambda p: p["assets"][0].update(url="https://127.0.0.1/file.exe"),
                 lambda p: p["assets"][0].update(url=p["assets"][0]["url"] + "?signature=x"),
                 lambda p: p["assets"][0].update(name="unexpected.exe")]
        for change in edits:
            item = release_pointer(); change(item)
            with self.subTest(item=item), self.assertRaises(ValueError):
                service.validate_release_pointer(item, PUBLIC_KEY)

    def test_same_version_content_has_distinct_identity(self):
        pointer = release_pointer()
        first = service.validate_release_pointer(pointer, PUBLIC_KEY)
        pointer["assets"][0]["sha256"] = "0" * 64
        self.assertNotEqual(first.identity, service.validate_release_pointer(pointer, PUBLIC_KEY).identity)

    def test_exact_web_attachment_urls_are_accepted_but_small_url_must_match_record(self):
        pointer = release_pointer()
        for asset in pointer["assets"]:
            asset["url"] = ("https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases"
                            f"/releases/download/v0.1.0/{asset['name']}")
        with self.assertRaisesRegex(ValueError, "small installer"):
            service.validate_release_pointer(pointer, PUBLIC_KEY)
        pointer["gitcode_url"] = next(asset["url"] for asset in pointer["assets"]
                                      if asset["name"] == "LanGame.Server.Manager_0.1.0_x64-setup.exe")
        self.assertEqual(service.validate_release_pointer(pointer, PUBLIC_KEY).version, "0.1.0")


class Response(io.BytesIO):
    def __init__(self, value: bytes, headers: dict | None = None, status: int = 200):
        super().__init__(value)
        self.headers = headers or {}
        self.status = status


class MetadataFetchTests(unittest.TestCase):
    def fetch_with(self, response: Response) -> bytes:
        with patch.object(service, "build_opener") as build:
            build.return_value.open.return_value = response
            value = service.fetch_pointer()
            request = build.return_value.open.call_args.args[0]
            self.assertEqual(request.full_url, service.POINTER_URL)
            self.assertNotIn("Authorization", request.headers)
            self.assertEqual(build.call_args.args[0].proxies, {})
            self.assertIsInstance(build.call_args.args[1], service.NoRedirects)
            return value

    def test_anonymous_fixed_url_one_request(self):
        value = encoded(release_pointer())
        self.assertEqual(self.fetch_with(Response(value)), value)

    def test_size_limit_with_and_without_content_length(self):
        for response in (Response(b"", {"Content-Length": str(service.MAX_POINTER_BYTES + 1)}),
                         Response(b"x" * (service.MAX_POINTER_BYTES + 1))):
            with self.subTest(response=response), self.assertRaisesRegex(ValueError, "256 KiB"):
                self.fetch_with(response)

    def test_http_error_redirect_and_deadline_fail_without_retry(self):
        with self.assertRaisesRegex(ValueError, "HTTP 200"):
            self.fetch_with(Response(b"not found", status=404))
        with self.assertRaisesRegex(ValueError, "redirects"):
            service.NoRedirects().redirect_request(None, None, 302, "Found", {}, "http://127.0.0.1/")
        with patch.object(service.time, "monotonic", side_effect=[0, 31]), self.assertRaises(TimeoutError):
            self.fetch_with(Response(b"partial"))


@unittest.skipUnless(os.name == "posix", "Linux deployment test requires native symlink/rename semantics")
class LinuxPublicationTests(unittest.TestCase):
    def test_initial_missing_current_upgrade_and_idempotent_recheck(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-refresh-") as temporary:
            root = Path(temporary)
            self.assertIsNone(service.current_release(root, PUBLIC_KEY))
            first = release_pointer()
            result = service.refresh_once(root, PUBLIC_KEY, lambda: encoded(first))
            self.assertEqual(result["status"], "published")
            self.assertTrue((root / "current").is_symlink())
            previous = (root / "current").resolve()
            self.assertEqual((root / "current/latest-global.json").read_bytes(), first["manifest_text"].encode())
            same = service.refresh_once(root, PUBLIC_KEY, lambda: encoded(first))
            self.assertEqual(same["status"], "unchanged")
            self.assertEqual((root / "current").resolve(), previous)
            service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer("0.2.0")))
            self.assertNotEqual((root / "current").resolve(), previous)
            self.assertTrue(previous.is_dir())
            self.assertEqual(service.current_release(root, PUBLIC_KEY).version, "0.2.0")
            self.assertFalse(list(root.glob(".current-*")))
            self.assertFalse(list((root / "revisions").glob(".prepare-*")))

    def test_downgrade_same_version_conflict_bad_metadata_and_network_failure_preserve_current(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-refresh-") as temporary:
            root = Path(temporary)
            original = release_pointer("0.2.0")
            service.refresh_once(root, PUBLIC_KEY, lambda: encoded(original))
            target = os.readlink(root / "current")
            conflict = deepcopy(original); conflict["assets"][0]["sha256"] = "0" * 64
            cases = [encoded(release_pointer("0.1.0")), encoded(conflict), b"invalid JSON",
                     b"x" * (service.MAX_POINTER_BYTES + 1)]
            for raw in cases:
                with self.subTest(raw=raw[:32]), self.assertRaises(ValueError):
                    service.refresh_once(root, PUBLIC_KEY, lambda: raw)
                self.assertEqual(os.readlink(root / "current"), target)
            def unavailable():
                raise URLError("network unavailable")
            with self.assertRaises(URLError):
                service.refresh_once(root, PUBLIC_KEY, unavailable)
            self.assertEqual(os.readlink(root / "current"), target)

    def test_atomic_switch_failure_retains_previous_and_next_run_recovers_prepared_revision(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-refresh-") as temporary:
            root = Path(temporary)
            service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer()))
            target = os.readlink(root / "current")
            with patch.object(service.os, "replace", side_effect=OSError("injected switch failure")):
                with self.assertRaisesRegex(OSError, "switch failure"):
                    service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer("0.2.0")))
            self.assertEqual(os.readlink(root / "current"), target)
            self.assertFalse(list(root.glob(".current-*")))
            self.assertEqual(service.refresh_once(root, PUBLIC_KEY,
                            lambda: encoded(release_pointer("0.2.0")))["status"], "published")

    def test_rejects_unowned_current_path_and_changed_saved_bytes(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-refresh-") as temporary:
            root = Path(temporary)
            (root / "current").symlink_to("../outside", target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "outside"):
                service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer()))
            (root / "current").unlink()
            service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer()))
            (root / "current/latest-cn.json").write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "differs"):
                service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer()))

    def test_failed_first_fetch_keeps_current_missing_and_lock_excludes_other_writer(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-refresh-") as temporary:
            root = Path(temporary)
            with self.assertRaises(ValueError):
                service.refresh_once(root, PUBLIC_KEY, lambda: b"bad JSON")
            self.assertFalse((root / "current").exists())
            with service.refresh_lock(root), self.assertRaises(BlockingIOError):
                service.refresh_once(root, PUBLIC_KEY, lambda: encoded(release_pointer()))


if __name__ == "__main__":
    unittest.main()
