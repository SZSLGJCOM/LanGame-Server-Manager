use super::*;

const SUPPORT_FILES: [&str; 5] = [
    "mods/dedicated_server_mods_setup.lua",
    "mods/modsettings.lua",
    "mods/lgsm_world_map_observer/modinfo.lua",
    "mods/lgsm_world_map_observer/modmain.lua",
    "mods/lgsm_world_map_observer/render_observer.lua",
];

async fn fixture() -> CleanCreationFixture {
    let mut fixture = CleanCreationFixture::new().await;
    fixture.paths.modules_root = fs::canonicalize(repo_root().join("modules")).unwrap();
    fixture.descriptor = app_modules::discover_modules(&fixture.paths.modules_root)
        .unwrap()
        .into_iter()
        .find(|descriptor| descriptor.summary.id == "dontstarve")
        .unwrap();
    fixture.source = fixture.paths.games_root.join("dontstarve");
    fs::create_dir_all(fixture.source.join("bin64")).unwrap();
    fs::write(
        fixture
            .source
            .join("bin64/dontstarve_dedicated_server_nullrenderer_x64.exe"),
        b"synthetic package fixture; never executed",
    )
    .unwrap();
    fs::write(fixture.source.join("payload.bin"), vec![7; 512 * 1024 + 1]).unwrap();
    sync_modules(&fixture.paths, std::slice::from_ref(&fixture.descriptor))
        .await
        .unwrap();
    fixture
}

async fn assert_private_preview(fixture: &CleanCreationFixture) {
    let plan = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        None,
    )
    .await
    .unwrap();
    assert_eq!(plan.action, "independent_install");
}

#[tokio::test]
async fn dst_default_support_allows_two_real_deletions_with_the_same_library() {
    let fixture = fixture().await;
    let first = record_and_create(&fixture).await;
    let initial: Value = serde_json::from_slice(
        &fs::read(fixture.source.join(".langame-initial-package.json")).unwrap(),
    )
    .unwrap();
    let generated = SUPPORT_FILES.map(|relative| {
        assert!(initial["files"].get(relative).is_none(), "{relative}");
        let bytes = fs::read(fixture.source.join(relative)).unwrap();
        (relative, bytes)
    });
    let library = read_library_program_install(&fixture.paths, "dontstarve")
        .await
        .unwrap()
        .unwrap();

    delete_created(&fixture, &first).await;
    assert_reuse_preview(&fixture).await;
    let second = create(&fixture, "Recreated DST without a full program copy")
        .await
        .unwrap();
    assert_reused(&fixture, &second);
    let binding = read_instance_program_install(&fixture.paths, &second.provisioning.summary.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(binding.install.id, library.id);
    assert_eq!(binding.install.scope, ProgramInstallScope::Library);
    delete_created(&fixture, &second).await;
    assert_reuse_preview(&fixture).await;
    assert!(list_instances(&fixture.paths).await.unwrap().is_empty());
    for (relative, bytes) in generated {
        assert_eq!(fs::read(fixture.source.join(relative)).unwrap(), bytes);
    }
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        vec![7; 512 * 1024 + 1]
    );
}

#[tokio::test]
async fn dst_edited_support_and_custom_mods_are_preserved_and_never_reused() {
    for relative in SUPPORT_FILES.into_iter().chain([
        "mods/workshop-123/modmain.lua",
        "mods/lgsm_world_map_observer/operator-notes.txt",
    ]) {
        let fixture = fixture().await;
        let first = record_and_create(&fixture).await;
        delete_created(&fixture, &first).await;
        let path = fixture.source.join(relative);
        let generated = path.exists().then(|| fs::read(&path).unwrap());
        let edited = if let Some(bytes) = &generated {
            // Keep the ownership marker and byte length: neither is sufficient
            // evidence that an operator has not edited a generated Lua file.
            let mut edited = bytes.clone();
            let last = edited.last_mut().unwrap();
            *last = if *last == b' ' { b'\t' } else { b' ' };
            edited
        } else {
            b"operator Mod data must survive".to_vec()
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &edited).unwrap();
        let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
        assert_private_preview(&fixture).await;
        let next = create(&fixture, "Fresh DST without old Lua or Mods")
            .await
            .unwrap();
        assert_ne!(
            fs::canonicalize(&next.effective_install_root).unwrap(),
            fs::canonicalize(&fixture.source).unwrap(),
            "{relative}"
        );
        if let Some(generated) = generated {
            assert_eq!(
                fs::read(next.effective_install_root.join(relative)).unwrap(),
                generated,
                "{relative}"
            );
        } else {
            assert!(!next.effective_install_root.join(relative).exists());
        }
        assert_eq!(
            crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
            before,
            "{relative}"
        );
    }
}

