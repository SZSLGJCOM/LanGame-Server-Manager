use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::ModuleStorageSpec;
use crate::{ModuleDiscoveryError, discover_modules};

struct CatalogFixture(PathBuf);

impl CatalogFixture {
    fn new() -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let root = std::env::temp_dir().join(format!(
            "langame-retained-paths-{}-{}-{}",
            std::process::id(),
            timestamp.as_nanos(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("fixture")).unwrap();
        Self(root)
    }

    fn write_storage(&self, storage: &str) {
        fs::write(
            self.0.join("fixture/module.toml"),
            format!("id = \"fixture\"\nname = \"Storage fixture\"\nversion = \"1.0.0\"\n{storage}"),
        )
        .unwrap();
    }
}

impl Drop for CatalogFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove owned temporary catalog");
    }
}

#[test]
fn storage_retained_paths_default_to_empty_without_changing_saves() {
    let defaults = ModuleStorageSpec::default();
    assert!(defaults.retained_paths.is_empty());
    assert!(defaults.runtime_copy_exclusions.is_empty());
    defaults.validate_retained_paths().unwrap();
    defaults.validate_runtime_copy_exclusions().unwrap();
    let fixture = CatalogFixture::new();
    for storage in [
        "",
        "[storage]\nsaves_path_template = '{{paths.config_dir}}/savegame'\n",
    ] {
        fixture.write_storage(storage);
        let modules = discover_modules(&fixture.0).unwrap();
        let actual = &modules[0].storage;
        assert!(actual.retained_paths.is_empty());
        assert!(actual.runtime_copy_exclusions.is_empty());
        assert_eq!(
            actual.saves_path_template.as_deref(),
            (!storage.is_empty()).then_some("{{paths.config_dir}}/savegame")
        );
    }
}

#[test]
fn storage_retained_paths_load_literal_files_and_directories() {
    let fixture = CatalogFixture::new();
    let retained = [
        "RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini",
        "Saved/Config",
        "Config With Spaces/server.ini",
        r"Saved\Config\WindowsServer",
    ];
    fixture.write_storage(&format!(
        "[storage]\nretained_paths = {}\n",
        serde_json::to_string(&retained).unwrap()
    ));
    let modules = discover_modules(&fixture.0).unwrap();
    assert_eq!(modules[0].storage.retained_paths, retained);
    modules[0].storage.validate_retained_paths().unwrap();
}

#[test]
fn storage_retained_paths_reject_unsafe_or_nonliteral_paths_at_discovery() {
    let fixture = CatalogFixture::new();
    for relative in [
        "",
        ".",
        "..",
        "../outside",
        "Config/../outside",
        "Config/./server.ini",
        "/outside",
        r"\outside",
        "C:/outside",
        r"C:\outside",
        "C:outside",
        r"\\server\share\outside",
        r"\\?\C:\outside",
        "Config//server.ini",
        "Config/",
        r"Config\..\outside",
        "Config/server.ini:identity",
        "Config/.. /outside",
        "Config/../outside.",
        "Config/ ",
        " Config/server.ini",
        "Config/server.ini ",
        "Config/*.ini",
        "Config/?.ini",
        "{{paths.install_root}}/Config/server.ini",
        "Config/line\nbreak.ini",
    ] {
        let spec = ModuleStorageSpec {
            retained_paths: vec![relative.to_owned()],
            ..Default::default()
        };
        assert!(
            spec.validate_retained_paths().is_err(),
            "runtime validation accepted {relative:?}"
        );
        fixture.write_storage(&format!(
            "[storage]\nretained_paths = {}\n",
            serde_json::to_string(&spec.retained_paths).unwrap()
        ));
        let error = discover_modules(&fixture.0).unwrap_err();
        assert!(
            matches!(
                error,
                ModuleDiscoveryError::InvalidStorage { ref path, ref message }
                    if path == &fixture.0.join("fixture/module.toml")
                        && message.contains("retained path")
            ),
            "unexpected discovery error for {relative:?}: {error}"
        );
    }
}

#[test]
fn storage_retained_paths_require_a_string_list() {
    let fixture = CatalogFixture::new();
    for value in ["'Config/server.ini'", "[42]"] {
        fixture.write_storage(&format!("[storage]\nretained_paths = {value}\n"));
        assert!(matches!(
            discover_modules(&fixture.0).unwrap_err(),
            ModuleDiscoveryError::ParseManifest { .. }
        ));
    }
}

#[test]
fn storage_retained_paths_keep_dragonwilds_native_identity() {
    let modules =
        discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules")).unwrap();
    let dragonwilds = modules
        .iter()
        .find(|descriptor| descriptor.summary.id == "runescapedragonwilds")
        .unwrap();
    assert_eq!(
        dragonwilds.storage.retained_paths,
        ["RSDragonwilds/Saved/Config/WindowsServer/DedicatedServer.ini"]
    );
}

