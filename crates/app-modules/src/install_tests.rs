use super::*;

#[test]
fn rimworld_release_integrity_reaches_the_install_contract() {
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
        Some(
            "https://github.com/RimWorld-Together/Rimworld-Together/releases/download/26.8.31.1/Server-win-x64.zip"
        )
    );
    let integrity = install.download_integrity_windows.as_ref().unwrap();
    assert_eq!(
        integrity.sha256,
        "f16c703f1e3e4e6de0f4877d8ae502a817c85482681956aa01e8a4b619bab68c"
    );
    assert_eq!(integrity.size, 29443247);
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
