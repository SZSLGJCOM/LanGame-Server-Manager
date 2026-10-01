use super::*;
use serde_json::json;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lg-windrose-world-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("runtime")).unwrap();
        Self(root)
    }

    fn runtime(&self) -> PathBuf {
        self.0.join("runtime")
    }

    fn write(&self, relative: &Path, bytes: &[u8]) {
        let path = self.runtime().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn server(&self, id: &str) {
        self.write(
            Path::new(SERVER),
            &serde_json::to_vec(&json!({
                "ServerDescription_Persistent": {"WorldIslandId": id, "Password": "synthetic-fixture"}
            }))
            .unwrap(),
        );
    }

    fn world(&self, version: &str, folder: &str, id: &str) {
        self.write(
            &Path::new(DATABASE)
                .join(version)
                .join("Worlds")
                .join(folder)
                .join("WorldDescription.json"),
            &serde_json::to_vec(&json!({"Version": 1, "WorldDescription": {"islandId": id}}))
                .unwrap(),
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.0.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn existing_acceptance_windrose_probe_reads_existing_world_without_mutation() {
    let fixture = Fixture::new();
    fixture.server("existing-world");
    fixture.world("build-a", "existing-world", "existing-world");
    let runtime = fixture.runtime().canonicalize().unwrap();
    let server_before = fs::read(runtime.join(SERVER)).unwrap();
    assert!(require_fresh(&runtime, &fixture.0).is_err());
    assert!(ready_existing(&runtime).unwrap());
    assert_eq!(fs::read(runtime.join(SERVER)).unwrap(), server_before);
    fixture.world("build-a", "existing-world", "another-world");
    assert!(!ready_existing(&runtime).unwrap());
}

#[test]
fn windrose_world_probe_requires_native_matching_world_and_rejects_preexisting_fresh_data() {
    let fixture = Fixture::new();
    require_fresh(&fixture.runtime(), &fixture.0).unwrap();
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.server("");
    require_fresh(&fixture.runtime(), &fixture.0).unwrap();
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());

    // Storage accepts a safe folder identity rather than imposing a GUID schema.
    fixture.server("native-world");
    assert!(require_fresh(&fixture.runtime(), &fixture.0).is_err());
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.world("build-a", "native-world", "different-world");
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.world("build-a", "native-world", "native-world");
    assert!(ready(&fixture.runtime(), &fixture.0).unwrap());
    assert!(
        ready(&fixture.runtime(), &fixture.0).unwrap(),
        "restart can reuse a native world"
    );
    fixture.server("");
    assert!(
        require_fresh(&fixture.runtime(), &fixture.0).is_err(),
        "unselected old worlds are not fresh"
    );
}

#[test]
fn windrose_world_probe_requires_case_sensitive_native_identity() {
    let fixture = Fixture::new();
    fixture.server("native-world");
    let path = Path::new(DATABASE).join("build-a/Worlds/native-world/WorldDescription.json");
    fixture.write(
        &path,
        &serde_json::to_vec(&json!({
            "Version": 1,
            "WorldDescription": {"IslandId": "native-world"}
        }))
        .unwrap(),
    );
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.world("build-a", "native-world", "native-world");
    assert!(ready(&fixture.runtime(), &fixture.0).unwrap());
}

#[test]
fn windrose_world_probe_waits_for_complete_json_and_rejects_ambiguous_versions() {
    let fixture = Fixture::new();
    fixture.write(Path::new(SERVER), b"{");
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.server("native-world");
    let first = Path::new(DATABASE).join("build-a/Worlds/native-world/WorldDescription.json");
    fixture.write(&first, b"{");
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.write(
        &first,
        b"\xEF\xBB\xBF{\"WorldDescription\":{\"islandId\":\"native-world\"}}",
    );
    assert!(ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.world("build-b", "native-world", "native-world");
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
    fixture.write(
        &Path::new(DATABASE).join("build-b/Worlds/native-world/WorldDescription.json"),
        b"{",
    );
    assert!(!ready(&fixture.runtime(), &fixture.0).unwrap());
}

#[test]
fn windrose_world_probe_bounds_documents_and_version_inventory_without_printing_identity() {
    let fixture = Fixture::new();
    fixture.write(
        Path::new(SERVER),
        &vec![b' '; MAX_DOCUMENT_BYTES as usize + 1],
    );
    assert!(ready(&fixture.runtime(), &fixture.0).is_err());
    fixture.server("native-world");
    for index in 0..=MAX_VERSION_ENTRIES {
        fs::create_dir_all(
            fixture
                .runtime()
                .join(DATABASE)
                .join(format!("build-{index}")),
        )
        .unwrap();
    }
    let error = ready(&fixture.runtime(), &fixture.0).unwrap_err();
    assert!(!error.contains("native-world"));
    assert!(!error.contains("fixture-secret"));
    assert!(require_fresh(&fixture.runtime(), &fixture.0).is_err());
}

#[test]
fn windrose_world_probe_rejects_traversal_and_nonfixture_roots() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    for id in [
        "../fixture-secret",
        "..\\fixture-secret",
        ".",
        "..",
        " leading",
    ] {
        fixture.server(id);
        let error = ready(&fixture.runtime(), &fixture.0).unwrap_err();
        assert!(!error.contains(id));
        assert!(!error.contains("fixture-secret"));
    }
    assert!(ready(&other.runtime(), &fixture.0).is_err());
    assert!(require_fresh(&other.runtime(), &fixture.0).is_err());
}

#[test]
fn windrose_world_probe_bounds_fresh_world_directory_entries() {
    let fixture = Fixture::new();
    let worlds = fixture.runtime().join(DATABASE).join("build-a/Worlds");
    for index in 0..=MAX_WORLD_ENTRIES {
        fs::create_dir_all(worlds.join(format!("world-{index}"))).unwrap();
    }
    assert!(require_fresh(&fixture.runtime(), &fixture.0).is_err());
}

#[cfg(windows)]
#[test]
fn windrose_world_probe_refuses_a_junction_before_reading_native_documents() {
    use std::os::windows::process::CommandExt;
    let fixture = Fixture::new();
    let other = Fixture::new();
    let junction = fixture.runtime().join("R5");
    let output = std::process::Command::new("cmd")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(other.runtime())
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "could not create the disposable junction fixture"
    );
    let readiness = ready(&fixture.runtime(), &fixture.0);
    let fresh = require_fresh(&fixture.runtime(), &fixture.0);
    fs::remove_dir(&junction).unwrap();
    assert!(readiness.is_err());
    assert!(fresh.is_err());
    assert!(other.runtime().is_dir());
}
