use super::*;

fn test_paths(root: &std::path::Path) -> StoragePaths {
    StoragePaths {
        app_data_root: root.join("app-data"),
        settings_path: root.join("app-data/settings.json"),
        database_path: root.join("app-data/db/lgs.db"),
        logs_root: root.join("logs"),
        modules_root: root.join("modules"),
        migrations_root: root.join("migrations"),
        steamcmd_root: root.join("steamcmd"),
        games_root: root.join("games"),
        instances_root: root.join("instances"),
        archives_root: root.join("instances").join(".trash"),
    }
}

#[tokio::test]
async fn cancelled_waiter_keeps_instance_locked_until_blocking_work_finishes() {
    let root = std::env::temp_dir().join(format!("instance-worker-lock-{}", uuid::Uuid::new_v4()));
    let paths = test_paths(&root);
    let lock = acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
    let (started, started_receiver) = tokio::sync::oneshot::channel();
    let (release, release_receiver) = std::sync::mpsc::channel();
    let marker = root.join("copy-complete");
    let worker_marker = marker.clone();
    let mut worker = lock.spawn_blocking(move || {
        started.send(()).unwrap();
        release_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        fs::write(worker_marker, "complete").unwrap();
    });
    {
        let worker = &mut worker;
        let caller = async move {
            let _caller_lock = lock;
            worker.await.unwrap();
        };
        tokio::pin!(caller);
        tokio::select! {
            () = &mut caller => panic!("blocked worker unexpectedly finished"),
            started = started_receiver => started.unwrap(),
        }
        // Dropping the polled caller cancels its await and drops its lock owner.
        // Keep the JoinHandle outside it so completion can be checked exactly.
    }
    assert!(matches!(
        acquire_instance_settings_mutation_lock(&paths, "one"),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    let other = acquire_instance_settings_mutation_lock(&paths, "two").unwrap();
    assert!(!marker.exists());
    release.send(()).unwrap();
    worker.await.unwrap();
    assert_eq!(fs::read_to_string(marker).unwrap(), "complete");
    let available = acquire_instance_settings_mutation_lock(&paths, "one").unwrap();
    drop(available);
    drop(other);
    fs::remove_dir_all(root).unwrap();
}
