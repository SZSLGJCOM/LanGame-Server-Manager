use super::*;

#[test]
fn asa_startup_updates_owned_loader_policy_without_upgrading_plugin_or_data() {
    let (fixture, profile) = inspection_fixture(&ASA, true);
    let previous = managed_configuration(&ASA, true).unwrap();
    let owner_path = fixture.bin().join(OWNER);
    let mut owned: Ownership = serde_json::from_slice(&fs::read(&owner_path).unwrap()).unwrap();
    owned.files.insert("config.json".into(), digest(&previous));
    fs::write(fixture.bin().join("config.json"), previous).unwrap();
    fs::write(&owner_path, serde_json::to_vec(&owned).unwrap()).unwrap();
    let retained: BTreeMap<_, _> = owned
        .files
        .keys()
        .filter(|path| !matches!(path.as_str(), "version.dll" | "config.json"))
        .map(|path| (path.clone(), fs::read(fixture.bin().join(path)).unwrap()))
        .collect();
    let cache = fixture.bin().join("ArkApi/Cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("cached_key.cache"), b"retained cache").unwrap();
    let saved = fixture.0.join("ShooterGame/Saved/world.ark");
    fs::create_dir_all(saved.parent().unwrap()).unwrap();
    fs::write(&saved, b"retained world").unwrap();

    super::super::startup::refresh_profile(&fixture.0, &profile, inspection_embedded(false))
        .unwrap();
    let current = embedded_files(&profile, inspection_embedded(false)).unwrap();
    for path in ["version.dll", "config.json"] {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), current[path]);
    }
    for (path, bytes) in retained {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), bytes);
    }
    assert_eq!(
        fs::read(cache.join("cached_key.cache")).unwrap(),
        b"retained cache"
    );
    assert_eq!(fs::read(saved).unwrap(), b"retained world");
    let owner = fs::read(&owner_path).unwrap();
    super::super::startup::refresh_profile(&fixture.0, &profile, inspection_embedded(false))
        .unwrap();
    assert_eq!(fs::read(owner_path).unwrap(), owner);
    assert!(!transaction::pending(&fixture.bin()).unwrap());
    fixture.finish();
}

#[test]
fn asa_startup_preserves_modified_owned_files_and_their_provenance() {
    for path in ["version.dll", "config.json", PLUGIN] {
        let (fixture, profile) = inspection_fixture(&ASA, true);
        let owner = fs::read(fixture.bin().join(OWNER)).unwrap();
        fs::write(fixture.bin().join(path), b"operator modified").unwrap();
        let error = super::super::startup::refresh_profile(
            &fixture.0,
            &profile,
            inspection_embedded(false),
        )
        .unwrap_err();
        assert!(error.contains(path));
        assert_eq!(
            fs::read(fixture.bin().join(path)).unwrap(),
            b"operator modified"
        );
        assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), owner);
        assert!(!transaction::pending(&fixture.bin()).unwrap());
        fixture.finish();
    }
}

#[test]
fn asa_startup_does_not_reinstall_a_removed_extension_or_adopt_an_unowned_loader() {
    let (fixture, profile) = inspection_fixture(&ASA, true);
    for path in [PLUGIN, INFO, NOTICES] {
        fs::remove_file(fixture.bin().join(path)).unwrap();
    }
    let current = embedded_files(&profile, inspection_embedded(false)).unwrap();
    super::super::startup::refresh_profile(&fixture.0, &profile, inspection_embedded(false))
        .unwrap();
    assert!(!fixture.bin().join(PLUGIN).exists());
    for path in ["version.dll", "config.json"] {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), current[path]);
    }
    fs::remove_file(fixture.bin().join(OWNER)).unwrap();
    fs::write(fixture.bin().join("version.dll"), b"operator loader").unwrap();
    super::super::startup::refresh_profile(&fixture.0, &profile, inspection_embedded(false))
        .unwrap();
    assert_eq!(
        fs::read(fixture.bin().join("version.dll")).unwrap(),
        b"operator loader"
    );
    assert!(!fixture.bin().join(OWNER).exists());
    fixture.finish();
}

#[test]
fn asa_retained_framework_update_rolls_back_without_reinstalling_removed_plugin() {
    let (fixture, profile) = inspection_fixture(&ASA, true);
    for path in [PLUGIN, INFO, NOTICES] {
        fs::remove_file(fixture.bin().join(path)).unwrap();
    }
    let original_owner = fs::read(fixture.bin().join(OWNER)).unwrap();
    let owned: Ownership = serde_json::from_slice(&original_owner).unwrap();
    let original: BTreeMap<_, _> = owned
        .files
        .keys()
        .filter(|path| !matches!(path.as_str(), PLUGIN | INFO | NOTICES))
        .map(|path| (path.clone(), fs::read(fixture.bin().join(path)).unwrap()))
        .collect();
    let current = embedded_files(&profile, inspection_embedded(false)).unwrap();
    let mut payload = original.clone();
    let mut wanted = owned.files;
    for path in ["version.dll", "config.json"] {
        payload.insert(path.into(), current[path].clone());
        wanted.insert(path.into(), digest(&current[path]));
    }
    assert!(
        publish_with(&fixture.0, &profile, &mut payload, &wanted, |_| Err(
            "injected publication failure".into()
        ))
        .is_err()
    );
    assert_eq!(fs::read(fixture.bin().join(OWNER)).unwrap(), original_owner);
    for (path, bytes) in original {
        assert_eq!(fs::read(fixture.bin().join(path)).unwrap(), bytes);
    }
    for path in [PLUGIN, INFO, NOTICES] {
        assert!(!fixture.bin().join(path).exists());
    }
    assert!(!transaction::pending(&fixture.bin()).unwrap());
    fixture.finish();
}
