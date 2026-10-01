use super::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-online-mod-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn target(&self) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "server".into(),
            module_id: "minecraft".into(),
            source_label: "Online Mods".into(),
            target_label: "mods".into(),
            target_path: self.root.join("mods"),
            accepts: vec!["jar".into(), "folder".into()],
            id_strategy: None,
        }
    }

    fn jar(&self, project: &str, version: &str) -> DownloadedManualModSource {
        let path = self.root.join(format!(
            "download-{}-{version}.jar",
            uuid::Uuid::new_v4().simple()
        ));
        fs::write(&path, version).unwrap();
        DownloadedManualModSource {
            path,
            label: version.into(),
            identity: OnlineModIdentity {
                provider: "modrinth".into(),
                project: project.into(),
                historical_names: vec![project.into(), "fabric-api".into()],
                version: version.into(),
                dependencies: Vec::new(),
            },
        }
    }

    fn plugins(&self, version: &str, entries: &[(&str, &str)]) -> DownloadedManualModSource {
        let path = self.root.join(format!(
            "unpack-{}-{version}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).unwrap();
        for (name, contents) in entries {
            let file = path.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, contents).unwrap();
        }
        DownloadedManualModSource {
            path,
            label: version.into(),
            identity: OnlineModIdentity {
                provider: "thunderstore".into(),
                project: "Author-Plugin".into(),
                historical_names: vec!["Author-Plugin".into()],
                version: version.into(),
                dependencies: Vec::new(),
            },
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn online_mod_replacement_respects_installed_consumers_and_preserves_rejected_state() {
    let fixture = Fixture::new();
    let mut dependency = fixture.plugins("1.2.3", &[("dependency.dll", "original")]);
    dependency.identity.project = "Author-Dependency".into();
    let mut consumer = fixture.plugins("2.0.0", &[("consumer.dll", "consumer")]);
    consumer.identity.project = "Author-Consumer".into();
    consumer.identity.dependencies = vec!["Author-Dependency-1.2.3".into()];
    let mut outer = fixture.plugins("3.0.0", &[("outer.dll", "outer")]);
    outer.identity.project = "Author-Outer".into();
    outer.identity.dependencies = vec!["Author-Consumer-2.0.0".into()];
    stage_online_mod_sources(fixture.target(), &[dependency, consumer.clone(), outer]).unwrap();
    let target = fixture.target().target_path;
    let manifest_path = target.join(ONLINE_MOD_MANIFEST);
    let mut manifest = read_manifest(&manifest_path).unwrap();
    manifest.packages.insert(
        "thunderstore-Author-Unrelated".into(),
        InstalledPackage {
            target_name: "thunderstore-Author-Unrelated".into(),
            files: BTreeMap::new(),
            version: None,
            dependencies: None,
        },
    );
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    // An unrelated old record must not prevent installing independent packages.
    stage_online_mod_sources(fixture.target(), &[fixture.jar("Independent", "one")]).unwrap();
    let before = fingerprint_tree(&target).unwrap();
    let mut replacement = fixture.plugins("2.0.0", &[("dependency.dll", "replacement")]);
    replacement.identity.project = "Author-Dependency".into();
    let error = stage_online_mod_sources(fixture.target(), &[replacement.clone()]).unwrap_err();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["code"], "mod_dependencies_unverified");
    assert_eq!(
        error["dependencies"],
        serde_json::json!(["Author-Dependency-1.2.3"])
    );
    assert_eq!(fingerprint_tree(&target).unwrap(), before);

    // Updating the consumer in the same transaction makes the replacement valid.
    consumer.identity.dependencies = vec!["Author-Dependency-2.0.0".into()];
    stage_online_mod_sources(fixture.target(), &[replacement, consumer]).unwrap();
    assert_eq!(
        fs::read_to_string(target.join("thunderstore-Author-Dependency/dependency.dll")).unwrap(),
        "replacement"
    );
}

#[test]
fn online_mod_replacement_rejects_non_normal_manifest_targets_before_filesystem_access() {
    let fixture = Fixture::new();
    let path = fixture.root.join(ONLINE_MOD_MANIFEST);
    for target_name in ["/", "\\", ".", "..", "nested/package", "C:\\"] {
        let manifest = serde_json::json!({
            "packages": {"thunderstore-Author-Dependency": {
                "target_name": target_name,
                "files": {}, "version": "1.2.3", "dependencies": []
            }}
        });
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(
            read_manifest(&path).is_err(),
            "accepted target {target_name:?}"
        );
    }
}

#[test]
fn online_mod_dependencies_require_recorded_versions_and_unchanged_owned_files() {
    let fixture = Fixture::new();
    let mut dependency = fixture.plugins("1.2.3", &[("dependency.dll", "original")]);
    dependency.identity.project = "Author-Dependency".into();
    let mut consumer = fixture.plugins("2.0.0", &[("consumer.dll", "consumer")]);
    consumer.identity.project = "Author-Consumer".into();
    consumer.identity.dependencies = vec!["Author-Dependency-1.2.3".into()];
    let target = fixture.target();
    assert!(
        stage_online_mod_sources(fixture.target(), &[consumer.clone()])
            .unwrap_err()
            .contains("Author-Dependency-1.2.3")
    );
    assert!(
        !target
            .target_path
            .join("thunderstore-Author-Consumer")
            .exists()
    );
    stage_online_mod_sources(fixture.target(), &[dependency]).unwrap();
    stage_online_mod_sources(fixture.target(), &[consumer.clone()]).unwrap();
    fs::write(
        target
            .target_path
            .join("thunderstore-Author-Dependency/dependency.dll"),
        "changed",
    )
    .unwrap();
    assert!(
        stage_online_mod_sources(fixture.target(), &[consumer])
            .unwrap_err()
            .contains("Author-Dependency-1.2.3")
    );
}

#[test]
fn online_mod_dependencies_reject_unknown_versions_and_follow_transitive_requirements() {
    let fixture = Fixture::new();
    let mut dependency = fixture.plugins("1.2.3", &[("dependency.dll", "original")]);
    dependency.identity.project = "Author-Dependency".into();
    let mut consumer = fixture.plugins("2.0.0", &[("consumer.dll", "consumer")]);
    consumer.identity.project = "Author-Consumer".into();
    consumer.identity.dependencies = vec!["Author-Dependency-1.2.3".into()];
    stage_online_mod_sources(fixture.target(), &[dependency]).unwrap();
    let path = fixture.target().target_path.join(ONLINE_MOD_MANIFEST);
    let mut manifest = read_manifest(&path).unwrap();
    manifest
        .packages
        .get_mut("thunderstore-Author-Dependency")
        .unwrap()
        .version = None;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(stage_online_mod_sources(fixture.target(), &[consumer.clone()]).is_err());
    let record = manifest
        .packages
        .get_mut("thunderstore-Author-Dependency")
        .unwrap();
    record.version = Some("1.2.3".into());
    record.dependencies = Some(vec!["Author-Transitive-1.0.0".into()]);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        stage_online_mod_sources(fixture.target(), &[consumer])
            .unwrap_err()
            .contains("Author-Transitive-1.0.0")
    );
}

#[test]
fn online_mod_complete_dependency_batch_is_atomic_and_bounded() {
    let fixture = Fixture::new();
    let mut dependency = fixture.plugins("1.2.3", &[("dependency.dll", "original")]);
    dependency.identity.project = "Author-Dependency".into();
    let mut consumer = fixture.plugins("2.0.0", &[("consumer.dll", "consumer")]);
    consumer.identity.project = "Author-Consumer".into();
    consumer.identity.dependencies = vec!["Author-Dependency-1.2.3".into()];
    stage_online_mod_sources(fixture.target(), &[consumer.clone(), dependency]).unwrap();
    consumer.identity.dependencies = (0..129)
        .map(|index| format!("Author-Dependency{index}-1.0.0"))
        .collect();
    assert!(
        stage_online_mod_sources(fixture.target(), &[consumer])
            .unwrap_err()
            .contains("limit")
    );
}

#[test]
fn online_mod_affected_root_names_exclude_manifest_and_untouched_packages() {
    let fixture = Fixture::new();
    stage_online_mod_sources(fixture.target(), &[fixture.jar("Untouched", "one")]).unwrap();
    let result = stage_online_mod_sources(
        fixture.target(),
        &[
            fixture.plugins("one", &[("plugin.dll", "plugin")]),
            fixture.jar("NewProject", "one"),
        ],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&result).unwrap()["affected_root_names"],
        serde_json::json!(["modrinth-NewProject.jar", "thunderstore-Author-Plugin"])
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("mods/modrinth-Untouched.jar")).unwrap(),
        "one"
    );
    assert!(
        fixture
            .root
            .join("mods")
            .join(ONLINE_MOD_MANIFEST)
            .is_file()
    );
}

