from pathlib import Path


PROJECT_LICENSE_TEXT = """LanGame Source-Available License 1.0

1. Scope
2. Permitted noncommercial use
3. Commercial use requires separate permission
4. No independent modified releases
5. Contributions
6. Notices and reserved rights
7. Termination
8. Disclaimer
"""


def write_project_license_fixture(repository: Path) -> None:
    (repository / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["apps/desktop/src-tauri", "crates/fixture"]\n'
        '[workspace.package]\nlicense-file = "LICENSE"\n',
        encoding="utf-8",
    )
    for member in ("apps/desktop/src-tauri", "crates/fixture"):
        manifest = repository / member / "Cargo.toml"
        manifest.parent.mkdir(parents=True, exist_ok=True)
        manifest.write_text('[package]\nlicense-file.workspace = true\n', encoding="utf-8")
    (repository / "LICENSE").write_bytes(PROJECT_LICENSE_TEXT.encode("utf-8"))
    (repository / "apps/desktop/LICENSE").write_bytes(PROJECT_LICENSE_TEXT.encode("utf-8"))
    (repository / "apps/desktop/package.json").write_text(
        '{"private":true,"license":"SEE LICENSE IN LICENSE"}', encoding="utf-8",
    )
    (repository / "apps/desktop/package-lock.json").write_text(
        '{"packages":{"":{"license":"SEE LICENSE IN LICENSE"},'
        '"node_modules/third-party":{"license":"MIT"}}}', encoding="utf-8",
    )
