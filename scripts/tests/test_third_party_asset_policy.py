from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.export_public_snapshot import (
    PUBLIC_SNAPSHOT_REQUIRED_PATHS,
    SnapshotError,
    capture_head_commit,
    scan_snapshot_files,
    write_snapshot_from_head,
)
from scripts.third_party_asset_policy import (
    INTER_LICENSE_PATH,
    THIRD_PARTY_ASSET_LICENSES,
    THIRD_PARTY_ASSET_SHA256,
    verify_third_party_asset_bytes,
)


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
REVIEWED_PATHS = tuple(Path(path) for path in THIRD_PARTY_ASSET_SHA256)


def write_reviewed_assets(repository: Path) -> None:
    for path in REVIEWED_PATHS:
        target = repository / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((REPOSITORY_ROOT / path).read_bytes())


def change_last_byte(path: Path) -> None:
    payload = path.read_bytes()
    path.write_bytes(payload[:-1] + bytes([payload[-1] ^ 1]))


class ThirdPartyAssetPolicyTests(unittest.TestCase):
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