#[test]
fn online_mod_same_project_reinstall_and_upgrade_keep_one_stable_jar() {
    let fixture = Fixture::new();
    for version in ["one", "one", "two"] {
        let source = fixture.jar("P7dR8mSH", version);
        let result = stage_online_mod_sources(fixture.target(), &[source]).unwrap();
        assert_eq!(
            result.items[0].target_path,
            fixture
                .root
                .join("mods")
                .join("modrinth-P7dR8mSH.jar")
                .to_string_lossy()
        );
    }
    let files = fs::read_dir(fixture.root.join("mods"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 2, "one jar and one ownership record");
    assert_eq!(
        fs::read_to_string(fixture.root.join("mods/modrinth-P7dR8mSH.jar")).unwrap(),
        "two"
    );
    assert_eq!(
        super::super::super::commands_mods::read_manual_mod_inventory_items(
            None,
            &fixture.root.join("mods")
        )
        .unwrap()
        .len(),
        1
    );
}

#[test]
fn online_mod_update_removes_old_payload_and_preserves_unowned_configuration() {
    let fixture = Fixture::new();
    let first = fixture.plugins("one", &[("old.dll", "old"), ("nested/current.dll", "one")]);
    stage_online_mod_sources(fixture.target(), &[first]).unwrap();
    let installed = fixture.root.join("mods/thunderstore-Author-Plugin");
    fs::write(installed.join("operator.cfg"), "operator setting").unwrap();
    let second = fixture.plugins("two", &[("nested/current.dll", "two")]);
    stage_online_mod_sources(fixture.target(), &[second]).unwrap();
    assert!(!installed.join("old.dll").exists());
    assert_eq!(
        fs::read_to_string(installed.join("nested/current.dll")).unwrap(),
        "two"
    );
    assert_eq!(
        fs::read_to_string(installed.join("operator.cfg")).unwrap(),
        "operator setting"
    );
    let third = fixture.plugins("three", &[("nested/current.dll", "three")]);
    stage_online_mod_sources(fixture.target(), &[third]).unwrap();
    assert_eq!(
        fs::read_to_string(installed.join("operator.cfg")).unwrap(),
        "operator setting"
    );
}

#[test]
fn online_mod_modified_owned_file_blocks_update_without_changing_record_or_payload() {
    let fixture = Fixture::new();
    stage_online_mod_sources(fixture.target(), &[fixture.jar("Example", "one")]).unwrap();
    let path = fixture.root.join("mods/modrinth-Example.jar");
    let manifest_path = fixture.root.join("mods").join(ONLINE_MOD_MANIFEST);
    let record = fs::read(&manifest_path).unwrap();
    fs::write(&path, "operator-modified").unwrap();
    let error =
        stage_online_mod_sources(fixture.target(), &[fixture.jar("Example", "two")]).unwrap_err();
    assert!(error.contains("modified outside the manager"));
    assert_eq!(fs::read_to_string(path).unwrap(), "operator-modified");
    assert_eq!(fs::read(manifest_path).unwrap(), record);
}

#[test]
fn online_mod_unowned_file_collision_blocks_the_complete_batch() {
    let fixture = Fixture::new();
    stage_online_mod_sources(
        fixture.target(),
        &[fixture.plugins("one", &[("plugin.dll", "one")])],
    )
    .unwrap();
    let installed = fixture.root.join("mods/thunderstore-Author-Plugin");
    fs::write(installed.join("operator.cfg"), "user").unwrap();
    let error = stage_online_mod_sources(
        fixture.target(),
        &[
            fixture.jar("Other", "one"),
            fixture.plugins("two", &[("plugin.dll", "two"), ("operator.cfg", "vendor")]),
        ],
    )
    .unwrap_err();
    assert!(error.contains("conflicts with user file"));
    assert!(!fixture.root.join("mods/modrinth-Other.jar").exists());
    assert_eq!(
        fs::read_to_string(installed.join("operator.cfg")).unwrap(),
        "user"
    );
    assert_eq!(
        fs::read_to_string(installed.join("plugin.dll")).unwrap(),
        "one"
    );
}

#[test]
fn online_mod_untracked_old_download_is_retained_and_cannot_gain_a_duplicate() {
    let fixture = Fixture::new();
    let target = fixture.root.join("mods");
    fs::create_dir_all(&target).unwrap();
    let historical =
        target.join("langame-mod-fabric-api-1.0-0123456789abcdef0123456789abcdef-fabric-api.jar");
    fs::write(&historical, "retained old package").unwrap();
    let error =
        stage_online_mod_sources(fixture.target(), &[fixture.jar("P7dR8mSH", "two")]).unwrap_err();
    assert!(error.contains(&historical.to_string_lossy().to_string()));
    assert_eq!(
        fs::read_to_string(historical).unwrap(),
        "retained old package"
    );
    assert_eq!(fs::read_dir(target).unwrap().count(), 1);
}

#[test]
fn online_mod_rejects_missing_canonical_identity_before_publishing() {
    let fixture = Fixture::new();
    assert!(
        stage_online_mod_sources(fixture.target(), &[fixture.jar("../outside", "one")]).is_err()
    );
    assert!(!fixture.root.join("mods").exists());
}

#[test]
fn online_mod_fingerprinting_bounds_empty_directories_depth_and_file_bytes() {
    let fixture = Fixture::new();
    let tree = fixture.root.join("tree");
    fs::create_dir_all(tree.join("one/two/three")).unwrap();
    assert!(
        fingerprint_tree_with_limits(&tree, 3, 64, 100)
            .unwrap_err()
            .contains("entry")
    );
    assert!(
        fingerprint_tree_with_limits(&tree, 20, 2, 100)
            .unwrap_err()
            .contains("depth")
    );
    fs::write(tree.join("payload"), b"12345").unwrap();
    assert!(
        fingerprint_tree_with_limits(&tree, 20, 64, 4)
            .unwrap_err()
            .contains("byte limit")
    );
    assert_eq!(
        fingerprint_tree_with_limits(&tree, 20, 64, 5)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn online_mod_historical_copy_of_another_package_does_not_block_installation() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.join("mods")).unwrap();
    let unrelated = fixture
        .root
        .join("mods/langame-mod-other-1.0-0123456789abcdef0123456789abcdef-other.jar");
    fs::write(&unrelated, "other package").unwrap();
    stage_online_mod_sources(fixture.target(), &[fixture.jar("P7dR8mSH", "two")]).unwrap();
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "other package");
    assert!(fixture.root.join("mods/modrinth-P7dR8mSH.jar").exists());
}

#[cfg(windows)]
#[test]
fn online_mod_publication_failure_rolls_back_payloads_and_ownership_record() {
    use std::os::windows::fs::OpenOptionsExt;
    let fixture = Fixture::new();
    stage_online_mod_sources(
        fixture.target(),
        &[fixture.jar("A", "one"), fixture.jar("Z", "one")],
    )
    .unwrap();
    let manifest_path = fixture.root.join("mods").join(ONLINE_MOD_MANIFEST);
    let previous = fs::read(&manifest_path).unwrap();
    // Permit hashing but deny rename/delete after the first package has published.
    let locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(fixture.root.join("mods/modrinth-Z.jar"))
        .unwrap();
    let result = stage_online_mod_sources(
        fixture.target(),
        &[fixture.jar("A", "two"), fixture.jar("Z", "two")],
    );
    drop(locked);
    assert!(result.unwrap_err().contains("rolled back"));
    assert_eq!(fs::read(&manifest_path).unwrap(), previous);
    for project in ["A", "Z"] {
        assert_eq!(
            fs::read_to_string(fixture.root.join(format!("mods/modrinth-{project}.jar"))).unwrap(),
            "one"
        );
    }
}
