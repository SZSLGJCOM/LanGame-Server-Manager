#![cfg(test)]

use super::*;

#[test]
fn rimworld_release_integrity_reaches_the_install_contract() {
    let release: serde_json::Value = serde_json::from_str(include_str!(
        "../../../modules/rimworld/reference-fixtures/2026-11-23-together-hotfix.json"
    ))
    .unwrap();
    let modules = discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules"))
        .expect("load module catalog");
    let install = modules
        .iter()
        .find(|module| module.summary.id == "rimworld")
        .unwrap()
        .install
        .as_ref()
        .unwrap();
    assert_eq!(
        install.download_url_windows.as_deref(),
        release["download_url"].as_str()
    );
    let integrity = install.download_integrity_windows.as_ref().unwrap();
    assert_eq!(
        integrity.sha256,
        release["archive_sha256"].as_str().unwrap()
    );
    assert_eq!(integrity.size, release["archive_bytes"].as_u64().unwrap());
}

#[test]
fn invalid_archive_integrity_rejects_the_manifest() {
    for integrity in [
        "sha256 = 'invalid'\nsize = 1",
        "size = 1",
        "sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
        "sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\nsize = 0",
    ] {
        let manifest = format!(
            "id = 'fixture'\nname = 'Fixture'\nversion = '1'\n\
             [install]\nshared_game_dir = 'fixture'\ndownload_url_windows = 'https://example.invalid/server.zip'\n\
             [install.download_integrity_windows]\n{integrity}\n"
        );
        assert!(
            toml::from_str::<ModuleToml>(&manifest).is_err(),
            "{integrity}"
        );
    }
}
