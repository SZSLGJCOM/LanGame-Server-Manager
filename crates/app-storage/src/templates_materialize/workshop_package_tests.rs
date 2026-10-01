use super::*;

struct Fixture {
    root: PathBuf,
    paths: StoragePaths,
    install: PathBuf,
    shared: PathBuf,
    config: PathBuf,
    settings: Map<String, Value>,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-workshop-stage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let install = root.join("instance/runtime");
        let shared = root.join("shared-games");
        let config = root.join("instance/config");
        fs::create_dir_all(&install).expect("create private runtime");
        fs::create_dir_all(&shared).expect("create shared install");
        fs::create_dir_all(&config).expect("create private config");
        let paths = StoragePaths {
            app_data_root: root.join("appdata"),
            settings_path: root.join("appdata/settings.json"),
            database_path: root.join("appdata/db/lgs.db"),
            logs_root: root.join("appdata/logs"),
            modules_root: root.join("modules"),
            migrations_root: root.join("migrations"),
            steamcmd_root: root.join("separate-steamcmd"),
            games_root: root.join("shared-games"),
            instances_root: root.join("instances"),
            archives_root: root.join("instances").join(".trash"),
        };
        let settings = Map::from_iter([(
            String::from("mod_workshop_ids"),
            Value::String(String::from("3000000001\n3000000002")),
        )]);
        Self {
            root,
            paths,
            install,
            shared,
            config,
            settings,
        }
    }

    fn context(&self, module_id: &'static str) -> ModuleSupportMaterializationContext<'_> {
        ModuleSupportMaterializationContext {
            storage_paths: &self.paths,
            module_id,
            install_root: &self.install,
            shared_install_root: &self.shared,
            config_dir: &self.config,
            saves_dir: &self.install,
            instance_id: "server",
            instance_running: false,
            settings: &self.settings,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_payload(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("payload parent")).expect("create payload directory");
    fs::write(path, contents).expect("write payload");
}

#[test]
fn barotrauma_uses_staged_packages_without_self_copy_and_keeps_cache_fallback() {
    let fixture = Fixture::new();
    let staged = fixture.config.join("LocalMods/3000000001");
    write_payload(&staged.join("Current/filelist.xml"), "<contentpackage />");
    write_payload(&staged.join("Current/payload.txt"), "new payload");
    // The shared cache may contain an older copy, but staged files take precedence.
    write_payload(
        &fixture
            .shared
            .join("steamapps/workshop/content/602960/3000000001/Old/filelist.xml"),
        "<contentpackage />",
    );
    write_payload(
        &fixture
            .paths
            .steamcmd_root
            .join("steamapps/workshop/content/602960/3000000002/filelist.xml"),
        "<contentpackage />",
    );
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files)
        .expect("materialize staged and cache packages");
    files.commit();

    let xml =
        fs::read_to_string(fixture.config.join("config_player.xml")).expect("read native list");
    assert!(xml.contains("LocalMods/3000000001/Current/filelist.xml"));
    assert!(!xml.contains("/Old/"));
    assert!(xml.contains("LocalMods/3000000002/filelist.xml"));
    assert_eq!(
        fs::read_to_string(staged.join("Current/payload.txt")).unwrap(),
        "new payload"
    );
    assert!(
        fixture
            .config
            .join("LocalMods/3000000002/filelist.xml")
            .is_file()
    );
}

#[test]
fn barotrauma_uses_shared_cache_without_copying_it_into_the_private_runtime() {
    let mut fixture = Fixture::new();
    fixture.settings.insert(
        String::from("mod_workshop_ids"),
        Value::String(String::from("3000000003")),
    );
    write_payload(
        &fixture
            .shared
            .join("steamapps/workshop/content/602960/3000000003/filelist.xml"),
        "<contentpackage name=\"Shared\" />",
    );
    assert!(!fixture.install.join("steamapps/workshop").exists());
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files)
        .expect("materialize from shared cache");
    files.commit();
    assert_eq!(
        fs::read_to_string(fixture.config.join("LocalMods/3000000003/filelist.xml")).unwrap(),
        "<contentpackage name=\"Shared\" />"
    );
    assert!(!fixture.install.join("steamapps/workshop").exists());
}
#[test]
fn conan_materializes_staged_and_cached_paks_in_configured_order() {
    let fixture = Fixture::new();
    let mods_root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&mods_root.join("3000000001/Current.pak"), "new pak");
    write_payload(
        &fixture
            .shared
            .join("steamapps/workshop/content/440900/3000000001/Old.pak"),
        "stale pak",
    );
    write_payload(
        &fixture
            .paths
            .steamcmd_root
            .join("steamapps/workshop/content/440900/3000000002/Second.pak"),
        "second pak",
    );
    let mut files = ManagedConfigMutation::new("conanexiles");
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files)
        .expect("materialize staged and cache paks");
    files.commit();

    assert_eq!(
        fs::read_to_string(mods_root.join("modlist.txt")).unwrap(),
        "Current.pak\nSecond.pak"
    );
    assert_eq!(
        fs::read_to_string(mods_root.join("Current.pak")).unwrap(),
        "new pak"
    );
    assert_eq!(
        fs::read_to_string(mods_root.join("Second.pak")).unwrap(),
        "second pak"
    );
    assert!(!mods_root.join("Old.pak").exists());
}

