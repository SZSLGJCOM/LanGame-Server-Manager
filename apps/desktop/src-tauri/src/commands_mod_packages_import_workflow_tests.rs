use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-mod-import-workflow-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn target(&self, module: &str, relative: &str, accepts: &[&str]) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "workflow-test".into(),
            module_id: module.into(),
            source_label: "local files".into(),
            target_label: relative.into(),
            target_path: self.0.join(relative),
            accepts: accepts.iter().map(|value| (*value).into()).collect(),
            id_strategy: None,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn external_manager_workflows_reject_mutations_without_changing_packages_or_configuration() {
    for (module, relative, extension) in [
        ("astroneer", "runtime/Astro/Saved/Paks", "pak"),
        ("satisfactory", "runtime/FactoryGame/Mods", "smod"),
    ] {
        let fixture = Fixture::new();
        let target = || fixture.target(module, relative, &[extension, "folder", "zip"]);
        let target_path = target().target_path;
        fs::create_dir_all(target_path.join("Example/config")).unwrap();
        fs::write(target_path.join("Example/payload.pak"), b"existing payload").unwrap();
        fs::write(
            target_path.join("Example/config/operator.ini"),
            b"\xef\xbb\xbfoperator=true\r\n",
        )
        .unwrap();
        // The derived Astro package and the Satisfactory loader are deliberately
        // present: merely moving the original package would be a false success.
        let external_file = if module == "astroneer" {
            target_path.join("999-AstroModIntegrator_P.pak")
        } else {
            fs::create_dir(target_path.join("SML")).unwrap();
            target_path.join("SML/SML.uplugin")
        };
        fs::write(external_file, b"external manager output").unwrap();
        let manifest = InstalledPackages {
            packages: [(
                "local-Example".into(),
                InstalledPackage {
                    thunderstore_intent: None,
                    target_name: "Example".into(),
                    files: fingerprint_tree(&target_path.join("Example")).unwrap(),
                    routed_files: BTreeMap::new(),
                    version: None,
                    dependencies: None,
                    enabled: true,
                },
            )]
            .into(),
        };
        fs::write(
            target_path.join(ONLINE_MOD_MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let source = fixture.0.join(format!("New.{extension}"));
        if module == "satisfactory" {
            let mut zip = zip::ZipWriter::new(fs::File::create(&source).unwrap());
            zip.start_file("New/New.uplugin", zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(br#"{"GameFeature":true}"#).unwrap();
            zip.finish().unwrap();
        } else {
            fs::write(&source, b"new mod payload").unwrap();
        }
        let before = fingerprint_tree(&fixture.0).unwrap();
        let error_code = if module == "astroneer" {
            "manual_mod_integration_required"
        } else {
            "mod_package_layout_unsupported"
        };
        assert!(
            stage_manual_mod_sources(target(), vec![source])
                .unwrap_err()
                .contains(error_code),
            "{module}"
        );
        for action in ["adopt", "enable", "disable", "delete"] {
            let key = if action == "adopt" {
                "Example"
            } else {
                "local-Example"
            };
            assert!(
                manage_package(target(), key, action)
                    .unwrap_err()
                    .contains(error_code),
                "{module}/{action}"
            );
        }
        assert_eq!(fingerprint_tree(&fixture.0).unwrap(), before, "{module}");
        assert!(
            !target_path
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".langame-mod-state-"))
        );
    }
}

#[test]
fn additional_file_workflows_import_directly_into_the_managed_lifecycle() {
    for (module, relative, extension) in [
        ("necesse", "runtime/mods", "jar"),
        ("rust", "runtime/oxide/plugins", "cs"),
        ("windrose", "runtime/R5/Content/Paks/~mods", "pak"),
        ("sonsoftheforest", "runtime/Mods", "dll"),
    ] {
        let fixture = Fixture::new();
        let target = || fixture.target(module, relative, &[extension]);
        let target_path = target().target_path;
        fs::create_dir_all(&target_path).unwrap();
        fs::write(target_path.join("operator.cfg"), b"operator settings\r\n").unwrap();
        let name = format!("Example.{extension}");
        let source = fixture.0.join(&name);
        fs::write(&source, b"Mod payload").unwrap();
        stage_manual_mod_sources(target(), vec![source]).unwrap();
        let key = format!("local-{name}");
        let manifest = read_manifest(&target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        assert!(manifest.packages[&key].enabled, "{module}");
        assert_eq!(manifest.packages[&key].files.len(), 1, "{module}");
        assert!(manifest.packages[&key].dependencies.is_none(), "{module}");
        manage_package(target(), &key, "disable").unwrap();
        assert!(!target_path.join(&name).exists(), "{module}");
        let refreshed =
            crate::commands::commands_mods::mod_inventory::read_inventory(target()).unwrap();
        let item = refreshed
            .items
            .iter()
            .find(|item| item.name == name)
            .unwrap();
        assert!(!item.management.as_ref().unwrap().enabled, "{module}");
        assert!(!Path::new(&item.path).starts_with(&target_path), "{module}");
        manage_package(target(), &key, "enable").unwrap();
        assert_eq!(
            fs::read(target_path.join(&name)).unwrap(),
            b"Mod payload",
            "{module}"
        );
        let deleted = manage_package(target(), &key, "delete").unwrap();
        assert_eq!(deleted.removed_file_count, 1, "{module}");
        assert!(
            !deleted.inventory.items.iter().any(|item| item.name == name),
            "{module}"
        );
        assert!(
            !read_manifest(&target_path.join(ONLINE_MOD_MANIFEST))
                .unwrap()
                .packages
                .contains_key(&key),
            "{module}"
        );
        assert_eq!(
            fs::read(target_path.join("operator.cfg")).unwrap(),
            b"operator settings\r\n",
            "{module}"
        );
    }
}

#[test]
fn windrose_zip_companions_are_managed_as_a_package_while_configuration_is_retained() {
    let fixture = Fixture::new();
    let target = || fixture.target("windrose", "runtime/R5/Content/Paks/~mods", &["zip"]);
    let archive = fixture.0.join("Example.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
    for (path, content) in [
        ("Example/Example.pak", "pak payload"),
        ("Example/Example.ucas", "ucas payload"),
        ("Example/Example.utoc", "utoc payload"),
        ("Example/config/operator.cfg", "operator settings\r\n"),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    stage_manual_mod_sources(target(), vec![archive]).unwrap();
    let target_path = target().target_path;
    let manifest = read_manifest(&target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    assert_eq!(manifest.packages["local-Example"].files.len(), 4);
    manage_package(target(), "local-Example", "disable").unwrap();
    let refreshed =
        crate::commands::commands_mods::mod_inventory::read_inventory(target()).unwrap();
    let item = refreshed
        .items
        .iter()
        .find(|item| item.name == "Example")
        .unwrap();
    assert!(!item.management.as_ref().unwrap().enabled);
    assert!(!Path::new(&item.path).starts_with(fixture.0.join("runtime/R5/Content/Paks")));
    manage_package(target(), "local-Example", "enable").unwrap();
    let deleted = manage_package(target(), "local-Example", "delete").unwrap();
    assert_eq!(deleted.removed_file_count, 3);
    let preserved = PathBuf::from(deleted.preserved_path.unwrap());
    assert_eq!(
        fs::read(preserved.join("config/operator.cfg")).unwrap(),
        b"operator settings\r\n"
    );
    assert!(!preserved.join("Example.pak").exists());
    assert!(!target_path.join("Example").exists());
}

#[test]
fn automatic_file_ownership_excludes_steam_and_native_enablement_workflows() {
    let fixture = Fixture::new();
    for module in [
        "squad",
        "arksurvivalascended",
        "arksurvivalevolved",
        "barotrauma",
        "conanexiles",
        "palworld",
        "terraria",
        "soulmask",
    ] {
        assert!(
            !manages_local_imports(&fixture.target(module, "Mods", &["folder"])),
            "{module}"
        );
    }
    let mut target = fixture.target("minecraft", "mods", &["jar"]);
    target.id_strategy = Some("numeric_folder".into());
    assert!(!manages_local_imports(&target));
}
