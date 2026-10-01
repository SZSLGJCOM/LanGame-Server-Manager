use super::*;

async fn native_fixture(shipped_config: bool) -> CleanCreationFixture {
    let mut fixture = CleanCreationFixture::new().await;
    fixture.paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
    fixture.descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "enshrouded")
        .unwrap();
    fixture.source = fixture.paths.games_root.join("enshrouded");
    fs::create_dir_all(&fixture.source).unwrap();
    fs::write(
        fixture.source.join("enshrouded_server.exe"),
        b"official fixture executable",
    )
    .unwrap();
    fs::write(fixture.source.join("payload.bin"), vec![7; 512 * 1024 + 1]).unwrap();
    if shipped_config {
        fs::write(fixture.source.join("enshrouded_server.json"), b"{}").unwrap();
    }
    sync_modules(&fixture.paths, std::slice::from_ref(&fixture.descriptor))
        .await
        .unwrap();
    fixture
}

async fn record_and_create(fixture: &CleanCreationFixture) -> CreateInstanceResult {
    fixture.record();
    register(fixture).await;
    let first = create(fixture, "Original exclusive server").await.unwrap();
    assert_eq!(
        fs::canonicalize(&first.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    first
}

async fn delete_created(fixture: &CleanCreationFixture, created: &CreateInstanceResult) {
    crate::delete_instance(&fixture.paths, &created.provisioning.summary.id)
        .await
        .unwrap();
    assert!(
        !fixture
            .paths
            .instances_root
            .join(&created.provisioning.summary.id)
            .exists()
    );
    assert!(fixture.source.is_dir());
}

async fn assert_reuse_preview(fixture: &CleanCreationFixture) {
    let inventory = crate::inspect_module_programs(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        Arc::new(AtomicBool::new(false)),
        false,
    )
    .await
    .unwrap();
    assert!(inventory.creation.can_create);
    assert_eq!(inventory.creation.action, "existing_install");
    assert_eq!(inventory.creation.additional_bytes, Some(0));
}

fn assert_reused(fixture: &CleanCreationFixture, created: &CreateInstanceResult) {
    assert_eq!(
        fs::canonicalize(&created.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    let root = fixture
        .paths
        .instances_root
        .join(&created.provisioning.summary.id);
    assert!(instance_uses_exclusive_program(&root).unwrap());
    assert!(!root.join("runtime/payload.bin").exists());
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[tokio::test]
async fn real_deletion_releases_a_clean_used_library_for_in_place_recreation() {
    let fixture = native_fixture(false).await;
    let first = record_and_create(&fixture).await;
    let library = read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
        .await
        .unwrap()
        .unwrap();
    delete_created(&fixture, &first).await;
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    assert_reuse_preview(&fixture).await;
    let next = create(&fixture, "Recreated without copying").await.unwrap();
    assert_reused(&fixture, &next);
    let binding = read_instance_program_install(&fixture.paths, &next.provisioning.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.install.id, library.id);
    assert_eq!(binding.install.scope, ProgramInstallScope::Library);
}

#[tokio::test]
async fn real_deletion_preserves_unknown_files_and_mods_and_requires_a_private_copy() {
    for relative in ["unknown-loader.dll", "mods/personal-mod.dll"] {
        let fixture = native_fixture(false).await;
        let first = record_and_create(&fixture).await;
        let untrusted = fixture.source.join(relative);
        fs::create_dir_all(untrusted.parent().unwrap()).unwrap();
        fs::write(&untrusted, b"operator data must survive").unwrap();
        delete_created(&fixture, &first).await;
        assert_eq!(fs::read(&untrusted).unwrap(), b"operator data must survive");
        let plan = inspect_instance_program_creation(
            &fixture.paths,
            &fixture.descriptor,
            Some(InstanceProgramMode::Independent),
            app_core::InstanceProgramSource::Verified,
            None,
        )
        .await
        .unwrap();
        assert_eq!(plan.action, "independent_install", "{relative}");
        let next = create(&fixture, "Fresh independent program").await.unwrap();
        assert_ne!(
            fs::canonicalize(&next.effective_install_root).unwrap(),
            fs::canonicalize(&fixture.source).unwrap(),
            "{relative}"
        );
        assert!(!next.effective_install_root.join(relative).exists());
        assert_eq!(fs::read(&untrusted).unwrap(), b"operator data must survive");
        assert_eq!(
            fs::read(next.effective_install_root.join("payload.bin")).unwrap(),
            vec![7; 512 * 1024 + 1]
        );
    }
}

#[tokio::test]
async fn real_deletion_may_remove_initial_only_native_defaults_without_forcing_a_copy() {
    let fixture = native_fixture(true).await;
    let first = record_and_create(&fixture).await;
    let initial: Value = serde_json::from_slice(
        &fs::read(fixture.source.join(".langame-initial-package.json")).unwrap(),
    )
    .unwrap();
    let clean: Value = serde_json::from_slice(
        &fs::read(fixture.source.join(".langame-clean-package.json")).unwrap(),
    )
    .unwrap();
    assert!(initial["files"].get("enshrouded_server.json").is_some());
    assert!(clean["files"].get("enshrouded_server.json").is_none());
    let native = fixture.source.join("enshrouded_server.json");
    assert!(native.is_file());
    delete_created(&fixture, &first).await;
    assert!(
        !native.exists(),
        "real deletion owns the native instance config"
    );
    assert_reuse_preview(&fixture).await;
    let next = create(&fixture, "New native defaults").await.unwrap();
    assert_reused(&fixture, &next);
    let regenerated: Value = serde_json::from_slice(&fs::read(native).unwrap()).unwrap();
    assert_eq!(regenerated["name"], "New native defaults");
}

#[tokio::test]
async fn modified_initial_only_defaults_are_preserved_and_never_reused_in_place() {
    let mut fixture = native_fixture(false).await;
    fixture
        .descriptor
        .storage
        .runtime_copy_exclusions
        .push("official-settings.ini".into());
    let defaults = fixture.source.join("official-settings.ini");
    fs::write(&defaults, b"shipped defaults").unwrap();
    let first = record_and_create(&fixture).await;
    delete_created(&fixture, &first).await;
    fs::write(&defaults, b"operator changed defaults").unwrap();
    let next = create(&fixture, "No old settings import").await.unwrap();
    assert_ne!(
        fs::canonicalize(&next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert_eq!(fs::read(defaults).unwrap(), b"operator changed defaults");
    assert!(
        !next
            .effective_install_root
            .join("official-settings.ini")
            .exists()
    );
    assert_eq!(
        fs::read(next.effective_install_root.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[tokio::test]
async fn archived_exclusive_source_is_copied_and_remains_restorable() {
    let fixture = native_fixture(true).await;
    let first = record_and_create(&fixture).await;
    let archived = crate::archive_instance(&fixture.paths, &first.provisioning.summary.id)
        .await
        .unwrap();
    let next = create(&fixture, "Separate from archived source")
        .await
        .unwrap();
    assert_ne!(
        fs::canonicalize(&next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    delete_created(&fixture, &next).await;
    let restored = crate::restore_instance_archive(&fixture.paths, &archived.archive_id)
        .await
        .unwrap();
    assert_eq!(restored.instance_id, first.provisioning.summary.id);
    assert_reused(&fixture, &first);
}

#[tokio::test]
async fn pending_deletion_reserves_a_clean_library_after_instance_files_are_moved() {
    let fixture = native_fixture(true).await;
    let first = record_and_create(&fixture).await;
    let archived = crate::archive_instance(&fixture.paths, &first.provisioning.summary.id)
        .await
        .unwrap();
    let pool = connect_pool(&fixture.paths).await.unwrap();
    // Simulate a committed deletion interrupted after moving the instance root.
    let updated = sqlx::query(
        "UPDATE instance_archives SET purpose='delete',state='purging' WHERE archive_id=?1",
    )
    .bind(&archived.archive_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(updated.rows_affected(), 1);
    pool.close().await;
    assert!(
        !fixture
            .paths
            .instances_root
            .join(&first.provisioning.summary.id)
            .exists()
    );
    assert!(!fixture.source.join("enshrouded_server.json").exists());
    assert!(
        crate::program_seed::retired_library_is_clean(
            &fixture.source,
            &fixture.descriptor.summary.id,
            true,
            None,
        )
        .unwrap()
    );
    assert!(
        crate::instance_archive::external::program_has_archive_reservation(
            &fixture.paths,
            &fixture.source,
        )
        .await
        .unwrap()
    );
    let plan = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        None,
    )
    .await
    .unwrap();
    assert!(plan.can_create);
    assert_eq!(plan.action, "independent_install");
    let next = create(&fixture, "Keep pending deletion recoverable")
        .await
        .unwrap();
    assert_ne!(
        fs::canonicalize(&next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert_eq!(
        fs::read(next.effective_install_root.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[cfg(windows)]
#[tokio::test]
async fn unchanged_recreation_reuses_hash_evidence_and_only_tolerates_empty_directories() {
    let fixture = native_fixture(false).await;
    let first = record_and_create(&fixture).await;
    delete_created(&fixture, &first).await;
    let empty = fixture.source.join("old-world/nested/empty");
    fs::create_dir_all(&empty).unwrap();
    let cancellation = Arc::new(AtomicBool::new(false));
    let reads = count_hash_reads(&cancellation);
    let inventory = crate::inspect_module_programs(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        Arc::clone(&cancellation),
        false,
    )
    .await
    .unwrap();
    assert_eq!(inventory.creation.action, "existing_install");
    assert_eq!(inventory.creation.additional_bytes, Some(0));
    assert_eq!(
        reads.chunks(),
        0,
        "preview must not hash unchanged payloads"
    );
    let next = create_instance_with_options(
        &fixture.paths,
        &fixture.descriptor,
        CreateInstanceInput {
            name: "Reuse verified bytes".into(),
            module_id: fixture.descriptor.summary.id.clone(),
        },
        InstanceCreationOptions {
            prefer_existing_install: true,
            require_clean_program: true,
            program_mode: Some(InstanceProgramMode::Independent),
            cancellation: Some(cancellation),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_reused(&fixture, &next);
    assert!(empty.is_dir());
    assert_eq!(
        reads.chunks(),
        0,
        "unchanged bytes reuse their verified identities"
    );
    delete_created(&fixture, &next).await;
    let unknown = empty.join("world.sav");
    fs::write(&unknown, b"new data is not an empty directory").unwrap();
    let copied = create(&fixture, "Keep the old world separate")
        .await
        .unwrap();
    assert_ne!(
        fs::canonicalize(&copied.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert!(!copied.effective_install_root.join("old-world").exists());
    assert_eq!(
        fs::read(unknown).unwrap(),
        b"new data is not an empty directory"
    );
}

#[tokio::test]
async fn mismatched_reuse_ownership_is_rejected_without_rewriting_records_or_files() {
    for (filename, field) in [
        (".langame-program-usage.json", "program_id"),
        (".langame-program-identity.json", "module_id"),
    ] {
        let fixture = native_fixture(false).await;
        let first = record_and_create(&fixture).await;
        delete_created(&fixture, &first).await;
        let path = fixture.source.join(filename);
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record[field] = Value::String("other-installation".into());
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        let files_before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
        let library_before =
            read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
                .await
                .unwrap()
                .unwrap();
        let preview = inspect_instance_program_creation(
            &fixture.paths,
            &fixture.descriptor,
            Some(InstanceProgramMode::Independent),
            app_core::InstanceProgramSource::Verified,
            None,
        )
        .await
        .unwrap_err();
        assert!(
            preview.to_string().contains("identity"),
            "{filename}: {preview}"
        );
        let error = create(&fixture, "No ownership repair by guessing")
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("identity"),
            "{filename}: {error}"
        );
        fixture.assert_no_instance().await;
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
            files_before,
            "{filename}"
        );
        let library_after =
            read_library_program_install(&fixture.paths, &fixture.descriptor.summary.id)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            (
                library_after.id,
                library_after.install_root,
                library_after.scope,
                library_after.owner_instance_id,
                library_after.install_state,
                library_after.current_version,
            ),
            (
                library_before.id,
                library_before.install_root,
                library_before.scope,
                library_before.owner_instance_id,
                library_before.install_state,
                library_before.current_version,
            )
        );
    }
}

#[cfg(windows)]
#[tokio::test]
async fn retired_library_junction_is_rejected_without_touching_external_files() {
    let fixture = native_fixture(false).await;
    let first = record_and_create(&fixture).await;
    delete_created(&fixture, &first).await;
    let source_before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let external = fixture.root.join("external-world");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("sentinel.sav"), b"external saved world").unwrap();
    let external_before = crate::test_file_snapshot::tree_snapshot(&external).unwrap();
    let link = fixture.source.join("linked-world");
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(external.to_string_lossy().replace('/', "\\"))
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let preview = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        None,
    )
    .await;
    let result = create(&fixture, "Do not traverse the external world").await;
    // Unlink the exact test junction before any assertion or recursive teardown.
    fs::remove_dir(&link).unwrap();
    assert!(preview.is_err());
    assert!(result.is_err());
    fixture.assert_no_instance().await;
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        source_before
    );
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&external).unwrap(),
        external_before
    );
}
