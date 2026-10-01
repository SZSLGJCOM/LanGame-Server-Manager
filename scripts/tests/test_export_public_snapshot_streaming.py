from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]


class PublicSnapshotStreamingTests(unittest.TestCase):
    def test_large_archive_batch_does_not_deadlock_on_windows_pipe_padding(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            repository = base / "repository"
            source = repository / "src"
            source.mkdir(parents=True)
            subprocess.run(["git", "init", "--quiet"], cwd=repository, check=True)
            subprocess.run(
                ["git", "config", "core.autocrlf", "false"],
                cwd=repository,
                check=True,
            )

            payload = b"snapshot-stream-regression\n" + (b"x" * 32_768)
            for index in range(256):
                (source / f"file-{index:03}.txt").write_bytes(payload)

            subprocess.run(["git", "add", "src"], cwd=repository, check=True)
            subprocess.run(
                [
                    "git",
                    "-c",
                    "user.name=Snapshot Test",
                    "-c",
                    "user.email=snapshot-test@example.invalid",
                    "commit",
                    "--quiet",
                    "-m",
                    "large archive fixture",
                ],
                cwd=repository,
                check=True,
            )

            output = base / "public-snapshot"
            child = subprocess.run(
                [
                    sys.executable,
                    "-B",
                    "-c",
                    (
                        "from pathlib import Path; "
                        "from scripts.export_public_snapshot import "
                        "capture_head_commit, discover_head_candidate_paths, "
                        "write_snapshot_from_head; "
                        "root=Path(__import__('sys').argv[1]); "
                        "output=Path(__import__('sys').argv[2]); "
                        "head=capture_head_commit(root); "
                        "paths=discover_head_candidate_paths(root, head); "
                        "write_snapshot_from_head(root, output, head, paths)"
                    ),
                    str(repository),
                    str(output),
                ],
                cwd=REPOSITORY_ROOT,
                check=False,
                capture_output=True,
                text=True,
                timeout=30,
            )

            self.assertEqual(child.returncode, 0, child.stderr)
            self.assertEqual(
                len(list((output / "src").glob("file-*.txt"))),
                256,
            )


if __name__ == "__main__":
    unittest.main()
