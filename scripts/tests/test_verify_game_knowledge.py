from __future__ import annotations

import copy
import datetime as dt
import json
import tempfile
import unittest
from pathlib import Path

from scripts import verify_game_knowledge as knowledge

TODAY = dt.date(2026, 9, 28)


def manifest() -> dict:
    return {
        "schema_version": 1, "module_id": "fixture", "scope": "Dedicated-server manual", "gaps": [],
        "sources": [{
            "id": "manual", "title": "Official server manual", "authority": "Game publisher",
            "kind": "official", "seeds": ["https://docs.example.com/servers/index"],
            "allowed_prefixes": ["https://docs.example.com/servers/"],
            "discover_links": True, "max_pages": 256,
            "authority_evidence": "https://www.example.com/server",
            "license_note": "Publisher copyright; retain attribution.",
            "reviewed_on": "2026-09-28",
        }],
    }


def write_module(root: Path, name: str = "fixture", data: dict | None = None) -> Path:
    module = root / "modules" / name
    module.mkdir(parents=True)
    (module / "module.toml").write_text(f'id = "{name}"\n', encoding="utf-8")
    data = manifest() if data is None else data
    lines = [f"{key} = {json.dumps(value)}" for key, value in data.items() if key != "sources"]
    for source in data["sources"]:
        lines.append("[[sources]]")
        lines.extend(f"{key} = {json.dumps(value)}" for key, value in source.items())
    (module / "knowledge-sources.toml").write_text("\n".join(lines), encoding="utf-8")
    return module


