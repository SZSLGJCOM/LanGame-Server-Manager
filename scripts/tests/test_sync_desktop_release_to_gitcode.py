"""Local HTTP/transport fixtures; cryptographic interoperability has separate tests."""
import base64
import contextlib
from copy import deepcopy
import hashlib
import http.server
import io
import json
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import MagicMock, patch
import urllib.parse

from scripts import sync_desktop_release_to_gitcode as sync

PUBLIC_FIXTURE = json.loads((Path(__file__).parent / "fixtures/desktop_update_signature.json").read_text())


def digest(data):
    return hashlib.sha256(data).hexdigest()


class FixtureTransport:
    def __init__(self):
        self.tag = "v1.2.3"
        self.version = "1.2.3"
        self.names = sync.asset_names(self.version)
        self.files = {n: ("fixture " + n).encode() for n in self.names}
        for name in (self.names[1], self.names[3]):
            self.files[name] = PUBLIC_FIXTURE["signature"].encode()
        self.manifest = {"version": self.version, "notes": "Fixture release", "platforms": {"windows-x86_64": {
            "url": f"https://github.com/{sync.GITHUB_REPO}/releases/download/{self.tag}/{self.names[0]}",
            "signature": self.files[self.names[1]].decode()}}}
        self.files["latest.json"] = (json.dumps(self.manifest, indent=2) + "\n").encode()
        for checksum, names in (("SHA256SUMS", (self.names[0], self.names[1], "latest.json")),
                                ("SHA256SUMS.offline", self.names[2:4])):
            self.files[checksum] = "".join(f"{digest(self.files[n])}  {n}\n" for n in names).encode()
        self.release = {"id": 123, "tag_name": self.tag, "draft": False, "prerelease": False, "assets": [
            {"id": i + 1, "name": n, "size": len(b), "digest": "sha256:" + digest(b), "state": "uploaded",
             "browser_download_url": f"https://github.com/{sync.GITHUB_REPO}/releases/download/{self.tag}/{n}"}
            for i, (n, b) in enumerate(self.files.items())]}
        self.remote = None
        self.remote_bytes = {}
        self.pointer = None
        self.calls = []
        self.fail_upload = None

    def remote_url(self, name):
        return f"{sync.GC_API}/releases/{self.tag}/attach_files/{name}/download"

    def api(self, provider, method, suffix, payload=None, missing_ok=False, timeout=60):
        self.calls.append((provider, method, suffix, deepcopy(payload)))
        if provider == "github":
            assert method == "GET"
            return deepcopy(self.release)
        if suffix.startswith("/contents/"):
            if method == "GET":
                if self.pointer is None:
                    return None
                raw = (json.dumps(self.pointer) + "\n").encode()
                return {"type": "file", "encoding": "base64", "path": sync.POINTER,
                        "sha": "a" * 40, "content": base64.b64encode(raw).decode()}
            self.pointer = json.loads(base64.b64decode(payload["content"]))
            return {"commit": {"sha": "b" * 40}}
        if suffix.startswith("/releases/tags/"):
            return deepcopy(self.remote)
        if suffix == "/releases" and method == "POST":
            self.remote = {**payload, "assets": []}
            return deepcopy(self.remote)
        if "/upload_url?" in suffix:
            name = urllib.parse.parse_qs(urllib.parse.urlsplit(suffix).query)["file_name"][0]
            return {"fixture_name": name}
        if method == "PATCH":
            self.remote.update(payload)
            return deepcopy(self.remote)
        raise AssertionError("Unexpected transport operation")

    def download(self, url, destination, size, sha256, provider):
        name = url.split("/")[-2] if provider == "gitcode" else url.rsplit("/", 1)[-1]
        value = self.files[name] if provider == "github" else self.remote_bytes[name]
        sync.require(len(value) == size and digest(value) == sha256, "Remote artifact mismatch")
        destination.write_bytes(value)

    def upload(self, ticket, source):
        name = ticket["fixture_name"]
        if name == self.fail_upload:
            raise sync.SyncError("Unknown upload result")
        self.remote_bytes[name] = source.read_bytes()
        self.remote["assets"].append({"name": name, "browser_download_url": self.remote_url(name)})


