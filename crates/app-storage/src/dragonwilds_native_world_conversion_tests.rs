use super::*;

#[tokio::test]
#[ignore = "requires a stopped isolated native Normal world and a new external output path"]
async fn native_normal_world_converts_to_custom_with_verified_backup_and_preserved_content() {
    let source = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SAVE_FIXTURE")
            .expect("supply the stopped isolated native save"),
    );
    let output = PathBuf::from(
        std::env::var_os("LGSM_DRAGONWILDS_SAVE_OUTPUT")
            .expect("supply a new external output path"),
    );
    assert!(source.is_absolute());
    assert!(
        output.is_absolute()
            && !output
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
    );
    assert!(!output.try_exists().unwrap(), "preserve existing output");
    let parent = output.parent().unwrap();
    let _guards = guard_directories(parent).unwrap();
    let repository =
        fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(!fs::canonicalize(parent).unwrap().starts_with(repository));

    let original = read_world(&source).unwrap();
    let native = decode_world_settings(&original).unwrap();
    assert_eq!(native.world_mode, 0, "supply a native Normal world");
    let fixture = Fixture::new(false).await;
    fs::write(&fixture.world, &original).unwrap();
    let config = fixture.instance.join("config/instance.json");
    let mut settings: serde_json::Value =
        serde_json::from_slice(&fs::read(&config).unwrap()).unwrap();
    settings["settings"]["default_world_name"] =
        serde_json::Value::String(native.world_name.clone());
    fs::write(&config, serde_json::to_vec(&settings).unwrap()).unwrap();

    let before = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert!(before.writable);
    let changed = write_dragonwilds_world_settings(
        &fixture.paths,
        fixture.input(DragonwildsWorldMode::Custom, &[(BUILDING, 1.5)]),
    )
    .await
    .unwrap();
    let mut expected = before.values;
    expected.insert(BUILDING.to_owned(), 1.5);
    assert_eq!(changed.values, expected);
    assert_eq!(changed.world_mode, Some(DragonwildsWorldMode::Custom));
    let actual = fs::read(&fixture.world).unwrap();
    let decoded = decode_world_settings(&actual).unwrap();
    assert_eq!(decoded.world_name, native.world_name);
    assert_eq!(decoded.world_mode, 3);
    assert_eq!(decoded.overrides.get(BUILDING), Some(&1.5));
    assert_eq!(
        patch_world_settings(&actual, native.world_mode, &native.overrides).unwrap(),
        original,
        "reversing the rule and mode edits must restore every native byte"
    );
    let reopened = read_dragonwilds_world_settings(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(reopened.values, changed.values);
    assert_eq!(reopened.revision, changed.revision);
    let backups = crate::list_instance_backups(&fixture.paths, ID)
        .await
        .unwrap();
    assert_eq!(backups.len(), 1);
    assert_eq!(changed.backup_id.as_ref(), Some(&backups[0].backup_id));
    assert_eq!(
        fs::read(Path::new(&backups[0].backup_path).join("saves/Fixture.sav")).unwrap(),
        original
    );
    crate::atomic_file::create_file_atomically(&output, &actual).unwrap();
    assert_eq!(fs::read(&output).unwrap(), actual);
    assert_eq!(fs::read(&source).unwrap(), original);
}
