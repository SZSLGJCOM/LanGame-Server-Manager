from pathlib import Path
import json
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.export_public_snapshot import (
    PUBLIC_SNAPSHOT_REQUIRED_PATHS,
    SnapshotPlan,
    SnapshotError,
    build_head_snapshot_plan,
    capture_head_commit,
    exclusion_reason,
    scan_snapshot_files,
    validate_public_snapshot_plan,
    write_snapshot_from_head,
)
from scripts.third_party_asset_policy import (
    ARK_MAP_ARTWORK_NOTICE,
    ARCHIVE_FIXTURE_COMPANION_SHA256,
    ARCHIVE_FIXTURE_ROOT,
    ARCHIVE_FIXTURE_SHA256,
    INTER_LICENSE_PATH,
    SQLX_VENDOR_REQUIRED_PATHS,
    SQLX_VENDOR_ROOT,
    THIRD_PARTY_ASSET_LICENSES,
    THIRD_PARTY_ASSET_SHA256,
    THIRD_PARTY_ARCHIVE_FIXTURES,
    THIRD_PARTY_REFERENCE_MEDIA,
    REFERENCE_MEDIA_SHA256,
    verify_third_party_asset_bytes,
)
from scripts.verify_open_source_boundary import is_unapproved_asset_path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
REVIEWED_PATHS = tuple(Path(path) for path in THIRD_PARTY_ASSET_SHA256)
ARCHIVE_PATHS = tuple(Path(path) for path in ARCHIVE_FIXTURE_SHA256 | ARCHIVE_FIXTURE_COMPANION_SHA256)


def write_reviewed_assets(repository: Path) -> None:
    for path in REVIEWED_PATHS:
        target = repository / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((REPOSITORY_ROOT / path).read_bytes())


def change_last_byte(path: Path) -> None:
    payload = path.read_bytes()
    path.write_bytes(payload[:-1] + bytes([payload[-1] ^ 1]))


