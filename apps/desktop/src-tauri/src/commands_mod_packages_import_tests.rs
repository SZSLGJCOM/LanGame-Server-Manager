use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "langame-mod-import-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn target(&self, module: &str) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "instance".into(),
            module_id: module.into(),
            source_label: "local".into(),
            target_label: "Mods".into(),
            target_path: self.0.join(if matches!(module, "valheim" | "vrising") {
                "BepInEx/plugins"
            } else {
                "Mods"
            }),
            accepts: vec!["folder".into(), "zip".into(), "jar".into()],
            id_strategy: None,
        }
    }

    fn source(&self, name: &str, entries: &[(&str, &str)]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        for (name, content) in entries {
            let file = path.join(name);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, content).unwrap();
        }
        path
    }

    fn zip(&self, name: &str, entries: &[(&str, &str)]) -> PathBuf {
        let path = self.0.join(name);
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, content) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn local_import_registers_new_bytes_and_keeps_unowned_configuration_unclaimed() {
    let fixture = Fixture::new();
    let source = fixture.source("Plugin", &[("plugin.dll", "new")]);
    let target = fixture.target("sevendaystodie");
    fs::create_dir_all(target.target_path.join("Plugin")).unwrap();
    fs::write(
        target.target_path.join("Plugin/operator.cfg"),
        "user configuration",
    )
    .unwrap();
    stage_manual_mod_sources(target, vec![source]).unwrap();
    let manifest = read_manifest(&fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST)).unwrap();
    let package = &manifest.packages["local-Plugin"];
    assert!(package.enabled);
    assert!(package.dependencies.is_none());
    assert!(package.version.is_none());
    assert_eq!(package.files.len(), 1);
    assert!(package.files.contains_key("plugin.dll"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("Mods/Plugin/operator.cfg")).unwrap(),
        "user configuration"
    );
}

#[test]
fn local_import_conflict_preserves_the_entire_batch_and_unowned_executable() {
    let fixture = Fixture::new();
    let source = fixture.source("Shared", &[("new.dll", "new")]);
    let target = fixture.target("enshrouded");
    fs::create_dir_all(target.target_path.join("Shared")).unwrap();
    fs::write(target.target_path.join("Shared/unrelated.dll"), "other Mod").unwrap();
    let independent = fixture.0.join("independent.jar");
    fs::write(&independent, "independent").unwrap();
    let error = stage_manual_mod_sources(target, vec![independent, source]).unwrap_err();
    assert!(error.contains("unowned or modified payload"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("Mods/Shared/unrelated.dll")).unwrap(),
        "other Mod"
    );
    assert!(!fixture.0.join("Mods/Shared/new.dll").exists());
    assert!(!fixture.0.join("Mods/independent.jar").exists());
    assert!(!fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST).exists());
}

#[test]
fn local_import_never_replaces_a_user_directory_with_a_file() {
    let fixture = Fixture::new();
    let source = fixture.source("Plugin", &[("nested", "file")]);
    let target = fixture.target("minecraft");
    fs::create_dir_all(target.target_path.join("Plugin/nested")).unwrap();
    fs::write(
        target.target_path.join("Plugin/nested/operator.cfg"),
        "keep",
    )
    .unwrap();
    let error = stage_manual_mod_sources(target, vec![source]).unwrap_err();
    assert!(error.contains("folder with a file"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("Mods/Plugin/nested/operator.cfg")).unwrap(),
        "keep"
    );
}

