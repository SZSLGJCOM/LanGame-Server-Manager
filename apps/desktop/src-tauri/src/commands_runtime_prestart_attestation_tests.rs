use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RepairAttestation {
    MissingInventory,
    UnknownVersion,
    IgnoredValidation,
}

async fn check_official_repair_attestation(case: RepairAttestation) -> TestResult {
    let _serial = crate::commands::tests::command_smoke_lock().lock().await;
    let fixture = Fixture::new(false).await?;
    let missing_inventory = case == RepairAttestation::MissingInventory;
    let repaired = case != RepairAttestation::IgnoredValidation;
    let mut library_before = Vec::new();
    for relative in [
        "x.bin",
        "steamapps/appmanifest_7.acf",
        ".langame-clean-package.json",
        ".langame-initial-package.json",
    ] {
        let path = fixture.library.join(relative);
        if path.exists() {
            library_before.push((path.clone(), fs::read(path)?));
        }
    }
    let operator = fixture.program.join("operator-only.cfg");
    fs::write(&operator, b"private operator data")?;
    let clean_before = if missing_inventory {
        None
    } else {
        Some(fs::read(
            fixture.program.join(".langame-clean-package.json"),
        )?)
    };
    if missing_inventory {
        for name in [
            ".langame-clean-package.json",
            ".langame-initial-package.json",
        ] {
            let path = fixture.program.join(name);
            if path.exists() {
                fs::remove_file(&path)?;
            }
            assert!(!path.exists());
        }
        fixture.damage(false)?;
    } else if case == RepairAttestation::UnknownVersion {
        fixture.damage(false)?;
        app_storage::sync_instance_game_install(
            &fixture.storage.paths,
            &fixture.instance.summary.id,
            &GameInstallSyncRecord {
                module_id: "valheim".into(),
                install_root: fixture.program.to_string_lossy().into_owned(),
                install_state: InstallState::Corrupted,
                current_version: None,
                mark_verified: false,
            },
        )
        .await?;
        let owner =
            app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.program)
                .await?
                .unwrap();
        assert_eq!(owner.scope, app_storage::ProgramInstallScope::Instance);
        assert!(owner.current_version.is_none());
        assert_eq!(
            fs::read(fixture.program.join(".langame-clean-package.json"))?.as_slice(),
            clean_before.as_deref().unwrap(),
        );
    } else {
        // Keep the executable and current build identity. A native success line
        // cannot certify damaged bytes merely because the quick probe is green.
        fs::write(fixture.program.join("x.bin"), b"xyz")?;
        fs::write(
            fixture
                .storage
                .paths
                .steamcmd_root
                .join("ignore-validation"),
            b"",
        )?;
        assert_eq!(
            fixture.module().summary.install_state,
            InstallState::Installed
        );
        assert_eq!(
            app_storage::program_launch_files_match_baseline(
                &fixture.program,
                &fixture.descriptor,
                None,
            )?,
            Some(false),
        );
    }
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&fixture.storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let state = app.state::<DesktopState>();
    app_steamcmd::ensure_steamcmd_installed(&state.install_locks, &fixture.storage.settings)
        .await?;
    let id = &fixture.instance.summary.id;
    let attempts = if repaired { 1 } else { 2 };
    for attempt in 0..attempts {
        let module = fixture.module();
        let reservation = match state.try_reserve_runtime_start(id, "manual")? {
            RuntimeStartReservationAttempt::Reserved(reservation) => reservation,
            other => panic!("unexpected start reservation: {other:?}"),
        };
        let _instance = state.acquire_instance_mutation(id).await;
        let guard = app_steamcmd::acquire_game_install_lifecycle(
            &state.install_locks,
            "valheim",
            std::slice::from_ref(&fixture.program),
        )
        .await?;
        let result = tokio::time::timeout(
            BUDGET,
            run_prestart_update_if_needed(
                &state,
                &fixture.storage,
                PrestartUpdateRequest {
                    descriptor: &fixture.descriptor,
                    module: &module,
                    instance: &fixture.instance,
                    install_guard: guard,
                    reservation: &reservation,
                    context: PrestartUpdateLogContext {
                        source: "manual",
                        console_log_path: None,
                    },
                },
            ),
        )
        .await
        .expect("official repair attestation must settle");
        assert_eq!(
            fs::read_to_string(fixture.storage.paths.steamcmd_root.join("native-commands"))?,
            "app_update 7 validate\n".repeat(attempt + 1),
        );
        let jobs = state.app_state.read().unwrap().jobs.clone();
        assert_eq!(jobs.len(), attempt + 1);
        assert_eq!(jobs[0].target_id.as_deref(), Some(id.as_str()));
        assert!(jobs[0].warnings.is_empty());
        if repaired {
            let outcome = result.map_err(|error| format!("official repair failed: {error}"))?;
            outcome
                .install_guard
                .ensure_scope("valheim", &fixture.program)?;
            assert!(outcome.warnings.is_empty());
            assert!(matches!(jobs[0].status, JobStatus::Completed));
            ensure_module_installed_for_start(&fixture.module())?;
            assert_eq!(fs::read(fixture.program.join("x.bin"))?, b"abc");
            for name in [
                ".langame-clean-package.json",
                ".langame-initial-package.json",
            ] {
                let inventory: Value =
                    serde_json::from_slice(&fs::read(fixture.program.join(name))?)?;
                let files = inventory["files"]
                    .as_object()
                    .expect("official file inventory");
                assert_eq!(files.len(), 2, "{name}: {files:?}");
                assert!(files.contains_key("steamapps/appmanifest_7.acf"));
                assert_eq!(
                    files["x.bin"],
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                );
                assert!(!files.contains_key("operator-only.cfg"));
            }
            assert!(app_storage::library_program_is_pristine(
                &fixture.program,
                &fixture.descriptor,
                None,
            )?);
            assert_eq!(
                app_storage::program_launch_files_match_baseline(
                    &fixture.program,
                    &fixture.descriptor,
                    None,
                )?,
                Some(true),
            );
            let owner =
                app_storage::read_program_install_owner(&fixture.storage.paths, &fixture.program)
                    .await?
                    .unwrap();
            assert_eq!(owner.current_version.as_deref(), Some("17"));
            app_steamcmd::read_program_install_revision(
                &fixture.storage.paths.instances_root,
                "valheim",
                &fixture.program,
            )?;
        } else {
            assert!(
                result.is_err(),
                "native success cannot authorize damaged bytes"
            );
            assert!(
                jobs.iter()
                    .all(|job| matches!(job.status, JobStatus::Failed))
            );
            assert_eq!(fs::read(fixture.program.join("x.bin"))?, b"xyz");
            // A failed verification must retain the evidence that made validation
            // necessary. Otherwise the next start could accept the same bad bytes.
            assert_eq!(
                fs::read(fixture.program.join(".langame-clean-package.json"))?.as_slice(),
                clean_before.as_deref().unwrap(),
                "attempt {} discarded or rewrote the pre-repair evidence",
                attempt + 1,
            );
            assert_eq!(
                app_storage::program_launch_files_match_baseline(
                    &fixture.program,
                    &fixture.descriptor,
                    None,
                )?,
                Some(false),
                "the next start must still detect damaged launch bytes",
            );
            assert!(!app_storage::library_program_is_pristine(
                &fixture.program,
                &fixture.descriptor,
                None,
            )?);
        }
        assert!(
            read_active_instance_run(&fixture.storage.paths, id)
                .await?
                .is_none()
        );
        assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    }
    assert_eq!(fs::read(operator)?, b"private operator data");
    assert_eq!(
        fs::read(Path::new(&fixture.instance.saves_path).join("world.sentinel"))?,
        b"existing world",
    );
    for (path, bytes) in library_before {
        assert_eq!(
            fs::read(&path)?,
            bytes,
            "library changed: {}",
            path.display()
        );
    }
    assert!(
        read_active_instance_run(&fixture.storage.paths, id)
            .await?
            .is_none()
    );
    assert!(!state.runtime_supervisor.lock().unwrap().is_tracked(id));
    Ok(())
}

#[tokio::test]
async fn private_legacy_repair_rebuilds_only_official_inventory_without_touching_library()
-> TestResult {
    check_official_repair_attestation(RepairAttestation::MissingInventory).await
}

#[tokio::test]
async fn successful_native_validate_that_leaves_damaged_bytes_cannot_authorize_start() -> TestResult
{
    check_official_repair_attestation(RepairAttestation::IgnoredValidation).await
}

#[tokio::test]
async fn private_repair_with_unknown_previous_version_rebuilds_official_inventory() -> TestResult {
    check_official_repair_attestation(RepairAttestation::UnknownVersion).await
}
