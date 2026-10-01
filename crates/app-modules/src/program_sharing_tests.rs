use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{ModuleProgramSharing, ModuleStorageSpec};
use crate::{ModuleDescriptor, ModuleDiscoveryError, discover_modules};

const SHARED_MANIFEST: &str = r#"
id = "fixture"
name = "Shared program fixture"
version = "1.0.0"

[install]
shared_game_dir = "fixture"

[process]
executable = "jre/bin/java.exe"
args_template = ["-jar", "{{paths.install_root}}/server.jar"]
working_directory_template = "{{paths.instance_root}}"

[storage]
program_sharing = "shared"
saves_path_template = "{{paths.instance_root}}/{{settings.level_name}}"

[mods.manual_staging]
target_template = "{{paths.instance_root}}/mods"
"#;

struct CatalogFixture(PathBuf);

impl CatalogFixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let root = std::env::temp_dir().join(format!(
            "langame-program-sharing-{}-{}-{}",
            std::process::id(),
            timestamp.as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("fixture")).unwrap();
        Self(root)
    }

    fn load(&self, manifest: &toml::Value) -> Result<Vec<ModuleDescriptor>, ModuleDiscoveryError> {
        fs::write(
            self.0.join("fixture/module.toml"),
            toml::to_string(manifest).unwrap(),
        )
        .unwrap();
        discover_modules(&self.0)
    }

    fn assert_invalid_storage(&self, manifest: &toml::Value) {
        let error = self.load(manifest).unwrap_err();
        assert!(
            matches!(
                error,
                ModuleDiscoveryError::InvalidStorage { ref path, .. }
                    if path == &self.0.join("fixture/module.toml")
            ),
            "unexpected discovery result: {error}"
        );
    }
}

impl Drop for CatalogFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned temporary catalog");
    }
}

fn shared_manifest() -> toml::Value {
    toml::from_str(SHARED_MANIFEST).unwrap()
}

#[test]
fn program_sharing_defaults_to_independent() {
    assert_eq!(
        ModuleStorageSpec::default().program_sharing,
        ModuleProgramSharing::Independent
    );
    let fixture = CatalogFixture::new();
    let mut manifest = shared_manifest();
    let table = manifest.as_table_mut().unwrap();
    table.remove("install");
    table.remove("process");
    table.remove("storage");
    assert_eq!(
        fixture.load(&manifest).unwrap()[0].storage.program_sharing,
        ModuleProgramSharing::Independent
    );
    manifest
        .as_table_mut()
        .unwrap()
        .insert("storage".into(), toml::Value::Table(toml::map::Map::new()));
    assert_eq!(
        fixture.load(&manifest).unwrap()[0].storage.program_sharing,
        ModuleProgramSharing::Independent
    );
    manifest["storage"]
        .as_table_mut()
        .unwrap()
        .insert("program_sharing".into(), "independent".into());
    assert_eq!(
        fixture.load(&manifest).unwrap()[0].storage.program_sharing,
        ModuleProgramSharing::Independent
    );
}

#[test]
fn program_sharing_accepts_shared_program_with_private_data() {
    let fixture = CatalogFixture::new();
    for executable in [
        "jre/bin/java.exe",
        "{{paths.install_root}}/jre/bin/java.exe",
    ] {
        let mut manifest = shared_manifest();
        manifest["process"]["executable"] = executable.into();
        let descriptor = fixture.load(&manifest).unwrap().remove(0);
        assert_eq!(
            descriptor.storage.program_sharing,
            ModuleProgramSharing::Shared
        );
        assert_eq!(
            descriptor.process.unwrap().args_template,
            ["-jar", "{{paths.install_root}}/server.jar"]
        );
    }
}

#[test]
fn program_sharing_rejects_unknown_mode_and_wrong_type() {
    let fixture = CatalogFixture::new();
    for mode in [
        "Shared".into(),
        "hardlink".into(),
        toml::Value::Boolean(true),
        toml::Value::Integer(42),
    ] {
        let mut manifest = shared_manifest();
        manifest["storage"]["program_sharing"] = mode;
        assert!(matches!(
            fixture.load(&manifest).unwrap_err(),
            ModuleDiscoveryError::ParseManifest { .. }
        ));
    }
}

