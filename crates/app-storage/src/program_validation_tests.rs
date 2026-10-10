use super::*;

const STEAM_METADATA: &str = "steamapps/appmanifest_896660.acf";

fn steam_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.descriptor.summary.steam_app_id = Some(896660);
    fixture
}

fn steam_app_state(last_updated: u64) -> Vec<u8> {
    format!(
        r#""AppState"
{{
    "appid" "896660"
    "StateFlags" "4"
    "buildid" "25730807"
    "LastUpdated" "{last_updated}"
    "BytesToDownload" "123"
    "BytesDownloaded" "123"
    "InstalledDepots" {{ "896662" {{ "manifest" "6497701753597013477" }} }}
}}
"#
    )
    .into_bytes()
}

#[test]
fn same_version_steam_validation_refreshes_only_known_metadata_and_retains_copy_state() {
    let fixture = steam_fixture();
    let source = fixture.source();
    put(&source, STEAM_METADATA, &steam_app_state(1));
    fixture.record(&source);
    let before = read_manifest(&source).unwrap().unwrap();
    let validated = steam_app_state(2);
    put(&source, STEAM_METADATA, &validated);
    put(&source, "steamapps/unknown.dll", b"untrusted program");
    put(
        &source,
        "steamapps/appmanifest_42.acf",
        b"untrusted other app",
    );

    assert!(retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap());
    let retained = read_manifest(&source).unwrap().unwrap();
    assert_ne!(retained.files[STEAM_METADATA], before.files[STEAM_METADATA]);
    for (key, hash) in &before.files {
        if key != STEAM_METADATA {
            assert_eq!(retained.files.get(key), Some(hash));
        }
    }
    assert_eq!(retained.files.len(), before.files.len());
    assert!(source.join(".langame-initial-package.json").is_file());
    assert!(library_program_is_pristine(&source, &fixture.descriptor, None).unwrap());
    let initial = require_initial_package_tree(&source, "fixture", None).unwrap();
    assert_eq!(
        initial.files[STEAM_METADATA],
        retained.files[STEAM_METADATA]
    );
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert_eq!(
        fs::read(seed.install_root.join(STEAM_METADATA)).unwrap(),
        validated
    );
    for key in ["steamapps/unknown.dll", "steamapps/appmanifest_42.acf"] {
        assert!(!seed.install_root.join(key).exists());
        assert!(source.join(key).is_file());
    }
}

#[test]
fn steam_metadata_refresh_cannot_certify_changed_programs_or_other_metadata() {
    for changed in [
        "server.bin",
        "assets/required.bin",
        "steamapps/appmanifest_42.acf",
    ] {
        let fixture = steam_fixture();
        let source = fixture.source();
        put(&source, STEAM_METADATA, &steam_app_state(1));
        put(
            &source,
            "steamapps/appmanifest_42.acf",
            b"original other manifest",
        );
        fixture.record(&source);
        put(&source, STEAM_METADATA, &steam_app_state(2));
        put(&source, changed, b"modified contents");
        assert!(
            !retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap()
        );
        assert!(!source.join(CLEAN_PACKAGE).exists());
        assert!(!source.join(".langame-initial-package.json").exists());
        assert_eq!(
            fs::read(source.join(changed)).unwrap(),
            b"modified contents"
        );
    }
}

#[test]
fn steam_metadata_requires_the_expected_app_and_completed_installation() {
    let valid = String::from_utf8(steam_app_state(2)).unwrap();
    for contents in [
        valid.replace("\"896660\"", "\"42\""),
        valid.replace("\"StateFlags\" \"4\"", "\"StateFlags\" \"1026\""),
        valid.replace("\"buildid\" \"25730807\"", "\"buildid\" \"0\""),
        valid.replace("\"BytesDownloaded\" \"123\"", "\"BytesDownloaded\" \"122\""),
        "invalid KeyValues".into(),
    ] {
        let fixture = steam_fixture();
        let source = fixture.source();
        put(&source, STEAM_METADATA, &steam_app_state(1));
        fixture.record(&source);
        put(&source, STEAM_METADATA, contents.as_bytes());
        assert!(
            !retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap()
        );
        assert!(!source.join(CLEAN_PACKAGE).exists());
        assert!(!source.join(".langame-initial-package.json").exists());
    }
}

