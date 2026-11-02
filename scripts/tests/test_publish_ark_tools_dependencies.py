"""Fixed-identity and mutation-recovery tests; no remote services are used."""
from copy import deepcopy
import contextlib
import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.parse
import zipfile

from scripts import publish_ark_tools_dependencies as publish


def digest(content):
    return hashlib.sha256(content).hexdigest()


class FixtureTransport:
    def __init__(self, manifest):
        self.files = {}
        for package in manifest["packages"]:
            stream = io.BytesIO()
            with zipfile.ZipFile(stream, "w") as archive:
                archive.writestr("fixture.txt", package["id"])
            content = stream.getvalue()
            package.update(size=len(content), sha256=digest(content))
            self.files[package["fileName"]] = content
        self.remote = None
        self.remote_bytes = {}
        self.calls = []
        self.uploads = []
        self.downloads = []
        self.upload_failure = None
        self.commit_failed_upload = False
        self.create_failure = False
        self.commit_failed_create = False

    def api(self, provider, method, suffix, payload=None, missing_ok=False, timeout=60):
        self.calls.append((provider, method, suffix, deepcopy(payload)))
        if provider != "gitcode":
            raise AssertionError("Unexpected authenticated upstream request")
        if suffix.startswith("/releases/tags/") and method == "GET":
            return deepcopy(self.remote)
        if suffix == "/releases" and method == "POST":
            if not self.create_failure or self.commit_failed_create:
                self.remote = {**payload, "assets": []}
            if self.create_failure:
                raise publish.SyncError("Lost creation response")
            return deepcopy(self.remote)
        if "/upload_url?" in suffix and method == "GET":
            return {"fixtureName": urllib.parse.parse_qs(urllib.parse.urlsplit(suffix).query)["file_name"][0]}
        raise AssertionError("Unexpected mutation/API path")

    def upload(self, ticket, source):
        name = ticket["fixtureName"]
        self.uploads.append(name)
        if name != self.upload_failure or self.commit_failed_upload:
            self.remote_bytes[name] = source.read_bytes()
            self.remote["assets"].append({"name": name})
        if name == self.upload_failure:
            raise publish.SyncError("Lost upload response")

    def download(self, url, destination, size, sha256, provider):
        name = url.split("/")[-2] if provider == "gitcode" else url.rsplit("/", 1)[-1]
        content = self.remote_bytes[name] if provider == "gitcode" else self.files[name]
        publish.require(len(content) == size and digest(content) == sha256, "Fixture artifact identity mismatch")
        self.downloads.append((provider, name, destination))
        if destination.exists():
            publish.require(destination.read_bytes() == content, "Existing local artifact conflicts")
        else:
            destination.write_bytes(content)


class DependencyPublicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.manifest, _ = publish.load_manifest()
        self.client = FixtureTransport(self.manifest)
        self.raw = publish.json_bytes(self.manifest)
        self.license = b"MIT License\nPermission is hereby granted (fixture only).\n"
        self.clock = 0.0
        def advance(seconds):
            self.clock += seconds
        for replacement in (patch.object(publish.time, "monotonic", side_effect=lambda: self.clock),
                            patch.object(publish.time, "sleep", side_effect=advance)):
            replacement.start()
            self.addCleanup(replacement.stop)
        self.output = contextlib.redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def prepare(self):
        return publish.prepare(self.client, self.manifest, self.raw, self.directory, lambda url: self.license)

    def run_publish(self):
        return publish.publish(self.client, self.prepare(), self.directory)

    def state(self):
        return json.loads((self.directory / "publish-state.json").read_bytes())

    def test_canonical_manifest_has_exact_packages_and_download_contract(self):
        manifest, _ = publish.load_manifest()
        self.assertEqual({p["id"] for p in manifest["packages"]}, {"ase", "asa"})
        for package in manifest["packages"]:
            self.assertEqual(package["mirrorUrl"], publish.attachment_url(manifest["mirrorTag"], package["fileName"]))

    def test_manifest_rejects_duplicate_or_untrusted_fields(self):
        for change in (
            lambda m: m.update(mirrorRepository="unrelated/repository"),
            lambda m: m.update(mirrorTag="latest"),
            lambda m: m["packages"][0].update(upstreamUrl="https://example.com/untrusted.zip"),
            lambda m: m["packages"][0].update(mirrorUrl="https://example.com/untrusted.zip"),
            lambda m: m["packages"][0].update(licenseUrl="https://raw.githubusercontent.com/ArkServerApi/AseApi/master/LICENSE"),
            lambda m: m["packages"][0].update(size=True),
            lambda m: m["packages"][0].update(size=32 * 1024 * 1024 + 1),
            lambda m: m["packages"][0].update(sha256="invalid"),
            lambda m: m["packages"].__setitem__(1, deepcopy(m["packages"][0])),
        ):
            with self.subTest(change=change):
                invalid = deepcopy(self.manifest)
                change(invalid)
                path = self.directory / "invalid.json"
                path.write_bytes(publish.json_bytes(invalid))
                with self.assertRaises(publish.SyncError):
                    publish.load_manifest(path)
        path.write_bytes(self.raw.replace(b'"schemaVersion": 1,', b'"schemaVersion": 1, "schemaVersion": 1,'))
        with self.assertRaisesRegex(publish.SyncError, "Duplicate JSON key"):
            publish.load_manifest(path)

    def test_preparation_has_no_gitcode_access_and_preserves_original_archives(self):
        prepared = self.prepare()
        self.assertEqual(self.client.calls, [])
        self.assertEqual(self.client.uploads, [])
        self.assertEqual(len(prepared["assets"]), 7)
        for name, content in self.client.files.items():
            self.assertEqual((self.directory / name).read_bytes(), content)
        checksums = (self.directory / "SHA256SUMS").read_text().splitlines()
        self.assertEqual(len(checksums), 6)
        for line in checksums:
            sha256, name = line.split("  ")
            self.assertEqual(publish.file_hash(self.directory / name), sha256)

    def test_corrupt_source_fails_before_publication(self):
        self.client.files[self.manifest["packages"][0]["fileName"]] = b"corrupt"
        with self.assertRaisesRegex(publish.SyncError, "identity mismatch"):
            self.run_publish()
        self.assertEqual(self.client.calls, [])

    def test_prepared_files_are_never_silently_replaced(self):
        self.prepare()
        path = self.directory / "ASA-LICENSE.txt"
        path.write_bytes(b"user evidence")
        with self.assertRaisesRegex(publish.SyncError, "Prepared file differs"):
            self.prepare()
        self.assertEqual(path.read_bytes(), b"user evidence")

    def test_local_change_after_preparation_prevents_remote_access(self):
        prepared = self.prepare()
        (self.directory / "SHA256SUMS").write_bytes(b"changed after preparation")
        with self.assertRaisesRegex(publish.SyncError, "changed before publication"):
            publish.publish(self.client, prepared, self.directory)
        self.assertEqual(self.client.calls, [])

    def test_complete_publication_stays_pre_and_produces_verified_receipt(self):
        result = self.run_publish()
        self.assertTrue(result["published"])
        self.assertEqual(result["verifiedAssets"], 7)
        self.assertEqual(self.client.remote["release_status"], "pre")
        self.assertEqual(self.state()["status"], "verified")
        self.assertIsNone(self.state()["pending"])
        writes = [call for call in self.client.calls if call[1] != "GET"]
        self.assertEqual([(call[1], call[2]) for call in writes], [("POST", "/releases")])
        self.assertEqual(len(self.client.uploads), len(set(self.client.uploads)))
        self.assertEqual(len(list(self.directory.glob("attempt-*/receipt.json"))), 1)

    def test_second_execution_anonymously_rechecks_every_asset_without_writes(self):
        self.run_publish()
        self.client.calls.clear()
        self.client.uploads.clear()
        self.client.downloads.clear()
        self.run_publish()
        self.assertEqual(self.client.uploads, [])
        self.assertTrue(all(call[1] == "GET" for call in self.client.calls))
        self.assertEqual(len([entry for entry in self.client.downloads if entry[0] == "gitcode"]), 7)

    def test_existing_conflict_prevents_all_append_operations(self):
        self.run_publish()
        name = self.manifest["packages"][0]["fileName"]
        self.client.remote_bytes[name] = b"conflicting remote bytes"
        missing = "THIRD_PARTY_NOTICES.txt"
        self.client.remote["assets"] = [entry for entry in self.client.remote["assets"] if entry["name"] != missing]
        self.client.calls.clear()
        self.client.uploads.clear()
        with self.assertRaisesRegex(publish.SyncError, "identity mismatch"):
            self.run_publish()
        self.assertEqual(self.client.uploads, [])
        self.assertTrue(all(call[1] == "GET" for call in self.client.calls))

    def test_lost_upload_response_is_reconciled_without_duplicate_put(self):
        self.client.upload_failure = self.manifest["packages"][0]["fileName"]
        self.client.commit_failed_upload = True
        self.assertTrue(self.run_publish()["published"])
        self.assertEqual(self.client.uploads.count(self.client.upload_failure), 1)

    def test_unresolved_upload_is_persisted_and_not_replayed_on_restart(self):
        failed = self.manifest["packages"][0]["fileName"]
        self.client.upload_failure = failed
        with self.assertRaisesRegex(publish.SyncError, "outcome remains unknown"):
            self.run_publish()
        self.assertEqual(self.state()["status"], "unknown")
        self.assertEqual(self.state()["pending"], {"kind": "upload", "name": failed})
        self.assertEqual(self.clock, 30)
        self.client.upload_failure = None
        before = list(self.client.uploads)
        with self.assertRaisesRegex(publish.SyncError, "Previous mutation is unresolved"):
            self.run_publish()
        self.assertEqual(self.client.uploads, before)
        # A delayed upstream callback is resolved by actual anonymous bytes.
        self.client.remote_bytes[failed] = (self.directory / failed).read_bytes()
        self.client.remote["assets"].append({"name": failed})
        self.assertTrue(self.run_publish()["published"])
        self.assertEqual(self.client.uploads.count(failed), 1)

    def test_lost_creation_response_is_reconciled_without_second_post(self):
        self.client.create_failure = True
        self.client.commit_failed_create = True
        self.assertTrue(self.run_publish()["published"])
        self.assertEqual(len([call for call in self.client.calls if call[1] == "POST"]), 1)

    def test_unresolved_creation_is_not_repeated(self):
        self.client.create_failure = True
        with self.assertRaisesRegex(publish.SyncError, "creation outcome unknown"):
            self.run_publish()
        self.client.create_failure = False
        with self.assertRaisesRegex(publish.SyncError, "Previous mutation is unresolved"):
            self.run_publish()
        self.assertEqual(len([call for call in self.client.calls if call[1] == "POST"]), 1)
        self.assertEqual(self.state()["pending"], {"kind": "create_release"})

    def test_release_identity_and_latest_status_cannot_be_overwritten(self):
        self.run_publish()
        for key, value in (("release_status", "latest"), ("tag_name", "unrelated"), ("body", "unrelated source")):
            with self.subTest(key=key):
                original = self.client.remote[key]
                self.client.remote[key] = value
                self.client.calls.clear()
                self.client.uploads.clear()
                with self.assertRaisesRegex(publish.SyncError, "identity/state differs"):
                    self.run_publish()
                self.assertEqual(self.client.uploads, [])
                self.assertTrue(all(call[1] == "GET" for call in self.client.calls))
                self.client.remote[key] = original


if __name__ == "__main__":
    unittest.main()