#[test]
fn local_import_replacement_retains_old_owned_files_but_rejects_modified_configuration() {
    let fixture = Fixture::new();
    let source = fixture.source(
        "Plugin",
        &[("old.dll", "old"), ("operator.cfg", "original")],
    );
    stage_manual_mod_sources(fixture.target("minecraft"), vec![source.clone()]).unwrap();
    fs::remove_file(source.join("old.dll")).unwrap();
    fs::write(source.join("new.dll"), "new").unwrap();
    stage_manual_mod_sources(fixture.target("minecraft"), vec![source.clone()]).unwrap();
    let path = fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST);
    let manifest = read_manifest(&path).unwrap();
    assert!(
        manifest.packages["local-Plugin"]
            .files
            .contains_key("old.dll")
    );
    assert!(
        manifest.packages["local-Plugin"]
            .files
            .contains_key("new.dll")
    );
    let previous = fs::read(&path).unwrap();
    fs::write(fixture.0.join("Mods/Plugin/operator.cfg"), "user change").unwrap();
    assert!(stage_manual_mod_sources(fixture.target("minecraft"), vec![source]).is_err());
    assert_eq!(fs::read(&path).unwrap(), previous);
    assert_eq!(
        fs::read_to_string(fixture.0.join("Mods/Plugin/operator.cfg")).unwrap(),
        "user change"
    );
}

#[test]
fn local_root_configuration_reimport_preserves_existing_bytes_record_and_entire_batch() {
    let fixture = Fixture::new();
    let config = "\u{feff}operator-setting=true\r\n# original bytes\r\n";
    let initial = fixture.zip(
        "initial.zip",
        &[
            ("operator.cfg", config),
            ("Plugin/plugin.dll", "original payload"),
        ],
    );
    stage_manual_mod_sources(fixture.target("minecraft"), vec![initial]).unwrap();
    let target = fixture.0.join("Mods");
    let manifest_path = target.join(ONLINE_MOD_MANIFEST);
    let mut manifest = read_manifest(&manifest_path).unwrap();
    // A previously recorded standalone configuration has a valid ownership
    // entry with "" as its file path. It must remain protected on reimport.
    manifest.packages.insert(
        "local-operator.cfg".into(),
        InstalledPackage {
            thunderstore_intent: None,
            routed_files: BTreeMap::new(),
            target_name: "operator.cfg".into(),
            files: fingerprint_tree(&target.join("operator.cfg")).unwrap(),
            version: None,
            dependencies: None,
            enabled: true,
        },
    );
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let record = fs::read(&manifest_path).unwrap();
    let before = fingerprint_tree(&target).unwrap();
    let replacement = fixture.zip(
        "replacement.zip",
        &[
            ("operator.cfg", "vendor-default=false\n"),
            ("Plugin/plugin.dll", "replacement payload"),
            ("Independent/plugin.dll", "new payload"),
        ],
    );
    let error =
        stage_manual_mod_sources(fixture.target("minecraft"), vec![replacement]).unwrap_err();
    assert!(
        error.contains("import would overwrite configuration"),
        "{error}"
    );
    assert_eq!(
        fs::read(target.join("operator.cfg")).unwrap(),
        config.as_bytes()
    );
    assert_eq!(fs::read(&manifest_path).unwrap(), record);
    assert_eq!(fingerprint_tree(&target).unwrap(), before);
    assert!(!target.join("Independent").exists());
}

#[test]
fn local_zip_multiple_roots_have_separate_ownership_and_exclude_configuration_and_first_party() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "mods.zip",
        &[
            ("One/plugin.dll", "one"),
            ("Two/plugin.dll", "two"),
            ("config/operator.cfg", "user"),
            ("0_TFP_Harmony/harmony.dll", "first party"),
        ],
    );
    stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap();
    let manifest = read_manifest(&fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST)).unwrap();
    assert_eq!(
        manifest.packages.keys().cloned().collect::<Vec<_>>(),
        vec!["local-One", "local-Two"]
    );
    assert_eq!(manifest.packages["local-One"].files.len(), 1);
    assert_eq!(manifest.packages["local-Two"].files.len(), 1);
    assert!(fixture.0.join("Mods/config/operator.cfg").exists());
    assert!(fixture.0.join("Mods/0_TFP_Harmony/harmony.dll").exists());
}

