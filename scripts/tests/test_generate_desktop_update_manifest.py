import base64
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from scripts.generate_desktop_update_manifest import (
    public_artifact_name,
    validate_artifact_url,
    validate_pub_date,
    validate_signature,
    validate_version,
    write_outputs,
)


def record(lines: list[str]) -> str:
    return base64.b64encode(("\n".join(lines) + "\n").encode()).decode()


def signature(key_id: int = 7) -> str:
    # Format fixtures only, not cryptographically valid signatures.
    return record([
        "untrusted comment: synthetic signature",
        base64.b64encode(b"ED" + bytes([key_id]) * 8 + b"s" * 64).decode(),
        "trusted comment: synthetic fixture",
        base64.b64encode(b"c" * 64).decode(),
    ])


PUBLIC_KEY = record([
    "untrusted comment: synthetic public key",
    base64.b64encode(b"Ed" + bytes([7]) * 8 + b"p" * 32).decode(),
])
ARTIFACT_NAME = "LanGame Server Manager_0.1.0_x64-setup.exe"
PUBLIC_ARTIFACT_NAME = "LanGame.Server.Manager_0.1.0_x64-setup.exe"
URL = "https://github.com/SZSLGJCOM/LanGame-Server-Manager/releases/download/v0.1.0/LanGame.Server.Manager_0.1.0_x64-setup.exe"


