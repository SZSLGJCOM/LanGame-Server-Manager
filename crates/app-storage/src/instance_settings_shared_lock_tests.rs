use super::*;

#[tokio::test]
async fn shared_read_lease_survives_cancellation_until_blocking_reader_finishes() {
    let root = std::env::temp_dir().join(format!("archive-read-lease-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let lock_path = root.join("inventory.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    file.try_lock_shared().unwrap();
    let lease = InstanceSettingsLock::new(file);
    let source = root.join("archive-data");
    fs::write(&source, b"retained archive bytes").unwrap();
    let (started, started_receiver) = tokio::sync::oneshot::channel();
    let (release, release_receiver) = std::sync::mpsc::channel();
    let mut worker = lease.spawn_blocking(move || {
        started.send(()).unwrap();
        release_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        fs::read(source).unwrap()
    });
    {
        let worker = &mut worker;
        let caller = async move {
            let _caller_lease = lease;
            worker.await.unwrap();
        };
        tokio::pin!(caller);
        tokio::select! {
            () = &mut caller => panic!("blocked reader unexpectedly finished"),
            started = started_receiver => started.unwrap(),
        }
        // Drop the polled caller while its real filesystem worker is paused.
    }

    assert!(matches!(
        acquire_lock_at_path(lock_path.clone()),
        Err(StorageError::InstanceSettingsLocked { .. })
    ));
    let another_reader = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    another_reader.try_lock_shared().unwrap();
    drop(another_reader);
    release.send(()).unwrap();
    assert_eq!(worker.await.unwrap(), b"retained archive bytes");
    let writer = acquire_lock_at_path(lock_path).unwrap();
    drop(writer);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}