#[test]
fn local_native_shared_root_wrapper_is_rejected_without_changing_existing_bytes() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("Mods/Existing")).unwrap();
    fs::write(fixture.0.join("Mods/Existing/plugin.dll"), "existing Mod").unwrap();
    fs::write(
        fixture.0.join("Mods/Existing/operator.cfg"),
        "operator setting",
    )
    .unwrap();
    let before = fingerprint_tree(&fixture.0.join("Mods")).unwrap();
    let source = fixture.zip(
        "wrapped.zip",
        &[
            ("Mods/New/plugin.dll", "new"),
            ("config/operator.cfg", "configuration"),
        ],
    );
    let error =
        stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap_err();
    assert!(error.contains("select the independent package folder"));
    assert_eq!(fingerprint_tree(&fixture.0.join("Mods")).unwrap(), before);
    assert!(!fixture.0.join("Mods/Mods").exists());
    assert!(!fixture.0.join("Mods/config").exists());
}

#[test]
fn local_thunderstore_archive_has_separate_payload_and_bounded_dependency_metadata() {
    let fixture = Fixture::new();
    let dependency = fixture.source("dependency", &[("dependency.dll", "dependency")]);
    stage_online_mod_sources(
        fixture.target("valheim"),
        &[DownloadedManualModSource {
            path: dependency,
            label: "dependency".into(),
            identity: OnlineModIdentity {
                thunderstore_intent: None,
                provider: "thunderstore".into(),
                project: "Author-Dependency".into(),
                historical_names: vec![],
                version: "1.0.0".into(),
                dependencies: vec![],
            },
        }],
    )
    .unwrap();
    let source = fixture.zip("Plugin-1.2.3.zip", &[
        ("manifest.json", r#"{"name":"Plugin","version_number":"1.2.3","dependencies":["Author-Dependency-1.0.0"]}"#),
        ("BepInEx/plugins/Plugin.dll", "plugin"),
    ]);
    let result = stage_manual_mod_sources(fixture.target("valheim"), vec![source]).unwrap();
    assert_eq!(result.affected_root_names, vec!["local-Plugin"]);
    assert_eq!(
        fs::read_to_string(fixture.0.join("BepInEx/plugins/local-Plugin/Plugin.dll")).unwrap(),
        "plugin"
    );
    assert!(!fixture.0.join("BepInEx/plugins/BepInEx").exists());
    let manifest =
        read_manifest(&fixture.0.join("BepInEx/plugins").join(ONLINE_MOD_MANIFEST)).unwrap();
    let package = &manifest.packages["local-local-Plugin"];
    assert_eq!(package.version.as_deref(), Some("1.2.3"));
    assert_eq!(
        package.dependencies.as_ref().unwrap(),
        &["Author-Dependency-1.0.0"]
    );
}

#[test]
fn local_thunderstore_import_rejects_invalid_metadata_without_publishing() {
    let fixture = Fixture::new();
    for (name, manifest) in [
        (
            "bad-path",
            r#"{"name":"../Plugin","version_number":"1.2.3","dependencies":[]}"#,
        ),
        (
            "bad-version",
            r#"{"name":"Plugin","version_number":"unverified","dependencies":[]}"#,
        ),
        (
            "bad-dependency",
            r#"{"name":"Plugin","version_number":"1.2.3","dependencies":["../../outside"]}"#,
        ),
    ] {
        let source = fixture.source(
            name,
            &[("manifest.json", manifest), ("plugin.dll", "payload")],
        );
        assert!(stage_manual_mod_sources(fixture.target("valheim"), vec![source]).is_err());
        assert!(!fixture.0.join("BepInEx/plugins").exists());
    }
    let source = fixture.source(
        "oversized",
        &[(
            "manifest.json",
            &" ".repeat(MAX_IMPORT_METADATA_BYTES as usize + 1),
        )],
    );
    assert!(
        local_import_metadata(&source)
            .unwrap_err()
            .contains("size limit")
    );
}

#[test]
fn local_unidentified_plugin_archive_is_isolated_with_unknown_dependencies() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "NexusPlugin.zip",
        &[("BepInEx/plugins/Plugin.dll", "payload")],
    );
    stage_manual_mod_sources(fixture.target("vrising"), vec![source]).unwrap();
    let manifest =
        read_manifest(&fixture.0.join("BepInEx/plugins").join(ONLINE_MOD_MANIFEST)).unwrap();
    let package = &manifest.packages["local-local-NexusPlugin"];
    assert!(package.dependencies.is_none());
    assert!(
        fixture
            .0
            .join("BepInEx/plugins/local-NexusPlugin/Plugin.dll")
            .exists()
    );
    assert!(!fixture.0.join("BepInEx/plugins/BepInEx").exists());
}

