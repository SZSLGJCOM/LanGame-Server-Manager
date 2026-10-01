from __future__ import annotations

import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest

from scripts import generate_rust_third_party_licenses as generator


def crate_archive(
    name: str,
    version: str,
    *,
    license_text: bytes | None = b"MIT License\n\nPermission is hereby granted.\n",
    declared_license: str = "MIT",
) -> bytes:
    prefix = f"{name}-{version}"
    manifest = (
        "[package]\n"
        f'name = "{name}"\n'
        f'version = "{version}"\n'
        f'license = "{declared_license}"\n'
        'repository = "https://example.invalid/project"\n'
        'authors = ["Example Author"]\n'
    ).encode()
    vcs = json.dumps({"git": {"sha1": "a" * 40}}).encode()
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as archive:
        for relative_path, payload in (
            ("Cargo.toml", manifest),
            (".cargo_vcs_info.json", vcs),
            ("LICENSE", license_text),
        ):
            if payload is None:
                continue
            info = tarfile.TarInfo(f"{prefix}/{relative_path}")
            info.size = len(payload)
            archive.addfile(info, io.BytesIO(payload))
    return stream.getvalue()


def lock_text(name: str, version: str, checksum: str) -> str:
    return (
        "version = 4\n\n"
        "[[package]]\n"
        f'name = "{name}"\n'
        f'version = "{version}"\n'
        f'source = "{generator.CRATES_IO_SOURCE}"\n'
        f'checksum = "{checksum}"\n'
    )


class RustLicenseGeneratorTests(unittest.TestCase):
    def test_reads_checksum_verified_archive_and_renders_deterministically(self) -> None:
        payload = crate_archive("sample-crate", "1.2.3")
        checksum = hashlib.sha256(payload).hexdigest()
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            lock_path = root / "Cargo.lock"
            lock_path.write_text(lock_text("sample-crate", "1.2.3", checksum), encoding="utf-8")
            cache = root / "cargo" / "registry" / "cache" / "index.crates.io-test"
            cache.mkdir(parents=True)
            (cache / "sample-crate-1.2.3.crate").write_bytes(payload)

            package = generator.load_locked_registry_packages(lock_path)[0]
            archived, downloaded = generator.archive_payload(
                package, cargo_home=root / "cargo", offline=True
            )
            record = generator.parse_crate(package, archived)

        self.assertFalse(downloaded)
        self.assertEqual(record.package_id, "sample-crate@1.2.3")
        self.assertEqual(record.declared_license, "MIT")
        self.assertEqual(record.vcs_sha1, "a" * 40)
        first = generator.render_document([record])
        second = generator.render_document([record])
        self.assertEqual(first, second)
        self.assertIn("sample-crate@1.2.3", first)
        self.assertIn("Permission is hereby granted.", first)

    def test_mpl_notice_identifies_exact_source_archive_and_preserves_source_rights(self) -> None:
        payload = crate_archive(
            "mpl-fixture", "1.2.3+build.4", declared_license="MPL-2.0",
            license_text=b"Mozilla Public License Version 2.0\n",
        )
        package = {
            "name": "mpl-fixture", "version": "1.2.3+build.4",
            "checksum": hashlib.sha256(payload).hexdigest(),
            "source": generator.CRATES_IO_SOURCE,
        }
        document = generator.render_document([generator.parse_crate(package, payload)])
        self.assertTrue(
            "Source archive / 源码归档: https://static.crates.io/crates/mpl-fixture/"
            "mpl-fixture-1.2.3%2Bbuild.4.crate" in document,
            "The notice must identify the exact, URL-encoded upstream source package.",
        )
        self.assertIn("Source Code Form under MPL-2.0", document)
        self.assertIn("modified MPL-covered files", document)
        self.assertIn("does not restrict recipients' MPL source rights", document)

    def test_checked_in_source_archives_match_every_locked_package(self) -> None:
        document = generator.OUTPUT_PATH.read_text(encoding="utf-8")
        inventory = document.split("LEGAL TEXT CATALOG / 法律文本目录", 1)[0]
        blocks = inventory.split("Package / 软件包: ")[1:]
        packages = generator.load_locked_registry_packages()
        self.assertEqual(len(blocks), len(packages))
        for block, package in zip(blocks, packages, strict=True):
            with self.subTest(package=package["name"], version=package["version"]):
                lines = block.splitlines()
                self.assertEqual(lines[0], f"{package['name']}@{package['version']}")
                self.assertIn(f"Archive SHA-256: {package['checksum']}", lines)
                expected = (
                    f"Source archive / 源码归档: https://static.crates.io/crates/{package['name']}/"
                    f"{package['name']}-{package['version'].replace('+', '%2B')}.crate"
                )
                self.assertEqual([line for line in lines if line.startswith("Source archive / ")], [expected])

    def test_rejects_cached_archive_that_does_not_match_lock(self) -> None:
        payload = crate_archive("sample-crate", "1.2.3")
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            cache = root / "registry" / "cache" / "index.crates.io-test"
            cache.mkdir(parents=True)
            (cache / "sample-crate-1.2.3.crate").write_bytes(payload)
            with self.assertRaisesRegex(
                generator.LicenseGenerationError, "do not match Cargo.lock checksum"
            ):
                generator.find_cached_archive(root, "sample-crate", "1.2.3", "0" * 64)

    def test_rejects_unknown_missing_license_text(self) -> None:
        payload = crate_archive("unreviewed-crate", "1.0.0", license_text=None)
        package = {
            "name": "unreviewed-crate",
            "version": "1.0.0",
            "source": generator.CRATES_IO_SOURCE,
            "checksum": hashlib.sha256(payload).hexdigest(),
        }
        record = generator.parse_crate(package, payload)
        with self.assertRaisesRegex(
            generator.LicenseGenerationError, "no reviewed exact-version rule"
        ):
            generator.apply_missing_license_rules([record])

    def test_download_fallback_is_checksum_verified_without_populating_cache(self) -> None:
        payload = crate_archive("downloaded-crate", "2.0.0")
        package = {
            "name": "downloaded-crate",
            "version": "2.0.0",
            "source": generator.CRATES_IO_SOURCE,
            "checksum": hashlib.sha256(payload).hexdigest(),
        }
        with tempfile.TemporaryDirectory() as temporary_directory:
            cargo_home = Path(temporary_directory) / "cargo"
            archived, downloaded = generator.archive_payload(
                package,
                cargo_home=cargo_home,
                offline=False,
                download=lambda _name, _version: payload,
            )
            self.assertFalse((cargo_home / "registry" / "cache").exists())
        self.assertTrue(downloaded)
        self.assertEqual(archived, payload)

    def test_checked_in_audited_sources_match_policy_hashes(self) -> None:
        sources = generator.audited_license_sources()
        self.assertEqual(set(sources), set(generator.AUDITED_SOURCES))


if __name__ == "__main__":
    unittest.main()
