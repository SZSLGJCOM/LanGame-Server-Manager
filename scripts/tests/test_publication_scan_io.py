from contextlib import redirect_stdout
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from scripts.export_public_snapshot import SnapshotError, scan_snapshot_files
from scripts.open_source_product_policy import scan_product_surfaces
from scripts.verify_no_tracked_secrets import run_scan, scan_paths
from scripts.verify_open_source_boundary import tracked_worktree_paths, verify_boundary


class PublicationScanReadTests(unittest.TestCase):
    def test_deleted_worktree_files_are_not_scan_candidates(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            source = repository / "config.txt"
            source.write_text("source\n", encoding="utf-8")
            subprocess.run(["git", "add", "config.txt"], cwd=repository, check=True)
            source.unlink()

            with redirect_stdout(io.StringIO()):
                result = run_scan(repository)

            self.assertEqual(result, 0)
            self.assertEqual(tracked_worktree_paths(repository), ())

    def test_secret_scan_reports_unreadable_source_without_error_content(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "config.json"
            source.write_text("{}", encoding="utf-8")
            with patch.object(Path, "read_bytes", side_effect=PermissionError("private detail")):
                findings = scan_paths([source])

        self.assertEqual([(item.path, item.rule) for item in findings], [(source, "unreadable-source-file")])
        self.assertNotIn("private detail", str(findings))

    def test_snapshot_scan_rejects_missing_selected_file(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(SnapshotError, "unreadable-source-file"):
                scan_snapshot_files(Path(directory), (Path("missing.txt"),))

    def test_product_scan_rejects_non_regular_source(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory)
            source = Path("docs/source.md")
            (repository / source).mkdir(parents=True)

            findings = scan_product_surfaces(repository, (source,))

        self.assertEqual([(item.path, item.rule) for item in findings], [(source, "non-regular-source-file")])

    def test_product_and_boundary_scans_refuse_paths_outside_repository(self):
        with tempfile.TemporaryDirectory() as directory:
            repository = Path(directory) / "repository"
            repository.mkdir()
            source = Path("../outside.md")
            (repository / source).write_text("private text", encoding="utf-8")

            product_findings = scan_product_surfaces(repository, (source,))
            boundary_findings = verify_boundary(repository, (source,), frozenset())

        self.assertEqual([item.rule for item in product_findings], ["unsafe-source-path"])
        self.assertIn("unsafe-source-path", {item.rule for item in boundary_findings})

    def test_secret_scan_rejects_symbolic_links_without_reading_targets(self):
        source = Path("linked.json")
        with patch.object(Path, "is_symlink", return_value=True), patch.object(Path, "read_bytes") as read:
            findings = scan_paths([source])

        self.assertEqual([item.rule for item in findings], ["non-regular-source-file"])
        read.assert_not_called()


if __name__ == "__main__":
    unittest.main()
