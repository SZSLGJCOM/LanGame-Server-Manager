use super::*;

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