class SourceManifestTests(unittest.TestCase):
    def test_discovery_selector_is_optional_independent_and_bounded(self) -> None:
        data = manifest()
        self.assertEqual(knowledge.validate_manifest(data, "fixture", TODAY)[0], [])
        data["sources"][0]["content_selector"] = "article"
        data["sources"][0]["discovery_selector"] = "#official-guides a[href]"
        self.assertEqual(knowledge.validate_manifest(data, "fixture", TODAY)[0], [])
        data["sources"][0]["discover_links"] = False
        self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])
        data["sources"][0]["discover_links"] = True
        for invalid in (None, True, [], "", "a\n[href]", "a" * 1025, "界" * 342):
            with self.subTest(value=invalid):
                data["sources"][0]["discovery_selector"] = invalid
                self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_reference_only_requires_an_explicit_boolean(self) -> None:
        data = manifest()
        data["sources"][0]["reference_only"] = True
        self.assertEqual(knowledge.validate_manifest(data, "fixture", TODAY)[0], [])
        data["sources"][0]["reference_only"] = "true"
        self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_valid_manifest_and_exact_query_seed(self) -> None:
        data = manifest()
        data["sources"][0]["seeds"].append("https://docs.example.com/manual?id=123")
        failures, records = knowledge.validate_manifest(data, "fixture", TODAY)
        self.assertEqual(failures, [])
        self.assertEqual(records[0]["seedCount"], 2)

    def test_shipped_module_coverage_is_complete(self) -> None:
        # Shipped sources evolve after the fixed dates used by isolated fixtures.
        failures, records, count = knowledge.verify_catalog(
            knowledge.ROOT, dt.datetime.now(dt.timezone.utc).date()
        )
        self.assertEqual(failures, [])
        self.assertEqual(count, 32)
        self.assertEqual({r["module"] for r in records},
                         {p.parent.name for p in (knowledge.ROOT / "modules").glob("*/module.toml")})

    def test_unsafe_urls_are_rejected(self) -> None:
        cases = [
            "http://docs.example.com/server", "https://user:fixture@docs.example.com/",
            "https://127.0.0.1/", "https://[::1]/", "https://localhost/",
            "https://docs.local/", "https://docs.example.com:443/",
            "https://docs.example.com/servers/../private",
            "https://docs.example.com/servers/%2e%2e/private",
            "https://docs.example.com/servers/%252e%252e/private",
            "https://docs.example.com/servers%2fprivate",
            "https://docs.example.com/servers\\private",
            "https://docs.example.com/%0aInjected",
            "https://docs.example.com/%zz", "https://docs.example.com/#fragment",
        ]
        for url in cases:
            with self.subTest(url=url):
                self.assertFalse(knowledge.source_url(url))

    def test_unicode_path_and_fixed_query_are_supported(self) -> None:
        self.assertTrue(knowledge.source_url("https://docs.example.com/Guide-%E2%80%90-Quickstart.md"))
        self.assertTrue(knowledge.source_url("https://steamcommunity.com/sharedfiles/filedetails/?id=123"))

    def test_prefix_requires_directory_boundary_and_same_seed_origin(self) -> None:
        for prefix in ("https://docs.example.com/servers", "https://docs.example.com/servers/?page=1",
                       "https://other.example.com/servers/"):
            data = manifest()
            data["sources"][0]["allowed_prefixes"] = [prefix]
            self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0], prefix)

    def test_discovery_cannot_be_unbounded(self) -> None:
        data = manifest()
        data["sources"][0]["allowed_prefixes"] = []
        self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])
        data["sources"][0]["discover_links"] = False
        self.assertEqual(knowledge.validate_manifest(data, "fixture", TODAY)[0], [])

    def test_unknown_fields_and_module_rebinding_are_rejected(self) -> None:
        for mutate in (
            lambda d: d.update(extra=True),
            lambda d: d.update(module_id="other"),
            lambda d: d["sources"][0].update(fetch_status="complete"),
            lambda d: d["sources"][0].pop("license_note"),
        ):
            data = manifest()
            mutate(data)
            self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_page_budget_is_integer_and_bounded(self) -> None:
        for value in (0, 513, True, "256"):
            data = manifest()
            data["sources"][0]["max_pages"] = value
            self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_malformed_field_types_are_reported_without_a_crash(self) -> None:
        for field, value in (("kind", []), ("id", {}), ("seeds", [{}]), ("allowed_prefixes", [0])):
            data = manifest()
            data["sources"][0][field] = value
            self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_duplicate_sources_or_urls_are_rejected(self) -> None:
        data = manifest()
        data["sources"].append(copy.deepcopy(data["sources"][0]))
        self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])
        data = manifest()
        data["sources"][0]["seeds"] *= 2
        self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_dates_are_real_canonical_and_nonfuture(self) -> None:
        for value in ("2026-09-29", "2026-02-29", "2026-9-28", "1969-12-31"):
            data = manifest()
            data["sources"][0]["reviewed_on"] = value
            self.assertTrue(knowledge.validate_manifest(data, "fixture", TODAY)[0])

    def test_review_age_does_not_claim_body_freshness(self) -> None:
        data = manifest()
        data["sources"][0]["reviewed_on"] = "2026-08-28"
        errors, records = knowledge.validate_manifest(data, "fixture", TODAY)
        self.assertEqual(errors, [])
        self.assertEqual(records[0]["directoryReview"], "overdue")
        self.assertNotIn("freshness", records[0])

    def test_missing_manifest_or_orphan_is_not_partial_success(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            module = write_module(root)
            (module / "knowledge-sources.toml").unlink()
            self.assertTrue(knowledge.verify_catalog(root, TODAY)[0])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            module = write_module(root)
            (module / "module.toml").unlink()
            errors, _, _ = knowledge.verify_catalog(root, TODAY)
            self.assertTrue(any("orphan" in error for error in errors))

    def test_descriptor_mismatch_and_oversized_file_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            module = write_module(root)
            (module / "module.toml").write_text('id = "other"', encoding="utf-8")
            self.assertTrue(knowledge.verify_catalog(root, TODAY)[0])
            (module / "module.toml").write_text('id = "fixture"', encoding="utf-8")
            (module / "knowledge-sources.toml").write_bytes(b" " * (knowledge.MAX_FILE_BYTES + 1))
            self.assertTrue(knowledge.verify_catalog(root, TODAY)[0])

    def test_empty_and_malformed_catalog_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "modules").mkdir()
            self.assertTrue(knowledge.verify_catalog(root, TODAY)[0])
            module = write_module(root)
            (module / "knowledge-sources.toml").write_bytes(b"\xff")
            self.assertTrue(knowledge.verify_catalog(root, TODAY)[0])


if __name__ == "__main__":
    unittest.main()