class DesktopUpdateManifestTests(unittest.TestCase):
    def test_offline_cli_rejects_update_metadata_or_feed_output(self):
        import subprocess
        import sys
        from scripts.generate_desktop_update_manifest import REPOSITORY_ROOT
        with tempfile.TemporaryDirectory(prefix="langame-offline-cli-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic offline installer")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(), encoding="utf-8")
            config = directory / "tauri.conf.json"
            config.write_text(json.dumps({
                "productName": "LanGame Server Manager", "version": "0.1.0",
                "plugins": {"updater": {"pubkey": PUBLIC_KEY}},
            }), encoding="utf-8")
            base = [sys.executable, "-B", str(REPOSITORY_ROOT / "scripts/generate_desktop_update_manifest.py"),
                    "--version", "0.1.0", "--offline-installer", "--artifact-file", str(artifact),
                    "--signature-file", str(sig), "--tauri-config", str(config)]
            for extra in (["--artifact-url", URL], ["--notes", "update notes"],
                          ["--pub-date", "2026-10-08T00:00:00Z"], []):
                with self.subTest(extra=extra):
                    output = directory / "release" / ("desktop-offline-artifacts.json" if extra else "latest.json")
                    result = subprocess.run([*base, "--output", str(output), *extra],
                                            capture_output=True, text=True, check=False, timeout=10)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("offline export", result.stderr)
                    self.assertFalse(output.parent.exists())

    def test_offline_export_has_separate_names_and_preserves_main_feed(self):
        with tempfile.TemporaryDirectory(prefix="langame-offline-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic offline installer")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(), encoding="utf-8")
            output = directory / "release" / "desktop-offline-artifacts.json"
            output.parent.mkdir()
            main_feed = output.parent / "latest.json"
            main_feed.write_bytes(b"retained small-installer feed")
            main_checksums = output.parent / "SHA256SUMS"
            main_checksums.write_bytes(b"retained small-installer hashes")
            write_outputs(output, {"version": "0.1.0", "signature": signature()}, artifact, sig, offline=True)
            summary = json.loads(output.read_text())
            name = "LanGame.Server.Manager_0.1.0_x64-offline-setup.exe"
            self.assertEqual([asset["name"] for asset in summary["artifacts"]], [name, name + ".sig"])
            self.assertNotIn("platforms", summary)
            self.assertEqual((output.parent / name).read_bytes(), artifact.read_bytes())
            self.assertEqual((output.parent / (name + ".sig")).read_bytes(), sig.read_bytes())
            for asset in summary["artifacts"]:
                self.assertEqual(asset["sha256"], hashlib.sha256((output.parent / asset["name"]).read_bytes()).hexdigest())
            self.assertEqual(main_feed.read_bytes(), b"retained small-installer feed")
            self.assertEqual(main_checksums.read_bytes(), b"retained small-installer hashes")
            original_summary = output.read_bytes()
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                write_outputs(output, {"version": "0.1.0", "signature": signature()}, artifact, sig, offline=True)
            self.assertEqual(output.read_bytes(), original_summary)

    def test_offline_export_rejects_feed_output_and_signature_mutation(self):
        with tempfile.TemporaryDirectory(prefix="langame-offline-reject-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic offline installer")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(8), encoding="utf-8")
            manifest = {"version": "0.1.0", "signature": signature()}
            with self.assertRaisesRegex(ValueError, "desktop-offline-artifacts.json"):
                write_outputs(directory / "release" / "latest.json", manifest, artifact, sig, offline=True)
            self.assertFalse((directory / "release").exists())
            with self.assertRaisesRegex(ValueError, "signature changed after validation"):
                write_outputs(directory / "release" / "desktop-offline-artifacts.json", manifest, artifact, sig, offline=True)
            self.assertEqual(list((directory / "release").iterdir()), [])

    def test_export_uses_github_safe_names_without_changing_signed_bytes(self):
        with tempfile.TemporaryDirectory(prefix="langame-release-names-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic installer fixture")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_bytes(("\ufeff" + signature() + "\r\n").encode("utf-8"))
            public_url = URL.rsplit("/", 1)[0] + "/" + PUBLIC_ARTIFACT_NAME
            manifest = {"version": "0.1.0", "platforms": {"windows-x86_64": {
                "signature": signature(), "url": public_url,
            }}}
            output = directory / "release" / "latest.json"

            write_outputs(output, manifest, artifact, sig)

            self.assertTrue((output.parent / PUBLIC_ARTIFACT_NAME).is_file())
            self.assertEqual((output.parent / PUBLIC_ARTIFACT_NAME).read_bytes(), artifact.read_bytes())
            self.assertEqual((output.parent / (PUBLIC_ARTIFACT_NAME + ".sig")).read_bytes(), sig.read_bytes())
            summary = json.loads((output.parent / "desktop-update-artifacts.json").read_text())
            self.assertEqual({asset["name"] for asset in summary["artifacts"]},
                             {PUBLIC_ARTIFACT_NAME, PUBLIC_ARTIFACT_NAME + ".sig", "latest.json"})
            self.assertFalse((output.parent / ARTIFACT_NAME).exists())
            self.assertEqual(json.loads(output.read_text())["platforms"]["windows-x86_64"]["url"], public_url)

    def test_requires_stable_version(self):
        self.assertEqual(validate_version("0.1.0"), "0.1.0")
        for value in ("v0.1.0", "1.2", "01.2.3", "1.2.3-beta.1", "1.2.3+build", "1.2.3\n"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate_version(value)

    def test_public_names_are_deterministic_and_reject_unsafe_characters(self):
        self.assertEqual(public_artifact_name(ARTIFACT_NAME), PUBLIC_ARTIFACT_NAME)
        self.assertEqual(public_artifact_name(PUBLIC_ARTIFACT_NAME), PUBLIC_ARTIFACT_NAME)
        for name in ("../setup.exe", "nested/setup.exe", "nested\\setup.exe", ".setup.exe",
                     "setup.exe.", "setup\tname.exe", "setup%20name.exe", "安装.exe", ""):
            with self.subTest(name=name), self.assertRaises(ValueError):
                public_artifact_name(name)

    def test_artifact_url_must_be_https_and_exact_version_filename(self):
        self.assertEqual(validate_artifact_url(URL, PUBLIC_ARTIFACT_NAME), URL)
        for value in (
            URL.replace("https:", "http:"), URL.replace("github.com", "user:pass@github.com"),
            URL + "?token=secret", URL + "#fragment", URL.replace("0.1.0_x64", "0.0.1_x64"),
            URL.replace("/releases/", "/%2e%2e/"), URL.replace("LanGame.Server.Manager", "LanGame Server Manager"),
            URL.replace("LanGame.Server.Manager", "LanGame%20Server%20Manager"),
            URL.replace("github.com/", "github.com\\"),
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate_artifact_url(value, PUBLIC_ARTIFACT_NAME)

    def test_export_rejects_preupload_url_and_keeps_existing_public_asset(self):
        with tempfile.TemporaryDirectory(prefix="langame-release-url-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic installer fixture")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(), encoding="utf-8")
            output = directory / "release" / "latest.json"
            manifest = {"version": "0.1.0", "platforms": {"windows-x86_64": {
                "signature": signature(),
                "url": URL.replace("LanGame.Server.Manager", "LanGame%20Server%20Manager"),
            }}}
            with self.assertRaisesRegex(ValueError, "URL filename must match"):
                write_outputs(output, manifest, artifact, sig)
            self.assertFalse(output.parent.exists())

            output.parent.mkdir()
            retained = output.parent / PUBLIC_ARTIFACT_NAME
            retained.write_bytes(b"existing release")
            manifest["platforms"]["windows-x86_64"]["url"] = URL
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                write_outputs(output, manifest, artifact, sig)
            self.assertEqual(retained.read_bytes(), b"existing release")
            self.assertEqual(list(output.parent.iterdir()), [retained])

    def test_signature_accepts_windows_bom_and_rejects_malformed_or_other_key(self):
        self.assertEqual(validate_signature("\ufeff" + signature() + "\r\n", PUBLIC_KEY), signature())
        for value in ("", "not a signature", signature(8), record(["untrusted comment: incomplete"])):
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate_signature(value, PUBLIC_KEY)

    def test_pub_date_requires_timezone_and_normalizes_to_utc(self):
        self.assertEqual(validate_pub_date("2026-09-30T08:00:00+08:00"), "2026-09-30T00:00:00Z")
        for value in ("2026-09-30", "2026-09-30T08:00:00", "invalid"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate_pub_date(value)

    def test_export_preserves_existing_files_and_hashes_copied_bytes(self):
        with tempfile.TemporaryDirectory(prefix="langame-manifest-") as temporary:
            directory = Path(temporary)
            source = directory / "source"
            source.mkdir()
            artifact = source / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic installer fixture")
            sig = source / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(), encoding="utf-8")
            output = directory / "release" / "latest.json"
            manifest = {"version": "0.1.0", "platforms": {"windows-x86_64": {
                "signature": signature(), "url": URL,
            }}}
            write_outputs(output, manifest, artifact, sig)
            summary = json.loads((output.parent / "desktop-update-artifacts.json").read_text())
            for asset in summary["artifacts"]:
                data = (output.parent / asset["name"]).read_bytes()
                self.assertEqual(asset["sha256"], hashlib.sha256(data).hexdigest())
            original_manifest = output.read_bytes()
            with self.assertRaisesRegex(ValueError, "refusing to overwrite"):
                write_outputs(output, manifest, artifact, sig)
            self.assertEqual(output.read_bytes(), original_manifest)

    def test_signature_change_after_validation_removes_exported_files(self):
        with tempfile.TemporaryDirectory(prefix="langame-signature-race-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"synthetic installer fixture")
            sig = directory / (ARTIFACT_NAME + ".sig")
            validated_signature = signature()
            manifest = {"version": "0.1.0", "platforms": {"windows-x86_64": {
                "signature": validated_signature, "url": URL,
            }}}
            # A competing writer replaces the signature after validation.
            sig.write_text(signature(8), encoding="utf-8")
            output = directory / "release" / "latest.json"
            with self.assertRaisesRegex(ValueError, "signature changed after validation"):
                write_outputs(output, manifest, artifact, sig)
            self.assertEqual(list(output.parent.iterdir()), [])
            self.assertTrue(artifact.is_file())
            self.assertTrue(sig.is_file())

    def test_copy_rejects_input_mutation_and_preserves_unrelated_output(self):
        import shutil
        from unittest.mock import patch
        with tempfile.TemporaryDirectory(prefix="langame-copy-race-") as temporary:
            directory = Path(temporary)
            artifact = directory / ARTIFACT_NAME
            artifact.write_bytes(b"original synthetic installer")
            sig = directory / (ARTIFACT_NAME + ".sig")
            sig.write_text(signature(), encoding="utf-8")
            output = directory / "release" / "latest.json"
            output.parent.mkdir()
            retained = output.parent / "operator-notes.txt"
            retained.write_text("keep this", encoding="utf-8")
            manifest = {"version": "0.1.0", "platforms": {"windows-x86_64": {
                "signature": signature(), "url": URL,
            }}}
            copy_file = shutil.copyfileobj

            def concurrent_write(source, target, length):
                if Path(source.name) == artifact:
                    artifact.write_bytes(b"changed synthetic installer")
                copy_file(source, target, length)

            with patch("scripts.generate_desktop_update_manifest.shutil.copyfileobj", concurrent_write):
                with self.assertRaisesRegex(ValueError, "input changed while copying"):
                    write_outputs(output, manifest, artifact, sig)
            self.assertEqual(list(output.parent.iterdir()), [retained])
            self.assertEqual(retained.read_text(encoding="utf-8"), "keep this")

    def test_refuses_output_within_repository(self):
        from scripts.generate_desktop_update_manifest import REPOSITORY_ROOT
        with self.assertRaisesRegex(ValueError, "outside the source repository"):
            write_outputs(REPOSITORY_ROOT / "latest.json", {}, None, Path("unused.sig"))


if __name__ == "__main__":
    unittest.main()