#[tokio::test]
async fn dst_generated_workshop_selection_is_not_a_reusable_default() {
    let fixture = fixture().await;
    let first = record_and_create(&fixture).await;
    let details = read_instance_details(&fixture.paths, &first.provisioning.summary.id)
        .await
        .unwrap();
    let mut settings: Value = serde_json::from_str(&details.settings_json).unwrap();
    settings["shared_workshop_mod_ids"] = json!("4111111111");
    update_instance(
        &fixture.paths,
        UpdateInstanceInput {
            id: details.summary.id,
            bind_ip: details.summary.bind_ip,
            auto_backup_on_stop: details.auto_backup_on_stop,
            backup_retention_count: details.backup_retention_count,
            settings_json: serde_json::to_string(&settings).unwrap(),
            ports: details.ports,
        },
    )
    .await
    .unwrap();
    let setup = fixture.source.join(SUPPORT_FILES[0]);
    let selected = fs::read_to_string(&setup).unwrap();
    assert!(selected.contains("ServerModSetup(\"4111111111\")"));
    delete_created(&fixture, &first).await;
    assert_private_preview(&fixture).await;
    let next = create(&fixture, "New DST without old Workshop selections")
        .await
        .unwrap();
    assert_ne!(
        fs::canonicalize(&next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert_eq!(fs::read_to_string(setup).unwrap(), selected);
    assert!(
        !fs::read_to_string(next.effective_install_root.join(SUPPORT_FILES[0]))
            .unwrap()
            .contains("4111111111")
    );
}

#[tokio::test]
async fn dst_generated_defaults_do_not_override_official_inventory_hashes() {
    let fixture = fixture().await;
    let relative = SUPPORT_FILES[0];
    let path = fixture.source.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"-- original shipped setup\n").unwrap();
    let first = record_and_create(&fixture).await;
    let generated = fs::read(&path).unwrap();
    assert!(crate::templates::is_default_dst_support_file(&path, relative).unwrap());
    delete_created(&fixture, &first).await;
    for verify_program in [false, true] {
        assert!(
            !crate::program_seed::retired_library_is_clean(
                &fixture.source,
                "dontstarve",
                verify_program,
                None,
            )
            .unwrap(),
            "an official initial-only file must retain its original hash"
        );
    }
    assert_private_preview(&fixture).await;
    let next = create(&fixture, "Keep original-package verification")
        .await
        .unwrap();
    assert_ne!(
        fs::canonicalize(&next.effective_install_root).unwrap(),
        fs::canonicalize(&fixture.source).unwrap()
    );
    assert_eq!(fs::read(path).unwrap(), generated);
}

#[tokio::test]
async fn dst_reuse_still_rejects_changed_program_bytes_and_cancelled_inspection() {
    let fixture = fixture().await;
    let first = record_and_create(&fixture).await;
    delete_created(&fixture, &first).await;
    assert_reuse_preview(&fixture).await;
    let before = crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap();
    let error = inspect_instance_program_creation(
        &fixture.paths,
        &fixture.descriptor,
        Some(InstanceProgramMode::Independent),
        app_core::InstanceProgramSource::Verified,
        Some(Arc::new(AtomicBool::new(true))),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, StorageError::InstanceCreationCancelled));
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&fixture.source).unwrap(),
        before
    );
    fs::write(fixture.source.join("payload.bin"), b"changed program bytes").unwrap();
    let error = create(&fixture, "Reject changed verified program")
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        StorageError::CleanLibraryProgramRequired { .. }
    ));
    fixture.assert_no_instance().await;
    assert_eq!(
        fs::read(fixture.source.join("payload.bin")).unwrap(),
        b"changed program bytes"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn dst_exact_generated_files_behind_a_junction_are_rejected() {
    let fixture = fixture().await;
    let first = record_and_create(&fixture).await;
    delete_created(&fixture, &first).await;
    let helper = fixture.source.join("mods/lgsm_world_map_observer");
    let external = fixture.root.join("external-observer");
    fs::rename(&helper, &external).unwrap();
    let before = crate::test_file_snapshot::tree_snapshot(&external).unwrap();
    let created = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(helper.to_string_lossy().replace('/', "\\"))
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
    let result = create(&fixture, "Do not follow a generated-file junction").await;
    // Unlink only the test junction before assertions or fixture cleanup.
    fs::remove_dir(&helper).unwrap();
    assert!(preview.is_err());
    assert!(result.is_err());
    fixture.assert_no_instance().await;
    assert_eq!(
        crate::test_file_snapshot::tree_snapshot(&external).unwrap(),
        before
    );
}
