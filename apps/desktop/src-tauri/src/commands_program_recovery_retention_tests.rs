use super::*;

fn recovery_fixture(root: &Path, minecraft: bool) -> TestResult<PathBuf> {
    let recovery = if minecraft {
        root.join(".langame/minecraft-recovery")
    } else {
        root.join(format!(
            ".langame-archive-recovery-{}",
            uuid::Uuid::new_v4()
        ))
    };
    let payloads: &[(&str, &[u8])] = if minecraft {
        &[
            (
                "fixture-publication/server.jar",
                b"displaced custom server jar",
            ),
            (
                "fixture-publication/jre/bin/java.exe",
                b"displaced original runtime",
            ),
        ]
    } else {
        &[
            ("files/server.bin", b"displaced custom program"),
            ("files/assets/required.bin", b"displaced asset bytes"),
            (
                "recovery.json",
                br#"{"version":1,"module_id":"astroneer","files":{"server.bin":{"preserved_sha256":"fixture-evidence"}}}"#,
            ),
        ]
    };
    for (relative, bytes) in payloads {
        let path = recovery.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, bytes)?;
    }
    Ok(recovery.canonicalize()?)
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_uninstall_preserves_recovery_bytes_without_retaining_programs()
-> TestResult {
    let _guard = command_smoke_lock().lock().await;
    for module in ["astroneer", "minecraft"] {
        let fixture = Fixture::new(module).await?;
        let private = fixture
            .create(&fixture.base, app_core::InstanceProgramMode::Independent)
            .await?;
        let instance_id = &private.provisioning.summary.id;
        let owner_before =
            app_storage::read_instance_program_install(&fixture.storage.paths, instance_id)
                .await?
                .unwrap();
        assert_eq!(
            owner_before.install.scope,
            app_storage::ProgramInstallScope::Instance
        );
        let recovery = recovery_fixture(&fixture.base, module == "minecraft")?;
        let private_recovery =
            recovery_fixture(&private.effective_install_root, module == "minecraft")?;
        let recovery_before = files(&recovery)?;
        let private_recovery_before = files(&private_recovery)?;
        let private_before = files(&private.effective_install_root)?;
        let ordinary_metadata = fixture.base.join(".langame/unrelated-manager.json");
        fs::create_dir_all(ordinary_metadata.parent().unwrap())?;
        fs::write(&ordinary_metadata, b"replaceable installation metadata")?;

        let result = fixture.uninstall().await?;

        assert_eq!(result.install_state, InstallState::NotInstalled);
        assert!(!result.executable_exists);
        assert!(result.cleanup.retained_installs.is_empty());
        assert!(reports_root(
            &result.cleanup.removed_install_roots,
            &fixture.base
        ));
        assert!(reports_root(
            &result.cleanup.preserved_data_paths,
            &recovery
        ));
        for program in if module == "minecraft" {
            ["server.jar", "jre/bin/java.exe"]
        } else {
            ["AstroServer.exe", "original.dll"]
        } {
            assert!(
                !fixture.base.join(program).exists(),
                "{module}: recovery protection must not retain original program {program}"
            );
        }
        assert!(
            !ordinary_metadata.exists(),
            "retain the recovery subtree, not every .langame metadata file"
        );
        assert_eq!(files(&recovery)?, recovery_before);
        assert_eq!(files(&private.effective_install_root)?, private_before);
        assert_eq!(files(&private_recovery)?, private_recovery_before);
        let owner_after =
            app_storage::read_instance_program_install(&fixture.storage.paths, instance_id)
                .await?
                .unwrap();
        assert_eq!(owner_after.install.id, owner_before.install.id);
        assert_eq!(owner_after.install.install_state, InstallState::Installed);

        // A subsequent explicit cleanup must not reinterpret the data-only
        // root as a new installation or erase its recovery receipt/payloads.
        fixture.uninstall().await?;
        assert_eq!(files(&recovery)?, recovery_before);
        assert_eq!(files(&private.effective_install_root)?, private_before);
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_recovery_protection_allows_archive_update_then_program_uninstall()
-> TestResult {
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("astroneer").await?;
    let recovery = recovery_fixture(&fixture.base, false)?;
    let recovery_before = files(&recovery)?;
    let source_before = files(&fixture.base)?;
    let mut descriptor = fixture.descriptor.clone();
    let install = descriptor.install.as_mut().unwrap();
    install.download_url_windows = Some("https://example.invalid/verified-fixture.zip".into());
    install.download_integrity_windows = Some(app_core::DownloadIntegritySpec {
        sha256: "1".repeat(64),
        size: 1,
    });
    let protected = crate::commands::commands_mods::collect_protected_install_data_paths(
        &fixture.storage.paths,
        &descriptor,
        &fixture.base,
    )
    .await?
    .into_iter()
    .map(|entry| entry.path.expect("fixture protection has concrete paths"))
    .collect::<Vec<_>>();
    assert!(protected.contains(&recovery));

    let updated = fixture
        .storage
        .paths
        .games_root
        .join(format!("astroneer-updated-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&updated)?;
    fs::write(updated.join("AstroServer.exe"), b"next verified server")?;
    fs::write(updated.join("original.dll"), b"next verified dependency")?;
    app_storage::record_library_program_baseline(&updated, &descriptor, true, None)?;
    app_storage::merge_archive_program_update(
        &fixture.base,
        &updated,
        &descriptor,
        &protected,
        None,
    )?;
    let updated_recovery = updated.join(recovery.file_name().unwrap());
    assert_eq!(files(&fixture.base)?, source_before);
    assert_eq!(files(&updated_recovery)?, recovery_before);
    assert_eq!(
        fs::read(updated.join("AstroServer.exe"))?,
        b"next verified server"
    );
    assert_eq!(
        fs::read(updated.join("original.dll"))?,
        b"next verified dependency"
    );
    assert!(app_storage::archive_program_is_current(
        &updated,
        &descriptor,
        None
    )?);
    sync_game_installs(
        &fixture.storage.paths,
        &[GameInstallSyncRecord {
            module_id: descriptor.summary.id.clone(),
            install_root: updated.to_string_lossy().into_owned(),
            install_state: InstallState::Installed,
            current_version: Some("next-fixture-build".into()),
            mark_verified: true,
        }],
    )
    .await?;

    let result = fixture.uninstall().await?;
    assert!(result.cleanup.retained_installs.is_empty());
    assert!(reports_root(
        &result.cleanup.removed_install_roots,
        &updated
    ));
    assert!(reports_root(
        &result.cleanup.preserved_data_paths,
        &updated_recovery
    ));
    assert!(!updated.join("AstroServer.exe").exists());
    assert!(!updated.join("original.dll").exists());
    assert_eq!(files(&updated_recovery)?, recovery_before);
    assert_eq!(files(&recovery)?, recovery_before);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn library_cleanup_terraria_recovery_executable_is_never_an_installed_program() -> TestResult
{
    let _guard = command_smoke_lock().lock().await;
    let fixture = Fixture::new("terraria").await?;
    let relative = fixture
        .descriptor
        .install
        .as_ref()
        .and_then(|install| install.verification_path.as_deref())
        .expect("real Terraria descriptor declares its official entry");
    assert!(
        fixture
            .descriptor
            .process
            .as_ref()
            .unwrap()
            .executable
            .contains("{{")
    );
    let program = fixture.base.join(relative);
    fs::create_dir_all(program.parent().unwrap())?;
    fs::write(&program, b"verified official Terraria entry")?;
    app_storage::record_library_program_baseline(&fixture.base, &fixture.descriptor, true, None)?;
    let recovery = fixture.base.join(format!(
        ".langame-archive-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    let displaced = recovery.join("files").join(relative);
    fs::create_dir_all(displaced.parent().unwrap())?;
    fs::write(&displaced, b"preserved damaged Terraria entry")?;
    fs::write(
        recovery.join("recovery.json"),
        br#"{"version":1,"module_id":"terraria","reason":"original program repaired"}"#,
    )?;
    let before = files(&recovery)?;

    for attempt in 0..2 {
        let removed = fixture.uninstall().await?;
        assert_eq!(removed.install_state, InstallState::NotInstalled);
        assert!(!removed.executable_exists, "{removed:?}");
        assert!(removed.cleanup.retained_installs.is_empty(), "{removed:?}");
        if attempt == 0 {
            assert!(reports_root(
                &removed.cleanup.removed_install_roots,
                &fixture.base
            ));
            assert!(reports_root(
                &removed.cleanup.preserved_data_paths,
                &recovery
            ));
        }
        for file in [program.clone(), fixture.base.join("original.dll")] {
            assert!(
                !file.exists(),
                "program payload remains: {}",
                file.display()
            );
        }
        assert!(app_steamcmd::has_retained_install_data(&fixture.base));
        assert_eq!(files(&recovery)?, before);
    }
    Ok(())
}
