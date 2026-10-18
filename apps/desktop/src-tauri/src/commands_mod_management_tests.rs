use super::*;

fn fixture(label: &str) -> (PathBuf, ResolvedManualModTarget) {
    let root = std::env::temp_dir().join(format!(
        "langame-managed-mod-{label}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let target_path = root.join("Mods");
    fs::create_dir_all(&target_path).unwrap();
    let target = ResolvedManualModTarget {
        instance_id: "management-test".into(),
        module_id: "7daystodie".into(),
        source_label: "local".into(),
        target_label: "Mods".into(),
        target_path,
        accepts: vec!["folder".into()],
        id_strategy: None,
    };
    (root, target)
}

fn clone_target(target: &ResolvedManualModTarget) -> ResolvedManualModTarget {
    ResolvedManualModTarget {
        instance_id: target.instance_id.clone(),
        module_id: target.module_id.clone(),
        source_label: target.source_label.clone(),
        target_label: target.target_label.clone(),
        target_path: target.target_path.clone(),
        accepts: target.accepts.clone(),
        id_strategy: target.id_strategy.clone(),
    }
}

fn write_package(target: &ResolvedManualModTarget, name: &str) {
    fs::create_dir_all(target.target_path.join(name).join("config")).unwrap();
    fs::write(target.target_path.join(name).join("plugin.dll"), b"plugin").unwrap();
    fs::write(
        target
            .target_path
            .join(name)
            .join("config")
            .join("operator.cfg"),
        b"operator settings",
    )
    .unwrap();
}

fn persist(target: &ResolvedManualModTarget, manifest: &InstalledPackages) {
    fs::write(
        target.target_path.join(ONLINE_MOD_MANIFEST),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn managed_mod_round_trip_survives_inventory_reload_outside_loading_tree() {
    let (root, target) = fixture("round-trip");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    let result = manage_package(clone_target(&target), "local-Example", "disable").unwrap();
    assert!(!target.target_path.join("Example").exists());
    let item = result
        .inventory
        .items
        .iter()
        .find(|item| item.name == "Example")
        .unwrap();
    assert!(!item.management.as_ref().unwrap().enabled);
    assert!(!Path::new(&item.path).starts_with(&target.target_path));
    assert_eq!(
        fs::read(Path::new(&item.path).join("config/operator.cfg")).unwrap(),
        b"operator settings"
    );
    let refreshed =
        crate::commands::commands_mods::mod_inventory::read_inventory(clone_target(&target))
            .unwrap();
    assert!(!refreshed.items[0].management.as_ref().unwrap().enabled);
    manage_package(clone_target(&target), "local-Example", "enable").unwrap();
    assert_eq!(
        fs::read(target.target_path.join("Example/plugin.dll")).unwrap(),
        b"plugin"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deletion_keeps_configuration_modified_and_unowned_files() {
    let (root, target) = fixture("delete-preserve");
    write_package(&target, "Example");
    fs::write(target.target_path.join("Example/changed.dll"), b"original").unwrap();
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    fs::write(
        target.target_path.join("Example/changed.dll"),
        b"operator modification",
    )
    .unwrap();
    fs::write(
        target.target_path.join("Example/operator-notes.md"),
        b"unowned notes",
    )
    .unwrap();
    fs::write(target.target_path.join("unrelated.txt"), b"leave in place").unwrap();
    let result = manage_package(clone_target(&target), "local-Example", "delete").unwrap();
    let retained = PathBuf::from(result.preserved_path.unwrap());
    assert_eq!(result.removed_file_count, 1);
    assert!(!retained.join("plugin.dll").exists());
    assert_eq!(
        fs::read(retained.join("config/operator.cfg")).unwrap(),
        b"operator settings"
    );
    assert_eq!(
        fs::read(retained.join("changed.dll")).unwrap(),
        b"operator modification"
    );
    assert_eq!(
        fs::read(retained.join("operator-notes.md")).unwrap(),
        b"unowned notes"
    );
    assert_eq!(
        fs::read(target.target_path.join("unrelated.txt")).unwrap(),
        b"leave in place"
    );
    assert!(!target.target_path.join("Example").exists());
    assert!(
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dependencies_block_reverse_mutations_and_enable_checks_payload_version_and_state() {
    let (root, target) = fixture("dependencies");
    for name in ["Library", "Consumer"] {
        write_package(&target, name);
    }
    let mut manifest = InstalledPackages::default();
    for (key, name, dependencies) in [
        ("thunderstore-Team-Library", "Library", vec![]),
        (
            "local-Consumer",
            "Consumer",
            vec!["Team-Library-1.0.0".into()],
        ),
    ] {
        manifest.packages.insert(
            key.into(),
            InstalledPackage {
                thunderstore_intent: None,
                routed_files: BTreeMap::new(),
                target_name: name.into(),
                files: fingerprint_tree(&target.target_path.join(name)).unwrap(),
                version: Some("1.0.0".into()),
                dependencies: Some(dependencies),
                enabled: true,
            },
        );
    }
    persist(&target, &manifest);
    assert!(
        manage_package(
            clone_target(&target),
            "thunderstore-Team-Library",
            "disable"
        )
        .unwrap_err()
        .contains("local-Consumer")
    );
    assert!(
        manage_package(clone_target(&target), "thunderstore-Team-Library", "delete")
            .unwrap_err()
            .contains("local-Consumer")
    );
    manage_package(clone_target(&target), "local-Consumer", "disable").unwrap();
    manage_package(
        clone_target(&target),
        "thunderstore-Team-Library",
        "disable",
    )
    .unwrap();
    assert!(
        manage_package(clone_target(&target), "local-Consumer", "enable")
            .unwrap_err()
            .contains("missing, disabled")
    );
    assert!(
        manage_package(clone_target(&target), "thunderstore-Team-Library", "delete")
            .unwrap_err()
            .contains("local-Consumer")
    );
    manage_package(clone_target(&target), "thunderstore-Team-Library", "enable").unwrap();
    fs::write(
        target.target_path.join("Library/plugin.dll"),
        b"changed dependency",
    )
    .unwrap();
    assert!(
        manage_package(clone_target(&target), "local-Consumer", "enable")
            .unwrap_err()
            .contains("missing or changed")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn management_rejects_conflicting_destinations_unowned_payloads_and_path_escapes() {
    let (root, target) = fixture("refusals");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    assert!(manage_package(clone_target(&target), "../Example", "adopt").is_err());
    write_package(&target, "0_TFP_Harmony");
    assert!(manage_package(clone_target(&target), "0_TFP_Harmony", "adopt").is_err());
    fs::write(
        target.target_path.join("Example/other-mod.dll"),
        b"unowned mod",
    )
    .unwrap();
    assert!(
        manage_package(clone_target(&target), "local-Example", "disable")
            .unwrap_err()
            .contains("unowned payload")
    );
    fs::remove_file(target.target_path.join("Example/other-mod.dll")).unwrap();
    manage_package(clone_target(&target), "local-Example", "disable").unwrap();
    fs::create_dir(target.target_path.join("Example")).unwrap();
    fs::write(
        target.target_path.join("Example/user.txt"),
        b"new directory",
    )
    .unwrap();
    assert!(
        manage_package(clone_target(&target), "local-Example", "enable")
            .unwrap_err()
            .contains("already exists")
    );
    assert_eq!(
        fs::read(target.target_path.join("Example/user.txt")).unwrap(),
        b"new directory"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn pure_xml_nexus_mod_can_be_disabled_and_deleted_while_preserving_configuration() {
    let (root, target) = fixture("xml-mod");
    let package = target.target_path.join("XmlMod");
    fs::create_dir_all(package.join("Config")).unwrap();
    fs::write(package.join("ModInfo.xml"), b"<xml><ModInfo/></xml>").unwrap();
    fs::write(package.join("Config/blocks.xml"), b"<configs/>").unwrap();
    manage_package(clone_target(&target), "XmlMod", "adopt").unwrap();
    manage_package(clone_target(&target), "local-XmlMod", "disable").unwrap();
    manage_package(clone_target(&target), "local-XmlMod", "enable").unwrap();
    let result = manage_package(clone_target(&target), "local-XmlMod", "delete").unwrap();
    let retained = PathBuf::from(result.preserved_path.unwrap());
    assert!(retained.join("ModInfo.xml").exists());
    assert!(retained.join("Config/blocks.xml").exists());
    assert_eq!(result.removed_file_count, 0);
    fs::remove_dir_all(root).unwrap();
}

fn prepare_interrupted_disable(target: &ResolvedManualModTarget) -> storage::ManagementPaths {
    let paths = storage::ManagementPaths::new(&target.target_path).unwrap();
    let mut original = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    let transaction =
        storage::ManagementTransaction::new("local-Example".into(), "disable", &mut original, None)
            .unwrap();
    let mut record = serde_json::to_value(transaction).unwrap();
    record["payload_files"] =
        serde_json::to_value(fingerprint_tree(&target.target_path.join("Example")).unwrap())
            .unwrap();
    fs::write(&paths.journal, serde_json::to_vec(&record).unwrap()).unwrap();
    fs::rename(
        target.target_path.join("Example"),
        paths.disabled.join("Example"),
    )
    .unwrap();
    paths
}

#[test]
fn durable_intent_recovers_rename_before_manifest_publication() {
    let (root, target) = fixture("recovery");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    let paths = prepare_interrupted_disable(&target);
    recover_management(&target).unwrap();
    assert!(!paths.journal.exists());
    assert!(
        !read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages["local-Example"]
            .enabled
    );
    manage_package(clone_target(&target), "local-Example", "enable").unwrap();
    assert!(target.target_path.join("Example/plugin.dll").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn recovery_refuses_payload_changed_after_interruption() {
    let (root, target) = fixture("recovery-changed");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    let paths = prepare_interrupted_disable(&target);
    fs::write(
        paths.disabled.join("Example/plugin.dll"),
        b"changed after interruption",
    )
    .unwrap();
    assert!(recover_management(&target).is_err());
    assert!(paths.journal.exists());
    assert_eq!(
        fs::read(paths.disabled.join("Example/plugin.dll")).unwrap(),
        b"changed after interruption"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn publish_failure_rolls_back_payload_and_keeps_original_record() {
    let (root, target) = fixture("rollback");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    storage::fail_next_publication();
    let error = manage_package(clone_target(&target), "local-Example", "disable").unwrap_err();
    assert!(error.contains("rolled back"));
    assert!(target.target_path.join("Example/plugin.dll").exists());
    assert!(
        read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST))
            .unwrap()
            .packages["local-Example"]
            .enabled
    );
    assert!(
        !storage::ManagementPaths::new(&target.target_path)
            .unwrap()
            .journal
            .exists()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_manifest_cannot_claim_parent_paths_or_windows_aliases() {
    let (root, target) = fixture("bad-record");
    write_package(&target, "Example");
    manage_package(clone_target(&target), "Example", "adopt").unwrap();
    let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    manifest
        .packages
        .get_mut("local-Example")
        .unwrap()
        .files
        .insert("../outside.dll".into(), "0".repeat(64));
    persist(&target, &manifest);
    assert!(manage_package(clone_target(&target), "local-Example", "delete").is_err());
    assert!(target.target_path.join("Example/plugin.dll").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn disabled_storage_is_outside_recursive_pak_and_plugin_containers() {
    for (target, parent) in [
        (
            "game/HumanitZServer/Content/Paks/~mods",
            "game/HumanitZServer/Content",
        ),
        ("game/R5/Content/Paks/~mods", "game/R5/Content"),
        ("game/Astro/Saved/Paks", "game/Astro/Saved"),
        ("game/BepInEx/plugins", "game"),
        (
            "Plugins/servers/game/BepInEx/plugins",
            "Plugins/servers/game",
        ),
        ("game/SquadGame/Plugins/Mods", "game/SquadGame"),
        ("game/Binaries/Win64/ue4ss/Mods", "game/Binaries/Win64"),
        ("game/Mods", "game"),
    ] {
        assert_eq!(
            storage::management_storage_parent(Path::new(target)).unwrap(),
            Path::new(parent)
        );
    }
}

#[test]
fn disabling_pak_moves_it_outside_the_entire_loading_container() {
    let (root, mut target) = fixture("pak-container");
    target.target_path = root.join("HumanitZServer/Content/Paks/~mods");
    fs::create_dir_all(&target.target_path).unwrap();
    fs::write(target.target_path.join("Example.pak"), b"pak payload").unwrap();
    manage_package(clone_target(&target), "Example.pak", "adopt").unwrap();
    let result = manage_package(clone_target(&target), "local-Example.pak", "disable").unwrap();
    let disabled = result
        .inventory
        .items
        .iter()
        .find(|item| item.name == "Example.pak")
        .unwrap();
    assert!(!Path::new(&disabled.path).starts_with(root.join("HumanitZServer/Content/Paks")));
    assert_eq!(fs::read(&disabled.path).unwrap(), b"pak payload");
    manage_package(clone_target(&target), "local-Example.pak", "enable").unwrap();
    assert_eq!(
        fs::read(target.target_path.join("Example.pak")).unwrap(),
        b"pak payload"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn adopting_existing_thunderstore_consumer_preserves_declared_reverse_dependencies() {
    let (root, target) = fixture("adopt-dependencies");
    write_package(&target, "Library");
    write_package(&target, "Consumer");
    let mut manifest = InstalledPackages::default();
    manifest.packages.insert(
        "thunderstore-Team-Library".into(),
        InstalledPackage {
            thunderstore_intent: None,
            routed_files: BTreeMap::new(),
            target_name: "Library".into(),
            files: fingerprint_tree(&target.target_path.join("Library")).unwrap(),
            version: Some("1.0.0".into()),
            dependencies: Some(Vec::new()),
            enabled: true,
        },
    );
    persist(&target, &manifest);
    fs::write(
        target.target_path.join("Consumer/manifest.json"),
        br#"{
        "name":"Consumer", "version_number":"2.0.0", "dependencies":["Team-Library-1.0.0"]
    }"#,
    )
    .unwrap();

    let adopted = manage_package(clone_target(&target), "Consumer", "adopt").unwrap();
    let state = adopted
        .inventory
        .items
        .iter()
        .find(|item| item.name == "Consumer")
        .unwrap()
        .management
        .as_ref()
        .unwrap();
    assert_eq!(state.package_id, "local-Consumer");
    assert_eq!(state.dependencies, ["Team-Library-1.0.0"]);
    assert_eq!(state.dependency_status, "complete");
    let recorded = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
    assert_eq!(
        recorded.packages["local-Consumer"].version.as_deref(),
        Some("2.0.0")
    );
    for action in ["disable", "delete"] {
        let error =
            manage_package(clone_target(&target), "thunderstore-Team-Library", action).unwrap_err();
        assert!(error.contains("local-Consumer"), "{error}");
    }
    assert!(target.target_path.join("Library/plugin.dll").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn adoption_can_record_an_existing_broken_consumer_for_safe_disablement() {
    let (root, target) = fixture("adopt-broken-dependency");
    write_package(&target, "Consumer");
    fs::write(
        target.target_path.join("Consumer/manifest.json"),
        br#"{
        "name":"Consumer", "version_number":"2.0.0", "dependencies":["Team-Missing-1.0.0"]
    }"#,
    )
    .unwrap();
    manage_package(clone_target(&target), "Consumer", "adopt").unwrap();
    manage_package(clone_target(&target), "local-Consumer", "disable").unwrap();
    let error = manage_package(clone_target(&target), "local-Consumer", "enable").unwrap_err();
    assert!(error.contains("Team-Missing-1.0.0"), "{error}");
    assert!(!target.target_path.join("Consumer").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn adoption_reuses_the_bounded_metadata_parser_before_recording_ownership() {
    let (root, target) = fixture("adopt-metadata-bound");
    write_package(&target, "Consumer");
    fs::write(
        target.target_path.join("Consumer/manifest.json"),
        vec![b' '; 256 * 1024 + 1],
    )
    .unwrap();
    let error = manage_package(clone_target(&target), "Consumer", "adopt").unwrap_err();
    assert!(error.contains("size limit"), "{error}");
    assert!(target.target_path.join("Consumer/plugin.dll").exists());
    assert!(!target.target_path.join(ONLINE_MOD_MANIFEST).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_owned_payload_preserves_root_and_nested_configuration_formats() {
    let (root, target) = fixture("configuration-formats");
    write_package(&target, "Example");
    let package = target.target_path.join("Example");
    let configuration = [
        "options.conf",
        "runtime.config",
        "settings.properties",
        "colors.hjson",
        "comments.jsonc",
        "settings.snbt",
        "player.nbt",
        "database.cnf",
        "user.prefs",
        ".env",
        ".env.production",
        "config/extensionless",
    ];
    for relative in configuration {
        fs::write(package.join(relative), b"original configuration").unwrap();
    }
    let mut manifest = InstalledPackages::default();
    manifest.packages.insert(
        "local-Example".into(),
        InstalledPackage {
            thunderstore_intent: None,
            routed_files: BTreeMap::new(),
            target_name: "Example".into(),
            files: fingerprint_tree(&package).unwrap(),
            version: None,
            dependencies: None,
            enabled: true,
        },
    );
    persist(&target, &manifest);
    fs::write(package.join("options.conf"), b"changed configuration").unwrap();
    let result = manage_package(clone_target(&target), "local-Example", "delete").unwrap();
    let retained = PathBuf::from(result.preserved_path.unwrap());
    assert_eq!(result.removed_file_count, 1);
    assert!(!retained.join("plugin.dll").exists());
    assert!(!package.exists());
    for relative in configuration {
        let expected: &[u8] = if relative == "options.conf" {
            b"changed configuration"
        } else {
            b"original configuration"
        };
        assert_eq!(
            fs::read(retained.join(relative)).unwrap(),
            expected,
            "{relative}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_a_recorded_configuration_root_file_preserves_its_bytes() {
    let (root, target) = fixture("configuration-root-file");
    for name in ["settings.properties", "settings.conf"] {
        let file = target.target_path.join(name);
        fs::write(&file, b"configuration root").unwrap();
        let mut manifest = read_manifest(&target.target_path.join(ONLINE_MOD_MANIFEST)).unwrap();
        let key = format!("local-{name}");
        manifest.packages.insert(
            key.clone(),
            InstalledPackage {
                thunderstore_intent: None,
                routed_files: BTreeMap::new(),
                target_name: name.into(),
                files: fingerprint_tree(&file).unwrap(),
                version: None,
                dependencies: None,
                enabled: true,
            },
        );
        persist(&target, &manifest);
        let result = manage_package(clone_target(&target), &key, "delete").unwrap();
        assert_eq!(result.removed_file_count, 0);
        assert_eq!(
            fs::read(result.preserved_path.unwrap()).unwrap(),
            b"configuration root"
        );
        assert!(!file.exists());
    }
    fs::remove_dir_all(root).unwrap();
}
