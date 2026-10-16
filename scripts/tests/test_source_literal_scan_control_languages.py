from pathlib import Path
import unittest

from scripts.verify_no_tracked_secrets import scan_payload


class ControlLanguageSecretScannerTests(unittest.TestCase):
    field = "to" + "ken"
    credential = "scanner-synthetic-credential-123"

    def findings(self, suffix: str, lines: list[str]) -> list[tuple[int, str]]:
        payload = ("\n".join(lines) + "\n").encode()
        return [
            (finding.line, finding.rule)
            for finding in scan_payload(Path("controller" + suffix), payload)
        ]

    def test_csharp_runtime_references_and_json_concatenation_are_not_literals(self):
        field = self.field
        self.assertEqual(self.findings(".cs", [
            f"string {field} = files.ReadSessionToken();",
            f"string {field} = new string('a', 32);",
            'string payload = "{\\"' + field + '\\":\\"" + sessionCredential + "\\"}";',
            f'string payload = $"{{\\"{field}\\":\\"{{sessionCredential}}\\"}}";',
            f'string {field} = $"{{settings.CurrentSessionCredential}}";',
        ]), [])

    def test_csharp_normal_verbatim_and_interpolated_literals_are_scanned(self):
        field, value = self.field, self.credential
        lines = [
            f'string {field} = "{value}";',
            f'string {field} = @"{value}";',
            f'string {field} = $"{value}";',
            f'string {field} = $@"{value}";',
            f'string {field} = @$"{value}";',
            'string payload = @"{""' + field + '"":""' + value + '""}";',
            f'string payload = $"{{{{{field}={value}}}}}";',
        ]
        self.assertEqual(self.findings(".cs", lines), [
            (number, "assigned-secret") for number in range(1, len(lines) + 1)
        ])

    def test_csharp_multiline_verbatim_quotes_and_interpolation_boundaries(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".cs", [
            'string payload = @"a ""quoted"" heading',
            f'{field}={value}',
            '";',
            f'string {field} = files.ReadSessionToken();',
            f'string payload = $"{field}={{Resolver(new {{ Value = \'x\' }})}}";',
            f'string payload = $"{{\"{field}={value}\"}}";',
        ]), [(2, "assigned-secret"), (6, "assigned-secret")])

    def test_lua_runtime_references_and_concatenation_are_not_literals(self):
        field = self.field
        self.assertEqual(self.findings(".lua", [
            f"local {field} = random_token()",
            f"{field} = previous.token",
            'local payload = "{\\"' + field + '\\":\\"" .. session_nonce .. "\\"}"',
            f"-- a quoted comment ' does not open a Lua string",
            f"local {field} = random_token()",
        ]), [])

    def test_lua_quoted_and_long_strings_and_comments_are_scanned(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".lua", [
            f'local {field} = "{value}"',
            f"local {field} = '{value}'",
            "local payload = [==[",
            "]] does not close this string",
            f"{field}={value}",
            "]==]",
            f"local {field} = random_token()",
            "--[=[",
            f"{field}={value}",
            "]=]",
            f"-- {field}={value}",
        ]), [(1, "assigned-secret"), (2, "assigned-secret"),
             (5, "assigned-secret"), (9, "assigned-secret"), (11, "assigned-secret")])

    def test_powershell_runtime_references_and_expandable_strings_are_not_literals(self):
        field = self.field
        self.assertEqual(self.findings(".ps1", [
            f"$request = @{{{field} = $session.token}}",
            f'${field} = "$sessionCredential"',
            f'${field} = "$(Get-SessionCredential)"',
            f"${field} = $(Get-SessionCredential)",
            f"${field} = ${{CurrentSessionCredential}}",
            f"${field} = $env:CURRENT_SESSION_CREDENTIAL",
        ]), [])

    def test_powershell_literals_bare_arguments_comments_and_here_strings_are_scanned(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".ps1", [
            f"${field} = '{value}'",
            f'${field} = "{value}"',
            f"{field}={value}",
            "$payload = @'",
            f"{field}={value}",
            "a mid-line '@ does not end a here-string",
            f"{field}={value}",
            "'@",
            f"$request = @{{{field} = $session.token}}",
            "$payload = @\"",
            f"{field}=$sessionCredential",
            f"{field}={value}",
            '"@',
            "<#",
            f"{field}={value}",
            "#>",
            f"# {field}={value}",
        ]), [(number, "assigned-secret") for number in (1, 2, 3, 5, 7, 12, 15, 17)])

    def test_powershell_quote_escapes_do_not_turn_later_code_into_literals(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".ps1", [
            f"$payload = 'a ''quote'' {field}={value}'",
            f'$payload = "a `"quote`" {field}={value}"',
            f'$payload = "$(Resolve-Value \'a)quote\')"; {field} = $session.token',
            f"$request = @{{{field} = $session.token}}",
        ]), [(1, "assigned-secret"), (2, "assigned-secret")])

    def test_powershell_expandable_here_string_preserves_escaped_dollar_literals(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".ps1", [
            '$payload = @"',
            f"{field}=`${value}",
            f"{field}=$sessionCredential",
            '"@',
        ]), [(2, "assigned-secret")])

    def test_comments_within_interpolation_expressions_remain_scanned(self):
        field, value = self.field, self.credential
        self.assertEqual(self.findings(".cs", [
            f'string payload = $"{{/* {field}={value} */ 42}}";',
            f'string payload = $"{{42 // {field}={value}',
            '}";',
            f'string {field} = files.ReadSessionToken();',
        ]), [(1, "assigned-secret"), (2, "assigned-secret")])
        self.assertEqual(self.findings(".ps1", [
            '$payload = "$(',
            f"# {field}={value}",
            "<#",
            f"{field}={value}",
            "#>",
            '42)"',
            f"$request = @{{{field} = $session.token}}",
        ]), [(2, "assigned-secret"), (4, "assigned-secret")])

    def test_provider_private_key_and_url_checks_remain_independent_of_source_literals(self):
        for suffix in (".cs", ".lua", ".ps1"):
            with self.subTest(suffix=suffix):
                self.assertEqual(self.findings(suffix, [
                    "gh" + "p_" + "a" * 32,
                    "-----BEGIN " + "PRIVATE KEY-----",
                    "https" + "://person:ScannerSyntheticPassword123@example.test",
                ]), [(1, "provider-token"), (2, "private-key"), (3, "url-credential")])


if __name__ == "__main__":
    unittest.main()