#[test]
fn program_sharing_requires_explicit_install_process_and_data_paths() {
    let fixture = CatalogFixture::new();
    for section in ["install", "process"] {
        let mut manifest = shared_manifest();
        manifest.as_table_mut().unwrap().remove(section);
        fixture.assert_invalid_storage(&manifest);
    }
    for (section, field) in [
        ("process", "working_directory_template"),
        ("storage", "saves_path_template"),
    ] {
        let mut manifest = shared_manifest();
        manifest[section].as_table_mut().unwrap().remove(field);
        fixture.assert_invalid_storage(&manifest);
    }
}

#[test]
fn program_sharing_rejects_shared_or_escaping_mutable_paths() {
    let fixture = CatalogFixture::new();
    for path in [
        "{{paths.install_root}}/Saved",
        "{{paths.instance_root}}-sibling",
        "{{paths.instance_root}}/../shared",
        "{{paths.config_dir}}/../../shared",
        "{{paths.data_dir}}//Saved",
        "{{paths.instance_root}}/Saved/.",
        "{{paths.instance_root}}/Saved/ ",
        "{{paths.instance_root}}/Saved:stream",
        "{{paths.instance_root}}/{{paths.install_root}}",
        "{{paths.instance_root}}/{{settings.level_name}",
        "{{paths.instance_root}}/{{settings...}}",
        "{{paths.saves_dir}}",
        "C:/shared/Saved",
        "Saved",
    ] {
        for (section, field) in [
            ("process", "working_directory_template"),
            ("storage", "saves_path_template"),
        ] {
            let mut manifest = shared_manifest();
            manifest[section][field] = path.into();
            fixture.assert_invalid_storage(&manifest);
        }
        let mut manifest = shared_manifest();
        manifest["mods"]["manual_staging"]["target_template"] = path.into();
        fixture.assert_invalid_storage(&manifest);
    }
}

#[test]
fn program_sharing_rejects_nonpackage_executables() {
    let fixture = CatalogFixture::new();
    for executable in [
        "C:/server.exe",
        "../server.exe",
        "{{paths.instance_root}}/server.exe",
        "{{paths.install_root}}/../server.exe",
        "{{settings.executable}}",
    ] {
        let mut manifest = shared_manifest();
        manifest["process"]["executable"] = executable.into();
        fixture.assert_invalid_storage(&manifest);
    }
}

#[test]
fn program_sharing_rejects_install_root_mutable_declarations() {
    let fixture = CatalogFixture::new();
    for field in ["retained_paths", "runtime_copy_exclusions"] {
        let mut manifest = shared_manifest();
        manifest["storage"]
            .as_table_mut()
            .unwrap()
            .insert(field.into(), toml::Value::Array(vec!["Saved".into()]));
        fixture.assert_invalid_storage(&manifest);
    }
    for staging in [
        "not a table".into(),
        toml::Value::Table(toml::map::Map::new()),
    ] {
        let mut manifest = shared_manifest();
        manifest["mods"]["manual_staging"] = staging;
        fixture.assert_invalid_storage(&manifest);
    }
}

#[test]
fn program_sharing_catalog_only_enables_reviewed_vanilla_minecraft() {
    let catalog =
        discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules")).unwrap();
    let shared: Vec<_> = catalog
        .iter()
        .filter(|module| module.storage.program_sharing == ModuleProgramSharing::Shared)
        .collect();
    assert_eq!(
        shared
            .iter()
            .map(|module| module.summary.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft"]
    );
    let minecraft = shared[0]
        .install
        .as_ref()
        .unwrap()
        .minecraft
        .as_ref()
        .unwrap();
    assert_eq!(minecraft.default_distribution, "vanilla");
    assert_eq!(minecraft.distributions.len(), 1);
    let vanilla = &minecraft.distributions[0];
    assert_eq!(vanilla.id, "vanilla");
    assert!(!vanilla.supports_mods && !vanilla.supports_plugins);
}
