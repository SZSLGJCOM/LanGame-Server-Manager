use super::*;

#[test]
fn captured_acquisition_restores_after_whole_directory_publication_then_finalizes() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let seed = fixture.seed(&source).unwrap();
    let handle = read_library_program_acquisition(&seed.install_root, &fixture.descriptor)
        .unwrap()
        .unwrap();
    let original = fs::read(seed.install_root.join(ACQUISITION)).unwrap();
    let previous = fixture.root.join("installer-rollback");
    fs::rename(&seed.install_root, &previous).unwrap();
    put(
        &seed.install_root,
        "server.bin",
        b"official archive replacement",
    );
    assert!(!seed.install_root.join(ACQUISITION).exists());
    restore_library_program_acquisition(&handle).unwrap();
    restore_library_program_acquisition(&handle).unwrap();
    assert_eq!(
        fs::read(seed.install_root.join(ACQUISITION)).unwrap(),
        original
    );
    assert_eq!(fs::read(previous.join(ACQUISITION)).unwrap(), original);
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
    record_library_program_baseline(&seed.install_root, &fixture.descriptor, true, None).unwrap();
    assert!(!seed.install_root.join(ACQUISITION).exists());
    assert!(library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap());
}

#[test]
fn captured_acquisition_rejects_missing_target_or_conflicting_ownership_without_overwrite() {
    let fixture = Fixture::new();
    let source = fixture.source();
    assert!(
        read_library_program_acquisition(&source, &fixture.descriptor)
            .unwrap()
            .is_none()
    );
    let seed = fixture.seed(&source).unwrap();
    let handle = read_library_program_acquisition(&seed.install_root, &fixture.descriptor)
        .unwrap()
        .unwrap();
    let marker = seed.install_root.join(ACQUISITION);
    let mut replacement: serde_json::Value =
        serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    replacement["id"] = uuid::Uuid::new_v4().to_string().into();
    let replacement = serde_json::to_vec(&replacement).unwrap();
    fs::write(&marker, &replacement).unwrap();
    assert!(restore_library_program_acquisition(&handle).is_err());
    assert_eq!(fs::read(&marker).unwrap(), replacement);
    fs::rename(&seed.install_root, fixture.root.join("moved-target")).unwrap();
    assert!(restore_library_program_acquisition(&handle).is_err());
    assert!(!seed.install_root.exists());
    assert!(!source.join(ACQUISITION).exists());
}

#[test]
fn incomplete_acquisition_survives_failed_validation_then_clears_after_success() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    fs::write(source.join("server.bin"), b"modified prior installation").unwrap();
    let seed = fixture.seed(&source).unwrap();
    assert!(seed.requires_validation);
    let marker = seed.install_root.join(ACQUISITION);
    let pending = fs::read(&marker).unwrap();
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
    assert!(!library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap());

    // Failed or interrupted official validation revokes package completeness,
    // while retaining the manager-owned location for the next validated retry.
    record_library_program_baseline(&seed.install_root, &fixture.descriptor, false, None).unwrap();
    assert_eq!(fs::read(&marker).unwrap(), pending);
    assert!(!seed.install_root.join(CLEAN_PACKAGE).exists());
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
    let cancelled = AtomicBool::new(true);
    assert!(matches!(
        record_library_program_baseline(
            &seed.install_root,
            &fixture.descriptor,
            true,
            Some(&cancelled)
        ),
        Err(StorageError::InstanceCreationCancelled)
    ));
    assert_eq!(fs::read(&marker).unwrap(), pending);

    fs::write(
        seed.install_root.join("server.bin"),
        b"official executable after validation",
    )
    .unwrap();
    record_library_program_baseline(&seed.install_root, &fixture.descriptor, true, None).unwrap();
    assert!(!marker.exists());
    assert!(
        !library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).unwrap()
    );
    assert!(library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap());
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"modified prior installation"
    );
}

#[test]
fn complete_filtered_seed_requires_no_pending_acquisition_marker() {
    let fixture = Fixture::new();
    let source = fixture.source();
    fixture.record(&source);
    put(&source, "foreign-mod.dll", b"not in the official package");
    let seed = fixture.seed(&source).unwrap();
    assert!(!seed.requires_validation);
    assert!(!seed.install_root.join(ACQUISITION).exists());
    assert!(!seed.install_root.join("foreign-mod.dll").exists());
    assert!(library_program_is_pristine(&seed.install_root, &fixture.descriptor, None).unwrap());
}

#[test]
fn acquisition_marker_never_claims_an_existing_unknown_source_or_another_location() {
    let fixture = Fixture::new();
    let source = fixture.source();
    assert!(!library_program_acquisition_is_trusted(&source, &fixture.descriptor).unwrap());
    let seed = fixture.seed(&source).unwrap();
    let original = fs::read(seed.install_root.join(ACQUISITION)).unwrap();
    assert!(!source.join(ACQUISITION).exists());
    fs::write(source.join(ACQUISITION), &original).unwrap();
    assert!(library_program_acquisition_is_trusted(&source, &fixture.descriptor).is_err());
    assert!(record_library_program_baseline(&source, &fixture.descriptor, true, None).is_err());
    assert!(!source.join(CLEAN_PACKAGE).exists());
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
    let mut foreign_module = fixture.descriptor.clone();
    foreign_module.summary.id = "other-module".into();
    assert!(library_program_acquisition_is_trusted(&seed.install_root, &foreign_module).is_err());
    assert_eq!(
        fs::read(seed.install_root.join(ACQUISITION)).unwrap(),
        original
    );
}

#[test]
fn acquisition_marker_is_bounded_and_rejects_corrupt_or_unsupported_records() {
    let fixture = Fixture::new();
    let source = fixture.source();
    let seed = fixture.seed(&source).unwrap();
    let marker = seed.install_root.join(ACQUISITION);
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("id", serde_json::json!("not-a-uuid")),
        ("target", serde_json::json!("relative-target")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut record = original.clone();
        record[field] = value;
        fs::write(&marker, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(
            library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor)
                .is_err()
        );
    }
    fs::write(&marker, b"invalid JSON").unwrap();
    assert!(
        library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor).is_err()
    );
    fs::File::create(&marker)
        .unwrap()
        .set_len(MAX_ACQUISITION_BYTES + 1)
        .unwrap();
    assert!(
        matches!(library_program_acquisition_is_trusted(&seed.install_root, &fixture.descriptor),
        Err(StorageError::PrivateRuntimeRefresh { message, .. }) if message.contains("too large"))
    );
    assert!(!seed.install_root.join(CLEAN_PACKAGE).exists());
    assert_eq!(
        fs::read(source.join("server.bin")).unwrap(),
        b"official executable"
    );
}
