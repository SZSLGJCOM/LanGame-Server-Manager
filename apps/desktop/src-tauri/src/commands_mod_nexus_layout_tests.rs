use super::*;

const MOD_INFO: &[u8] = b"<xml><Name value=\"FixtureMod\"/><Version value=\"1.0\"/></xml>";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "langame-nexus-layout-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn target(&self, module: &str) -> ResolvedManualModTarget {
        ResolvedManualModTarget {
            instance_id: "nexus-layout-fixture".into(),
            module_id: module.into(),
            source_label: "Nexus Mods".into(),
            target_label: "Mods".into(),
            target_path: self.0.join("runtime/Mods"),
            accepts: vec!["folder".into(), "zip".into()],
            id_strategy: None,
        }
    }

    fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut archive = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        for (name, bytes) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
        path
    }

    fn stage(&self, source: PathBuf, project_id: &str) -> Result<ManualModStageResult, String> {
        stage_website_mod_source(
            self.target("sevendaystodie"),
            source,
            ManualModImportPresentation {
                display_name: Some("Public Nexus title".into()),
                icon_url: None,
            },
            Some(project_id),
        )
    }

    fn manifest(&self) -> Value {
        serde_json::from_slice(
            &fs::read(
                self.target("sevendaystodie")
                    .target_path
                    .join(ONLINE_MOD_MANIFEST),
            )
            .unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn nexus_seven_days_routes_wrapped_multiple_mods_with_separate_ownership() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "payload.zip",
        &[
            ("Release/Mods/First/ModInfo.xml", MOD_INFO),
            ("Release/Mods/First/Config/items.xml", b"<configs/>"),
            ("Release/Mods/Second/ModInfo.xml", MOD_INFO),
            ("Release/Mods/Second/plugin.dll", b"second"),
        ],
    );
    let result = fixture.stage(source, "13866").unwrap();
    assert_eq!(result.affected_root_names, vec!["First", "Second"]);
    let target = fixture.target("sevendaystodie").target_path;
    assert!(target.join("First/ModInfo.xml").is_file());
    assert!(target.join("First/Config/items.xml").is_file());
    assert!(target.join("Second/ModInfo.xml").is_file());
    assert!(!target.join("Release").exists());
    assert!(!target.join("Mods").exists());
    let manifest = fixture.manifest();
    assert_eq!(manifest["packages"].as_object().unwrap().len(), 2);
    for name in ["First", "Second"] {
        let record = &manifest["packages"][format!("local-{name}")];
        assert_eq!(record["target_name"], name);
        assert_eq!(record["display_name"], "Public Nexus title");
        assert!(record["files"].get("ModInfo.xml").is_some());
    }
    for action in ["disable", "enable"] {
        manage_package(fixture.target("sevendaystodie"), "local-Second", action).unwrap();
    }
    assert_eq!(
        fs::read(target.join("Second/plugin.dll")).unwrap(),
        b"second"
    );
    assert_eq!(
        fixture.manifest()["packages"]["local-Second"]["display_name"],
        "Public Nexus title"
    );
}

#[test]
fn nexus_seven_days_root_packages_use_project_identity_across_updates() {
    let fixture = Fixture::new();
    for (project, folder, bytes) in [
        ("13866", "first", b"first".as_slice()),
        ("13880", "second", b"second".as_slice()),
        ("13866", "updated", b"updated".as_slice()),
    ] {
        let source = fixture.zip(
            &format!("{folder}/payload.zip"),
            &[("ModInfo.xml", MOD_INFO), ("plugin.dll", bytes)],
        );
        fixture.stage(source, project).unwrap();
    }
    let target = fixture.target("sevendaystodie").target_path;
    assert_eq!(
        fs::read(target.join("nexus-7daystodie-13866/plugin.dll")).unwrap(),
        b"updated"
    );
    assert_eq!(
        fs::read(target.join("nexus-7daystodie-13880/plugin.dll")).unwrap(),
        b"second"
    );
    assert!(!target.join("payload").exists());
    assert!(!target.join("ModInfo.xml").exists());
    assert_eq!(fixture.manifest()["packages"].as_object().unwrap().len(), 2);
}

#[test]
fn nexus_seven_days_shared_root_with_manifest_gets_stable_independent_name() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "payload.zip",
        &[
            ("Mods/ModInfo.xml", MOD_INFO),
            ("Mods/Config/items.xml", b"<configs/>"),
        ],
    );
    fixture.stage(source, "13866").unwrap();
    assert!(
        fixture
            .target("sevendaystodie")
            .target_path
            .join("nexus-7daystodie-13866/Config/items.xml")
            .is_file()
    );
}

