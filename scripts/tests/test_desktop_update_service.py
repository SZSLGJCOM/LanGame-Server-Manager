import base64
from copy import deepcopy
from datetime import date, timedelta
import hashlib
import ipaddress
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from scripts.prepare_desktop_update_service import (
    GITHUB_RELEASES, PLATFORM, REPOSITORY_ROOT, gitcode_artifact_url,
    china_networks, nginx_configs, prepare_bundle, read_json, regional_manifest,
)


def b64(value: bytes) -> str:
    return base64.b64encode(value).decode()


PUBLIC_KEY = b64(("untrusted comment: test key\n" + b64(b"Ed" + b"k" * 8 + b"p" * 32) + "\n").encode())
# Structural fixtures only: never claimed to be cryptographically signed installers.
SIGNATURE = b64(("untrusted comment: test signature\n" + b64(b"ED" + b"k" * 8 + b"s" * 64)
                 + "\ntrusted comment: fixture\n" + b64(b"c" * 64) + "\n").encode())
NAME = "LanGame.Server.Manager_0.1.0_x64-setup.exe"
CN_URL = f"https://api.gitcode.com/api/v5/repos/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/v0.1.0/attach_files/{NAME}/download"


def manifest() -> dict:
    return {"version": "0.1.0", "notes": "更新\nKeep user data", "pub_date": "2026-10-08T00:00:00Z",
            "platforms": {PLATFORM: {"signature": SIGNATURE,
                                    "url": f"{GITHUB_RELEASES}/v0.1.0/{NAME}"}}}


def apnic(day: date | None = None, extra: str = "") -> bytes:
    day = day or date.today()
    rows = ["apnic|CN|ipv4|1.2.3.1|6|20200101|allocated",
            "apnic|CN|ipv6|2400:1000::|32|20200101|assigned",
            "apnic|US|ipv4|8.8.8.0|256|20200101|allocated",
            "apnic|HK|ipv4|1.3.0.0|256|20200101|allocated",
            "apnic|CN|ipv4|1.4.0.0|256|20200101|available"]
    if extra:
        rows.append(extra)
    summaries = [f"apnic|*|{kind}|*|{sum(row.split('|')[2] == kind for row in rows)}|summary"
                 for kind in ("ipv4", "ipv6")]
    return (f"# synthetic APNIC fixture\n2|apnic|{day:%Y%m%d}|{len(rows)}|20200101|{day:%Y%m%d}|+1000\n"
            + "\n".join(summaries + rows) + "\n").encode()


