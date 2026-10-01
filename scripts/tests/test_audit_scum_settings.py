import subprocess
import sys
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class ScumRepositoryContractTests(unittest.TestCase):
    def test_repository_verification_does_not_require_internal_ledger_writer(self) -> None:
        script = ROOT / "scripts" / "audit_scum_settings.py"
        bootstrap = """
import builtins
import runpy
import sys

real_import = builtins.__import__

def isolated_import(name, *args, **kwargs):
    if name == "audit_scum_ledger":
        raise ModuleNotFoundError(name)
    return real_import(name, *args, **kwargs)

builtins.__import__ = isolated_import
sys.argv = [sys.argv[1], *sys.argv[2:]]
runpy.run_path(sys.argv[0], run_name="__main__")
"""
        completed = subprocess.run(
            [
                sys.executable,
                "-B",
                "-c",
                bootstrap,
                str(script),
                "--verify-repository",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stdout + completed.stderr,
        )

    def test_exact_inventory_schema_ledger_and_writers_agree(self) -> None:
        completed = subprocess.run(
            [
                sys.executable,
                "-B",
                str(ROOT / "scripts" / "audit_scum_settings.py"),
                "--verify-repository",
            ],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )

        self.assertEqual(
            completed.returncode,
            0,
            completed.stdout + completed.stderr,
        )


if __name__ == "__main__":
    unittest.main()