#[test]
fn nexus_seven_days_local_archives_and_folders_share_the_manifest_routes() {
    for directory in [false, true] {
        let fixture = Fixture::new();
        let entries = [
            ("Release/Mods/RealMod/ModInfo.xml", MOD_INFO),
            ("Release/Mods/RealMod/data.bin", b"data".as_slice()),
        ];
        let source = if directory {
            let source = fixture.0.join("Downloaded package");
            for (name, bytes) in entries {
                let path = source.join(name);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            source
        } else {
            fixture.zip("Downloaded package.zip", &entries)
        };
        stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap();
        assert!(
            fixture
                .target("sevendaystodie")
                .target_path
                .join("RealMod/ModInfo.xml")
                .is_file()
        );
        assert_eq!(
            fixture.manifest()["packages"]["local-RealMod"]["target_name"],
            "RealMod"
        );
    }
}

#[test]
fn nexus_seven_days_no_manifest_does_not_invent_a_website_package() {
    let fixture = Fixture::new();
    let source = fixture.zip("payload.zip", &[("Unknown/plugin.dll", b"data")]);
    let error = fixture.stage(source.clone(), "13866").unwrap_err();
    assert!(error.contains("no ModInfo.xml"), "{error}");
    assert!(!fixture.target("sevendaystodie").target_path.exists());
    // Existing local file imports remain supported without inferring a package layout.
    stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap();
    assert!(
        fixture
            .target("sevendaystodie")
            .target_path
            .join("Unknown/plugin.dll")
            .is_file()
    );
}

#[test]
fn nexus_seven_days_ambiguous_roots_fail_before_publication() {
    for roots in [
        ["Release/One/ModInfo.xml", "Other/one/ModInfo.xml"],
        ["ModInfo.xml", "Nested/ModInfo.xml"],
    ] {
        let fixture = Fixture::new();
        let source = fixture.zip("payload.zip", &[(roots[0], MOD_INFO), (roots[1], MOD_INFO)]);
        assert!(fixture.stage(source, "13866").is_err());
        assert!(!fixture.target("sevendaystodie").target_path.exists());
    }
}

#[test]
fn nexus_seven_days_conflicts_preserve_existing_files_and_entire_batch() {
    let fixture = Fixture::new();
    let target = fixture.target("sevendaystodie").target_path;
    fs::create_dir_all(target.join("Taken")).unwrap();
    fs::write(target.join("Taken/other.dll"), b"operator payload").unwrap();
    let source = fixture.zip(
        "payload.zip",
        &[
            ("Release/Taken/ModInfo.xml", MOD_INFO),
            ("Release/Independent/ModInfo.xml", MOD_INFO),
        ],
    );
    let error = fixture.stage(source, "13866").unwrap_err();
    assert!(error.contains("unowned or modified payload"), "{error}");
    assert_eq!(
        fs::read(target.join("Taken/other.dll")).unwrap(),
        b"operator payload"
    );
    assert!(!target.join("Independent").exists());
    assert!(!target.join("Taken/ModInfo.xml").exists());
    assert!(!target.join(ONLINE_MOD_MANIFEST).exists());
}

#[test]
fn nexus_seven_days_does_not_discard_outside_files_or_weaken_archive_validation() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "payload.zip",
        &[
            ("Release/Actual/ModInfo.xml", MOD_INFO),
            ("README.txt", b"author notes"),
        ],
    );
    fixture.stage(source, "13866").unwrap();
    assert_eq!(
        fs::read(
            fixture
                .target("sevendaystodie")
                .target_path
                .join("README.txt")
        )
        .unwrap(),
        b"author notes"
    );
    let unsafe_source = fixture.zip(
        "unsafe.zip",
        &[
            ("Release/Other/ModInfo.xml", MOD_INFO),
            ("../escape.dll", b"unsafe"),
        ],
    );
    assert!(fixture.stage(unsafe_source, "13866").is_err());
    assert!(!fixture.0.join("escape.dll").exists());
    assert!(
        !fixture
            .target("sevendaystodie")
            .target_path
            .join("Other")
            .exists()
    );
}

#[test]
fn nexus_seven_days_layout_keeps_unrelated_games_unchanged() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "payload.zip",
        &[("Release/Mods/Actual/ModInfo.xml", MOD_INFO)],
    );
    stage_website_mod_source(
        fixture.target("minecraft"),
        source,
        ManualModImportPresentation {
            display_name: None,
            icon_url: None,
        },
        Some("13866"),
    )
    .unwrap();
    assert!(
        fixture
            .target("minecraft")
            .target_path
            .join("Release/Mods/Actual/ModInfo.xml")
            .is_file()
    );
    assert!(
        !fixture
            .target("minecraft")
            .target_path
            .join("Actual")
            .exists()
    );
}

#[test]
fn nexus_seven_days_root_naming_rejects_non_project_context() {
    for project in [
        "",
        "0",
        "../other",
        "123?file_id=456",
        "18446744073709551616",
    ] {
        let fixture = Fixture::new();
        let source = fixture.zip("payload.zip", &[("ModInfo.xml", MOD_INFO)]);
        assert!(fixture.stage(source, project).is_err());
        assert!(!fixture.target("sevendaystodie").target_path.exists());
    }
}

#[test]
fn nexus_seven_days_directory_depth_limit_prevents_publication() {
    let fixture = Fixture::new();
    let source = fixture.0.join("DeepMod");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("ModInfo.xml"), MOD_INFO).unwrap();
    let mut nested = source.clone();
    for _ in 0..33 {
        nested.push("d");
    }
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("payload.bin"), b"data").unwrap();
    let error =
        stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap_err();
    assert!(error.contains("entry or depth limit"), "{error}");
    assert!(!fixture.target("sevendaystodie").target_path.exists());
}

#[test]
fn nexus_seven_days_local_root_mod_uses_source_name_without_shared_root_collision() {
    let fixture = Fixture::new();
    let source = fixture.zip(
        "Mods.zip",
        &[
            ("ModInfo.xml", MOD_INFO),
            ("Config/items.xml", b"<configs/>"),
        ],
    );
    stage_manual_mod_sources(fixture.target("sevendaystodie"), vec![source]).unwrap();
    assert!(
        fixture
            .target("sevendaystodie")
            .target_path
            .join("7daystodie-Mods/ModInfo.xml")
            .is_file()
    );
    assert_eq!(
        fixture.manifest()["packages"]["local-7daystodie-Mods"]["target_name"],
        "7daystodie-Mods"
    );
}
