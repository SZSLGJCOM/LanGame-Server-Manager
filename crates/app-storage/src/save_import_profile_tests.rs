use super::*;

#[test]
fn save_import_necesse_keeps_native_zip_and_requires_selected_world() {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fixture.context.record.summary.module_id = "necesse".into();
    fixture.context.settings =
        serde_json::from_value(json!({"world_name":"Colony","zip_saves":true})).unwrap();
    let native = b"PK\x03\x04synthetic native world container";
    fs::write(fixture.source.join("Colony.zip"), native).unwrap();
    let checked = fixture.preview();
    fixture.import(&checked.fingerprint).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("Colony.zip")).unwrap(),
        native
    );
    assert_eq!(fs::read(fixture.source.join("Colony.zip")).unwrap(), native);
    fixture
        .context
        .settings
        .insert("world_name".into(), json!("Different"));
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fixture
        .context
        .settings
        .insert("world_name".into(), json!("Colony"));
    fs::write(fixture.source.join("Colony.zip"), b"not a native ZIP").unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_necesse_directory_keeps_world_settings_but_refuses_server_configuration() {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir_all(fixture.source.join("Colony/levels")).unwrap();
    fixture.context.record.summary.module_id = "necesse".into();
    fixture.context.settings =
        serde_json::from_value(json!({"world_name":"Colony","zip_saves":false})).unwrap();
    for (path, bytes) in [
        ("Colony/world.dat", "world"),
        ("Colony/worldSettings.cfg", "native world rules"),
        ("Colony/levels/island.dat", "island"),
    ] {
        fs::write(fixture.source.join(path), bytes).unwrap();
    }
    fixture.import(&fixture.preview().fingerprint).unwrap();
    assert_eq!(
        fs::read(fixture.context.target.join("Colony/worldSettings.cfg")).unwrap(),
        b"native world rules"
    );
    fs::write(
        fixture.source.join("server.cfg"),
        b"unrelated server configuration",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::remove_file(fixture.source.join("server.cfg")).unwrap();
    fs::remove_file(fixture.source.join("Colony/world.dat")).unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}

#[test]
fn save_import_barotrauma_keeps_campaign_character_pair_and_matching_backup_pair() {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fixture.context.record.summary.module_id = "barotrauma".into();
    for prefix in ["Campaign", ".Campaign"] {
        let suffix = if prefix.starts_with('.') { ".bk0" } else { "" };
        fs::write(
            fixture.source.join(format!("{prefix}.save{suffix}")),
            b"\x1f\x8b\x08synthetic campaign container",
        )
        .unwrap();
        fs::write(
            fixture
                .source
                .join(format!("{prefix}_CharacterData.xml{suffix}")),
            b"<?xml version=\"1.0\"?><CharacterData />",
        )
        .unwrap();
    }
    let expected = io::inventory(&fixture.source, None).unwrap();
    let checked = fixture.preview();
    assert!(
        checked
            .warnings
            .iter()
            .any(|warning| warning.contains("campaign"))
    );
    fixture.import(&checked.fingerprint).unwrap();
    assert_eq!(
        io::inventory(&fixture.context.target, None).unwrap(),
        expected
    );
    assert_eq!(io::inventory(&fixture.source, None).unwrap(), expected);
}

#[test]
fn save_import_barotrauma_refuses_missing_characters_and_unrelated_xml() {
    let mut fixture = Fixture::new();
    fs::remove_dir_all(&fixture.source).unwrap();
    fs::create_dir(&fixture.source).unwrap();
    fixture.context.record.summary.module_id = "barotrauma".into();
    fs::write(
        fixture.source.join("Campaign.save"),
        b"\x1f\x8b\x08synthetic campaign container",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(
        fixture.source.join("Campaign_CharacterData.xml"),
        b"<ServerSettings />",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
    fs::write(
        fixture.source.join("Campaign_CharacterData.xml"),
        b"<CharacterData />",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_ok());
    fs::write(
        fixture.source.join("serversettings.xml"),
        b"<ServerSettings />",
    )
    .unwrap();
    assert!(preview(&fixture.context, &fixture.source).is_err());
}