class ThirdPartyAssetPolicyTests(unittest.TestCase):
    def test_sqlx_vendor_is_complete_and_rejects_changed_source_or_missing_license(self):
        paths = tuple(sorted(
            path.relative_to(REPOSITORY_ROOT)
            for path in (REPOSITORY_ROOT / SQLX_VENDOR_ROOT).rglob("*") if path.is_file()
        ))
        self.assertEqual(verify_third_party_asset_bytes(REPOSITORY_ROOT, paths), [])
        scan_snapshot_files(REPOSITORY_ROOT, paths)
        self.assertTrue(SQLX_VENDOR_REQUIRED_PATHS <= PUBLIC_SNAPSHOT_REQUIRED_PATHS)
        for path in paths:
            self.assertIsNone(exclusion_reason(path, frozenset()))
        missing_license = tuple(path for path in paths if path.name != "LICENSE-MIT")
        with self.assertRaisesRegex(SnapshotError, "third-party-source-integrity"):
            scan_snapshot_files(REPOSITORY_ROOT, missing_license)
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            for path in paths:
                target = repository / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((REPOSITORY_ROOT / path).read_bytes())
            change_last_byte(repository / SQLX_VENDOR_ROOT / "src/lib.rs")
            with self.assertRaisesRegex(SnapshotError, "third-party-source-integrity"):
                scan_snapshot_files(repository, paths)

    def test_archive_fixtures_match_the_source_ledger_and_require_snapshot_inclusion(self):
        sources = json.loads((REPOSITORY_ROOT / ARCHIVE_FIXTURE_ROOT / "sources.json").read_text())
        self.assertEqual(
            {f"{ARCHIVE_FIXTURE_ROOT}/{item['fixture']}": item["sha256"] for item in sources},
            ARCHIVE_FIXTURE_SHA256,
        )
        self.assertEqual(len(THIRD_PARTY_ARCHIVE_FIXTURES), 8)
        self.assertEqual(verify_third_party_asset_bytes(REPOSITORY_ROOT, ARCHIVE_PATHS), [])
        scan_snapshot_files(REPOSITORY_ROOT, ARCHIVE_PATHS)
        for path in ARCHIVE_PATHS:
            self.assertIn(path.as_posix(), PUBLIC_SNAPSHOT_REQUIRED_PATHS)
            self.assertIsNone(exclusion_reason(path, frozenset()))
            self.assertFalse(is_unapproved_asset_path(path))
        for name in ("unreviewed.rar", "unreviewed.7z"):
            path = Path(ARCHIVE_FIXTURE_ROOT) / name
            self.assertEqual(exclusion_reason(path, frozenset()), "unlicensed-asset-or-binary")
            self.assertTrue(is_unapproved_asset_path(path))
        for omitted in ARCHIVE_PATHS:
            with self.subTest(omitted=omitted):
                plan = SnapshotPlan(
                    included=tuple(Path(path) for path in PUBLIC_SNAPSHOT_REQUIRED_PATHS if path != omitted.as_posix()),
                    excluded=(),
                )
                with self.assertRaisesRegex(SnapshotError, "missing required path"):
                    validate_public_snapshot_plan(plan)

    def test_archive_fixtures_require_all_license_and_provenance_companions(self):
        for companion in ARCHIVE_FIXTURE_COMPANION_SHA256:
            with self.subTest(companion=companion):
                selected = tuple(path for path in ARCHIVE_PATHS if path.as_posix() != companion)
                findings = verify_third_party_asset_bytes(REPOSITORY_ROOT, selected)
                self.assertEqual(
                    [(item.rule, item.path) for item in findings],
                    [("third-party-fixture-notice", companion)],
                )

    def test_replaced_archive_or_companion_bytes_are_rejected(self):
        for changed in ARCHIVE_PATHS:
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                for path in ARCHIVE_PATHS:
                    target = repository / path
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_bytes((REPOSITORY_ROOT / path).read_bytes())
                change_last_byte(repository / changed)
                findings = verify_third_party_asset_bytes(repository, ARCHIVE_PATHS)
                self.assertEqual(
                    [(item.rule, item.path) for item in findings],
                    [("third-party-asset-integrity", changed.as_posix())],
                )

    def test_head_snapshot_preserves_archive_fixtures_and_their_legal_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            repository.mkdir()
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            (repository / ".gitattributes").write_bytes((REPOSITORY_ROOT / ".gitattributes").read_bytes())
            for path in ARCHIVE_PATHS:
                target = repository / path
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes((REPOSITORY_ROOT / path).read_bytes())
            subprocess.run(["git", "add", "."], cwd=repository, check=True)
            subprocess.run([
                "git", "-c", "user.name=Asset Policy Test", "-c",
                "user.email=asset-policy@example.invalid", "commit", "--quiet", "-m", "fixture",
            ], cwd=repository, check=True)
            source_commit = capture_head_commit(repository)
            plan = build_head_snapshot_plan(repository, source_commit)
            self.assertTrue(set(ARCHIVE_PATHS) <= set(plan.included))
            output = base / "snapshot"
            write_snapshot_from_head(repository, output, source_commit, plan.included)
            for path in ARCHIVE_PATHS:
                self.assertEqual((output / path).read_bytes(), (REPOSITORY_ROOT / path).read_bytes())

    def test_reference_maps_match_the_catalog_and_keep_their_separate_rights_notice(self):
        selected = tuple(Path(path) for path in REFERENCE_MEDIA_SHA256)
        manifest = json.loads((REPOSITORY_ROOT / "assets/ark-map-artwork/2026-10-25/manifest.json").read_text())
        self.assertEqual({item["relativePath"] for item in manifest["images"]}, THIRD_PARTY_REFERENCE_MEDIA)
        self.assertEqual(
            {item["relativePath"]: item["sha256"] for item in manifest["images"]},
            {path: REFERENCE_MEDIA_SHA256[path] for path in THIRD_PARTY_REFERENCE_MEDIA},
        )
        self.assertEqual(verify_third_party_asset_bytes(REPOSITORY_ROOT, selected), [])
        scan_snapshot_files(REPOSITORY_ROOT, selected)
        without_notice = tuple(path for path in selected if path.as_posix() != ARK_MAP_ARTWORK_NOTICE)
        with self.assertRaisesRegex(SnapshotError, "third-party-asset-notice"):
            scan_snapshot_files(REPOSITORY_ROOT, without_notice)

    def test_replaced_reference_map_is_rejected_without_reclassifying_it_as_project_artwork(self):
        image = Path("assets/ark-map-artwork/2026-10-25/The_Center_Topographic_Map.jpg")
        selected = (image, Path(ARK_MAP_ARTWORK_NOTICE))
        self.assertNotIn(image.as_posix(), THIRD_PARTY_ASSET_LICENSES)
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            for path in selected:
                (repository / path).parent.mkdir(parents=True, exist_ok=True)
                (repository / path).write_bytes((REPOSITORY_ROOT / path).read_bytes())
            change_last_byte(repository / image)
            with self.assertRaisesRegex(SnapshotError, "third-party-asset-integrity"):
                scan_snapshot_files(repository, selected)

    def test_current_reviewed_files_pass_the_shared_check_and_snapshot_scan(self):
        self.assertEqual(verify_third_party_asset_bytes(REPOSITORY_ROOT, REVIEWED_PATHS), [])
        scan_snapshot_files(REPOSITORY_ROOT, REVIEWED_PATHS)
        self.assertIn("scripts/third_party_asset_policy.py", PUBLIC_SNAPSHOT_REQUIRED_PATHS)
        self.assertEqual(
            set(THIRD_PARTY_ASSET_SHA256),
            set(THIRD_PARTY_ASSET_LICENSES) | set(THIRD_PARTY_ASSET_LICENSES.values()),
        )

    def test_same_name_replacements_of_either_font_or_license_are_rejected(self):
        for path in REVIEWED_PATHS:
            with self.subTest(path=path), tempfile.TemporaryDirectory() as directory:
                repository = Path(directory)
                write_reviewed_assets(repository)
                change_last_byte(repository / path)
                findings = verify_third_party_asset_bytes(repository, REVIEWED_PATHS)
                self.assertEqual(
                    [(item.rule, item.path) for item in findings],
                    [("third-party-asset-integrity", path.as_posix())],
                )
                with self.assertRaisesRegex(SnapshotError, "third-party-asset-integrity"):
                    scan_snapshot_files(repository, REVIEWED_PATHS)

    def test_font_requires_selected_license_even_when_license_exists_on_disk(self):
        font_paths = tuple(Path(path) for path in THIRD_PARTY_ASSET_LICENSES)
        findings = verify_third_party_asset_bytes(REPOSITORY_ROOT, font_paths)
        self.assertEqual(
            [(item.rule, item.path) for item in findings],
            [("third-party-asset-license", INTER_LICENSE_PATH)],
        )
        with self.assertRaisesRegex(SnapshotError, "third-party-asset-license"):
            scan_snapshot_files(REPOSITORY_ROOT, font_paths)

    def test_selected_missing_license_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            write_reviewed_assets(repository)
            (repository / INTER_LICENSE_PATH).unlink()
            findings = verify_third_party_asset_bytes(repository, REVIEWED_PATHS)
        self.assertEqual(
            [(item.rule, item.path) for item in findings],
            [("unreadable-source-file", INTER_LICENSE_PATH)],
        )

    def test_unrelated_snapshots_do_not_require_fonts_and_license_alone_is_valid(self):
        self.assertEqual(verify_third_party_asset_bytes(REPOSITORY_ROOT, (Path("README.md"),)), [])
        self.assertEqual(
            verify_third_party_asset_bytes(REPOSITORY_ROOT, (Path(INTER_LICENSE_PATH),)), [],
        )

    def test_safe_reader_refuses_symlinks_without_reading_the_target(self):
        with patch.object(Path, "is_symlink", return_value=True), patch.object(Path, "read_bytes") as read:
            findings = verify_third_party_asset_bytes(REPOSITORY_ROOT, (Path(INTER_LICENSE_PATH),))
        self.assertEqual([item.rule for item in findings], ["non-regular-source-file"])
        read.assert_not_called()

    def test_read_failure_does_not_leak_private_operating_system_details(self):
        with patch.object(Path, "read_bytes", side_effect=PermissionError("private path or credential")):
            findings = verify_third_party_asset_bytes(REPOSITORY_ROOT, (Path(INTER_LICENSE_PATH),))
        self.assertEqual([item.rule for item in findings], ["unreadable-source-file"])
        self.assertNotIn("private path or credential", str(findings))

    def test_head_export_rejects_unreviewed_bytes_or_omitted_license_and_cleans_staging(self):
        for rejected in (*REVIEWED_PATHS, None):
            with self.subTest(rejected=rejected), tempfile.TemporaryDirectory() as directory:
                base = Path(directory)
                repository = base / "repository"
                repository.mkdir()
                subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
                (repository / ".gitattributes").write_bytes((REPOSITORY_ROOT / ".gitattributes").read_bytes())
                write_reviewed_assets(repository)
                if rejected is not None:
                    change_last_byte(repository / rejected)
                subprocess.run(["git", "add", "."], cwd=repository, check=True)
                subprocess.run([
                    "git", "-c", "user.name=Asset Policy Test", "-c",
                    "user.email=asset-policy@example.invalid", "commit", "--quiet", "-m", "fixture",
                ], cwd=repository, check=True)
                source_commit = capture_head_commit(repository)
                # Repaired working files must not hide invalid bytes stored in the selected HEAD.
                if rejected is not None:
                    (repository / rejected).write_bytes((REPOSITORY_ROOT / rejected).read_bytes())
                selected = REVIEWED_PATHS if rejected is not None else tuple(
                    path for path in REVIEWED_PATHS if path.as_posix() != INTER_LICENSE_PATH
                )
                expected = "third-party-asset-integrity" if rejected is not None else "third-party-asset-license"
                with self.assertRaisesRegex(SnapshotError, expected):
                    write_snapshot_from_head(repository, base / "snapshot", source_commit, selected)
                self.assertEqual(list(base.iterdir()), [repository])


if __name__ == "__main__":
    unittest.main()
