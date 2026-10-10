use super::*;

/// Copy only plain files from a previously acquired isolated Steam fixture.
/// Game manager metadata belongs to the new acquisition, never to its source.
fn copy_fixture_tree(source: &Path, target: &Path, omit_metadata: bool) -> TestResult {
    app_steamcmd::steam_depot::check_plain(source, true)?;
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        if omit_metadata && entry.file_name().to_string_lossy().starts_with(".langame-") {
            continue;
        }
        let from = entry.path();
        let to = target.join(entry.file_name());
        let directory = entry.file_type()?.is_dir();
        app_steamcmd::steam_depot::check_plain(&from, directory)?;
        if directory {
            copy_fixture_tree(&from, &to, omit_metadata)?;
        } else {
            check(
                !to.exists(),
                "fixture copy would overwrite an existing file",
            )?;
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in: requires LANGAME_VALHEIM_REUSE_FIXTURE with an isolated real Steam package; validates real Steam twice with a deterministic interrupted completion; no server is started"]
async fn native_valheim_interrupted_download_retry_reuse() -> TestResult {
    let _serial = command_smoke_lock().lock().await;
    let cache =
        PathBuf::from(std::env::var_os("LANGAME_VALHEIM_REUSE_FIXTURE").ok_or(
            "set LANGAME_VALHEIM_REUSE_FIXTURE to a previously downloaded isolated fixture",
        )?);
    let mut root = ProbeRoot::new()?;
    let _environment = ProgramDataEnvGuard::set(&root.path.join("programdata"));
    save_app_settings(fixture_settings(&root.path))?;
    let storage = bootstrap_storage()?;
    initialize_database(&storage.paths).await?;
    let descriptors = discover_modules(&storage.paths.modules_root)?;
    sync_modules(&storage.paths, &descriptors).await?;
    let descriptor = find_descriptor(&descriptors, MODULE)?;
    let source = storage.paths.games_root.join(MODULE);
    copy_fixture_tree(&cache.join("steamcmd"), &storage.paths.steamcmd_root, false)?;
    let app = mock_app(&storage)?;
    Box::pin(ensure_steamcmd_ready(
        app.state::<DesktopState>(),
        "valheim-retry".into(),
    ))
    .await?;
    // Exercise the ordinary first library install: no preallocated marker.
    // Cancel before the native command downloads the package, then reuse our
    // already acquired files as the partial payload left by that attempt.
    let state = app.state::<DesktopState>();
    let initial_operation = state.begin_storage_context_operation("first empty Valheim install")?;
    let initial_guard =
        app_steamcmd::acquire_game_install_lifecycle(MODULE, std::slice::from_ref(&source)).await?;
    let initial_module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        Some(source.to_string_lossy().as_ref()),
    );
    let initial_cancellation = app_steamcmd::InstallCancellation::new();
    let initial = Box::pin(
        crate::commands::commands_program_storage::install_program_with_baseline(
            crate::commands::commands_program_storage::ProgramUpdateRequest {
                storage: &storage,
                descriptor,
                module: &initial_module,
                root: &source,
                operation: &initial_operation,
                guard: initial_guard,
                validate: false,
                cancellation: &initial_cancellation,
            },
            |_| initial_cancellation.cancel(),
        ),
    )
    .await;
    check(
        matches!(
            initial,
            Err(app_steamcmd::SteamCmdError::InstallCancelled { .. })
        ),
        "first empty installation did not cancel",
    )?;
    let pending = fs::read(source.join(".langame-program-acquisition.json"))?;
    check(
        !source.join(CLEAN_PACKAGE).exists(),
        "cancelled first install left a clean inventory",
    )?;
    drop(initial_operation);
    println!("VALHEIM_DOWNLOAD_REUSE first_empty_failure_pending_preserved=true");
    copy_fixture_tree(&cache.join("games/valheim"), &source, true)?;
    let executable_sha = digest(&source.join("valheim_server.exe"))?;
    // Model a partial download without downloading the entire package again.
    // The real official validation must repair this missing executable.
    fs::remove_file(source.join("valheim_server.exe"))?;
    fs::write(
        source.join("custom-loader.dll"),
        b"operator file retained across retry",
    )?;
    let operation = app
        .state::<DesktopState>()
        .begin_storage_context_operation("interrupted Valheim completion")?;
    let guard =
        app_steamcmd::acquire_game_install_lifecycle(MODULE, std::slice::from_ref(&source)).await?;
    let module = map_module_details_with_install_state(
        &storage.settings,
        descriptor,
        Some(source.to_string_lossy().as_ref()),
    );
    let cancellation = app_steamcmd::InstallCancellation::new();
    let mut interrupted = false;
    let result = Box::pin(
        crate::commands::commands_program_storage::install_program_with_baseline(
            crate::commands::commands_program_storage::ProgramUpdateRequest {
                storage: &storage,
                descriptor,
                module: &module,
                root: &source,
                operation: &operation,
                guard,
                validate: false,
                cancellation: &cancellation,
            },
            |update| {
                // A deterministic failure after native Steam repaired the partial
                // payload but before the manager publishes its clean inventory.
                if update.progress_percent == 99.0
                    && update.detail == "正在校验程序文件并准备后续实例所需的干净基线…"
                {
                    interrupted = true;
                    cancellation.cancel();
                }
            },
        ),
    )
    .await;
    check(interrupted, "did not reach the injected completion failure")?;
    check(
        matches!(
            result,
            Err(app_steamcmd::SteamCmdError::InstallCancelled { .. })
        ),
        "interrupted acquisition did not report cancellation",
    )?;
    check(
        source.join(".langame-program-acquisition.json").is_file(),
        "failure cleared pending ownership",
    )?;
    check(
        !source.join(CLEAN_PACKAGE).exists(),
        "failed completion certified a package",
    )?;
    check(
        digest(&source.join("valheim_server.exe"))? == executable_sha,
        "real validation did not repair the missing executable",
    )?;
    drop(operation);

    let installed = Box::pin(install_module_game(
        app.state::<DesktopState>(),
        MODULE.into(),
    ))
    .await?;
    report_install(&installed, "retry_success");
    let completed = evidence(&source, "after_retry_success")?;
    check(
        completed.files.is_some() && !completed.acquisition_present,
        "successful retry did not finalize its inventory and pending marker",
    )?;
    check(
        !completed
            .files
            .as_ref()
            .unwrap()
            .contains_key("custom-loader.dll"),
        "retry trusted an operator file",
    )?;
    create_without_download(&app, &storage, &source, &completed, None, "retry_first").await?;
    create_without_download(&app, &storage, &source, &completed, None, "retry_second").await?;
    let validated = Box::pin(validate_module_game(
        app.state::<DesktopState>(),
        MODULE.into(),
    ))
    .await?;
    check(
        validated.current_version == installed.current_version,
        "live package changed version during the same-version probe",
    )?;
    let verified = evidence(&source, "after_retry_validation")?;
    check(
        verified.files.is_some(),
        "same-version validation discarded the clean package",
    )?;
    create_without_download(&app, &storage, &source, &verified, None, "retry_third").await?;
    // Model the durable state left by a successful retry in the old version:
    // Installed registration plus complete official bytes, but pending remains.
    fs::remove_file(source.join(CLEAN_PACKAGE))?;
    if source.join(".langame-initial-package.json").exists() {
        fs::remove_file(source.join(".langame-initial-package.json"))?;
    }
    fs::write(source.join(".langame-program-acquisition.json"), &pending)?;
    create_without_download(
        &app,
        &storage,
        &source,
        &verified,
        None,
        "old_completed_acquisition",
    )
    .await?;
    check(
        !source.join(".langame-program-acquisition.json").exists(),
        "creation did not recover the old completed acquisition",
    )?;
    // The old raw download path could lose its inventory without ever having
    // an acquisition marker. A complete registered package still recovers locally.
    fs::remove_file(source.join(CLEAN_PACKAGE))?;
    fs::remove_file(source.join(".langame-initial-package.json"))?;
    create_without_download(
        &app,
        &storage,
        &source,
        &verified,
        None,
        "old_missing_inventory",
    )
    .await?;
    check(
        fs::read(source.join("custom-loader.dll"))? == b"operator file retained across retry",
        "retry or creation changed the operator file",
    )?;
    // Preserve successful real evidence too, so no subsequent investigation
    // needs another game download. This remains inside the managed task work.
    drop(app);
    root.cleaned = true;
    println!(
        "VALHEIM_DOWNLOAD_REUSE retry_passed retained_root={} creation_installer_calls=0",
        root.path.display()
    );
    Ok(())
}
