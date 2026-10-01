from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scripts import verify_game_config_provenance as provenance


class GameConfigProvenanceVerifierTests(unittest.TestCase):
    def _write_valid_module(self, modules_dir: Path) -> None:
        module_root = modules_dir / "example"
        module_root.mkdir(parents=True)
        (module_root / "module.toml").write_text('id = "example"\n', encoding="utf-8", newline="\n")
        (module_root / "schema.json").write_text(
            json.dumps({"properties": {
                "setting": {
                    "type": "string",
                    "x-lsgm-source": "official",
                    "x-lsgm-source-key": "Setting",
                    "x-lsgm-source-surface": "config_file",
                }
            }}),
            encoding="utf-8",
            newline="\n",
        )
        (module_root / "config-sources.toml").write_text(
            "status = \"best_effort_verified\"\n"
            "last_verified = \"2026-08-11\"\n"
            "notes = \"Fixture ledger.\"\n\n"
            "[[sources]]\n"
            "id = \"official\"\n"
            "kind = \"documentation\"\n"
            "path = \"https://example.invalid/docs\"\n"
            "authority = \"official\"\n"
            "description = \"Fixture source.\"\n\n"
            "[[items]]\n"
            "source = \"official\"\n"
            "key = \"Setting\"\n"
            "schema_key = \"setting\"\n"
            "surface = \"config_file\"\n",
            encoding="utf-8",
            newline="\n",
        )

    def test_check_reports_missing_reports_without_creating_them(self) -> None:
        self.assertTrue(
            hasattr(provenance, "run_verification"),
            "the verifier must expose a testable check/write boundary",
        )
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            modules_dir = root / "modules"
            docs_dir = root / "docs"
            self._write_valid_module(modules_dir)

            result = provenance.run_verification(modules_dir, docs_dir, "check")

            self.assertEqual(result, 1)
            self.assertFalse((docs_dir / "game-config-source-ledger.csv").exists())
            self.assertFalse((docs_dir / "game-config-source-ledger.md").exists())

    def test_check_reports_stale_reports_without_mutating_them(self) -> None:
        self.assertTrue(hasattr(provenance, "run_verification"))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            modules_dir = root / "modules"
            docs_dir = root / "docs"
            self._write_valid_module(modules_dir)
            docs_dir.mkdir()
            csv_path = docs_dir / "game-config-source-ledger.csv"
            md_path = docs_dir / "game-config-source-ledger.md"
            csv_path.write_text("stale\n", encoding="utf-8", newline="\n")
            md_path.write_text("stale\n", encoding="utf-8", newline="\n")

            result = provenance.run_verification(modules_dir, docs_dir, "check")

            self.assertEqual(result, 1)
            self.assertEqual(csv_path.read_text(encoding="utf-8"), "stale\n")
            self.assertEqual(md_path.read_text(encoding="utf-8"), "stale\n")

    def test_check_rejects_crlf_report_bytes_without_mutating_them(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            modules_dir = root / "modules"
            docs_dir = root / "docs"
            self._write_valid_module(modules_dir)
            self.assertEqual(provenance.run_verification(modules_dir, docs_dir, "write"), 0)
            csv_path = docs_dir / "game-config-source-ledger.csv"
            crlf_bytes = csv_path.read_bytes().replace(b"\n", b"\r\n")
            csv_path.write_bytes(crlf_bytes)

            result = provenance.run_verification(modules_dir, docs_dir, "check")

            self.assertEqual(result, 1)
            self.assertEqual(csv_path.read_bytes(), crlf_bytes)

    def test_write_updates_both_reports_only_after_ledger_validation_succeeds(self) -> None:
        self.assertTrue(hasattr(provenance, "run_verification"))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            modules_dir = root / "modules"
            docs_dir = root / "docs"
            self._write_valid_module(modules_dir)

            self.assertEqual(provenance.run_verification(modules_dir, docs_dir, "write"), 0)
            csv_path = docs_dir / "game-config-source-ledger.csv"
            md_path = docs_dir / "game-config-source-ledger.md"
            original_csv = csv_path.read_text(encoding="utf-8")
            original_md = md_path.read_text(encoding="utf-8")
            (modules_dir / "example" / "config-sources.toml").write_text("status = \"invalid\"\n", encoding="utf-8")

            self.assertEqual(provenance.run_verification(modules_dir, docs_dir, "write"), 1)
            self.assertEqual(csv_path.read_text(encoding="utf-8"), original_csv)
            self.assertEqual(md_path.read_text(encoding="utf-8"), original_md)

    def test_rendered_reports_are_utf8_lf_and_deterministic(self) -> None:
        self.assertTrue(hasattr(provenance, "collect_summaries"))
        with tempfile.TemporaryDirectory() as directory:
            modules_dir = Path(directory) / "modules"
            self._write_valid_module(modules_dir)

            failures, summaries = provenance.collect_summaries(modules_dir)
            csv_report = provenance.render_csv_report(summaries)
            markdown_report = provenance.render_markdown_report(summaries)

        self.assertEqual(failures, [])
        self.assertEqual(csv_report, provenance.render_csv_report(summaries))
        self.assertTrue(csv_report.endswith("\n") and markdown_report.endswith("\n"))
        self.assertNotIn("\r", csv_report + markdown_report)
        self.assertFalse(csv_report.encode("utf-8").startswith(b"\xef\xbb\xbf"))

    def test_ledger_rejects_local_absolute_paths_in_evidence_text(self) -> None:
        local_paths = (
            "C:/temp/proof.txt",
            r"C:\temp\proof.txt",
            r"\\server\share\proof.txt",
            "/tmp/proof.txt",
            "file:///C:/temp/proof.txt",
        )
        evidence_fields = ("notes", "path", "url", "authority", "description", "reason")

        for field in evidence_fields:
            for local_path in local_paths:
                with self.subTest(field=field, local_path=local_path):
                    with tempfile.TemporaryDirectory() as directory:
                        modules_dir = Path(directory) / "modules"
                        self._write_valid_module(modules_dir)
                        ledger_path = modules_dir / "example" / "config-sources.toml"
                        ledger_text = ledger_path.read_text(encoding="utf-8")
                        toml_path = local_path.replace("\\", "\\\\")
                        if field == "notes":
                            ledger_text = ledger_text.replace(
                                'notes = "Fixture ledger."',
                                f'notes = "Evidence at {toml_path}."',
                            )
                        elif field == "path":
                            ledger_text = ledger_text.replace(
                                "https://example.invalid/docs",
                                toml_path,
                            )
                        elif field == "url":
                            ledger_text = ledger_text.replace(
                                'path = "https://example.invalid/docs"',
                                'path = "https://example.invalid/docs"\n'
                                f'url = "{toml_path}"',
                            )
                        elif field == "authority":
                            ledger_text = ledger_text.replace(
                                'authority = "official"',
                                f'authority = "{toml_path}"',
                            )
                        elif field == "reason":
                            ledger_text += (
                                "\n[[exclusions]]\n"
                                'source = "official"\n'
                                'key = "UnusedSetting"\n'
                                f'reason = "Evidence at {toml_path}."\n'
                            )
                        else:
                            ledger_text = ledger_text.replace(
                                'description = "Fixture source."',
                                f'description = "Evidence at {toml_path}."',
                            )
                        ledger_path.write_text(
                            ledger_text,
                            encoding="utf-8",
                            newline="\n",
                        )

                        failures, _ = provenance.collect_summaries(modules_dir)

                    self.assertTrue(
                        any(f"{field} must" in failure for failure in failures),
                        failures,
                    )

    def test_ledger_allows_remote_and_symbolic_evidence_paths(self) -> None:
        for value in (
            "https://example.invalid/docs/C:/published-proof",
            "steam://run/123",
            "local:<validated-install-root>/proof.txt",
            "<validated-instance-root>/proof.txt",
            "Unreal object /Game/Maps/World/L_World",
        ):
            with self.subTest(value=value):
                self.assertFalse(provenance.contains_local_absolute_path(value))

    def test_committed_reports_match_fresh_render_and_abiotic_factor_has_59_items(self) -> None:
        failures, summaries = provenance.collect_summaries(provenance.MODULES_DIR)
        expected = {
            "game-config-source-ledger.csv": provenance.render_csv_report(summaries),
            "game-config-source-ledger.md": provenance.render_markdown_report(summaries),
        }
        abiotic = next(summary for summary in summaries if summary.module_id == "abioticfactor")

        self.assertEqual(failures, [])
        self.assertEqual(provenance.check_reports(provenance.DOCS_DIR, expected), [])
        self.assertEqual((abiotic.schema_fields, abiotic.ledger_items), (59, 59))


if __name__ == "__main__":
    unittest.main()