class DesktopUpdateServiceTests(unittest.TestCase):
    def test_cn_manifest_changes_only_url_and_does_not_mutate_input(self):
        self.assertEqual(gitcode_artifact_url("0.1.0", NAME), CN_URL)
        source = manifest()
        original = deepcopy(source)
        result = regional_manifest(source, CN_URL, PUBLIC_KEY)
        self.assertEqual(source, original)
        self.assertEqual(result["platforms"][PLATFORM]["url"], CN_URL)
        result["platforms"][PLATFORM]["url"] = original["platforms"][PLATFORM]["url"]
        self.assertEqual(result, original)

    def test_cn_url_rejects_other_hosts_repositories_versions_and_escaping(self):
        for url in ["", CN_URL.replace("https:", "http:"), CN_URL.replace("gitcode.com", "127.0.0.1"),
                    CN_URL.replace("gitcode.com", "gitcode.com.evil.test"),
                    CN_URL.replace("gitcode.com", "gitcode.com:443"),
                    CN_URL.replace("gitcode.com", "user@gitcode.com"),
                    CN_URL.replace("SZSLGJCOM", "Other"), CN_URL.replace("v0.1.0", "v0.1.1"),
                    CN_URL.replace("_x64-setup", "_x64-offline-setup"), CN_URL + "?token=x", CN_URL + "#x",
                    CN_URL.replace("/v0.1.0/", "/v0.1.0/../v0.1.0/"),
                    CN_URL.replace("/v0.1.0/", "/%760.1.0/"), CN_URL + "\n",
                    f"https://gitcode.com/SZSLGJCOM/Other/releases/download/v0.1.0/{NAME}"]:
            with self.subTest(url=url), self.assertRaisesRegex(ValueError, "GitCode URL"):
                regional_manifest(manifest(), url, PUBLIC_KEY)

    def test_observed_gitcode_web_permanent_form_is_also_accepted_exactly(self):
        web_url = f"https://gitcode.com/SZSLGJCOM/LanGame-Server-Manager-Releases/releases/download/v0.1.0/{NAME}"
        self.assertEqual(regional_manifest(manifest(), web_url, PUBLIC_KEY)["platforms"][PLATFORM]["url"], web_url)
        for bad in (web_url + "?access_token=x", web_url.replace("/v0.1.0/", "/v0.1.1/")):
            with self.subTest(url=bad), self.assertRaises(ValueError):
                regional_manifest(manifest(), bad, PUBLIC_KEY)

    def test_bad_manifest_and_signature_are_rejected(self):
        bad = []
        item = manifest(); item["version"] = "0.1.0-beta.1"; bad.append(item)
        item = manifest(); item["platforms"][PLATFORM]["signature"] = "invalid"; bad.append(item)
        item = manifest(); item["platforms"][PLATFORM]["url"] = CN_URL; bad.append(item)
        item = manifest(); item["platforms"]["linux-x86_64"] = {}; bad.append(item)
        item = manifest(); item["platforms"][PLATFORM]["url"] += "?mirror=x"; bad.append(item)
        for item in bad:
            with self.subTest(item=item), self.assertRaises(ValueError):
                regional_manifest(item, CN_URL, PUBLIC_KEY)
        with self.assertRaisesRegex(ValueError, "duplicate"):
            read_json(b'{"version":"0.1.0","version":"0.2.0"}')

    def test_apnic_counts_not_prefix_lengths_and_exact_country_membership(self):
        networks, info = china_networks(apnic())
        selected = lambda value: any(ipaddress.ip_address(value) in block for block in networks)
        for value in ("1.2.3.1", "1.2.3.6", "2400:1000::1", "2400:1000:ffff:ffff::1"):
            self.assertTrue(selected(value), value)
        for value in ("1.2.3.0", "1.2.3.7", "8.8.8.8", "1.3.0.1", "1.4.0.1", "2400:1001::1"):
            self.assertFalse(selected(value), value)
        self.assertEqual(info["ipv4_cidrs"], 4)
        self.assertEqual(info["ipv6_cidrs"], 1)

    def test_apnic_rejects_truncation_stale_future_empty_and_malformed_ranges(self):
        today = date.today()
        bad = [b"", apnic(today - timedelta(days=32)), apnic(today + timedelta(days=1)),
               apnic().rsplit(b"\n", 2)[0], apnic().replace(b"|6|20200101", b"|0|20200101"),
               apnic().replace(b"2400:1000::|32", b"2400:1000::1|32"),
               apnic().replace(b"2400:1000::|32", b"2400:1000::|129"),
               apnic().replace(b"1.2.3.1|6", b"255.255.255.255|2"),
               apnic().replace(b"1.2.3.1|6", b"127.0.0.1|6"),
               apnic().replace(b"CN|ipv6", b"US|ipv6")]
        for raw in bad:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                china_networks(raw, today=today)

    def test_nginx_uses_connection_ip_no_region_headers_no_shared_cache(self):
        http, location = nginx_configs("/var/lib/langame/desktop-updates/releases/0.1.0")
        self.assertIn("geo $remote_addr $lgsm_update_cn", http)
        self.assertIn("default 0;", http)
        self.assertIn("location = /updates/server-manager/latest.json", location)
        self.assertIn('Cache-Control "private, no-store" always;', location)
        self.assertIn("if_modified_since off;", location)
        self.assertIn("etag off;", location)
        self.assertIn("open_file_cache off;", location)
        self.assertIn("limit_except GET", location)
        self.assertNotIn("$http_", http + location)
        self.assertNotIn("proxy_pass", http + location)
        self.assertNotIn("proxy_recursive", http + location)
        for root in ("relative", "/", "/tmp/../www", "/tmp//www", "/tmp/$arg_path", '/tmp/x;return 200;', "/tmp/a\nb"):
            with self.subTest(root=root), self.assertRaises(ValueError):
                nginx_configs(root)
        http, _ = nginx_configs("/etc/langame/geo/revision", "/var/lib/langame/desktop-updates/current")
        self.assertIn("include /etc/langame/geo/revision/china-cidrs.conf;", http)
        self.assertIn("/var/lib/langame/desktop-updates/current/latest-cn.json;", http)
        with self.assertRaises(ValueError):
            nginx_configs("/etc/langame/geo/revision", "/tmp/$arg_region")

    def test_bundle_preserves_global_bytes_and_reports_only_actual_validation(self):
        with tempfile.TemporaryDirectory(prefix="lgsm-update-service-") as temporary:
            base = Path(temporary)
            source = base / "latest.json"
            source.write_bytes(json.dumps(manifest(), ensure_ascii=False).encode() + b"\r\n")
            data = base / "delegated-apnic-latest"; data.write_bytes(apnic())
            config = base / "tauri.json"
            config.write_text(json.dumps({"plugins": {"updater": {"pubkey": PUBLIC_KEY}}}))
            output = base / "bundle"
            receipt = prepare_bundle(source, data, CN_URL, config, output, "/var/lib/lgsm/releases/0.1.0")
            self.assertEqual((output / "latest-global.json").read_bytes(), source.read_bytes())
            cn = json.loads((output / "latest-cn.json").read_bytes())
            self.assertEqual(cn, regional_manifest(manifest(), CN_URL, PUBLIC_KEY))
            self.assertEqual(len(list(output.iterdir())), 6)
            for name, info in receipt["files"].items():
                content = (output / name).read_bytes()
                self.assertEqual(info, {"bytes": len(content), "sha256": hashlib.sha256(content).hexdigest()})
            self.assertIn("required-upstream", receipt["signature_validation"])
            with self.assertRaises(FileExistsError):
                prepare_bundle(source, data, CN_URL, config, output, "/var/lib/lgsm/releases/0.1.0")
            rejected = base / "rejected"
            with self.assertRaises(ValueError):
                prepare_bundle(source, data, "", config, rejected, "/var/lib/lgsm/releases/0.1.0")
            self.assertFalse(rejected.exists())
            with self.assertRaisesRegex(ValueError, "outside"):
                prepare_bundle(source, data, CN_URL, config, REPOSITORY_ROOT / "generated", "/var/lib/lgsm")

    def test_cli_requires_explicit_gitcode_attachment(self):
        result = subprocess.run([sys.executable, "-B", str(REPOSITORY_ROOT / "scripts/prepare_desktop_update_service.py"),
                                 "--verified-manifest", "missing", "--apnic-file", "missing",
                                 "--deploy-root", "/var/lib/lgsm", "--output-dir", "missing"],
                                capture_output=True, text=True, timeout=10, check=False)
        self.assertEqual(result.returncode, 2)
        self.assertIn("--gitcode-url", result.stderr)


if __name__ == "__main__":
    unittest.main()