#[test]
fn local_loader_archive_is_rejected_before_installation() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "BepInExPack.zip",
        &[
            ("BepInEx/core/BepInEx.dll", "loader"),
            ("winhttp.dll", "proxy"),
        ],
    );
    let error = stage_manual_mod_sources(fixture.target("valheim"), vec![source]).unwrap_err();
    let error = error.replace('\\', "/");
    assert!(
        error.contains("loader") || error.contains("reviewed"),
        "{error}"
    );
    assert!(!fixture.0.join("BepInEx/plugins").exists());
}

#[test]
fn local_mixed_plugin_configuration_archive_installs_and_preserves_runtime_bytes() {
    let fixture = Fixture::new();
    let mut target = fixture.target("valheim");
    target.target_path = fixture.0.join("BepInEx/plugins");
    fs::create_dir_all(target.target_path.join("Existing")).unwrap();
    fs::create_dir_all(fixture.0.join("BepInEx/config")).unwrap();
    let config_path = fixture.0.join("BepInEx/config/operator.cfg");
    let config_bytes = b"\xef\xbb\xbfoperator-setting=true\r\n# retained bytes\r\n";
    fs::write(&config_path, config_bytes).unwrap();
    fs::write(
        target.target_path.join("Existing/plugin.dll"),
        b"existing plugin",
    )
    .unwrap();
    let source = fixture.zip(
        "mixed.zip",
        &[
            (
                "manifest.json",
                r#"{"name":"Plugin","version_number":"1.0.0","dependencies":[]}"#,
            ),
            ("BepInEx/plugins/Plugin.dll", "new plugin"),
            ("BepInEx/config/operator.cfg", "vendor default"),
        ],
    );
    stage_manual_mod_sources(target, vec![source]).unwrap();
    assert_eq!(fs::read(config_path).unwrap(), config_bytes);
    assert_eq!(
        fs::read(fixture.0.join("BepInEx/plugins/Existing/plugin.dll")).unwrap(),
        b"existing plugin"
    );
    assert_eq!(
        fs::read(fixture.0.join("BepInEx/plugins/local-Plugin/Plugin.dll")).unwrap(),
        b"new plugin"
    );
    let manifest =
        read_manifest(&fixture.0.join("BepInEx/plugins").join(ONLINE_MOD_MANIFEST)).unwrap();
    assert!(
        manifest.packages["local-local-Plugin"]
            .routed_files
            .contains_key("BepInEx/config/operator.cfg")
    );
}

