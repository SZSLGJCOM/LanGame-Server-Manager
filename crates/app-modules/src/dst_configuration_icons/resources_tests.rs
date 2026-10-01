use super::super::tests::TestRoot;
use super::*;

#[test]
fn resource_reader_accepts_only_fixed_atlas_files() {
    let root = TestRoot::new();
    root.write("data/images/worldgen_customization.xml", b"bounded XML");
    root.write("data/images/secret.xml", b"private");
    let resources = Resources::open(&root.0).unwrap().unwrap();
    assert_eq!(
        resources.read("worldgen_customization", "xml", 32).unwrap(),
        Some(b"bounded XML".to_vec())
    );
    assert!(resources.read("../outside", "xml", 32).is_err());
    assert!(resources.read("secret", "xml", 32).is_err());
    assert!(
        resources
            .read("worldgen_customization", "../secret.xml", 32)
            .is_err()
    );
    assert!(
        resources
            .read("worldsettings_customization", "xml", 32)
            .unwrap()
            .is_none()
    );
}

#[test]
fn resource_reader_bounds_file_size_and_actual_read_bytes() {
    let root = TestRoot::new();
    root.write("data/images/worldgen_customization.xml", &[0; 33]);
    let resources = Resources::open(&root.0).unwrap().unwrap();
    assert!(
        resources
            .read("worldgen_customization", "xml", 32)
            .unwrap_err()
            .contains("size limit")
    );
    assert!(
        read_limited(&[0u8; 33][..], 32)
            .unwrap_err()
            .contains("byte limit")
    );
}

#[test]
fn directories_cannot_be_read_as_atlas_files() {
    let root = TestRoot::new();
    root.write("data/images/worldgen_customization.xml/nested", b"");
    let resources = Resources::open(&root.0).unwrap().unwrap();
    assert!(resources.read("worldgen_customization", "xml", 32).is_err());
}

#[cfg(unix)]
#[test]
fn linked_asset_directories_are_rejected() {
    let root = TestRoot::new();
    let outside = TestRoot::new();
    root.write("data/placeholder", b"");
    outside.write("worldgen_customization.xml", b"private");
    std::os::unix::fs::symlink(&outside.0, root.0.join("data/images")).unwrap();
    let resources = Resources::open(&root.0).unwrap().unwrap();
    assert!(
        resources
            .read("worldgen_customization", "xml", 32)
            .unwrap_err()
            .contains("reparse")
    );
}
