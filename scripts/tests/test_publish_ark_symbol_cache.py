import copy
import hashlib
import io
import struct
import tempfile
import unittest
from pathlib import Path
import zipfile

from scripts import publish_ark_symbol_cache as symbols
from scripts.tests.test_publish_ark_tools_dependencies import FixtureTransport


def entry(name=b"NativeFunction", width=8):
    return struct.pack("<Q", len(name)) + name + bytes(width)


def archive(entries):
    data = io.BytesIO()
    with zipfile.ZipFile(data, "w") as output:
        for name, value in entries:
            output.writestr(name, value)
    return data.getvalue()


class SymbolPublicationTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.package = copy.deepcopy(symbols.load_manifest()["packages"][0])
        self.bytes = archive([("cached_offsets.cache", entry()), ("cached_bitfields.cache", entry(b"Bit", 32))])
        self.package["size"] = len(self.bytes)
        self.package["sha256"] = hashlib.sha256(self.bytes).hexdigest()
        self.package["mirrorTag"] = "ark-tools-symbols-asa-" + self.package["sha256"][:16]
        self.package["mirrorUrl"] = symbols.dependencies.attachment_url(self.package["mirrorTag"], self.package["fileName"])

    def prepare(self):
        def source(package, destination):
            symbols.dependencies.write_immutable(destination, self.bytes)
        return symbols.prepare(self.package, self.directory, source,
                               lambda _: b"MIT License\nPermission is hereby granted (fixture only).\n")

    def test_manifest_binds_executable_and_content_addressed_mirror(self):
        manifest = symbols.load_manifest()
        self.assertEqual(len(manifest["packages"]), 1)
        for change in (
            lambda x: x.update(mirrorRepository="unrelated/repo"),
            lambda x: x["packages"][0].update(executableSha256="a" * 64),
            lambda x: x["packages"][0].update(sha256="invalid"),
            lambda x: x["packages"][0].update(size=True),
            lambda x: x["packages"][0].update(mirrorTag="latest"),
            lambda x: x["packages"][0].update(upstreamUrl="https://example.com/cache.zip"),
            lambda x: x["packages"].append(copy.deepcopy(x["packages"][0])),
        ):
            candidate = copy.deepcopy(manifest)
            change(candidate)
            path = self.directory / "invalid.json"
            path.write_bytes(symbols.dependencies.json_bytes(candidate))
            with self.assertRaises(symbols.SyncError):
                symbols.load_manifest(path)

    def test_maps_reject_truncation_duplicate_and_wrong_value_width(self):
        for raw, width in ((b"", 8), (entry()[:-1], 8), (entry() * 2, 8), (entry(), 32)):
            with self.subTest(width=width, size=len(raw)), self.assertRaises(symbols.SyncError):
                symbols.map_entries(io.BytesIO(raw), len(raw), width)

    def test_archive_rejects_unknown_member_and_missing_or_invalid_map(self):
        for entries in (
            [("cached_offsets.cache", entry()), ("../other", entry())],
            [("cached_offsets.cache", entry()), ("cached_offsets.txt", b"name")],
            [("cached_offsets.cache", entry()), ("cached_bitfields.cache", entry())],
        ):
            path = self.directory / "invalid.zip"
            path.write_bytes(archive(entries))
            with self.assertRaises(symbols.SyncError):
                symbols.inspect_archive(path)

    def test_preparation_preserves_original_zip_and_publishes_only_reviewed_assets(self):
        prepared = self.prepare()
        self.assertEqual((self.directory / self.package["fileName"]).read_bytes(), self.bytes)
        self.assertEqual({x["name"] for x in prepared["assets"]}, {
            self.package["fileName"], "ASA-SDK-LICENSE.txt", "THIRD_PARTY_NOTICES.txt", "symbols.json", "SHA256SUMS"})
        self.assertEqual([x["entries"] for x in prepared["archiveMembers"]], [1, 1])
        self.assertEqual(self.prepare(), prepared)
        (self.directory / "THIRD_PARTY_NOTICES.txt").write_text("changed")
        with self.assertRaisesRegex(symbols.SyncError, "Prepared file differs"):
            self.prepare()

    def test_shared_publication_rechecks_all_symbols_without_replacing_assets(self):
        prepared = self.prepare()
        client = FixtureTransport(symbols.dependencies.load_manifest()[0])
        result = symbols.dependencies.publish(client, prepared, self.directory)
        self.assertEqual(result["verifiedAssets"], 5)
        client.calls.clear()
        client.uploads.clear()
        client.downloads.clear()
        symbols.dependencies.publish(client, self.prepare(), self.directory)
        self.assertEqual(client.uploads, [])
        self.assertTrue(all(call[1] == "GET" for call in client.calls))
        self.assertEqual(len(client.downloads), 5)


if __name__ == "__main__":
    unittest.main()
