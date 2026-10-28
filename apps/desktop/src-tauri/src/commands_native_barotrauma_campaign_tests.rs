use super::*;

#[test]
fn native_default_folder_accepts_only_the_verified_empty_knownfolder_literal() {
    let config = std::env::temp_dir().join("baro_owned_config");
    let mut native = NativeRoots {
        savepath: config.clone(),
        default_multiplayer: EMPTY_KNOWN_FOLDER.into(),
        local_app_data_empty: true,
    };
    let (folder, must_exist) = default_multiplayer(&native, &config, &config).unwrap();
    assert_eq!(folder, config.join(EMPTY_KNOWN_FOLDER));
    assert!(folder.starts_with(&config));
    assert!(!must_exist);
    native.local_app_data_empty = false;
    assert!(default_multiplayer(&native, &config, &config).is_err());
    for path in ["../foreign/Multiplayer", "other/Multiplayer", ""] {
        native.default_multiplayer = path.into();
        native.local_app_data_empty = true;
        assert!(default_multiplayer(&native, &config, &config).is_err());
    }
    native.default_multiplayer = config.join("absolute_fallback");
    assert!(default_multiplayer(&native, &config, &config).is_err());
    native.local_app_data_empty = false;
    assert_eq!(
        default_multiplayer(&native, &config, &config).unwrap(),
        (native.default_multiplayer, true)
    );
}

#[test]
fn missing_absolute_fallback_is_allowed_only_in_the_exact_disposable_profile() {
    let fixture = std::env::temp_dir().join("baro_owned_fixture");
    let config = fixture.join("barotrauma");
    let owned = fixture
        .join("profile/AppData/Local")
        .join(EMPTY_KNOWN_FOLDER);
    let mut native = NativeRoots {
        savepath: config.clone(),
        default_multiplayer: owned.clone(),
        local_app_data_empty: false,
    };
    assert_eq!(
        default_multiplayer(&native, &config, &fixture).unwrap(),
        (owned, false)
    );
    for folder in [
        fixture
            .join("other-profile/AppData/Local")
            .join(EMPTY_KNOWN_FOLDER),
        fixture.join("profile/AppData/Local/../foreign/Multiplayer"),
        std::env::temp_dir().join("external-profile/Multiplayer"),
    ] {
        native.default_multiplayer = folder.clone();
        assert_eq!(
            default_multiplayer(&native, &config, &fixture).unwrap(),
            (folder, true)
        );
    }
}

#[test]
fn native_folder_probe_diagnostics_exclude_messages_paths_and_unknown_metadata() {
    assert_eq!(
        probe_failure_detail(
            b"LGSM_BARO_PROBE_FAILURE stage=parse type=System.Xml.XmlException\r\n"
        ),
        Some("stage=parse type=System.Xml.XmlException".into())
    );
    for bytes in [
        b"LGSM_BARO_PROBE_FAILURE stage=open type=System.Error message=secret".as_slice(),
        b"LGSM_BARO_PROBE_FAILURE stage=D:/private type=System.Error",
        b"LGSM_BARO_PROBE_FAILURE stage=parse type=D:/private",
        &[b'x'; 4097],
    ] {
        assert!(probe_failure_detail(bytes).is_none());
    }
}

#[cfg(windows)]
#[tokio::test]
async fn native_folder_probe_reads_canonical_file_streams_and_rejects_xml_entities() {
    let root = std::env::temp_dir().join(format!("baro_xml_{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    let config = root.join("config_player.xml");
    let path = root
        .to_string_lossy()
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;");
    fs::write(&config, format!("<config savepath=\"{path}\"/>")).unwrap();
    let canonical = config.canonicalize().unwrap();
    for candidate in [&config, &canonical] {
        let actual = read_native_roots(candidate).await.unwrap();
        assert_eq!(
            actual.savepath.canonicalize().unwrap(),
            root.canonicalize().unwrap()
        );
        assert!(actual.default_multiplayer.is_absolute());
    }
    fs::write(&config,b"<!DOCTYPE config [<!ENTITY blocked SYSTEM 'file:///invalid'>]><config savepath='&blocked;'/>").unwrap();
    let error = read_native_roots(&config.canonicalize().unwrap())
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("stage=parse type=System.Xml.XmlException"));
    assert!(!error.contains("file:"));
    fs::remove_file(config).unwrap();
    fs::remove_dir(root).unwrap();
}

#[test]
fn native_campaign_prelude_requires_fresh_creation_and_exact_restart_file() {
    let root =
        std::env::temp_dir().join(format!("baro_campaign_{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&root).unwrap();
    // Expand short TEMP aliases before checking ordinary versus verbatim paths.
    let root = dunce::canonicalize(&root).unwrap();
    let save = root.join(format!("{SAVE_NAME}.save"));
    assert!(validate_campaign_files(&root, &save, true).is_ok());
    assert!(validate_campaign_files(&root, &save, false).is_err());
    fs::write(&save, b"native-boundary fixture, not a game save").unwrap();
    assert!(validate_campaign_files(&root, &save, true).is_err());
    assert!(validate_campaign_files(&root, &save, false).is_ok());
    assert!(
        validate_selector(
            &format!("Saved campaigns:\n 0. {}\n", save.display()),
            &save
        )
        .is_ok()
    );
    assert!(
        validate_selector(
            &format!("Saved campaigns:\n 0. {}\n", save.display()),
            &save.canonicalize().unwrap()
        )
        .is_ok()
    );
    assert_eq!(
        selector_path_spelling(r"\\?\D:\fixture\world.save"),
        "d:/fixture/world.save"
    );
    for text in [
        format!("1. {}\n", save.display()),
        format!("0. {}\n1. other.save\n", save.display()),
        "No save files found.".into(),
    ] {
        assert!(validate_selector(&text, &save).is_err());
    }
    assert!(require_empty_fallback(&root, true).is_err());
    fs::remove_file(save).unwrap();
    assert!(require_empty_fallback(&root, true).is_ok());
    fs::remove_dir(root).unwrap();
}