#[test]
fn exact_official_allowlist_recording_excludes_unknown_existing_files() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "user.cfg", b"official defaults");
    fixture.record(&source);
    let inventory = require_initial_package_tree(&source, "fixture", None).unwrap();
    put(&source, "unknown.dll", b"untrusted program");
    put(&source, "steamapps/operator.txt", b"operator metadata");
    write_acquisition(&source, &source, "fixture").unwrap();
    package::record_verified_library_program_baseline(
        &source,
        &fixture.descriptor,
        inventory.files,
        inventory.directories,
        None,
    )
    .unwrap();
    assert!(!source.join(".langame-program-acquisition.json").exists());
    let clean = read_manifest(&source).unwrap().unwrap();
    assert!(!clean.files.contains_key("unknown.dll"));
    assert!(!clean.files.contains_key("steamapps/operator.txt"));
    assert!(!clean.files.contains_key("user.cfg"));
    let initial = require_initial_package_tree(&source, "fixture", None).unwrap();
    assert!(initial.files.contains_key("user.cfg"));
    assert!(!initial.files.contains_key("unknown.dll"));
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    for key in ["unknown.dll", "steamapps/operator.txt", "user.cfg"] {
        assert!(!seed.install_root.join(key).exists());
        assert!(source.join(key).is_file());
    }
}

#[test]
fn steam_metadata_refresh_rejects_pending_downloads_and_excessive_metadata() {
    for oversized in [false, true] {
        let fixture = steam_fixture();
        let source = fixture.source();
        put(&source, STEAM_METADATA, &steam_app_state(1));
        fixture.record(&source);
        if oversized {
            put(&source, STEAM_METADATA, &vec![b' '; 1024 * 1024 + 1]);
        } else {
            put(&source, STEAM_METADATA, &steam_app_state(2));
            put(
                &source,
                "steamapps/downloading/896660/pending.bin",
                b"unfinished program",
            );
        }
        assert!(
            !retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap()
        );
        assert!(!source.join(CLEAN_PACKAGE).exists());
        assert!(!source.join(".langame-initial-package.json").exists());
    }
}

#[test]
fn steam_validation_does_not_add_a_manifest_absent_from_the_official_allowlist() {
    let fixture = steam_fixture();
    let source = fixture.source();
    fixture.record(&source);
    put(&source, STEAM_METADATA, &steam_app_state(2));
    assert!(retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap());
    assert!(
        !read_manifest(&source)
            .unwrap()
            .unwrap()
            .files
            .contains_key(STEAM_METADATA)
    );
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert!(!seed.install_root.join(STEAM_METADATA).exists());
}

#[test]
fn exact_official_allowlist_recording_rejects_changed_payload_before_publication() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let inventory = require_initial_package_tree(&source, "fixture", None).unwrap();
    let before = fs::read(source.join(CLEAN_PACKAGE)).unwrap();
    put(&source, "server.bin", b"changed program");
    assert!(matches!(
        package::record_verified_library_program_baseline(
            &source,
            &fixture.descriptor,
            inventory.files,
            inventory.directories,
            None,
        ),
        Err(StorageError::CleanLibraryProgramRequired { .. })
    ));
    assert_eq!(fs::read(source.join(CLEAN_PACKAGE)).unwrap(), before);
}

#[test]
fn no_op_validation_preserves_existing_manifest_bytes_without_trusting_extra_files() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    let clean = fs::read(source.join(CLEAN_PACKAGE)).unwrap();
    let initial = fs::read(source.join(".langame-initial-package.json")).unwrap();
    put(&source, "custom-loader.dll", b"user modification");

    assert!(retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap());
    assert_eq!(fs::read(source.join(CLEAN_PACKAGE)).unwrap(), clean);
    assert_eq!(
        fs::read(source.join(".langame-initial-package.json")).unwrap(),
        initial
    );
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert!(!seed.install_root.join("custom-loader.dll").exists());
    assert_eq!(
        fs::read(source.join("custom-loader.dll")).unwrap(),
        b"user modification"
    );
}