class ReleaseSyncTests(unittest.TestCase):
    def setUp(self):
        transport = patch.dict(sync.os.environ, {"LGSM_GITCODE_UPLOAD_TRANSPORT": "urllib"})
        transport.start()
        self.addCleanup(transport.stop)
        output = contextlib.redirect_stdout(io.StringIO())
        output.__enter__()
        self.addCleanup(output.__exit__, None, None, None)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.client = FixtureTransport()
        # Only the signature-verifier subprocess boundary is replaced here.
        # Real minisign and trusted-comment checks run in the Node tests/live check.
        self.crypto = patch.object(sync, "verify_signature")
        self.verifier = self.crypto.start()
        self.addCleanup(self.crypto.stop)
        self.key = patch.object(sync, "public_key", return_value=PUBLIC_FIXTURE["pubkey"])
        self.key.start()
        self.addCleanup(self.key.stop)

    def run_sync(self, execute=True, name="run"):
        return sync.synchronize(self.client, "v1.2.3", 123, self.root / name, ["node"], execute)

    def test_inspection_downloads_and_validates_without_any_gitcode_request(self):
        result = self.run_sync(False)
        self.assertFalse(result["published"])
        self.assertEqual(self.verifier.call_count, 2)
        self.assertFalse(any(c[0] == "gitcode" for c in self.client.calls))

    def test_complete_verified_pointer_precedes_latest_status(self):
        self.assertTrue(self.run_sync()["published"])
        self.assertEqual(self.verifier.call_count, 4)
        self.assertEqual(len(self.client.remote_bytes), 7)
        pointer = self.client.pointer
        self.assertEqual(pointer["manifest_text"].encode(), self.client.files["latest.json"])
        self.assertEqual(pointer["manifest"], json.loads(pointer["manifest_text"]))
        writes = [c for c in self.client.calls if c[1] != "GET"]
        self.assertEqual(writes[0][3]["release_status"], "pre")
        self.assertTrue(writes[-2][2].startswith("/contents/"))
        self.assertEqual(writes[-1][1], "PATCH")

    def test_second_new_workspace_run_verifies_without_upload_or_pointer_rewrite(self):
        self.run_sync()
        self.client.calls.clear()
        self.assertEqual(self.run_sync(name="second")["pointer"], "unchanged")
        self.assertFalse(any(c[1] != "GET" for c in self.client.calls))

    def test_uploads_small_assets_first_without_changing_canonical_pointer_order(self):
        self.run_sync()
        names = [urllib.parse.parse_qs(urllib.parse.urlsplit(c[2]).query)["file_name"][0]
                 for c in self.client.calls if "/upload_url?" in c[2]]
        self.assertEqual(names, sorted(self.client.names, key=lambda name: (len(self.client.files[name]), name)))
        self.assertEqual([asset["name"] for asset in self.client.pointer["assets"]], list(self.client.names))

    def test_interrupted_upload_keeps_previous_pointer_and_can_resume(self):
        self.client.fail_upload = self.client.names[2]
        with self.assertRaisesRegex(sync.SyncError, "Unknown upload"):
            self.run_sync()
        self.assertIsNone(self.client.pointer)
        self.assertEqual(self.client.remote["release_status"], "pre")
        already_uploaded = set(self.client.remote_bytes)
        self.client.fail_upload = None
        self.client.calls.clear()
        self.run_sync(name="resume")
        uploads = [c for c in self.client.calls if "/upload_url?" in c[2]]
        uploaded_names = {urllib.parse.parse_qs(urllib.parse.urlsplit(c[2]).query)["file_name"][0] for c in uploads}
        self.assertEqual(uploaded_names, set(self.client.names) - already_uploaded)
        self.assertEqual(len(uploads), len(uploaded_names))

    def test_delayed_upload_callback_uses_only_bounded_read_probes(self):
        self.run_sync()
        original = self.client.api
        reads = 0
        name = self.client.names[0]
        def delayed(*args, **kwargs):
            nonlocal reads
            reads += 1
            value = original(*args, **kwargs)
            if reads < 3:
                value["assets"] = [a for a in value["assets"] if a["name"] != name]
            return value
        self.client.api = delayed
        self.client.calls.clear()
        with patch.object(sync.time, "sleep") as sleep:
            result = sync.wait_for_attachment(self.client, "/releases/tags/v1.2.3", "v1.2.3", name)
        self.assertIn(name, sync.gitcode_assets(result, "v1.2.3"))
        self.assertEqual([c.args[0] for c in sleep.call_args_list], [2, 4])
        self.assertTrue(all(c[1] == "GET" for c in self.client.calls))

    def test_missing_callback_expires_without_replaying_upload(self):
        self.run_sync()
        self.client.remote["assets"].clear()
        self.client.calls.clear()
        clock = [0.0]
        def sleep(seconds):
            clock[0] += seconds
        with patch.object(sync.time, "monotonic", side_effect=lambda: clock[0]), patch.object(sync.time, "sleep", side_effect=sleep):
            with self.assertRaisesRegex(sync.SyncError, "30 seconds"):
                sync.wait_for_attachment(self.client, "/releases/tags/v1.2.3", "v1.2.3", self.client.names[0])
        self.assertEqual(clock[0], 30)
        self.assertEqual(len(self.client.calls), 5)
        self.assertTrue(all(c[1] == "GET" for c in self.client.calls))

    def test_same_name_remote_conflicting_bytes_are_never_replaced(self):
        self.run_sync()
        self.client.remote_bytes[self.client.names[0]] = b"corrupt"
        self.client.calls.clear()
        with self.assertRaisesRegex(sync.SyncError, "Remote artifact"):
            self.run_sync(name="conflict")
        self.assertFalse(any(c[1] != "GET" for c in self.client.calls))

    def test_signature_failure_prevents_any_gitcode_write(self):
        self.verifier.side_effect = sync.SyncError("Invalid signature")
        with self.assertRaisesRegex(sync.SyncError, "signature"):
            self.run_sync()
        self.assertFalse(any(c[0] == "gitcode" for c in self.client.calls))

    def test_version_and_release_id_are_frozen(self):
        for key, value in (("id", 124), ("tag_name", "v2.0.0"), ("draft", True), ("prerelease", True)):
            with self.subTest(key=key):
                release = deepcopy(self.client.release)
                release[key] = value
                with self.assertRaises(sync.SyncError):
                    sync.freeze_release(release, "v1.2.3", 123)

    def test_asset_shape_digest_and_duplicate_identity_are_rejected(self):
        for kind in ("extra", "digest", "id", "path"):
            release = deepcopy(self.client.release)
            if kind == "extra":
                release["assets"].append(deepcopy(release["assets"][0]))
            elif kind == "digest":
                release["assets"][0]["digest"] = None
            elif kind == "id":
                release["assets"][0]["id"] = release["assets"][1]["id"]
            else:
                release["assets"][0]["browser_download_url"] += "?token=hidden"
            with self.subTest(kind=kind), self.assertRaises((sync.SyncError, TypeError)):
                sync.freeze_release(release, "v1.2.3", 123)

    def test_checksums_must_bind_every_file_exactly_once(self):
        file = self.root / "sums"
        file.write_text("a" * 64 + "  file.exe\n" + "a" * 64 + "  file.exe\n")
        with self.assertRaisesRegex(sync.SyncError, "duplicated"):
            sync.checksums(file, ["file.exe"], {"file.exe": {"sha256": "a" * 64}})

    def test_small_installer_matches_client_limit_while_offline_can_be_larger(self):
        release = deepcopy(self.client.release)
        indexed = {a["name"]: a for a in release["assets"]}
        indexed[self.client.names[2]]["size"] = 300 * 1024 * 1024
        sync.freeze_release(release, "v1.2.3", 123)
        indexed[self.client.names[0]]["size"] = 256 * 1024 * 1024 + 1
        with self.assertRaisesRegex(sync.SyncError, "256 MiB"):
            sync.freeze_release(release, "v1.2.3", 123)

    def test_pointer_cannot_downgrade_or_mutate_same_version(self):
        self.run_sync()
        pointer = deepcopy(self.client.pointer)
        for version in ("1.2.2", "1.2.3"):
            pointer["version"] = version
            pointer["manifest"]["notes"] = "Changed"
            with self.subTest(version=version), self.assertRaisesRegex(sync.SyncError, "downgrade"):
                sync.publish_pointer(self.client, pointer)

    def test_pointer_update_uses_blob_sha(self):
        self.run_sync()
        pointer = deepcopy(self.client.pointer)
        pointer["version"] = "1.2.4"
        pointer["github"]["tag"] = "v1.2.4"
        self.assertEqual(sync.publish_pointer(self.client, pointer), "published")
        put = [c for c in self.client.calls if c[1] == "PUT"][-1]
        self.assertEqual(put[3]["sha"], "a" * 40)

    def test_public_redirects_reject_credentials_and_unrelated_origins(self):
        credential_url = urllib.parse.urlunsplit(("https", ":".join(("user", "synthetic")) + "@github.com", "/file", "", ""))
        for url in ("http://github.com/file", credential_url, "https://github.com.evil.test/file", "https://127.0.0.1/file"):
            with self.subTest(url=url), self.assertRaises(sync.SyncError):
                sync.public_url(url, "github")

    def test_authenticated_api_never_follows_redirects(self):
        with self.assertRaisesRegex(sync.SyncError, "redirected"):
            sync.NoRedirect().redirect_request(None, None, 302, "", {}, "https://other.test")

    def test_token_is_only_in_header_not_query(self):
        response = MagicMock()
        response.__enter__.return_value.read.return_value = b'{"ok":true}'
        opener = MagicMock()
        opener.open.return_value = response
        with patch.object(sync.urllib.request, "build_opener", return_value=opener):
            self.assertEqual(sync.Transport(gitcode_token="fixture-secret").api("gitcode", "GET", "/releases/tags/v1.2.3"), {"ok": True})
        request = opener.open.call_args.args[0]
        self.assertEqual(request.get_header("Authorization"), "Bearer fixture-secret")
        self.assertNotIn("fixture-secret", request.full_url)
        self.assertNotIn("access_token", request.full_url)

    def test_provider_routes_keep_github_proxy_and_bypass_it_for_gitcode(self):
        for provider in ("github", "gitcode"):
            with self.subTest(provider=provider):
                response = MagicMock()
                response.__enter__.return_value.read.return_value = b'{}'
                with patch.object(sync.urllib.request, "build_opener") as build:
                    build.return_value.open.return_value = response
                    sync.Transport().api(provider, "GET", "/releases/latest")
                    proxies = [handler for handler in build.call_args.args
                               if isinstance(handler, sync.urllib.request.ProxyHandler)]
                    self.assertEqual([handler.proxies for handler in proxies], [{}] if provider == "gitcode" else [])
                response.__enter__.return_value.status = 200
                response.__enter__.return_value.read.side_effect = [b"fixture", b""]
                with patch.object(sync.urllib.request, "build_opener") as build:
                    build.return_value.open.return_value = response
                    sync.Transport().download(f"https://{provider}.com/fixture", self.root / (provider + ".bin"),
                                              7, hashlib.sha256(b"fixture").hexdigest(), provider)
                    proxies = [handler for handler in build.call_args.args
                               if isinstance(handler, sync.urllib.request.ProxyHandler)]
                    self.assertEqual([handler.proxies for handler in proxies], [{}] if provider == "gitcode" else [])

    def test_unrelated_upload_origin_or_credential_headers_are_rejected(self):
        client = sync.Transport(gitcode_token="fixture-secret")
        for ticket in ({"url": "https://other.test/upload", "headers": {"Content-Type": "application/octet-stream"}},
                       {"url": "https://bucket.obs.cn-north-4.myhuaweicloud.com/file", "headers": {"Authorization": "secret"}},
                       {"url": "https://bucket.obs.cn-north-4.myhuaweicloud.com/file", "headers": {"Transfer-Encoding": "chunked"}},
                       {"url": "https://bucket.obs.cn-north-4.myhuaweicloud.com/file", "headers": {"Content-Length": "1"}}):
            with self.subTest(ticket=ticket), self.assertRaises(sync.SyncError):
                client.upload(ticket, self.root / "nonexistent")

    def test_upload_stream_preserves_bytes_headers_and_bounds_socket_write(self):
        source = self.root / "upload.bin"
        source.write_bytes(b"bounded upload fixture" * 65536)
        ticket = {"url": "https://bucket.obs.cn-north-4.myhuaweicloud.com/file",
                  "headers": {"Content-Type": "application/octet-stream", "x-obs-callback": "opaque-fixture"}}
        response = MagicMock()
        response.__enter__.return_value.status = 200
        opener = MagicMock()
        bodies = []
        def consume(request, timeout):
            self.assertEqual(timeout, 60)
            self.assertEqual(request.get_method(), "PUT")
            self.assertEqual(request.get_header("Content-length"), str(source.stat().st_size))
            self.assertEqual(request.get_header("Content-type"), ticket["headers"]["Content-Type"])
            self.assertEqual(request.get_header("X-obs-callback"), "opaque-fixture")
            self.assertIsNone(request.get_header("Transfer-encoding"))
            chunks = list(request.data)
            self.assertEqual(len(chunks[0]), 1024 * 1024)
            self.assertTrue(all(len(chunk) <= 1024 * 1024 for chunk in chunks))
            bodies.append(b"".join(chunks))
            return response
        opener.open.side_effect = consume
        logs = io.StringIO()
        with patch.object(sync.urllib.request, "build_opener", return_value=opener) as build, contextlib.redirect_stdout(logs):
            sync.Transport().upload(ticket, source)
        self.assertEqual(build.call_args.args[0].proxies, {})
        self.assertEqual(bodies, [source.read_bytes()])
        self.assertEqual(opener.open.call_count, 1)
        self.assertNotIn("opaque-fixture", logs.getvalue())
        self.assertNotIn(ticket["url"], logs.getvalue())

    def test_upload_body_deadline_does_not_retry_or_report_acknowledgement(self):
        with patch.object(sync.time, "monotonic", side_effect=[0, 1801]):
            body = sync.UploadBody(io.BytesIO(b"not read"), 8)
            with self.assertRaisesRegex(sync.SyncError, "remote outcome must be inspected"):
                body.read(8)
        self.assertEqual(body.read_bytes, 0)

    def test_release_assets_change_prevents_publication(self):
        original = self.client.api
        count = 0
        def changed(provider, method, suffix, *args, **kwargs):
            nonlocal count
            value = original(provider, method, suffix, *args, **kwargs)
            if provider == "github":
                count += 1
                if count > 1:
                    value["assets"][0]["digest"] = "sha256:" + "f" * 64
            return value
        self.client.api = changed
        with self.assertRaisesRegex(sync.SyncError, "changed"):
            self.run_sync()
        self.assertIsNone(self.client.remote)


class CurlUploadTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "upload fixture.bin"
        self.source.write_bytes(b"signed artifact fixture\x00" * 8192)
        self.size = self.source.stat().st_size
        self.opaque_header = 'opaque "quote" \\slash callback'
        self.url = "https://bucket.obs.cn-north-4.myhuaweicloud.com/file?signed=fixture-secret"
        self.headers = {"Content-Type": "application/octet-stream", "x-obs-callback": self.opaque_header}

    def statistics(self, **changes):
        stats = {"http_code": "201", "size_upload": self.size, "speed_upload": 123.5,
                 **{name: 0.1 for name in sync.CURL_TIMINGS}}
        stats.update(changes)
        return json.dumps(stats).encode()

    def test_production_command_hides_ticket_and_has_fixed_transfer_limits(self):
        completed = subprocess.CompletedProcess([], 0, self.statistics(), b"stderr must not be printed")
        output = io.StringIO()
        with patch.object(sync.subprocess, "run", return_value=completed) as run, contextlib.redirect_stdout(output):
            sync._curl_upload(self.url, self.headers, self.source, self.size)
        args = run.call_args.args[0]
        self.assertEqual(args[:4], ["curl", "-q", "--config", "-"])
        for flag, value in {"--proto": "=https", "--proto-redir": "=https", "--retry": "0", "--noproxy": "*",
                            "--max-redirs": "0", "--connect-timeout": "15", "--max-time": "1800",
                            "--speed-limit": "32768", "--speed-time": "60"}.items():
            self.assertEqual(args[args.index(flag) + 1], value)
        self.assertIn("--http1.1", args)
        self.assertIn("--globoff", args)
        self.assertNotIn("--location", args)
        self.assertNotIn("-L", args)
        self.assertNotIn(self.url, args)
        self.assertNotIn(self.opaque_header, args)
        self.assertEqual(run.call_args.kwargs["timeout"], 1815)
        self.assertTrue(run.call_args.kwargs["capture_output"])
        config = run.call_args.kwargs["input"].decode()
        self.assertIn('url = "' + self.url + '"\n', config)
        self.assertIn('header = "x-obs-callback: opaque \\"quote\\" \\\\slash callback"\n', config)
        self.assertIn(f'header = "Content-Length: {self.size}"\n', config)
        report = json.loads(output.getvalue())
        self.assertEqual(report["upload_host"], "bucket.obs.cn-north-4.myhuaweicloud.com")
        self.assertNotIn("fixture-secret", output.getvalue())
        self.assertNotIn(self.opaque_header, output.getvalue())
        self.assertNotIn("stderr", output.getvalue())
        self.assertEqual(run.call_count, 1)

    def test_statistics_are_strict_and_unknown_output_is_never_repeated(self):
        invalid = [self.statistics(size_upload=True), self.statistics(size_upload=1.5),
                   self.statistics(speed_upload=float("nan")), self.statistics(time_total=float("inf")),
                   self.statistics(time_connect=-1), self.statistics(http_code="200\nsecret"),
                   self.statistics(url_effective=self.url), b"secret response body", b"x" * 4097]
        for raw in invalid:
            with self.subTest(raw=raw[:35]):
                output = io.StringIO()
                with patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, raw, b"secret")) as run:
                    with contextlib.redirect_stdout(output), self.assertRaises(sync.SyncError) as error:
                        sync._curl_upload(self.url, self.headers, self.source, self.size)
                self.assertIn("unknown", str(error.exception))
                self.assertNotIn("secret", str(error.exception))
                self.assertEqual(output.getvalue(), "")
                self.assertEqual(run.call_count, 1)

    def test_failed_partial_or_non_success_response_is_unknown_without_retry(self):
        for code, raw in [(28, self.statistics()), (0, self.statistics(http_code="302")),
                          (0, self.statistics(size_upload=self.size - 1))]:
            with self.subTest(code=code), contextlib.redirect_stdout(io.StringIO()):
                with patch.object(sync.subprocess, "run", return_value=subprocess.CompletedProcess([], code, raw, b"secret")) as run:
                    with self.assertRaisesRegex(sync.SyncError, "outcome unknown"):
                        sync._curl_upload(self.url, self.headers, self.source, self.size)
                    self.assertEqual(run.call_count, 1)

    def test_curl_config_rejects_line_injection_and_transport_selection_is_closed(self):
        for value in ["https://good/\nurl=https://other/", "header\rsecret", "nul\x00value"]:
            with self.assertRaises(sync.SyncError):
                sync._curl_quote(value)
        for choice in ("urllib", "curl"):
            with patch.dict(sync.os.environ, {"LGSM_GITCODE_UPLOAD_TRANSPORT": choice}):
                self.assertEqual(sync.Transport().upload_transport, choice)
        with patch.dict(sync.os.environ, {"LGSM_GITCODE_UPLOAD_TRANSPORT": "shell"}):
            with self.assertRaises(sync.SyncError):
                sync.Transport()

    def test_production_rejects_http_and_forbidden_headers_before_starting_curl(self):
        tickets = [{"url": "http://127.0.0.1:12345/upload", "headers": self.headers}]
        for name in ("Host", "Transfer-Encoding", "Content-Length", "Authorization", "Proxy-Authorization"):
            tickets.append({"url": self.url, "headers": {**self.headers, name: "forbidden"}})
        tickets.extend([{"url": self.url, "headers": {"x-obs-callback": "value\r\nHost: other"}},
                        {"url": self.url, "headers": {"x-obs-callback": "a", "X-Obs-Callback": "b"}}])
        with patch.dict(sync.os.environ, {"LGSM_GITCODE_UPLOAD_TRANSPORT": "curl"}):
            with patch.object(sync.subprocess, "run") as run:
                for ticket in tickets:
                    with self.subTest(ticket=ticket), self.assertRaises(sync.SyncError):
                        sync.Transport().upload(ticket, self.source)
                run.assert_not_called()

    @contextlib.contextmanager
    def fixture(self, behavior):
        requests = []
        class Handler(http.server.BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def do_PUT(self):
                self.connection.settimeout(5)
                size = int(self.headers.get("Content-Length", "0"))
                body = self.rfile.read(size)
                requests.append({"method": self.command, "path": self.path, "headers": self.headers, "body": body})
                if behavior == "disconnect":
                    self.connection.shutdown(socket.SHUT_RDWR)
                    self.connection.close()
                    self.close_connection = True
                    return
                status = 302 if behavior == "redirect" else 201
                self.send_response(status)
                if status == 302:
                    self.send_header("Location", "/redirect-target")
                self.send_header("Content-Length", "20")
                self.send_header("Connection", "close")
                self.end_headers()
                self.wfile.write(b"secret response body")

            def log_message(self, *_args):
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        original_command = sync._curl_command
        def local_command(source):
            command = original_command(source)
            command[command.index("--proto") + 1] = "=http"
            return command
        try:
            # This test-only command seam admits only our loopback HTTP fixture.
            # The actual curl process, socket, headers and body remain real.
            with patch.object(sync, "_curl_command", side_effect=local_command):
                with patch.dict(sync.os.environ, {"NO_PROXY": "127.0.0.1", "no_proxy": "127.0.0.1"}):
                    yield f"http://127.0.0.1:{server.server_port}/upload?signature=fixture-secret", requests
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            self.assertFalse(thread.is_alive())

    def test_real_curl_put_preserves_body_length_headers_and_config_escaping(self):
        self.assertIsNotNone(shutil.which("curl"), "Tests require the runner-provided curl")
        output = io.StringIO()
        with self.fixture("success") as (url, requests), contextlib.redirect_stdout(output):
            sync._curl_upload(url, {**self.headers, "x-empty": ""}, self.source, self.size)
        self.assertEqual(len(requests), 1)
        received = requests[0]
        self.assertEqual(received["method"], "PUT")
        self.assertEqual(received["body"], self.source.read_bytes())
        self.assertEqual(received["headers"].get_all("Content-Length"), [str(self.size)])
        self.assertIsNone(received["headers"].get("Transfer-Encoding"))
        self.assertEqual(received["headers"]["x-obs-callback"], self.opaque_header)
        self.assertEqual(received["headers"]["x-empty"], "")
        self.assertEqual(json.loads(output.getvalue())["size_upload"], self.size)
        self.assertNotIn("secret", output.getvalue())
        self.assertNotIn("callback", output.getvalue())

    def test_real_curl_does_not_follow_redirect_or_replay_disconnected_upload(self):
        for behavior in ("redirect", "disconnect"):
            with self.subTest(behavior=behavior), self.fixture(behavior) as (url, requests):
                with contextlib.redirect_stdout(io.StringIO()), self.assertRaisesRegex(sync.SyncError, "outcome unknown"):
                    sync._curl_upload(url, self.headers, self.source, self.size)
                self.assertEqual(len(requests), 1)
                self.assertEqual(requests[0]["body"], self.source.read_bytes())


if __name__ == "__main__":
    unittest.main()
