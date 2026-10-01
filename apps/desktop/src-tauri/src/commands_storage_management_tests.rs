use super::*;
use std::cell::Cell;
use std::future::ready;

#[tokio::test(flavor = "current_thread")]
async fn archive_and_restore_complete_during_an_unrelated_install()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = crate::commands::tests::command_smoke_lock().lock().await;
    let (_environment, app, storage, created) =
        crate::commands::tests::retirement_fixture(false).await?;
    let instance_id = created.summary.id;
    let config_before = std::fs::read(&created.config_file_path)?;
    let instance = read_instance_details(&storage.paths, &instance_id).await?;
    let world = std::path::PathBuf::from(&instance.saves_path).join("concurrent-world.dat");
    std::fs::create_dir_all(world.parent().unwrap())?;
    std::fs::write(&world, b"world survives unrelated installation")?;

    // Own the exact production lease retained throughout a download. The
    // synthetic package needs no network or real server process to exercise
    // the scheduling boundary that previously serialized all games.
    let install_roots = [storage.paths.games_root.join("other-install-fixture")];
    let _download =
        app_steamcmd::acquire_game_install_lifecycle("other-install-fixture", &install_roots)
            .await?;
    let archived = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        archive_instance_record(app.handle().clone(), instance_id.clone()),
    )
    .await??;
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert!(!world.exists());
    let restored = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        restore_instance_archive(
            app.handle().clone(),
            InstanceArchiveInput {
                archive_id: archived.archive_id,
            },
        ),
    )
    .await??;
    assert_eq!(restored.instance_id, instance_id);
    assert_eq!(std::fs::read(&created.config_file_path)?, config_before);
    assert_eq!(
        std::fs::read(&world)?,
        b"world survives unrelated installation"
    );
    let instances = list_instances(&storage.paths).await?;
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].id, instance_id);
    Ok(())
}

fn uninitialized_storage() -> StorageBootstrap {
    let root =
        std::env::temp_dir().join(format!("langame-archive-recovery-{}", uuid::Uuid::new_v4()));
    let paths = app_storage::StoragePaths {
        app_data_root: root.clone(),
        settings_path: root.join("settings.json"),
        database_path: root.join("db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    };
    StorageBootstrap {
        settings: paths.settings(),
        storage_status: StorageStatus::default(),
        paths,
    }
}

#[tokio::test]
async fn recovery_locks_instances_added_before_the_archive_lease() {
    let state = DesktopState::default();
    let storage = uninitialized_storage();
    let operation = state
        .begin_storage_context_operation("archive recovery lock regression")
        .unwrap();
    let _new_instance = state
        .try_acquire_instance_mutation("new-pending")
        .await
        .unwrap();
    let reads = Cell::new(0);
    let error = recover_archives_with_pending(&state, &storage, &operation, || {
        let previous_reads = reads.replace(reads.get() + 1);
        let ids = if previous_reads == 0 {
            assert!(state.storage_management.acquire().is_ok());
            vec![String::from("old-pending")]
        } else {
            assert!(state.storage_management.acquire().is_err());
            vec![String::from("old-pending"), String::from("new-pending")]
        };
        ready(Ok(ids))
    })
    .await
    .unwrap_err();

    assert_eq!(reads.get(), 2);
    assert_eq!(
        error,
        "Instance new-pending is busy; archive recovery must wait."
    );
    assert!(state.storage_management.acquire().is_ok());
    assert!(
        state
            .try_acquire_instance_mutation("old-pending")
            .await
            .is_some()
    );
    assert!(!storage.paths.app_data_root.exists());
}

#[tokio::test]
async fn recovery_without_pending_archives_does_not_acquire_the_archive_lease() {
    let state = DesktopState::default();
    let storage = uninitialized_storage();
    let operation = state
        .begin_storage_context_operation("empty archive recovery regression")
        .unwrap();
    let _active_scan = state.storage_management.acquire().unwrap();
    let reads = Cell::new(0);
    recover_archives_with_pending(&state, &storage, &operation, || {
        reads.set(reads.get() + 1);
        ready(Ok(Vec::new()))
    })
    .await
    .unwrap();

    assert_eq!(reads.get(), 1);
    assert!(state.storage_management.acquire().is_err());
    assert!(!storage.paths.app_data_root.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn disconnected_restore_finishes_storage_cache_and_success_log()
-> Result<(), Box<dyn std::error::Error>> {
    let _lock = crate::commands::tests::command_smoke_lock().lock().await;
    let (_environment, app, storage, created) =
        crate::commands::tests::retirement_fixture(false).await?;
    let instance_id = created.summary.id;
    let instance_root = storage.paths.instances_root.join(&instance_id);
    let config = std::fs::read(&created.config_file_path)?;
    let archived = archive_instance_record(app.handle().clone(), instance_id.clone()).await?;
    let state = app.state::<DesktopState>();
    assert!(!instance_root.exists());
    assert!(list_instances(&storage.paths).await?.is_empty());
    assert!(state.app_state.read().unwrap().instances.is_empty());

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = tokio::sync::oneshot::channel();
    let (completed_tx, completed_rx) = tokio::sync::oneshot::channel();
    let mut command = Box::pin(restore_instance_archive_with(
        app.handle().clone(),
        InstanceArchiveInput {
            archive_id: archived.archive_id.clone(),
        },
        move |paths, id| async move {
            started_tx
                .send(())
                .expect("test waits for the admitted worker");
            release_rx.await.expect("test releases the admitted worker");
            let result = app_storage::restore_instance_archive(&paths, &id).await;
            let _ = completed_tx.send(());
            result
        },
    ));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::select! {
            started = started_rx => started.expect("worker should enter its owned operation"),
            result = &mut command => panic!("restoration completed before its gate: {result:?}"),
        }
    })
    .await?;
    drop(command);
    assert!(state.storage_management.acquire().is_err());
    assert!(
        state
            .try_acquire_instance_mutation(&instance_id)
            .await
            .is_none()
    );
    assert!(state.begin_storage_context_transition().is_err());
    release_tx
        .send(())
        .expect("owned worker must survive caller disconnection");
    tokio::time::timeout(std::time::Duration::from_secs(10), completed_rx).await??;
    let completed = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.acquire_instance_mutation(&instance_id),
    )
    .await?;

    assert_eq!(std::fs::read(&created.config_file_path)?, config);
    let stored = list_instances(&storage.paths).await?;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].id, instance_id);
    assert!(matches!(
        stored[0].status,
        app_core::InstanceStatus::Stopped
    ));
    assert!(!stored[0].autostart);
    {
        let cache = state.app_state.read().unwrap();
        assert_eq!(cache.instances.len(), 1);
        assert_eq!(cache.instances[0].id, instance_id);
        assert!(matches!(
            cache.instances[0].status,
            app_core::InstanceStatus::Stopped
        ));
        assert!(!cache.instances[0].autostart);
    }
    let archives = app_storage::list_instance_archives(&storage.paths).await?;
    assert!(archives.archives.is_empty());
    assert!(archives.pending_deletions.is_empty());
    let log = std::fs::read_to_string(desktop_app_log_path(&storage))?;
    let entries = log
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert!(entries.iter().any(|entry| {
        entry["action"] == "instance.archive.restored"
            && entry["context"]["archive_id"] == archived.archive_id
            && entry["context"]["instance_id"] == instance_id
            && entry["context"]["refresh_error"].is_null()
    }));
    assert!(state.storage_management.acquire().is_ok());
    assert!(state.begin_storage_context_transition().is_ok());
    drop(completed);
    Ok(())
}