#[test]
fn storage_runtime_copy_exclusions_load_literal_files_and_directories() {
    let fixture = CatalogFixture::new();
    let excluded = [
        "mods/dedicated_server_mods_setup.lua",
        "Saved/Config",
        "steamapps/workshop/content",
        r"Server\Mods",
    ];
    fixture.write_storage(&format!(
        "[storage]\nruntime_copy_exclusions = {}\n",
        serde_json::to_string(&excluded).unwrap()
    ));
    let modules = discover_modules(&fixture.0).unwrap();
    assert_eq!(modules[0].storage.runtime_copy_exclusions, excluded);
    modules[0]
        .storage
        .validate_runtime_copy_exclusions()
        .unwrap();
}

#[test]
fn storage_runtime_copy_exclusions_reject_unsafe_or_nonliteral_paths() {
    let fixture = CatalogFixture::new();
    for relative in [
        "",
        "../outside",
        "Config/../outside",
        "/outside",
        r"\outside",
        "C:/outside",
        "Config//server.ini",
        "Config/./server.ini",
        "Config/server.ini:identity",
        "Config/*.ini",
        "{{paths.install_root}}/Config/server.ini",
    ] {
        let spec = ModuleStorageSpec {
            runtime_copy_exclusions: vec![relative.to_owned()],
            ..Default::default()
        };
        assert!(
            spec.validate_runtime_copy_exclusions().is_err(),
            "runtime validation accepted {relative:?}"
        );
        fixture.write_storage(&format!(
            "[storage]\nruntime_copy_exclusions = {}\n",
            serde_json::to_string(&spec.runtime_copy_exclusions).unwrap()
        ));
        let error = discover_modules(&fixture.0).unwrap_err();
        assert!(
            matches!(
                error,
                ModuleDiscoveryError::InvalidStorage { ref path, ref message }
                    if path == &fixture.0.join("fixture/module.toml")
                        && message.contains("runtime copy exclusion")
            ),
            "unexpected discovery error for {relative:?}: {error}"
        );
    }
}

#[test]
fn storage_runtime_copy_exclusions_require_a_string_list() {
    let fixture = CatalogFixture::new();
    for value in ["'Saved/Config'", "[42]"] {
        fixture.write_storage(&format!("[storage]\nruntime_copy_exclusions = {value}\n"));
        assert!(matches!(
            discover_modules(&fixture.0).unwrap_err(),
            ModuleDiscoveryError::ParseManifest { .. }
        ));
    }
}

fn path_is_same_or_below(path: &str, root: &str) -> bool {
    let path = path.replace(char::from(92), "/").to_ascii_lowercase();
    let root = root.replace(char::from(92), "/").to_ascii_lowercase();
    path == root || path.starts_with(&format!("{root}/"))
}

#[test]
fn catalog_runtime_copy_exclusions_cover_retained_data_and_mod_staging() {
    let modules =
        discover_modules(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules")).unwrap();
    for descriptor in modules {
        let exclusions = &descriptor.storage.runtime_copy_exclusions;
        if let Some(save_path) = descriptor
            .storage
            .saves_path_template
            .as_deref()
            .and_then(|template| template.strip_prefix("{{paths.install_root}}/"))
        {
            let literal_prefix = save_path.split("{{").next().unwrap().trim_end_matches('/');
            assert!(
                exclusions
                    .iter()
                    .any(|exclusion| path_is_same_or_below(literal_prefix, exclusion)),
                "{} native save path {save_path:?} can be inherited by a new instance",
                descriptor.summary.id
            );
        }

        for retained in &descriptor.storage.retained_paths {
            assert!(
                exclusions
                    .iter()
                    .any(|exclusion| path_is_same_or_below(retained, exclusion)),
                "{} retained path {retained:?} can be inherited by a new instance",
                descriptor.summary.id
            );
        }

        let manifest: toml::Value = toml::from_str(&descriptor.manifest_toml).unwrap();
        let manual_target = manifest
            .get("mods")
            .and_then(|mods| mods.get("manual_staging"))
            .and_then(|staging| staging.get("target_template"))
            .and_then(toml::Value::as_str);
        if let Some(target) =
            manual_target.and_then(|target| target.strip_prefix("{{paths.install_root}}/"))
        {
            let excluded = exclusions
                .iter()
                .any(|exclusion| path_is_same_or_below(target, exclusion));
            if matches!(descriptor.summary.id.as_str(), "sevendaystodie" | "squad") {
                // These packages ship first-party assets inside their mod staging
                // directories. The whole directory cannot be treated as user data.
                assert!(
                    !excluded,
                    "{} exclusion would remove packaged mod assets from {target:?}",
                    descriptor.summary.id
                );
            } else {
                assert!(
                    excluded,
                    "{} mod staging target {target:?} can be inherited by a new instance",
                    descriptor.summary.id
                );
            }
        }

        for package_path in [
            descriptor
                .process
                .as_ref()
                .map(|process| process.executable.as_str()),
            descriptor
                .install
                .as_ref()
                .and_then(|install| install.verification_path.as_deref()),
        ]
        .into_iter()
        .flatten()
        .filter(|path| !path.contains("{{"))
        {
            assert!(
                exclusions
                    .iter()
                    .all(|exclusion| !path_is_same_or_below(package_path, exclusion)),
                "{} runtime exclusion removes package entry {package_path:?}",
                descriptor.summary.id
            );
        }
    }
}