#[test]
fn manually_staged_numeric_prefix_packages_are_loaded_from_their_real_paths() {
    let fixture = Fixture::new();
    let mods = fixture.install.join("ConanSandbox/Mods");
    write_payload(&mods.join("3000000001-Manual.pak"), "manual pak");
    write_payload(&mods.join("3000000002-Nested/Second.pak"), "nested pak");
    let mut files = ManagedConfigMutation::new("conanexiles");
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
    files.commit();
    assert_eq!(
        fs::read_to_string(mods.join("modlist.txt")).unwrap(),
        "3000000001-Manual.pak\nSecond.pak"
    );

    write_payload(
        &fixture
            .config
            .join("LocalMods/3000000001-Manual/filelist.xml"),
        "<contentpackage />",
    );
    write_payload(
        &fixture.config.join("LocalMods/3000000002-Standalone.xml"),
        "<contentpackage />",
    );
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).unwrap();
    files.commit();
    let xml = fs::read_to_string(fixture.config.join("config_player.xml")).unwrap();
    assert!(xml.contains("LocalMods/3000000001-Manual/filelist.xml"));
    assert!(xml.contains("LocalMods/3000000002-Standalone.xml"));
}

#[test]
fn conan_rejects_same_named_packages_before_overwriting_any_native_payload() {
    let fixture = Fixture::new();
    let mods = fixture.install.join("ConanSandbox/Mods");
    write_payload(&mods.join("3000000001/Shared.pak"), "first pak");
    write_payload(&mods.join("3000000002/shared.pak"), "second pak");
    write_payload(&mods.join("Shared.pak"), "existing native pak");
    write_payload(&mods.join("modlist.txt"), "existing.pak");
    let mut files = ManagedConfigMutation::new("conanexiles");
    let error = materialize_conan_modlist(&fixture.context("conanexiles"), &mut files)
        .expect_err("different packages cannot publish the same native filename");
    assert!(error.to_string().contains("same native PAK filename"));
    assert_eq!(
        fs::read_to_string(mods.join("Shared.pak")).unwrap(),
        "existing native pak"
    );
    assert_eq!(
        fs::read_to_string(mods.join("modlist.txt")).unwrap(),
        "existing.pak"
    );
}

#[test]
fn barotrauma_preserves_the_actual_instance_config_when_refreshing_packages() {
    let mut fixture = Fixture::new();
    fixture
        .settings
        .insert("mod_workshop_ids".into(), Value::String(String::new()));
    write_payload(
        &fixture.install.join("config_player.xml"),
        "<config language=\"Package\"><contentpackages><corepackage path=\"Content/Vanilla.xml\"/><regularpackages/></contentpackages></config>",
    );
    write_payload(
        &fixture.config.join("config_player.xml"),
        "<config language=\"Operator\"><contentpackages><corepackage path=\"LocalMods/Core/filelist.xml\"/><regularpackages/></contentpackages></config>",
    );
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).unwrap();
    files.commit();
    let xml = fs::read_to_string(fixture.config.join("config_player.xml")).unwrap();
    assert!(xml.contains("language=\"Operator\""));
    assert!(xml.contains("LocalMods/Core/filelist.xml"));
    assert!(!xml.contains("Content/Vanilla.xml"));
}

#[test]
fn missing_or_ambiguous_enabled_packages_fail_without_changing_native_lists() {
    for module in ["conanexiles", "barotrauma"] {
        let mut fixture = Fixture::new();
        fixture.settings.insert(
            "mod_workshop_ids".into(),
            Value::String("3000000001".into()),
        );
        let (root, native) = if module == "conanexiles" {
            let root = fixture.install.join("ConanSandbox/Mods");
            (root.clone(), root.join("modlist.txt"))
        } else {
            (
                fixture.config.join("LocalMods"),
                fixture.config.join("config_player.xml"),
            )
        };
        write_payload(&native, "retained native settings");
        let materialize = |files: &mut ManagedConfigMutation| {
            if module == "conanexiles" {
                materialize_conan_modlist(&fixture.context(module), files)
            } else {
                materialize_barotrauma_workshop_mods(&fixture.context(module), files)
            }
        };
        let mut files = ManagedConfigMutation::new(module);
        assert!(
            materialize(&mut files)
                .unwrap_err()
                .to_string()
                .contains("no installed payload")
        );
        for suffix in ["One", "Two"] {
            let name = if module == "conanexiles" {
                "Example.pak"
            } else {
                "filelist.xml"
            };
            write_payload(&root.join(format!("3000000001-{suffix}/{name}")), "package");
        }
        assert!(
            materialize(&mut files)
                .unwrap_err()
                .to_string()
                .contains("Multiple local packages")
        );
        assert_eq!(
            fs::read_to_string(native).unwrap(),
            "retained native settings"
        );
    }
}