#[test]
fn validation_drops_changed_initial_defaults_but_keeps_verified_program_allowlist() {
    let fixture = Fixture::new();
    let source = fixture.source();
    put(&source, "user.cfg", b"shipped default");
    fixture.record(&source);
    let clean = fs::read(source.join(CLEAN_PACKAGE)).unwrap();
    put(&source, "user.cfg", b"user settings");

    assert!(retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap());
    assert_eq!(fs::read(source.join(CLEAN_PACKAGE)).unwrap(), clean);
    assert!(!source.join(".langame-initial-package.json").exists());
    assert_eq!(fs::read(source.join("user.cfg")).unwrap(), b"user settings");
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert!(!seed.install_root.join("user.cfg").exists());
}

#[test]
fn validation_never_certifies_changed_programs_or_sources_without_an_allowlist() {
    for had_manifest in [false, true] {
        let fixture = Fixture::new();
        let source = fixture.source();
        if had_manifest {
            fixture.record(&source);
        }
        put(&source, "server.bin", b"changed program");
        put(&source, "unknown.dll", b"untrusted program");

        assert!(
            !retain_verified_library_program_baseline(&source, &fixture.descriptor, None).unwrap()
        );
        assert!(!source.join(CLEAN_PACKAGE).exists());
        assert!(!source.join(".langame-initial-package.json").exists());
        assert_eq!(
            fs::read(source.join("server.bin")).unwrap(),
            b"changed program"
        );
        assert_eq!(
            fs::read(source.join("unknown.dll")).unwrap(),
            b"untrusted program"
        );
        let seed = fixture.seed(&source).unwrap();
        assert!(seed.requires_validation);
        assert!(!seed.install_root.join("server.bin").exists());
    }
}

#[tokio::test]
async fn registered_library_seed_reuses_only_matching_program_files_and_preserves_source() {
    for changed_program in [false, true] {
        let fixture = Fixture::new();
        let paths = fixture.paths();
        let source = fixture.source();
        for path in ["Saved/world.sav", "mods/mod.dll", "user.cfg"] {
            put(&source, path, b"private data");
        }
        fixture.record(&source);
        put(&source, "unknown.dll", b"untrusted program");
        if changed_program {
            put(&source, "server.bin", b"changed program");
        }
        fs::create_dir_all(paths.database_path.parent().unwrap()).unwrap();
        crate::initialize_database(&paths).await.unwrap();
        crate::sync_modules(&paths, std::slice::from_ref(&fixture.descriptor))
            .await
            .unwrap();
        crate::sync_game_installs(
            &paths,
            &[crate::GameInstallSyncRecord {
                module_id: fixture.descriptor.summary.id.clone(),
                install_root: source.to_string_lossy().into_owned(),
                install_state: app_core::InstallState::Installed,
                current_version: Some("official-build-1".into()),
                mark_verified: true,
            }],
        )
        .await
        .unwrap();

        let seed =
            prepare_clean_library_seed_at(&paths, &fixture.descriptor, &fixture.target(), None)
                .await
                .unwrap();
        assert_eq!(seed.requires_validation, changed_program);
        assert_eq!(
            seed.install_root.join("server.bin").is_file(),
            !changed_program
        );
        assert_eq!(
            fs::read(seed.install_root.join("assets/required.bin")).unwrap(),
            b"official assets"
        );
        for path in ["Saved/world.sav", "mods/mod.dll", "user.cfg", "unknown.dll"] {
            assert!(
                !seed.install_root.join(path).exists(),
                "copied user path: {path}"
            );
            assert!(source.join(path).is_file(), "removed user path: {path}");
        }
        assert_eq!(
            fs::read(source.join("server.bin")).unwrap(),
            if changed_program {
                b"changed program".as_slice()
            } else {
                b"official executable".as_slice()
            }
        );
        let registered =
            crate::read_library_program_install(&paths, &fixture.descriptor.summary.id)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(registered.install_root, source);
        fixture.no_staging();
    }
}
