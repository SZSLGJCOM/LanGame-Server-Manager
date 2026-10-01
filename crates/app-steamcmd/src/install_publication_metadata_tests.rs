use super::*;
use serde_json::json;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = crate::tests::unique_test_root();
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn marker(&self) -> PathBuf {
        self.0.join(RETAINED_LIBRARY_MARKER)
    }

    fn value(&self) -> serde_json::Value {
        json!({
            "version": 1,
            "module_id": "fixture-game",
            "program_root": fs::canonicalize(&self.0).unwrap().to_string_lossy(),
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn retained_library_publication_metadata_accepts_only_bound_bytes() {
    let fixture = Fixture::new();
    assert!(
        InstallPublicationMetadata::read(&fixture.0, "fixture-game", false)
            .unwrap()
            .retained_library_sha256
            .is_none()
    );
    let bytes = serde_json::to_vec_pretty(&fixture.value()).unwrap();
    fs::write(fixture.marker(), &bytes).unwrap();
    let captured = InstallPublicationMetadata::read(&fixture.0, "fixture-game", true).unwrap();
    assert!(captured.preserve_retained_data);
    assert_eq!(
        captured.retained_library_sha256,
        Some(
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect()
        )
    );
    assert_eq!(fs::read(fixture.marker()).unwrap(), bytes);
}

#[test]
fn retained_library_publication_metadata_rejects_invalid_or_foreign_records() {
    let fixture = Fixture::new();
    for (field, value) in [
        ("version", json!(2)),
        ("module_id", json!("other-game")),
        ("program_root", json!(fixture.0.join("other-library"))),
        ("unexpected", json!(true)),
    ] {
        let mut record = fixture.value();
        record[field] = value;
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(fixture.marker(), &bytes).unwrap();
        assert!(InstallPublicationMetadata::read(&fixture.0, "fixture-game", false).is_err());
        assert_eq!(fs::read(fixture.marker()).unwrap(), bytes);
    }
    for bytes in [
        b"invalid JSON".to_vec(),
        b"{\"version\":1,\"version\":1,\"module_id\":\"fixture-game\",\"program_root\":\"invalid\"}".to_vec(),
        vec![b' '; MAX_RETAINED_LIBRARY_BYTES as usize + 1],
    ] {
        fs::write(fixture.marker(), &bytes).unwrap();
        assert!(InstallPublicationMetadata::read(&fixture.0, "fixture-game", false).is_err());
        assert_eq!(fs::read(fixture.marker()).unwrap(), bytes);
    }
    fs::remove_file(fixture.marker()).unwrap();
    fs::create_dir(fixture.marker()).unwrap();
    assert!(InstallPublicationMetadata::read(&fixture.0, "fixture-game", false).is_err());
    assert!(fixture.marker().is_dir());
}
