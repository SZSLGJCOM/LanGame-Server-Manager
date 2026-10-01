use super::tests::{TestRoot, fixture_xml};
use super::*;
use std::fs;

const CACHE_IMAGES: &str = "cache/dontstarve-configuration-icons/data/images";

fn schema() -> String {
    serde_json::json!({"properties": {
        "master_weather": {"x-lsgm-section":"mastersettings", "x-lsgm-source-key":"overrides.weather"},
        "caves_weather": {"x-lsgm-section":"cavessettings", "x-lsgm-source-key":"overrides.weather"},
        "master_world_size": {"x-lsgm-section":"mastergen", "x-lsgm-source-key":"overrides.world_size"},
        "caves_world_size": {"x-lsgm-section":"cavesgen", "x-lsgm-source-key":"overrides.world_size"}
    }}).to_string()
}

fn install_artwork(install: &TestRoot, color: [u8; 4]) {
    let texture = texture::tests::texture_fixture(4, 2, 2, &color.repeat(4));
    for (atlas, element) in [
        ("worldgen_customization", "world_size.tex"),
        ("worldsettings_customization", "rain.tex"),
    ] {
        install.write(
            &format!("data/images/{atlas}.xml"),
            &fixture_xml(atlas, element),
        );
        install.write(&format!("data/images/{atlas}.tex"), &texture);
    }
}

#[test]
fn successful_install_artwork_is_cached_and_survives_missing_installation() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255, 0, 0, 255]);
    let loaded = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.icons.len(), 4);
    let cached =
        load_dst_configuration_icons(&install.0.join("missing"), &app.0, &schema()).unwrap();
    assert_eq!(cached.icons, loaded.icons);
    assert!(cached.warnings.is_empty());
    let filenames: Vec<_> = fs::read_dir(app.0.join(CACHE_IMAGES))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(filenames.len(), 4);
    for filename in filenames {
        assert_eq!(
            fs::read(app.0.join(CACHE_IMAGES).join(&filename)).unwrap(),
            fs::read(install.0.join("data/images").join(filename)).unwrap()
        );
    }
    assert_eq!(
        fs::read_dir(app.0.join("cache/dontstarve-configuration-icons/data"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn installed_updates_take_priority_and_replace_both_cached_pairs() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255, 0, 0, 255]);
    let old = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    install_artwork(&install, [0, 0, 255, 255]);
    let updated = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert!(updated.warnings.is_empty());
    assert_ne!(old.icons, updated.icons);
    let cached =
        load_dst_configuration_icons(&install.0.join("missing"), &app.0, &schema()).unwrap();
    assert_eq!(cached.icons, updated.icons);
}

#[test]
fn incomplete_and_corrupt_installed_pairs_use_valid_cache_with_warnings() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255, 0, 0, 255]);
    let original = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    install.write("data/images/worldgen_customization.xml", b"invalid XML");
    fs::remove_file(
        install
            .0
            .join("data/images/worldsettings_customization.tex"),
    )
    .unwrap();
    let loaded = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert_eq!(loaded.icons, original.icons);
    assert_eq!(loaded.warnings.len(), 2);
    assert!(
        loaded
            .warnings
            .iter()
            .all(|warning| warning.contains("installed DST atlas"))
    );
    assert_eq!(
        fs::read(app.0.join(CACHE_IMAGES).join("worldgen_customization.xml")).unwrap(),
        fixture_xml("worldgen_customization", "world_size.tex")
    );
}

#[test]
fn failed_cache_update_preserves_loaded_icons_and_unrelated_cache_files() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255, 0, 0, 255]);
    let original = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    app.write(
        &format!("{CACHE_IMAGES}/operator-notes.txt"),
        b"keep this file",
    );
    install_artwork(&install, [0, 0, 255, 255]);
    let updated = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert_eq!(updated.icons.len(), 4);
    assert_ne!(updated.icons, original.icons);
    assert!(
        updated
            .warnings
            .iter()
            .any(|warning| warning.contains("cache was not updated"))
    );
    assert_eq!(
        fs::read(app.0.join(CACHE_IMAGES).join("operator-notes.txt")).unwrap(),
        b"keep this file"
    );
    let cached =
        load_dst_configuration_icons(&install.0.join("missing"), &app.0, &schema()).unwrap();
    assert_eq!(cached.icons, original.icons);
}

#[test]
fn unsafe_cache_directory_does_not_hide_valid_installed_icons() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255; 4]);
    app.write("cache", b"existing file");
    let loaded = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert_eq!(loaded.icons.len(), 4);
    assert!(!loaded.warnings.is_empty());
    assert_eq!(fs::read(app.0.join("cache")).unwrap(), b"existing file");
}

#[test]
fn malformed_or_oversized_cache_fails_when_no_installed_artwork_exists() {
    let app = TestRoot::new();
    app.write(
        &format!("{CACHE_IMAGES}/worldgen_customization.xml"),
        &vec![b' '; atlas::MAX_ATLAS_XML_BYTES + 1],
    );
    let error =
        load_dst_configuration_icons(&app.0.join("missing"), &app.0, &schema()).unwrap_err();
    assert!(error.contains("cached DST atlas"));
    assert!(error.contains("size limit"));
}

#[test]
fn interrupted_directory_publication_can_read_the_previous_complete_cache() {
    let install = TestRoot::new();
    let app = TestRoot::new();
    install_artwork(&install, [255; 4]);
    let original = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    let data = app.0.join("cache/dontstarve-configuration-icons/data");
    fs::rename(data.join("images"), data.join("images.previous")).unwrap();
    let recovered =
        load_dst_configuration_icons(&install.0.join("missing"), &app.0, &schema()).unwrap();
    assert_eq!(recovered.icons, original.icons);
    assert!(
        recovered
            .warnings
            .iter()
            .any(|warning| warning.contains("interrupted update"))
    );
    let refreshed = load_dst_configuration_icons(&install.0, &app.0, &schema()).unwrap();
    assert_eq!(refreshed.icons, original.icons);
    assert!(refreshed.warnings.is_empty());
    assert!(data.join("images").is_dir());
}
