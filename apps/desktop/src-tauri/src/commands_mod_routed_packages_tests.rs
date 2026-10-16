use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-routed-package-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(root.join("runtime")).unwrap();
        Self(root)
    }
    fn target(&self) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "test".into(),
            module_id: "valheim".into(),
            source_label: "local".into(),
            target_label: "plugins".into(),
            target_path: self.0.join("runtime/BepInEx/plugins"),
            accepts: vec!["folder".into()],
            id_strategy: None,
        }
    }
    fn source(&self, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let source = self.0.join(name);
        fs::create_dir_all(&source).unwrap();
        for (relative, bytes) in files {
            let path = source.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        source
    }
    fn install(&self, source: PathBuf) -> Result<ManualModStageResult, String> {
        crate::commands::commands_mod_staging::stage_manual_mod_sources(self.target(), vec![source])
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn routed_config_and_patchers_survive_round_trip_and_preserve_operator_bytes() {
    let fixture = Fixture::new();
    let source = fixture.source(
        "Example",
        &[
            ("plugins/Plugin.dll", b"plugin"),
            ("config/Plugin.cfg", b"vendor"),
            ("patchers/Patch.dll", b"patcher"),
        ],
    );
    fixture.install(source.clone()).unwrap();
    let runtime = fixture.0.join("runtime");
    let config = runtime.join("BepInEx/config/Plugin.cfg");
    let patcher = runtime.join("BepInEx/patchers/Patch.dll");
    assert_eq!(fs::read(&patcher).unwrap(), b"patcher");
    fs::write(&config, b"\xef\xbb\xbfoperator=true\r\n").unwrap();
    fixture.install(source).unwrap();
    assert_eq!(fs::read(&config).unwrap(), b"\xef\xbb\xbfoperator=true\r\n");
    management::manage_package(fixture.target(), "local-local-Example", "disable").unwrap();
    assert!(!patcher.exists());
    assert!(!runtime.join("BepInEx/plugins/local-Example").exists());
    assert!(config.exists());
    management::manage_package(fixture.target(), "local-local-Example", "enable").unwrap();
    assert_eq!(fs::read(&patcher).unwrap(), b"patcher");
    management::manage_package(fixture.target(), "local-local-Example", "delete").unwrap();
    assert!(!patcher.exists());
    assert_eq!(fs::read(config).unwrap(), b"\xef\xbb\xbfoperator=true\r\n");
}

#[test]
fn changed_unowned_and_cross_package_patchers_never_get_overwritten() {
    let fixture = Fixture::new();
    let source = fixture.source(
        "Example",
        &[
            ("Plugin.dll", b"plugin"),
            ("patchers/Patch.dll", b"patcher"),
        ],
    );
    fixture.install(source.clone()).unwrap();
    let patcher = fixture.0.join("runtime/BepInEx/patchers/Patch.dll");
    fs::write(&patcher, b"operator patcher").unwrap();
    assert!(fixture.install(source).unwrap_err().contains("modified"));
    let another = fixture.source(
        "Another",
        &[
            ("Another.dll", b"other"),
            ("patchers/Patch.dll", b"replacement"),
        ],
    );
    assert!(
        fixture
            .install(another)
            .unwrap_err()
            .contains("belongs to another")
    );
    assert_eq!(fs::read(patcher).unwrap(), b"operator patcher");
    assert!(
        !fixture
            .0
            .join("runtime/BepInEx/plugins/local-Another")
            .exists()
    );
}

#[test]
fn package_update_retires_removed_patcher_in_the_same_publish() {
    let fixture = Fixture::new();
    let source = fixture.source(
        "Example",
        &[("Plugin.dll", b"v1"), ("patchers/Old.dll", b"old patcher")],
    );
    fixture.install(source.clone()).unwrap();
    fs::remove_file(source.join("patchers/Old.dll")).unwrap();
    fs::write(source.join("Plugin.dll"), b"v2").unwrap();
    fixture.install(source).unwrap();
    assert!(!fixture.0.join("runtime/BepInEx/patchers/Old.dll").exists());
    assert_eq!(
        fs::read(
            fixture
                .0
                .join("runtime/BepInEx/plugins/local-Example/Plugin.dll")
        )
        .unwrap(),
        b"v2"
    );
}

#[test]
fn failed_management_publication_restores_plugins_and_every_patcher() {
    let fixture = Fixture::new();
    let source = fixture.source(
        "Example",
        &[
            ("Plugin.dll", b"plugin"),
            ("patchers/One.dll", b"one"),
            ("patchers/Two.dll", b"two"),
        ],
    );
    fixture.install(source).unwrap();
    let before = fingerprint_tree(&fixture.0.join("runtime/BepInEx")).unwrap();
    management::storage::fail_next_publication();
    assert!(
        management::manage_package(fixture.target(), "local-local-Example", "disable")
            .unwrap_err()
            .contains("rolled back")
    );
    assert_eq!(
        fingerprint_tree(&fixture.0.join("runtime/BepInEx")).unwrap(),
        before
    );
}
