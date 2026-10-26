import hashlib
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from scripts import prepare_ark_map_artwork as artwork


def jpeg_header_fixture(width=1024, height=1024, progressive=False):
    """JPEG marker fixture for the header probe, not a decodable image."""
    frame = bytes([8]) + struct.pack(">HHB", height, width, 1) + b"\x01\x11\x00"
    marker = b"\xff\xc2" if progressive else b"\xff\xc0"
    scan = b"\x01\x01\x00\x00\x3f\x00"
    return (b"\xff\xd8" + marker + struct.pack(">H", len(frame) + 2) + frame
            + b"\xff\xda" + struct.pack(">H", len(scan) + 2) + scan
            + b"\x01\xff\x00\x02\xff\xd0\x03\xff\xd9")


def catalog_entry():
    return {
        "edition": "arksurvivalevolved", "mapName": "TheIsland",
        "image": "The_Island_Topographic_Map.jpg", "width": 1024, "height": 1024,
        "bounds": {"minX": 7.2, "maxX": 92.8, "minY": 7.2, "maxY": 92.8},
        "coordinateSource": "https://ark.wiki.gg/wiki/Data:Maps/Resources/The_Island?oldid=548222",
    }


class PrepareArkMapArtworkTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="ark-artwork-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.images = self.root / "images"
        self.images.mkdir()
        self.output = self.root / "publication"
        self.catalog = self.root / "catalog.json"
        self.entry = catalog_entry()
        self.payload = jpeg_header_fixture()
        (self.images / self.entry["image"]).write_bytes(self.payload)
        self.write_catalog()

    def write_catalog(self, entries=None, verified_on="2026-10-25"):
        self.catalog.write_text(json.dumps({
            "verifiedOn": verified_on, "maps": entries or [self.entry],
        }), encoding="utf-8")

    def prepare(self):
        return artwork.prepare_bundle(self.images, self.output, self.catalog)

    def test_bundle_preserves_bytes_and_records_hash_geometry_sources_and_unpublished_urls(self):
        unrelated = self.images / "unrelated.txt"
        unrelated.write_text("retain", encoding="utf-8")
        receipt = self.prepare()
        manifest = json.loads((self.output / "manifest.json").read_text(encoding="utf-8"))
        self.assertEqual(receipt, manifest)
        self.assertFalse(manifest["published"])
        self.assertEqual(manifest["remoteValidation"], "not-performed")
        self.assertEqual(manifest["imageCount"], 1)
        self.assertEqual(manifest["imageBytes"], len(self.payload))
        item = manifest["images"][0]
        relative = "assets/ark-map-artwork/2026-10-25/The_Island_Topographic_Map.jpg"
        self.assertEqual(item["relativePath"], relative)
        self.assertEqual((self.output / relative).read_bytes(), self.payload)
        self.assertEqual(item["sha256"], hashlib.sha256(self.payload).hexdigest())
        self.assertEqual(item["size"], len(self.payload))
        self.assertEqual(item["bounds"], self.entry["bounds"])
        self.assertEqual(item["coordinateSource"], self.entry["coordinateSource"])
        self.assertEqual(item["sourceURL"], "https://ark.wiki.gg/images/The_Island_Topographic_Map.jpg")
        self.assertEqual(item["licenseSourceURL"],
                         "https://ark.wiki.gg/wiki/File:The_Island_Topographic_Map.jpg")
        self.assertEqual(item["proposedURLs"], {
            "github": "https://raw.githubusercontent.com/SZSLGJCOM/LanGame-Server-Manager/master/" + relative,
            "gitcode": "https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/raw/"
                       + relative + "?ref=main",
        })
        self.assertIn("not published", (self.output / "README.md").read_text(encoding="utf-8"))
        self.assertIn("not covered by the project's source-code license",
                      (self.output / "NOTICE.md").read_text(encoding="utf-8"))
        self.assertEqual((self.images / self.entry["image"]).read_bytes(), self.payload)
        self.assertEqual(unrelated.read_text(encoding="utf-8"), "retain")
        self.assertEqual(len([p for p in self.output.rglob("*") if p.is_file()]), 4)

    def test_dimension_mismatch_and_truncated_or_non_jpeg_are_rejected_before_output(self):
        for payload in (jpeg_header_fixture(2048, 1024), self.payload[:-2],
                        b"<html>not an image</html>", b"\xff\xd8\xff\xe0\x00\x01\xff\xd9"):
            with self.subTest(payload=payload[:16]):
                (self.images / self.entry["image"]).write_bytes(payload)
                with self.assertRaises(ValueError):
                    self.prepare()
                self.assertFalse(self.output.exists())

    def test_header_probe_handles_progressive_markers_stuffing_and_restart(self):
        self.assertEqual(artwork.jpeg_dimensions(jpeg_header_fixture(2048, 1024, True)), (2048, 1024))

    def test_missing_and_oversized_images_are_rejected(self):
        image = self.images / self.entry["image"]
        image.unlink()
        with self.assertRaises((ValueError, OSError)):
            self.prepare()
        with image.open("wb") as stream:
            stream.truncate(16 * 1024 * 1024 + 1)
        with self.assertRaisesRegex(ValueError, "16 MiB"):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_catalog_filenames_cannot_escape_or_use_windows_aliases(self):
        for image in ("../outside.jpg", "nested/map.jpg", "nested\\map.jpg", "C:\\map.jpg",
                      "/absolute.jpg", "map.jpg:stream", "%2e%2e.jpg", "CON.jpg"):
            with self.subTest(image=image):
                self.write_catalog([{**self.entry, "image": image}])
                with self.assertRaises(ValueError):
                    self.prepare()
                self.assertFalse(self.output.exists())

    def test_duplicate_casefolded_images_and_invalid_catalog_fields_are_rejected(self):
        cases = [
            [self.entry, {**self.entry, "mapName": "Other", "image": self.entry["image"].lower()}],
            [{**self.entry, "width": True}],
            [{**self.entry, "bounds": {**self.entry["bounds"], "maxX": 0}}],
            [{**self.entry, "coordinateSource": "https://example.invalid/unverified"}],
        ]
        for entries in cases:
            with self.subTest(entries=entries):
                self.write_catalog(entries)
                with self.assertRaises(ValueError):
                    self.prepare()
        self.write_catalog(verified_on="../escape")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.output.exists())

    def test_existing_output_is_never_overwritten_even_if_empty(self):
        self.output.mkdir()
        with self.assertRaises((ValueError, FileExistsError)):
            self.prepare()
        sentinel = self.output / "keep.txt"
        sentinel.write_text("retain", encoding="utf-8")
        with self.assertRaises((ValueError, FileExistsError)):
            self.prepare()
        self.assertEqual(sentinel.read_text(encoding="utf-8"), "retain")

    def test_output_must_be_outside_repository_and_input_directory(self):
        fake_repository = self.root / "repository"
        fake_repository.mkdir()
        with patch.object(artwork, "REPOSITORY_ROOT", fake_repository):
            for output in (fake_repository / "bundle", self.images / "bundle"):
                with self.subTest(output=output), self.assertRaises(ValueError):
                    artwork.prepare_bundle(self.images, output, self.catalog)
                self.assertFalse(output.exists())

    def test_symlink_image_is_rejected_without_copying_its_target(self):
        image = self.images / self.entry["image"]
        target = self.root / "outside.jpg"
        target.write_bytes(self.payload)
        image.unlink()
        try:
            image.symlink_to(target)
        except OSError as error:
            self.skipTest(f"symlink creation unavailable: {error}")
        with self.assertRaises(ValueError):
            self.prepare()
        self.assertFalse(self.output.exists())
        self.assertEqual(target.read_bytes(), self.payload)

    @unittest.skipUnless(sys.platform == "win32", "Windows junction contract")
    def test_windows_junction_ancestors_are_rejected_for_input_and_output(self):
        junction = self.root / "image-junction"
        quoted_junction = str(junction).replace("'", "''")
        quoted_target = str(self.images).replace("'", "''")
        result = subprocess.run([
            "powershell.exe", "-NoProfile", "-Command",
            "$ErrorActionPreference = 'Stop'; New-Item -ItemType Junction "
            f"-Path '{quoted_junction}' -Target '{quoted_target}' | Out-Null",
        ], capture_output=True, text=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        with self.assertRaisesRegex(ValueError, "reparse"):
            artwork.prepare_bundle(junction, self.output, self.catalog)
        with self.assertRaisesRegex(ValueError, "reparse"):
            artwork.prepare_bundle(self.images, junction / "publication", self.catalog)
        self.assertFalse(self.output.exists())
        self.assertFalse((self.images / "publication").exists())
        self.assertEqual((self.images / self.entry["image"]).read_bytes(), self.payload)

    def test_write_failure_removes_only_this_incomplete_bundle(self):
        original_open = Path.open

        def fail_notice(path, mode="r", *args, **kwargs):
            if path.name == "NOTICE.md" and mode == "xb":
                raise OSError("injected output failure")
            return original_open(path, mode, *args, **kwargs)

        with patch.object(Path, "open", new=fail_notice), self.assertRaisesRegex(OSError, "injected"):
            self.prepare()
        self.assertFalse(self.output.exists())
        self.assertEqual((self.images / self.entry["image"]).read_bytes(), self.payload)

    def test_cli_failure_does_not_print_success(self):
        (self.images / self.entry["image"]).write_bytes(b"invalid")
        result = subprocess.run([
            sys.executable, "-B", str(Path(artwork.__file__)), "--catalog", str(self.catalog),
            "--images-dir", str(self.images), "--output-dir", str(self.output),
        ], capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("preparation failed", result.stderr)
        self.assertFalse(self.output.exists())


if __name__ == "__main__":
    unittest.main()
