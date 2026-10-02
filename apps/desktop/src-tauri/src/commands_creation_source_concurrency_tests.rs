use super::*;

#[tokio::test(flavor = "current_thread")]
async fn installed_astroneer_creation_completes_while_asa_owns_steamcmd()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    // Keep runtime discovery inside the disposable fixture. This file is never
    // executed: the held lease models the resources owned by an ASA download.
    fs::create_dir_all(&storage.paths.steamcmd_root)?;
    fs::write(storage.paths.steamcmd_root.join("steamcmd.exe"), b"fixture")?;
    let asa = app_steamcmd::acquire_game_install_lifecycle(
        "arksurvivalascended",
        &[storage.paths.games_root.join("arksurvivalascended")],
    )
    .await?;
    let steamcmd = asa.acquire_steamcmd(&storage.settings).await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let created = tokio::time::timeout(
        Duration::from_secs(10),
        CREATION_INSTALL_FORBIDDEN.scope(
            true,
            create_instance_record_inner(app.state::<DesktopState>(), input()),
        ),
    )
    .await??;
    let binding = app_storage::read_instance_program_install(&storage.paths, &created.summary.id)
        .await?
        .expect("creation must publish its program ownership");
    assert_eq!(
        binding.install.scope,
        app_storage::ProgramInstallScope::Library
    );
    assert_eq!(
        fs::canonicalize(&binding.install.install_root)?,
        fs::canonicalize(storage.paths.games_root.join("astroneer"))?
    );
    assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    assert_eq!(
        fs::read(binding.install.install_root.join("AstroServer.exe"))?,
        b"synthetic package fixture"
    );
    // The creation result and persisted binding exist before either download
    // lease is released, with the real installer explicitly forbidden.
    drop(steamcmd);
    drop(asa);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn creation_from_a_private_source_completes_while_an_unrelated_archive_catalog_is_locked()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = command_smoke_lock().lock().await;
    let (_root, _environment, storage) = prepare_fixture().await?;
    let app = tauri::test::mock_builder()
        .manage(DesktopState::from_storage(&storage))
        .build(tauri::test::mock_context(tauri::test::noop_assets()))?;
    let owner = create_instance_record_inner(app.state::<DesktopState>(), input()).await?;
    let independent = create_instance_record_inner(app.state::<DesktopState>(), input()).await?;
    let source =
        app_storage::read_instance_program_install(&storage.paths, &independent.summary.id)
            .await?
            .unwrap()
            .install;
    assert_eq!(source.scope, app_storage::ProgramInstallScope::Instance);
    let original = fs::read(source.install_root.join("AstroServer.exe"))?;
    app_storage::delete_instance(&storage.paths, &owner.summary.id).await?;
    uninstall_module_game(app.handle().clone(), "astroneer".into()).await?;
    assert!(!storage.paths.games_root.join("astroneer").exists());

    let lock_path = storage
        .paths
        .instances_root
        .join(".langame/locks/instance-settings/archive-inventory.lock");
    fs::create_dir_all(lock_path.parent().unwrap())?;
    let catalog = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    catalog.try_lock()?;
    assert!(
        app_storage::read_archived_program_sources(&storage.paths)
            .await
            .is_err(),
        "the test must hold the real archive catalog lock"
    );
    let created = tokio::time::timeout(
        Duration::from_secs(10),
        CREATION_INSTALL_FORBIDDEN.scope(
            true,
            create_instance_record_inner(app.state::<DesktopState>(), input()),
        ),
    )
    .await??;
    let program = app_storage::read_instance_program_install(&storage.paths, &created.summary.id)
        .await?
        .unwrap()
        .install
        .install_root;
    assert_eq!(fs::read(program.join("AstroServer.exe"))?, original);
    assert_eq!(
        fs::read(source.install_root.join("AstroServer.exe"))?,
        original
    );
    assert!(
        app_storage::read_archived_program_sources(&storage.paths)
            .await
            .is_err(),
        "creation must finish before the unrelated catalog lease is released"
    );
    assert!(!storage.paths.steamcmd_root.join("steamcmd.exe").exists());
    Ok(())
}
