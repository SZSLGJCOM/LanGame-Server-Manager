use super::*;

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_refresh_finishes_an_exit_collected_before_directory_removal()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    let state = app.state::<DesktopState>();
    let id = created.summary.id.clone();
    let run = mark_instance_process_started_with_identity(
        &storage.paths,
        &StartedInstanceProcess {
            instance_id: &id,
            session_id: Some("removed-directory-session"),
            process_key: "main",
            display_name: "Fixture",
            pid: 42,
            log_path: "fixture.log",
            is_primary: true,
        },
        None,
    )
    .await?;
    let exited = app_runtime::ExitedManagedProcess {
        summary: created.summary,
        session_id: Some("removed-directory-session".into()),
        run_id: run.run_id,
        process_key: "main".into(),
        display_name: "Fixture".into(),
        pid: 42,
        log_path: "fixture.log".into(),
        is_primary: true,
        exit_code: Some(0),
    };
    let owner = Arc::clone(&state.runtime_reconciliation);
    let admission = owner.acquire().await;
    let operation = state.begin_storage_context_operation("removed directory exit fixture")?;
    let mutation = Arc::new(state.acquire_instance_mutation(&id).await);
    owner
        .collect(
            Arc::clone(&admission),
            operation,
            HashMap::from([(id.clone(), mutation)]),
            move || Ok((vec![exited], Vec::new())),
        )
        .await?;
    drop(admission);
    remove_fixture_instance_root(&storage, &id)?;

    let instances = tokio::time::timeout(
        Duration::from_secs(10),
        list_instances_from_storage(app.state::<DesktopState>()),
    )
    .await??;
    assert!(instances.is_empty());
    assert!(owner.next()?.is_none());
    assert!(list_active_instance_runs(&storage.paths).await?.is_empty());
    assert!(state.try_acquire_instance_mutation(&id).await.is_some());
    Ok(())
}

fn remove_fixture_instance_root(storage: &StorageBootstrap, id: &str) -> std::io::Result<()> {
    let root = storage.paths.instances_root.join(id);
    assert!(root.starts_with(std::env::temp_dir()));
    assert_eq!(root.parent(), Some(storage.paths.instances_root.as_path()));
    fs::remove_dir_all(root)
}

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_parent_is_never_recreated_or_treated_as_individual_deletions()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    let original = &storage.paths.instances_root;
    let moved = original.with_file_name("instances-unavailable-fixture");
    assert!(original.starts_with(std::env::temp_dir()));
    assert_eq!(original.parent(), moved.parent());
    fs::rename(original, &moved)?;
    for _ in 0..2 {
        assert!(
            list_instances_from_storage(app.state::<DesktopState>())
                .await
                .is_err()
        );
        assert!(!original.exists());
        assert_eq!(list_instances(&storage.paths).await?.len(), 1);
    }
    fs::rename(&moved, original)?;
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );
    assert_eq!(
        read_instance_details(&storage.paths, &created.summary.id)
            .await?
            .summary
            .id,
        created.summary.id
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_protection_preserves_explicit_storage_path_initialization()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    delete_instance_record(app.handle().clone(), created.summary.id).await?;
    let instances = storage
        .paths
        .instances_root
        .with_file_name("explicit-new-instances");
    let archives = instances.join(".trash");
    assert!(!instances.exists());
    update_app_settings(
        app.state::<DesktopState>(),
        AppPathSettingsInput {
            servers_root: instances.to_string_lossy().into_owned(),
            archives_root: archives.to_string_lossy().into_owned(),
            games_root: storage.settings.games_root,
            steamcmd_root: storage.settings.steamcmd_root,
        },
    )
    .await?;
    assert!(instances.is_dir());
    assert!(archives.is_dir());
    assert_eq!(bootstrap_storage()?.paths.instances_root, instances);
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_refresh_removes_record_and_preserves_external_data()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(true).await?;
    let details = read_instance_details(&storage.paths, &created.summary.id).await?;
    let saves = PathBuf::from(&details.saves_path);
    fs::create_dir_all(&saves)?;
    fs::write(saves.join("world.dat"), b"keep external world")?;
    let library = app_storage::read_library_program_install(&storage.paths, "retirementfixture")
        .await?
        .unwrap();
    remove_fixture_instance_root(&storage, &created.summary.id)?;

    let instances = list_instances_from_storage(app.state::<DesktopState>()).await?;
    assert!(instances.is_empty());
    assert!(
        app.state::<DesktopState>()
            .app_state
            .read()
            .unwrap()
            .instances
            .is_empty()
    );
    assert_eq!(fs::read(saves.join("world.dat"))?, b"keep external world");
    assert_eq!(
        fs::read(library.install_root.join("server.bin"))?,
        b"inert retirement fixture program"
    );
    // Reopen the database through the public API: this is persisted removal.
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .is_empty()
    );
    let archives = app_storage::list_instance_archives(&storage.paths).await?;
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_refresh_waits_for_mutation_start_and_inventory_owners()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    remove_fixture_instance_root(&storage, &created.summary.id)?;
    let state = app.state::<DesktopState>();
    let mutation = state.acquire_instance_mutation(&created.summary.id).await;
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );
    drop(mutation);
    let start = state.try_reserve_runtime_start(&created.summary.id, "reconciliation-test")?;
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );
    drop(start);
    let inventory = state.storage_management.acquire()?;
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );
    drop(inventory);
    assert!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn missing_instance_runtime_can_be_explicitly_deleted_through_desktop_command()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = command_smoke_lock().lock().await;
    let (_environment, app, storage, created) = retirement_fixture(false).await?;
    let instance_root = storage.paths.instances_root.join(&created.summary.id);
    let runtime = instance_root.join("runtime");
    assert!(runtime.starts_with(std::env::temp_dir()));
    assert!(runtime.is_dir());
    fs::remove_dir_all(&runtime)?;
    assert_eq!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .len(),
        1
    );
    let preview =
        app_storage::inspect_instance_removal(&storage.paths, &created.summary.id).await?;
    assert_eq!(PathBuf::from(preview.data_path), instance_root);
    let deleted = delete_instance_record(app.handle().clone(), created.summary.id.clone()).await?;
    assert_eq!(deleted.instance_id, created.summary.id);
    assert!(!instance_root.exists());
    assert!(
        list_instances_from_storage(app.state::<DesktopState>())
            .await?
            .is_empty()
    );
    Ok(())
}
