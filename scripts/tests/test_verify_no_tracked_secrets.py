from contextlib import redirect_stdout
import io
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

from scripts.verify_no_tracked_secrets import run_scan, scan_paths, scan_payload, tracked_paths


class TrackedSecretScannerTests(unittest.TestCase):
    def test_detects_concrete_secret_assignments_and_lan_tokens(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.json"
            path.write_text(
                '{"client_' + 'se' + 'cret":"live-' + 'secret-value-123456",'
                '"url":"http://host/#langame'
                + 'To'
                + 'ken=abcdefghijklmnopqrstuvwxyz012345"}',
                encoding="utf-8",
            )
            findings = scan_paths([path])
            self.assertEqual(
                {finding.rule for finding in findings},
                {"assigned-secret", "lan-token"},
            )

    def test_repository_scan_is_rooted_when_called_from_a_nested_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            root_secret = repository / "root-secret.txt"
            root_secret.write_text(
                "sec" + "ret=RootOnlySecret123\n", encoding="utf-8"
            )
            subprocess.run(
                ["git", "add", "root-secret.txt"], cwd=repository, check=True
            )
            nested = repository / "apps" / "desktop"
            nested.mkdir(parents=True)
            previous_directory = Path.cwd()
            output = io.StringIO()
            try:
                os.chdir(nested)
                paths = tracked_paths(repository)
                with redirect_stdout(output):
                    result = run_scan(repository)
            finally:
                os.chdir(previous_directory)

            self.assertEqual(paths, [root_secret.resolve()])
            self.assertTrue(paths[0].is_absolute())
            self.assertEqual(result, 1)
            self.assertIn("root-secret.txt:1: assigned-secret", output.getvalue())
            self.assertNotIn(str(repository.resolve()), output.getvalue())

    def test_allows_redacted_values_and_source_field_names(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "safe.txt"
            path.write_text(
                'const field = "client_secret";\n'
                '{"client_secret":"<redacted>"}\n'
                '{"admin_password":"change-me-admin"}\n'
                'password="{{settings.server_password}}"\n',
                encoding="utf-8",
            )
            self.assertEqual(scan_paths([path]), [])

    def test_repository_scan_includes_untracked_source_but_not_ignored_local_data(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            (repository / ".gitignore").write_text("private/\n", encoding="utf-8")
            (repository / "settings.json").write_text(
                '{"api_' + 'key":"UntrackedSecretValue123"}\n', encoding="utf-8"
            )
            (repository / "private").mkdir()
            (repository / "private" / "settings.json").write_text(
                '{"api_' + 'key":"IgnoredLocalSecret123"}\n', encoding="utf-8"
            )
            output = io.StringIO()

            with redirect_stdout(output):
                result = run_scan(repository)

            self.assertEqual(result, 1)
            self.assertIn("settings.json:1: assigned-secret", output.getvalue())
            self.assertIn("2 working-tree file(s)", output.getvalue())
            self.assertNotIn("private", output.getvalue())
            self.assertNotIn("UntrackedSecretValue123", output.getvalue())
            self.assertNotIn("IgnoredLocalSecret123", output.getvalue())

    def test_scans_test_template_and_api_mock_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [
                root / "tests" / "credentials.test.json",
                root / "templates" / "server.ini.hbs",
                root / "api-mock" / "network.ts",
            ]
            for path in paths:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    'client_' + 'se' + 'cret="live-' + 'secret-value-123456"\n',
                    encoding="utf-8",
                )

            findings = scan_paths(paths)

            self.assertEqual(len(findings), 3)
            self.assertTrue(all(finding.rule == "assigned-secret" for finding in findings))

    def test_detects_unquoted_ini_values_and_cli_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "launch.txt"
            path.write_text(
                "DedicatedServerClient"
                + "Se"
                + "cret=live-client-secret-123456\n"
                "server --admin-" + "password live-admin-password-123456\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "cli-secret"],
            )

    def test_scans_every_assignment_on_a_line(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            path.write_text(
                '{"admin_password":"change-me-admin",'
                '"refresh_' + 'to' + 'ken":"live-refresh-value-123456789"}',
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual([finding.rule for finding in findings], ["assigned-secret"])

    def test_detects_private_key_headers_and_provider_tokens(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "credentials.txt"
            path.write_text(
                "-----BEGIN " + "PRIVATE KEY-----\n"
                + "ghp_"
                + "a" * 32
                + "\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["private-key", "provider-token"],
            )

    def test_detects_backtick_literals_in_typescript(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.ts"
            path.write_text(
                "const config = { admin_"
                + "pass"
                + "word: `Backtick9` };\n"
                + "const command = `server --rcon-"
                + "pass"
                + "word=RconPass9`;\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "cli-secret"],
            )

    def test_detects_multiline_typescript_templates_without_flagging_identifiers(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.ts"
            path.write_text(
                "const config = `\n"
                + "admin_"
                + "pass"
                + "word=TemplatePass9\n"
                + "--admin-"
                + "pass"
                + "word CliPass99\n"
                + "`;\n"
                + "const admin_"
                + "pass"
                + "word = persisted.adminPassword;\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [(finding.line, finding.rule) for finding in findings],
                [(2, "assigned-secret"), (3, "cli-secret")],
            )

    def test_detects_same_line_and_multiline_rust_raw_string_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.rs"
            path.write_text(
                'let inline = r#"{\\"admin_'
                + "pass"
                + 'word\\":\\"RustInline9\\"}"#;\n'
                + 'let multiline = r#"\n'
                + "admin_"
                + "pass"
                + "word=RustMulti9\n"
                + "--admin-"
                + "pass"
                + "word RustCli99\n"
                + '"#;\n'
                + "let admin_"
                + "pass"
                + "word = persisted.admin_password;\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [(finding.line, finding.rule) for finding in findings],
                [
                    (1, "assigned-secret"),
                    (3, "assigned-secret"),
                    (4, "cli-secret"),
                ],
            )

    def test_detects_go_python_and_cpp_multiline_literal_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            go_path = root / "settings.go"
            python_path = root / "settings.py"
            cpp_path = root / "settings.cpp"
            go_path.write_text(
                "package main\nvar config = `\nTO"
                + "KEN=GoTokenValue123\n`\n"
                + "var to"
                + "ken = persistedToken\n",
                encoding="utf-8",
            )
            python_path.write_text(
                'config = """\nsec'
                + 'ret=PythonSecretValue123\n"""\n'
                + "sec"
                + "ret = persisted_secret\n",
                encoding="utf-8",
            )
            cpp_path.write_text(
                'const char* config = R"CFG(\nadmin_'
                + "pass"
                + 'word=CppPass99\n)CFG";\n'
                + "auto admin_"
                + "pass"
                + "word = persisted;\n",
                encoding="utf-8",
            )

            findings = scan_paths([go_path, python_path, cpp_path])

            self.assertEqual(
                [(finding.path.suffix, finding.line, finding.rule) for finding in findings],
                [
                    (".go", 3, "assigned-secret"),
                    (".py", 2, "assigned-secret"),
                    (".cpp", 2, "assigned-secret"),
                ],
            )

    def test_detects_bare_secret_and_token_keys_case_insensitively(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.json"
            path.write_text(
                '{"SeC' + 'ReT":"BareSecretValue123"}\n'
                + '{"TO' + 'KEN":"BareTokenValue123"}\n',
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "assigned-secret"],
            )

    def test_detects_aws_secret_access_key_and_session_token_assignments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "aws.env"
            path.write_text(
                "AWS_"
                + "SECRET_ACCESS_KEY=AwsSecretValue1234567890\n"
                + "aws_"
                + "session_token=AwsSessionValue1234567890\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "assigned-secret"],
            )

    def test_bare_names_require_key_boundaries_and_source_identifiers_are_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.ts"
            path.write_text(
                'const scripts = { "smoke:ai-secret": "cargo test long-command" };\n'
                + 'const publicKeyToken = "6595b64144ccf1df";\n'
                + "const to"
                + "ken = persisted.tokenValue;\n",
                encoding="utf-8",
            )

            self.assertEqual(scan_paths([path]), [])

    def test_escaped_config_lines_and_query_parameters_stop_at_native_delimiters(self):
        with tempfile.TemporaryDirectory() as directory:
            safe_path = Path(directory) / "fixture.json"
            safe_path.write_text(
                '{"content":"ServerPass' + 'word=fixture-server-password\\nFuture=keep",'
                '"url":"Map?ServerPass' + 'word=fixture-server-password?SessionName=Fixture"}',
                encoding="utf-8",
            )
            unsafe_path = Path(directory) / "unsafe.json"
            unsafe_path.write_text(
                '{"content":"ServerPass' + 'word=ConcretePass99\\nFuture=keep"}',
                encoding="utf-8",
            )

            self.assertEqual(scan_paths([safe_path]), [])
            self.assertEqual(
                [finding.rule for finding in scan_paths([unsafe_path])],
                ["assigned-secret"],
            )

    def test_python_mapping_references_are_not_literal_secret_values(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.py"
            path.write_text(
                'url = f"?ServerPassword={settings[\'server_password\']}"\n',
                encoding="utf-8",
            )

            self.assertEqual(scan_paths([path]), [])

    def test_detects_secret_assignments_in_source_comments(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "source.ts"
            path.write_text(
                "// TO" + "KEN=CommentTokenValue123\n"
                + "/* admin_"
                + "pass"
                + "word=CommentPass9 */\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "assigned-secret"],
            )

    def test_decodes_utf16_bom_text_and_blocks_nul_text_without_a_bom(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            secret_text = "sec" + "ret=Utf16SecretValue123\n"
            little_endian = root / "little.ps1"
            big_endian = root / "big.reg"
            no_bom = root / "unknown.ini"
            little_endian.write_bytes(b"\xff\xfe" + secret_text.encode("utf-16-le"))
            big_endian.write_bytes(b"\xfe\xff" + secret_text.encode("utf-16-be"))
            no_bom.write_bytes(b"plain\0text")

            findings = scan_paths([little_endian, big_endian, no_bom])

            self.assertEqual(
                [(finding.path.name, finding.rule) for finding in findings],
                [
                    ("little.ps1", "assigned-secret"),
                    ("big.reg", "assigned-secret"),
                    ("unknown.ini", "nul-byte-text"),
                ],
            )

    def test_passwords_use_eight_character_minimum_but_other_secrets_use_twelve(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.ini"
            path.write_text(
                "pass" + "word=Abcd1234\n"
                "api_" + "key=abcdefghijk\n"
                "auth_" + "to" + "ken=abcdefghijkl\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [(finding.line, finding.rule) for finding in findings],
                [(1, "assigned-secret"), (3, "assigned-secret")],
            )

    def test_fixture_and_test_prefixes_are_not_allowlisted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fixture.ini"
            path.write_text(
                "pass" + "word=fixture-active-password\n"
                "admin_" + "pass" + "word=test-active-password\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "assigned-secret"],
            )

    def test_exact_assertion_placeholders_are_scoped_to_test_source(self):
        values = ("previous-key", "replacement-fixture-key", "synthetic-test-only")
        for value in values:
            payload = ('{"api' + 'Key": "' + value + '"}\n').encode()
            for name in ("settings.test.cjs", "settings.test.ts", "settings_tests.rs"):
                with self.subTest(value=value, path=name):
                    self.assertEqual(scan_payload(Path(name), payload), [])
            for name in ("settings.ts", "settings.rs", "tests/settings.json"):
                with self.subTest(value=value, path=name):
                    findings = scan_payload(Path(name), payload)
                    self.assertEqual([finding.rule for finding in findings], ["assigned-secret"])

    def test_test_sources_reject_modified_placeholders_and_provider_tokens(self):
        values = (
            "previous-key-active",
            "replacement-fixture-key-active",
            "synthetic-test-only-active",
            "fixture-active-password",
            "test-active-password",
        )
        for value in values:
            payload = ('{"api' + 'Key": "' + value + '"}\n').encode()
            with self.subTest(value=value):
                findings = scan_payload(Path("settings.test.cjs"), payload)
                self.assertEqual([finding.rule for finding in findings], ["assigned-secret"])
        provider_payload = ('const key = "ghp_' + "a" * 32 + '";\n').encode()
        findings = scan_payload(Path("settings.test.cjs"), provider_payload)
        self.assertEqual([finding.rule for finding in findings], ["provider-token"])

    def test_detects_database_urls_with_embedded_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "database.env"
            path.write_text(
                "DATABASE_URL=postgres"
                + "ql://dbuser:P0stgresPass@db.internal/app\n"
                + "REPORTING_URL=my"
                + "sql://reporter:MysqlPass9@db.internal/reporting\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["url-credential", "url-credential"],
            )

    def test_detects_generic_http_authorization_credentials(self):
        bearer = "GenericHttpCredential123"
        basic = "dXNlcj" + "pwYXNz"
        for name, payload in (
            ("request.http", "Authorization: Bearer " + bearer),
            ("proxy.txt", "Proxy-Authorization: Basic " + basic),
            ("settings.json", '{"authorization": "Bearer ' + bearer + '"}'),
            ("request.ts", 'const headers = { Authorization: "Bearer ' + bearer + '" };'),
            ("request.rs", 'let headers = r#"{\"Authorization\":\"Basic ' + basic + '\"}"#;'),
        ):
            with self.subTest(path=name):
                findings = scan_payload(Path(name), payload.encode())
                self.assertEqual([finding.rule for finding in findings], ["http-authorization"])

    def test_http_authorization_references_are_not_concrete_credentials(self):
        for value in ("${ACCESS_TOKEN}", "{{token}}", "<redacted>", "%ACCESS_TOKEN%"):
            payload = ('Authorization: Bearer ' + value).encode()
            with self.subTest(value=value):
                self.assertEqual(scan_payload(Path("request.http"), payload), [])
        payload = b'const headers = { Authorization: provider.authorization };'
        self.assertEqual(scan_payload(Path("request.ts"), payload), [])

    def test_detects_slack_stripe_google_and_jwt_tokens(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "provider-tokens.txt"
            slack = "xox" + "b-1234567890-abcdefghijklmno"
            stripe = "sk_" + "live_" + "a" * 24
            google = "AI" + "za" + "b" * 35
            jwt = "eyJ" + "a" * 12 + "." + "b" * 12 + "." + "c" * 12
            path.write_text(
                "\n".join((slack, stripe, google, jwt)) + "\n",
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["provider-token"] * 4,
            )

    def test_source_expressions_are_not_mistaken_for_literal_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "settings.ts"
            path.write_text(
                "const settings = { api" + "Key: persisted.apiKey };\n"
                'const literal = { api' + 'Key: "live-api-key-1234567890" };\n'
                'const argument = "DedicatedServerClient'
                + 'Se'
                + 'cret=live-client-secret-123456";\n',
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual(
                [finding.rule for finding in findings],
                ["assigned-secret", "assigned-secret"],
            )

    def test_i18n_prose_only_skips_the_ambiguous_assignment_rule(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "i18n" / "messages.ts"
            path.parent.mkdir(parents=True)
            path.write_text(
                '"settings.rolePass' + 'word": "Choose a password before sharing."\n'
                'const url = "http://host/#langame'
                'To'
                'ken=abcdefghijklmnopqrstuvwxyz012345";\n',
                encoding="utf-8",
            )

            findings = scan_paths([path])

            self.assertEqual([finding.rule for finding in findings], ["lan-token"])

    def test_translation_paths_do_not_exempt_credentials_in_source_or_values(self):
        payload = (
            '"settings.rolePass' + 'word": "Choose a password before sharing.",\n'
            'const api' + 'Key = "LiveTranslationKey123";\n'
            'const options = { client_' + 'sec' + 'ret: "LiveTranslationSecret123" };\n'
            '"settings.pass' + 'word": "Use admin_' + 'pass' + 'word=LiveTranslationPass123",\n'
            '"settings.launch": "server --rcon-' + 'pass' + 'word LiveTranslationRcon123",\n'
            '"api' + 'Key": "LiveUnscopedTranslationKey123",\n'
        ).encode()
        for name in ("i18n/messages.ts", "i18n-messages-en.ts"):
            with self.subTest(path=name):
                findings = scan_payload(Path(name), payload)
                self.assertEqual(
                    [(finding.line, finding.rule) for finding in findings],
                    [
                        (2, "assigned-secret"),
                        (3, "assigned-secret"),
                        (4, "assigned-secret"),
                        (5, "cli-secret"),
                        (6, "assigned-secret"),
                    ],
                )

    def test_translation_labels_do_not_exempt_provider_tokens_or_url_credentials(self):
        payload = (
            '"settings.apiKey": "ghp_' + "a" * 32 + '",\n'
            '"settings.endpoint": "https' + '://test:LiveTranslationPass123@example.test",\n'
        ).encode()
        findings = scan_payload(Path("i18n/messages.ts"), payload)
        self.assertEqual(
            [(finding.line, finding.rule) for finding in findings],
            [(1, "provider-token"), (2, "url-credential")],
        )


if __name__ == "__main__":
    unittest.main()