#[test]
fn conan_repeated_materialization_recognizes_its_numeric_prefix_native_mirror() {
    let mut fixture = Fixture::new();
    fixture.settings.insert(
        "mod_workshop_ids".into(),
        Value::String("3000000001".into()),
    );
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&root.join("3000000001/3000000001-Example.pak"), "original");
    for value in ["original", "updated"] {
        write_payload(&root.join("3000000001/3000000001-Example.pak"), value);
        let mut files = ManagedConfigMutation::new("conanexiles");
        materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
        files.commit();
        assert_eq!(
            fs::read_to_string(root.join("3000000001-Example.pak")).unwrap(),
            value
        );
    }
}

#[test]
fn conan_native_mirror_cannot_be_mistaken_for_another_enabled_id() {
    let fixture = Fixture::new();
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&root.join("3000000001/3000000002-First.pak"), "first");
    write_payload(&root.join("3000000002/Second.pak"), "second");
    for _ in 0..2 {
        let mut files = ManagedConfigMutation::new("conanexiles");
        materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
        files.commit();
        assert_eq!(
            fs::read_to_string(root.join("modlist.txt")).unwrap(),
            "3000000002-First.pak\nSecond.pak"
        );
    }
}

#[test]
fn conan_does_not_infer_ownership_of_an_untracked_same_named_operator_pak() {
    let mut fixture = Fixture::new();
    fixture.settings.insert(
        "mod_workshop_ids".into(),
        Value::String("3000000001".into()),
    );
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(
        &root.join("3000000001/3000000001-Example.pak"),
        "downloaded",
    );
    write_payload(&root.join("3000000001-Example.pak"), "operator variant");
    let mut files = ManagedConfigMutation::new("conanexiles");
    let error = materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap_err();
    assert!(error.to_string().contains("Multiple local packages"));
    assert_eq!(
        fs::read_to_string(root.join("3000000001-Example.pak")).unwrap(),
        "operator variant"
    );
    assert!(!root.join("modlist.txt").exists());
}

#[test]
fn barotrauma_seeds_only_a_missing_native_config_and_propagates_read_failures() {
    let mut fixture = Fixture::new();
    fixture
        .settings
        .insert("mod_workshop_ids".into(), Value::String(String::new()));
    let baseline = fixture.install.join("config_player.xml");
    let native = fixture.config.join("config_player.xml");
    write_payload(
        &baseline,
        "<config language=\"Initial\"><regularpackages/></config>",
    );
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).unwrap();
    files.commit();
    assert!(
        fs::read_to_string(&native)
            .unwrap()
            .contains("language=\"Initial\"")
    );
    // An existing instance config must not depend on a readable package baseline.
    fs::remove_file(&baseline).unwrap();
    fs::create_dir(&baseline).unwrap();
    let mut files = ManagedConfigMutation::new("barotrauma");
    materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).unwrap();
    files.commit();
    fs::remove_file(&native).unwrap();
    fs::create_dir(&native).unwrap();
    let mut files = ManagedConfigMutation::new("barotrauma");
    assert!(
        materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).is_err()
    );
    assert!(native.is_dir());
}

#[test]
fn barotrauma_does_not_reset_an_existing_malformed_native_config() {
    let mut fixture = Fixture::new();
    fixture
        .settings
        .insert("mod_workshop_ids".into(), Value::String(String::new()));
    let native = fixture.config.join("config_player.xml");
    write_payload(&native, "<config broken");
    let mut files = ManagedConfigMutation::new("barotrauma");
    assert!(
        materialize_barotrauma_workshop_mods(&fixture.context("barotrauma"), &mut files).is_err()
    );
    assert_eq!(fs::read_to_string(native).unwrap(), "<config broken");
}

