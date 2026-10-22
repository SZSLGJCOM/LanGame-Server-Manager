"""Exercise the real Windows builder entry points before any toolchain runs."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
BUILDERS = {
    "build_ue4ss_runtime.ps1": "ue4ss-runtime",
    "build_enshrouded_control.ps1": "enshrouded-control",
    "build_valheim_control.ps1": "valheim-control",
    "build_dragonwilds_player_query.ps1": "dragonwilds-player-query",
    "build_windrose_save_control.ps1": "windrose-save-control",
    "build_unreal_save_control.ps1": "native-world-save",
    "build_scum_save_control.ps1": "scum-save-control",
    "build_unreal_loader_proxies.ps1": "unreal-loader-proxies",
}


class ControlBuildOutputTests(unittest.TestCase):
    def test_all_builders_share_validation_and_cargo_tracks_it(self):
        for name, leaf in BUILDERS.items():
            with self.subTest(builder=name):
                source = (ROOT / "scripts" / name).read_text(encoding="utf-8")
                self.assertIn(". (Join-Path $PSScriptRoot 'build_control_output.ps1')", source)
                self.assertIn(f"-CargoLeaf '{leaf}'", source)
                effect = source.index("CreateDirectory") if "CreateDirectory" in source else source.index("& (Join-Path $projectRoot")
                self.assertLess(source.index("Resolve-ControlBuildOutput"), effect)
        build = (ROOT / "apps/desktop/src-tauri/build.rs").read_text(encoding="utf-8")
        self.assertIn('root.join("scripts/build_control_output.ps1").display()', build)

    @unittest.skipUnless(os.name == "nt", "Windows path and junction semantics require Windows PowerShell")
    def test_real_builder_path_boundaries(self):
        powershell = Path(os.environ["SystemRoot"]) / "System32/WindowsPowerShell/v1.0/powershell.exe"
        with tempfile.TemporaryDirectory(prefix="c-", dir=os.environ.get("LGSM_BUILD_PATH_TEST_ROOT")) as directory:
            result = subprocess.run(
                [str(powershell), "-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass",
                 "-File", str(ROOT / "scripts/tests/test_control_build_output.ps1"), "-TestRoot", directory],
                cwd=ROOT, capture_output=True, text=True, timeout=60, check=False,
            )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        summary = json.loads(result.stdout)
        self.assertEqual(summary["checks"], 200)
        self.assertEqual(summary["failed"], 0)


if __name__ == "__main__":
    unittest.main()