#[test]
fn local_known_dependencies_reject_missing_disabled_and_wrong_versions_before_publish() {
    for state in ["missing", "disabled", "wrong-version"] {
        let fixture = Fixture::new();
        if state != "missing" {
            let source = fixture.source("dependency-download", &[("dependency.dll", "dependency")]);
            stage_online_mod_sources(
                fixture.target("valheim"),
                &[DownloadedManualModSource {
                    path: source,
                    label: "dependency".into(),
                    identity: OnlineModIdentity {
                        thunderstore_intent: None,
                        provider: "thunderstore".into(),
                        project: "Author-Dependency".into(),
                        historical_names: vec![],
                        version: if state == "wrong-version" {
                            "2.0.0"
                        } else {
                            "1.0.0"
                        }
                        .into(),
                        dependencies: vec![],
                    },
                }],
            )
            .unwrap();
            if state == "disabled" {
                let path = fixture.0.join("BepInEx/plugins").join(ONLINE_MOD_MANIFEST);
                let mut manifest = read_manifest(&path).unwrap();
                manifest
                    .packages
                    .get_mut("thunderstore-Author-Dependency")
                    .unwrap()
                    .enabled = false;
                fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
        }
        let before = fingerprint_tree(&fixture.0.join("BepInEx/plugins")).unwrap();
        let source = fixture.zip("consumer.zip", &[
            ("manifest.json", r#"{"name":"Consumer","version_number":"1.0.0","dependencies":["Author-Dependency-1.0.0"]}"#),
            ("plugins/consumer.dll", "consumer"),
        ]);
        let error = stage_manual_mod_sources(fixture.target("valheim"), vec![source]).unwrap_err();
        assert!(
            error.contains("Author-Dependency-1.0.0"),
            "{state}: {error}"
        );
        assert_eq!(
            fingerprint_tree(&fixture.0.join("BepInEx/plugins")).unwrap(),
            before,
            "{state}"
        );
        assert!(!fixture.0.join("BepInEx/plugins/local-Consumer").exists());
    }
}

#[test]
fn local_batch_verifies_replacement_dependencies_against_the_complete_staged_state() {
    let fixture = Fixture::new();
    let mut sources = Vec::new();
    for name in ["Dependency", "Consumer"] {
        let path = fixture.source(&format!("download-{name}"), &[("plugin.dll", "original")]);
        sources.push(DownloadedManualModSource {
            path,
            label: name.into(),
            identity: OnlineModIdentity {
                thunderstore_intent: None,
                provider: "thunderstore".into(),
                project: format!("Author-{name}"),
                historical_names: vec![],
                version: "1.0.0".into(),
                dependencies: if name == "Consumer" {
                    vec!["Author-Dependency-1.0.0".into()]
                } else {
                    vec![]
                },
            },
        });
    }
    stage_online_mod_sources(fixture.target("minecraft"), &sources).unwrap();
    let dependency = fixture.source(
        "thunderstore-Author-Dependency",
        &[
            ("plugin.dll", "replacement"),
            (
                "manifest.json",
                r#"{"name":"Dependency","version_number":"2.0.0","dependencies":[]}"#,
            ),
        ],
    );
    let consumer = fixture.source("thunderstore-Author-Consumer", &[
        ("plugin.dll", "replacement consumer"),
        ("manifest.json", r#"{"name":"Consumer","version_number":"2.0.0","dependencies":["Author-Dependency-2.0.0"]}"#),
    ]);
    let before = fingerprint_tree(&fixture.0.join("Mods")).unwrap();
    assert!(
        stage_manual_mod_sources(fixture.target("minecraft"), vec![dependency.clone()])
            .unwrap_err()
            .contains("Author-Dependency-1.0.0")
    );
    assert_eq!(fingerprint_tree(&fixture.0.join("Mods")).unwrap(), before);
    stage_manual_mod_sources(fixture.target("minecraft"), vec![dependency, consumer]).unwrap();
    let manifest = read_manifest(&fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST)).unwrap();
    assert_eq!(
        manifest.packages["thunderstore-Author-Dependency"]
            .version
            .as_deref(),
        Some("2.0.0")
    );
    assert_eq!(
        fs::read_to_string(
            fixture
                .0
                .join("Mods/thunderstore-Author-Dependency/plugin.dll")
        )
        .unwrap(),
        "replacement"
    );
}

#[test]
fn disabled_local_mod_cannot_be_reimported_as_a_duplicate_active_copy() {
    let fixture = Fixture::new();
    let source = fixture.source("Plugin", &[("plugin.dll", "payload")]);
    stage_manual_mod_sources(fixture.target("enshrouded"), vec![source.clone()]).unwrap();
    let path = fixture.0.join("Mods").join(ONLINE_MOD_MANIFEST);
    let mut manifest = read_manifest(&path).unwrap();
    manifest.packages.get_mut("local-Plugin").unwrap().enabled = false;
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let previous = fs::read(&path).unwrap();
    assert!(
        stage_manual_mod_sources(fixture.target("enshrouded"), vec![source])
            .unwrap_err()
            .contains("disabled")
    );
    assert_eq!(fs::read(&path).unwrap(), previous);
}