#[test]
fn conan_rejects_untracked_and_modified_native_targets_before_copying_any_package() {
    let fixture = Fixture::new();
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&root.join("3000000001/First.pak"), "first");
    write_payload(&root.join("3000000002/Second.pak"), "second");
    write_payload(&root.join("Second.pak"), "operator variant");
    let mut files = ManagedConfigMutation::new("conanexiles");
    let error = materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap_err();
    assert!(error.to_string().contains("untracked native PAK"));
    assert!(!root.join("First.pak").exists());
    assert_eq!(
        fs::read_to_string(root.join("Second.pak")).unwrap(),
        "operator variant"
    );
    fs::remove_file(root.join("Second.pak")).unwrap();
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
    files.commit();
    let record = fixture.config.join(".lgsm-conan-mod-payloads.json");
    let original_record = fs::read(&record).unwrap();
    write_payload(&root.join("Second.pak"), "edited"); // Same length as the installed PAK.
    write_payload(&root.join("3000000001/First.pak"), "first update");
    let mut files = ManagedConfigMutation::new("conanexiles");
    let error = materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap_err();
    assert!(error.to_string().contains("modified outside LGSM"));
    assert_eq!(fs::read_to_string(root.join("First.pak")).unwrap(), "first");
    assert_eq!(
        fs::read_to_string(root.join("Second.pak")).unwrap(),
        "edited"
    );
    assert_eq!(fs::read(record).unwrap(), original_record);
}

#[test]
fn conan_second_package_failure_keeps_first_payload_and_record_consistent_for_retry() {
    let fixture = Fixture::new();
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&root.join("3000000001/3000000001-First.pak"), "first");
    write_payload(&root.join("3000000002/3000000002-Second.pak"), "second");
    package_staging::fail_next_publication_for_test(&root.join("3000000002-Second.pak"));
    let mut files = ManagedConfigMutation::new("conanexiles");
    assert!(materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).is_err());
    let record: Value = serde_json::from_slice(
        &fs::read(fixture.config.join(".lgsm-conan-mod-payloads.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["3000000001-first.pak"]["item_id"], "3000000001");
    assert!(record.get("3000000002-second.pak").is_none());
    assert_eq!(
        fs::read_to_string(root.join("3000000001-First.pak")).unwrap(),
        "first"
    );
    assert!(!root.join("3000000002-Second.pak").exists());
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
    files.commit();
    assert_eq!(
        fs::read_to_string(root.join("modlist.txt")).unwrap(),
        "3000000001-First.pak\n3000000002-Second.pak"
    );
}

#[test]
fn conan_configuration_rollback_does_not_orphan_independently_published_payloads() {
    let mut fixture = Fixture::new();
    fixture.settings.insert(
        "mod_workshop_ids".into(),
        Value::String("3000000001".into()),
    );
    let root = fixture.install.join("ConanSandbox/Mods");
    write_payload(&root.join("3000000001/3000000001-First.pak"), "first");
    let mut files = ManagedConfigMutation::new("conanexiles");
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
    let _ = files.rollback_after(StorageError::ModuleSupportMaterialization {
        module_id: "conanexiles".into(),
        path: root.clone(),
        message: "injected configuration commit failure".into(),
    });
    assert!(!root.join("modlist.txt").exists());
    assert!(
        fixture
            .config
            .join(".lgsm-conan-mod-payloads.json")
            .is_file()
    );
    let mut files = ManagedConfigMutation::new("conanexiles");
    materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
    files.commit();
    assert_eq!(
        fs::read_to_string(root.join("modlist.txt")).unwrap(),
        "3000000001-First.pak"
    );
}

#[test]
fn conan_pending_ownership_recovers_before_or_after_payload_publication() {
    for published in [false, true] {
        let mut fixture = Fixture::new();
        fixture.settings.insert(
            "mod_workshop_ids".into(),
            Value::String("3000000001".into()),
        );
        let root = fixture.install.join("ConanSandbox/Mods");
        let source = root.join("3000000001/3000000001-First.pak");
        let target = root.join("3000000001-First.pak");
        let record = fixture.config.join(".lgsm-conan-mod-payloads.json");
        write_payload(&source, "first");
        let mut files = ManagedConfigMutation::new("conanexiles");
        materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
        files.commit();
        let old_hash = package_staging::file_sha256(&target).unwrap();
        write_payload(&source, "updated");
        let new_hash = package_staging::file_sha256(&source).unwrap();
        let pending = serde_json::json!({"3000000001-first.pak": {
            "item_id": "3000000001", "sha256": new_hash, "previous_sha256": old_hash,
        }});
        fs::write(&record, serde_json::to_vec(&pending).unwrap()).unwrap();
        if published {
            write_payload(&target, "updated");
        }
        let mut files = ManagedConfigMutation::new("conanexiles");
        materialize_conan_modlist(&fixture.context("conanexiles"), &mut files).unwrap();
        files.commit();
        let final_record: Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "updated");
        assert!(
            final_record["3000000001-first.pak"]
                .get("previous_sha256")
                .is_none()
        );
    }
}
