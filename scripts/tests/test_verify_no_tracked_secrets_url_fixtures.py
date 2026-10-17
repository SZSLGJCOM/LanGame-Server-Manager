from pathlib import Path
import unittest

from scripts.verify_no_tracked_secrets import scan_payload


class SyntheticUrlCredentialScannerTests(unittest.TestCase):
    username = "user"
    redaction_marker = "sec" + "ret"
    unapproved_value = "SyntheticCredentialValue123"

    def url(self, authority: str, *, scheme: str = "http", username: str | None = None,
            password: str | None = None) -> str:
        return (scheme + "://" + (username or self.username) + ":"
                + (password or self.redaction_marker) + "@" + authority)

    def rules(self, name: str, text: str) -> list[str]:
        return [finding.rule for finding in scan_payload(Path(name), text.encode())]

    def test_exact_redaction_pair_requires_test_source_http_loopback_and_explicit_port(self):
        for name in ("test_proxy.py", "proxy.test.cjs", "proxy_tests.rs"):
            for authority in ("127.0.0.1:80", "localhost:18765", "[::1]:65535"):
                with self.subTest(name=name, authority=authority):
                    self.assertEqual(self.rules(name, self.url(authority)), [])

    def test_redaction_pair_in_production_documents_and_configuration_is_still_rejected(self):
        for name in ("proxy.py", "proxy.ts", "proxy.ini", "README.md", "proxy.test.json"):
            with self.subTest(name=name):
                self.assertEqual(self.rules(name, self.url("127.0.0.1:80")), ["url-credential"])

    def test_redaction_pair_does_not_exempt_other_origins_or_schemes(self):
        for authority in ("example.test:80", "192.0.2.1:80", "127.0.0.1.example.test:80"):
            with self.subTest(authority=authority):
                self.assertEqual(self.rules("test_proxy.py", self.url(authority)), ["url-credential"])
        for scheme in ("https", "postgresql", "redis"):
            with self.subTest(scheme=scheme):
                self.assertEqual(self.rules("test_proxy.py", self.url("127.0.0.1:80", scheme=scheme)),
                                 ["url-credential"])

    def test_missing_or_invalid_ports_do_not_qualify_as_synthetic_fixture(self):
        for authority in ("127.0.0.1", "127.0.0.1:", "127.0.0.1:0", "127.0.0.1:65536",
                          "127.0.0.1:invalid", "[::1]:99999"):
            with self.subTest(authority=authority):
                self.assertEqual(self.rules("test_proxy.py", self.url(authority)), ["url-credential"])

    def test_nonfixture_usernames_passwords_and_encoded_values_remain_rejected(self):
        for username, password in (("operator", self.redaction_marker), ("User", self.redaction_marker),
                                   (self.username, self.unapproved_value),
                                   (self.username, self.redaction_marker + "-suffix"),
                                   (self.username, "%73" + self.redaction_marker[1:])):
            with self.subTest(username=username, password=password):
                self.assertEqual(self.rules("test_proxy.py", self.url("127.0.0.1:80",
                                 username=username, password=password)), ["url-credential"])

    def test_safe_fixture_does_not_hide_other_credentials_on_the_same_line(self):
        safe = self.url("127.0.0.1:80")
        unsafe = self.url("127.0.0.1:80", password=self.unapproved_value)
        self.assertEqual(self.rules("test_proxy.py", safe + " " + unsafe), ["url-credential"])

    def test_safe_fixture_does_not_suppress_other_secret_rules(self):
        safe = self.url("127.0.0.1:80")
        payload = (safe + '\napi_' + 'key="SyntheticCredentialValue123"\n'
                   + "gh" + "p_" + "a" * 32 + "\n-----BEGIN " + "PRIVATE KEY-----\n")
        self.assertEqual(self.rules("test_proxy.py", payload),
                         ["assigned-secret", "provider-token", "private-key"])

    def test_existing_exact_synthetic_pairs_keep_their_previous_scope(self):
        for password in ("password", "pass", "fixture"):
            value = self.url("example.test:443", scheme="https", password=password)
            with self.subTest(password=password):
                self.assertEqual(self.rules("test_proxy.py", value), [])
                self.assertEqual(self.rules("proxy.py", value), ["url-credential"])


if __name__ == "__main__":
    unittest.main()
