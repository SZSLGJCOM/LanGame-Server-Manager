"""Exercise the real PowerShell archive resolver against a local HTTP server."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading
import unittest


ROOT = Path(__file__).resolve().parents[2]
BUILDER = ROOT / "scripts/build_ark_tools.ps1"
ARCHIVE = "AsaApi-sdk.zip"
CURRENT = b"pinned SDK fixture"
SHA256 = hashlib.sha256(CURRENT).hexdigest()


def quote(value):
    return "'" + str(value).replace("'", "''") + "'"


@unittest.skipUnless(os.name == "nt", "Requires Windows PowerShell")
class ArkBuildArchiveTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="lgsm-ark-archives-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.output = self.root / "output"
        self.cache = self.root / "synchronized-inputs"
        self.output.mkdir()
        self.cache.mkdir()
        self.requests = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                owner.requests.append(self.path)
                body = b"wrong SDK" if self.path == "/corrupt" else CURRENT
                self.send_response(503 if self.path == "/unavailable" else 200)
                self.send_header("Content-Length", str(64 * 1024 * 1024 + 1 if self.path == "/oversized" else len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.close_server)
        self.url = f"http://127.0.0.1:{self.server.server_port}"

    def close_server(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)

    def resolve(self, *, offline=False, endpoint="/archive", cache=True):
        wrapper = self.root / "resolve.ps1"
        wrapper.write_text(
            f"""$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Net.Http
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile({quote(BUILDER)}, [ref]$tokens, [ref]$errors)
if ($errors.Count) {{ throw ($errors | Out-String) }}
foreach ($name in @('Get-ArchiveSha256', 'Get-VerifiedArchive')) {{
    $function = $ast.Find({{ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }}, $true)
    if ($null -eq $function) {{ throw "Missing production function: $name" }}
    Invoke-Expression $function.Extent.Text
}}
$archiveRoot = {quote(self.cache) if offline else '$null'}
$archiveCacheRoot = {quote(self.cache) if cache else '$null'}
$outputRoot = {quote(self.output)}
$resolved = Get-VerifiedArchive {quote(ARCHIVE)} {quote(self.url + endpoint)} {quote(SHA256)}
Write-Output ('RESULT:' + (ConvertTo-Json -Compress -InputObject $resolved))
""",
            encoding="utf-8-sig",
        )
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(wrapper)],
            cwd=ROOT, capture_output=True, text=True, timeout=20, check=False,
        )
        return result

    def assert_resolved(self, result, expected):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        outputs = [line.removeprefix("RESULT:") for line in result.stdout.splitlines() if line.startswith("RESULT:")]
        self.assertEqual(len(outputs), 1, result.stdout)
        self.assertEqual(Path(json.loads(outputs[0])), expected)
        self.assertEqual(expected.read_bytes(), CURRENT)

    def test_old_synchronized_sdk_downloads_pinned_version_once(self):
        original = self.cache / ARCHIVE
        original.write_bytes(b"old SDK commit")
        self.assert_resolved(self.resolve(), self.output / ARCHIVE)
        self.assert_resolved(self.resolve(), self.output / ARCHIVE)
        self.assertEqual(self.requests, ["/archive"])
        self.assertEqual(original.read_bytes(), b"old SDK commit")

    def test_valid_synchronized_sdk_reuses_without_network_or_output(self):
        archive = self.cache / ARCHIVE
        archive.write_bytes(CURRENT)
        self.assert_resolved(self.resolve(), archive)
        self.assertEqual(self.requests, [])
        self.assertEqual(list(self.output.iterdir()), [])

    def test_missing_synchronized_sdk_downloads_current_version(self):
        self.assert_resolved(self.resolve(), self.output / ARCHIVE)
        self.assertEqual(self.requests, ["/archive"])
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_stale_build_cache_is_replaced_after_verification(self):
        archive = self.output / ARCHIVE
        archive.write_bytes(b"previous pinned SDK")
        self.assert_resolved(self.resolve(cache=False), archive)
        self.assertEqual(self.requests, ["/archive"])
        self.assertFalse((self.output / (ARCHIVE + ".download")).exists())

    def test_failed_downloads_preserve_old_cache_and_remove_partial(self):
        archive = self.output / ARCHIVE
        for endpoint, error in [("/corrupt", "SHA-256 mismatch"), ("/unavailable", "503"), ("/oversized", "exceeds 64 MiB")]:
            with self.subTest(endpoint=endpoint):
                archive.write_bytes(b"previous SDK")
                result = self.resolve(endpoint=endpoint)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(error, result.stderr)
                self.assertEqual(archive.read_bytes(), b"previous SDK")
                self.assertFalse((self.output / (ARCHIVE + ".download")).exists())

    def test_explicit_offline_input_stays_strict_even_with_good_build_cache(self):
        (self.output / ARCHIVE).write_bytes(CURRENT)
        for content in [None, b"old offline SDK"]:
            with self.subTest(content=content):
                if content is not None:
                    (self.cache / ARCHIVE).write_bytes(content)
                result = self.resolve(offline=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Missing ARK archive" if content is None else "SHA-256 mismatch", result.stderr)
        self.assertEqual(self.requests, [])

    def test_valid_offline_input_reuses_without_network(self):
        archive = self.cache / ARCHIVE
        archive.write_bytes(CURRENT)
        self.assert_resolved(self.resolve(offline=True), archive)
        self.assertEqual(self.requests, [])

    def test_real_entrypoint_rejects_output_inside_archive_cache(self):
        environment = dict(os.environ, LANGAME_ARK_ARCHIVE_CACHE_DIR=str(self.cache))
        environment.pop("LANGAME_ARK_ARCHIVE_DIR", None)
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(BUILDER),
             "-OutDir", str(self.cache / "output")],
            env=environment, capture_output=True, text=True, timeout=20, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("outside the read-only archive cache", result.stderr)
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_real_entrypoint_offline_input_takes_precedence_over_missing_cache(self):
        environment = dict(os.environ, LANGAME_ARK_ARCHIVE_DIR=str(self.cache),
                           LANGAME_ARK_ARCHIVE_CACHE_DIR=str(self.root / "absent-cache"))
        result = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", str(BUILDER),
             "-OutDir", str(self.output)],
            env=environment, capture_output=True, text=True, timeout=20, check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing ARK archive in LANGAME_ARK_ARCHIVE_DIR", result.stderr)
        self.assertNotIn("LANGAME_ARK_ARCHIVE_CACHE_DIR must", result.stderr)
        self.assertEqual(self.requests, [])


if __name__ == "__main__":
    unittest.main()
