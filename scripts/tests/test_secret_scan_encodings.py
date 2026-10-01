"""Publication checks inspect credentials in BOM-marked Unicode text."""

import codecs
from pathlib import Path
import unittest

from scripts.verify_no_tracked_secrets import scan_payload


class SecretScanEncodingTests(unittest.TestCase):
    def test_utf32_credentials_are_decoded_before_scanning(self):
        text = "safe first line\napi_" + "key=EncodedCredential123\n"
        for bom, encoding in (
            (codecs.BOM_UTF32_LE, "utf-32-le"),
            (codecs.BOM_UTF32_BE, "utf-32-be"),
        ):
            with self.subTest(encoding=encoding):
                findings = scan_payload(Path("settings.txt"), bom + text.encode(encoding))
                self.assertEqual(
                    [(item.line, item.rule) for item in findings],
                    [(2, "assigned-secret")],
                )

    def test_utf32_text_without_credentials_passes(self):
        for encoding in ("utf-32-le", "utf-32-be"):
            bom = codecs.BOM_UTF32_LE if encoding.endswith("le") else codecs.BOM_UTF32_BE
            with self.subTest(encoding=encoding):
                self.assertEqual(
                    scan_payload(Path("settings.txt"), bom + "name=示例\n".encode(encoding)),
                    [],
                )


if __name__ == "__main__":
    unittest.main()
